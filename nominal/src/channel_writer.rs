//! Native bidirectional gRPC writes with stream-local schemas.
//!
//! Each operation waits for its acknowledgment: at most one message is outstanding, so
//! memory is bounded and callers naturally respect server backpressure. A declaration
//! must succeed before its schema IDs can be used. IDs belong to one connection only.
//!
//! Disconnects, timeouts and cancelled operations close the session. Reopen a stream,
//! redeclare schemas and explicitly replay unanswered chunks. Delivery is **at least
//! once**: an error does not prove a chunk had no effects. There are no transparent retries.

use std::time::Duration;

use prost::Message;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status, Streaming};

use crate::core::grpc::GrpcConnection;

mod wire {
    pub mod google {
        pub mod rpc {
            tonic::include_proto!("google.rpc");
        }
    }
    pub mod nominal {
        pub mod channel_writer {
            pub mod v2 {
                tonic::include_proto!("nominal.channel_writer.v2");
            }
        }
    }
}

pub use prost_types::Timestamp;
/// The v2 wire types, including all numeric, string, array, struct and log variants.
pub use wire::nominal::channel_writer::v2 as proto;

use proto::nominal_channel_writer_service_client::NominalChannelWriterServiceClient;
use proto::write_stream_request::Request;
use proto::write_stream_response::Response;
use proto::{ChannelData, DeclareSchemas, Schema, WriteChunk, WriteStreamRequest};

#[cfg(test)]
mod tests;

const MAX_MESSAGE_BYTES: usize = 50 * 1024 * 1024;

/// Per-operation limits. There is intentionally no call-lifetime deadline on a stream.
#[derive(Clone, Copy, Debug)]
pub struct WriteStreamOptions {
    /// Maximum time to open a stream, receive an acknowledgment, or finish draining.
    pub response_timeout: Duration,
}

impl Default for WriteStreamOptions {
    fn default() -> Self {
        Self {
            response_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteStreamError {
    /// Transport, authentication, authorization or server stream termination.
    #[error("stream terminated: {0}")]
    Rpc(#[source] Box<Status>),
    /// A per-message rejection; this stream remains usable. Partial effects are possible.
    #[error("message rejected: {0}")]
    Rejected(#[source] Box<Status>),
    #[error("stream operation timed out; reopen and redeclare schemas before replaying")]
    Timeout,
    #[error("stream is closed; reopen and redeclare schemas before replaying")]
    Closed,
    #[error("invalid stream response: {0}")]
    Protocol(&'static str),
    #[error("stream message exceeds the 50 MiB limit")]
    MessageTooLarge,
    #[error("response_timeout must be positive")]
    InvalidTimeout,
}

type Result<T> = std::result::Result<T, WriteStreamError>;

/// Opens authenticated streaming writes against the same API host as [`crate::NominalClient`].
#[derive(Clone)]
pub struct ChannelWriterClient {
    connection: GrpcConnection,
}

impl ChannelWriterClient {
    pub(crate) fn new(connection: &GrpcConnection) -> Self {
        Self {
            connection: connection.clone(),
        }
    }

    /// Open a stream and wait for the initial schemas to be acknowledged.
    pub async fn write_stream(&self, schemas: Vec<Schema>) -> Result<WriteStream> {
        self.write_stream_with_options(schemas, WriteStreamOptions::default())
            .await
    }

    pub async fn write_stream_with_options(
        &self,
        schemas: Vec<Schema>,
        options: WriteStreamOptions,
    ) -> Result<WriteStream> {
        if options.response_timeout.is_zero() {
            return Err(WriteStreamError::InvalidTimeout);
        }
        let request = declaration(1, schemas);
        check_size(&request)?;
        let (sender, receiver) = mpsc::channel(1);
        // Java gRPC may withhold response headers until the first response. Queue the
        // declaration before awaiting headers, rather than opening an empty stream.
        sender
            .try_send(request)
            .map_err(|_| WriteStreamError::Closed)?;
        let mut client =
            NominalChannelWriterServiceClient::new(self.connection.streaming_channel())
                .max_encoding_message_size(MAX_MESSAGE_BYTES)
                .max_decoding_message_size(MAX_MESSAGE_BYTES);
        let state = tokio::time::timeout(options.response_timeout, async {
            let responses = client
                .write_stream(ReceiverStream::new(receiver))
                .await
                .map_err(|status| WriteStreamError::Rpc(Box::new(status)))?
                .into_inner();
            let mut state = State { sender, responses };
            acknowledge(&mut state.responses, 1, Expected::Declaration).await?;
            Ok(state)
        })
        .await
        .map_err(|_| WriteStreamError::Timeout)??;
        Ok(WriteStream {
            state: Some(state),
            next_id: 2,
            options,
        })
    }
}

struct State {
    sender: mpsc::Sender<WriteStreamRequest>,
    responses: Streaming<proto::WriteStreamResponse>,
}

/// One schema namespace and an ordered sequence of acknowledged operations.
///
/// A mutable borrow serializes operations (one outstanding message). Use independent
/// streams for parallelism. Dropping the stream cancels it; use [`Self::finish`] for
/// a graceful half-close. Server rotation is surfaced as a terminal RPC error.
pub struct WriteStream {
    state: Option<State>,
    next_id: u64,
    options: WriteStreamOptions,
}

impl WriteStream {
    /// Atomically add schemas, waiting for acknowledgment before returning.
    pub async fn declare_schemas(&mut self, schemas: Vec<Schema>) -> Result<()> {
        self.exchange(declaration(self.next_id, schemas), Expected::Declaration)
            .await
    }

    /// Write parallel timestamp/value columns and wait for durable acceptance.
    /// Acceptance is not a guarantee of immediate downstream visibility.
    pub async fn write_chunk(&mut self, channels: Vec<ChannelData>) -> Result<()> {
        self.exchange(
            WriteStreamRequest {
                request: Some(Request::Chunk(WriteChunk {
                    message_id: self.next_id,
                    channels,
                })),
            },
            Expected::Chunk,
        )
        .await
    }

    async fn exchange(&mut self, request: WriteStreamRequest, expected: Expected) -> Result<()> {
        check_size(&request)?;
        let id = self.next_id;
        let next_id = id
            .checked_add(1)
            .ok_or(WriteStreamError::Protocol("message IDs exhausted"))?;
        // Taking ownership before the first await makes cancellation safe: an abandoned
        // operation drops the connection instead of leaving an unread ack for the next call.
        let mut state = self.state.take().ok_or(WriteStreamError::Closed)?;
        self.next_id = next_id;
        let result = tokio::time::timeout(self.options.response_timeout, async {
            state
                .sender
                .send(request)
                .await
                .map_err(|_| WriteStreamError::Closed)?;
            acknowledge(&mut state.responses, id, expected).await
        })
        .await
        .map_err(|_| WriteStreamError::Timeout)?;
        if result.is_ok() || matches!(result, Err(WriteStreamError::Rejected(_))) {
            self.state = Some(state);
        }
        result
    }

    /// Half-close the request side and wait for clean server completion.
    pub async fn finish(mut self) -> Result<()> {
        let State {
            sender,
            mut responses,
        } = self.state.take().ok_or(WriteStreamError::Closed)?;
        drop(sender);
        tokio::time::timeout(self.options.response_timeout, async {
            while let Some(response) = responses.message().await.map_err(|status| WriteStreamError::Rpc(Box::new(status)))? {
                if !matches!(response.response, Some(Response::Accepted(ref ack)) if ack.message_ids.is_empty()) {
                    return Err(WriteStreamError::Protocol("unexpected response while draining"));
                }
            }
            Ok(())
        }).await.map_err(|_| WriteStreamError::Timeout)?
    }
}

#[derive(Clone, Copy)]
enum Expected {
    Declaration,
    Chunk,
}

fn declaration(message_id: u64, schemas: Vec<Schema>) -> WriteStreamRequest {
    WriteStreamRequest {
        request: Some(Request::DeclareSchemas(DeclareSchemas {
            message_id,
            schemas,
        })),
    }
}

fn check_size(request: &WriteStreamRequest) -> Result<()> {
    if request.encoded_len() > MAX_MESSAGE_BYTES {
        Err(WriteStreamError::MessageTooLarge)
    } else {
        Ok(())
    }
}

async fn acknowledge(
    responses: &mut Streaming<proto::WriteStreamResponse>,
    id: u64,
    expected: Expected,
) -> Result<()> {
    loop {
        let response = responses
            .message()
            .await
            .map_err(|status| WriteStreamError::Rpc(Box::new(status)))?
            .ok_or(WriteStreamError::Protocol(
                "server completed before acknowledging message",
            ))?;
        match response.response {
            Some(Response::Accepted(ack)) if ack.message_ids.is_empty() => continue,
            Some(Response::SchemasDeclared(ack))
                if matches!(expected, Expected::Declaration) && ack.message_id == id =>
            {
                return Ok(());
            }
            Some(Response::Accepted(ack))
                if matches!(expected, Expected::Chunk) && ack.message_ids == [id] =>
            {
                return Ok(());
            }
            Some(Response::Rejected(rejected)) if rejected.message_id == id => {
                let status = rejected
                    .status
                    .ok_or(WriteStreamError::Protocol("rejection has no status"))?;
                if !(1..=16).contains(&status.code) {
                    return Err(WriteStreamError::Protocol(
                        "rejection has invalid status code",
                    ));
                }
                return Err(WriteStreamError::Rejected(Box::new(Status::with_details(
                    Code::from_i32(status.code),
                    status.message.clone(),
                    status.encode_to_vec().into(),
                ))));
            }
            _ => {
                return Err(WriteStreamError::Protocol(
                    "response kind or message ID does not match request",
                ));
            }
        }
    }
}
