use std::collections::BTreeSet;
use std::sync::atomic::AtomicU64;

use rand::Rng;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status, Streaming};

use super::*;
use proto::nominal_channel_writer_service_client::NominalChannelWriterServiceClient;
use proto::write_stream_request::Request;
use proto::write_stream_response::Response;

pub(super) struct Connection {
    pub sender: mpsc::Sender<proto::WriteStreamRequest>,
    pub responses: Streaming<proto::WriteStreamResponse>,
}

pub(super) fn declaration(
    message_id: u64,
    schemas: Vec<proto::Schema>,
) -> proto::WriteStreamRequest {
    proto::WriteStreamRequest {
        request: Some(Request::DeclareSchemas(proto::DeclareSchemas {
            message_id,
            schemas,
        })),
    }
}

pub(super) fn validate_schemas(schemas: &[proto::Schema]) -> Result<()> {
    if schemas.is_empty()
        || schemas.len() > 10_000
        || schemas.iter().map(Message::encoded_len).sum::<usize>() > 8 * 1024 * 1024
    {
        return Err(WriteStreamError::InvalidSchemas(
            "require 1..=10000 schemas totaling at most 8 MiB",
        ));
    }
    let mut ids = BTreeSet::new();
    if schemas
        .iter()
        .any(|schema| schema.schema_id == 0 || !ids.insert(schema.schema_id))
    {
        return Err(WriteStreamError::InvalidSchemas(
            "schema IDs must be nonzero and unique",
        ));
    }
    Ok(())
}

pub(super) async fn open_with_retry(
    grpc: &GrpcConnection,
    schemas: &[proto::Schema],
    ids: &AtomicU64,
    options: WriteStreamOptions,
) -> Result<Connection> {
    validate_schemas(schemas)?;
    for attempt in 0..=options.max_retries {
        match open(grpc, schemas, ids, options).await {
            Ok(connection) => return Ok(connection),
            Err(error) if retryable(&error) => {
                if attempt == options.max_retries {
                    return Err(WriteStreamError::RetryExhausted);
                }
                backoff(options, attempt).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

pub(super) async fn open(
    grpc: &GrpcConnection,
    schemas: &[proto::Schema],
    ids: &AtomicU64,
    options: WriteStreamOptions,
) -> Result<Connection> {
    tokio::time::timeout(options.response_timeout, async {
        // The server may defer response headers until a declaration is received.
        let mut batches = schemas.chunks(100);
        let first = batches
            .next()
            .ok_or(WriteStreamError::Protocol("no schemas to restore"))?;
        let id = next_id(ids)?;
        let (sender, receiver) = mpsc::channel(1);
        sender
            .try_send(declaration(id, first.to_vec()))
            .map_err(|_| WriteStreamError::Closed)?;
        let mut client = NominalChannelWriterServiceClient::new(grpc.streaming_channel())
            .max_encoding_message_size(MAX_MESSAGE_BYTES)
            .max_decoding_message_size(MAX_MESSAGE_BYTES);
        let responses = client
            .write_stream(ReceiverStream::new(receiver))
            .await
            .map_err(rpc)?
            .into_inner();
        let mut connection = Connection { sender, responses };
        declared(&mut connection.responses, id).await?;
        for schemas in batches {
            let id = next_id(ids)?;
            connection
                .sender
                .send(declaration(id, schemas.to_vec()))
                .await
                .map_err(|_| rpc(Status::unavailable("request transport closed")))?;
            declared(&mut connection.responses, id).await?;
        }
        Ok(connection)
    })
    .await
    .map_err(|_| WriteStreamError::Timeout)?
}

async fn declared(responses: &mut Streaming<proto::WriteStreamResponse>, id: u64) -> Result<()> {
    loop {
        let response = responses.message().await.map_err(rpc)?.ok_or_else(|| {
            rpc(Status::unavailable(
                "server completed during schema restoration",
            ))
        })?;
        match response.response {
            Some(Response::Accepted(ack)) if ack.message_ids.is_empty() => {}
            Some(Response::SchemasDeclared(ack)) if ack.message_id == id => return Ok(()),
            Some(Response::Rejected(rejection)) if rejection.message_id == id => {
                return Err(rejected(rejection)?);
            }
            _ => {
                return Err(WriteStreamError::Protocol(
                    "unexpected schema restoration response",
                ));
            }
        }
    }
}

pub(super) fn rejected(rejection: proto::Rejected) -> Result<WriteStreamError> {
    let status = rejection
        .status
        .ok_or(WriteStreamError::Protocol("rejection has no status"))?;
    if !(1..=16).contains(&status.code) {
        return Err(WriteStreamError::Protocol("invalid rejection status code"));
    }
    Ok(WriteStreamError::Rejected(Box::new(Status::with_details(
        Code::from_i32(status.code),
        status.message.clone(),
        status.encode_to_vec().into(),
    ))))
}

pub(super) fn rpc(status: Status) -> WriteStreamError {
    WriteStreamError::Rpc(Box::new(status))
}

pub(super) fn retryable(error: &WriteStreamError) -> bool {
    match error {
        WriteStreamError::Timeout => true,
        WriteStreamError::Rpc(status) => matches!(
            status.code(),
            Code::Unavailable | Code::ResourceExhausted | Code::DeadlineExceeded | Code::Cancelled
        ),
        _ => false,
    }
}

pub(super) async fn backoff(options: WriteStreamOptions, attempt: u32) {
    let ceiling = options
        .initial_backoff
        .saturating_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX))
        .min(options.max_backoff);
    let seconds = if ceiling.is_zero() {
        0.0
    } else {
        rand::rng().random_range(0.0..ceiling.as_secs_f64())
    };
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
}

pub(super) async fn finish(connection: Connection, timeout: Duration) -> Result<()> {
    let Connection {
        sender,
        mut responses,
    } = connection;
    drop(sender);
    tokio::time::timeout(timeout, async {
        while let Some(response) = responses.message().await.map_err(rpc)? {
            if !matches!(response.response, Some(Response::Accepted(ack)) if ack.message_ids.is_empty()) {
                return Err(WriteStreamError::Protocol("unexpected response while draining"));
            }
        }
        Ok(())
    }).await.map_err(|_| WriteStreamError::Timeout)?
}
