use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Request as RpcRequest, Response as RpcResponse};

use super::*;
use crate::NominalClient;
use proto::nominal_channel_writer_service_server::{
    NominalChannelWriterService, NominalChannelWriterServiceServer,
};

struct FakeService {
    replies: Mutex<Option<mpsc::Receiver<std::result::Result<proto::WriteStreamResponse, Status>>>>,
    events: mpsc::UnboundedSender<Option<WriteStreamRequest>>,
    auth: Arc<Mutex<Option<String>>>,
}

#[tonic::async_trait]
impl NominalChannelWriterService for FakeService {
    async fn write(
        &self,
        _: RpcRequest<proto::WriteRequest>,
    ) -> std::result::Result<RpcResponse<proto::WriteResponse>, Status> {
        Err(Status::unimplemented("unary v2 writes are not supported"))
    }

    type WriteStreamStream =
        ReceiverStream<std::result::Result<proto::WriteStreamResponse, Status>>;

    async fn write_stream(
        &self,
        request: RpcRequest<Streaming<WriteStreamRequest>>,
    ) -> std::result::Result<RpcResponse<Self::WriteStreamStream>, Status> {
        *self.auth.lock().unwrap() = request
            .metadata()
            .get("authorization")
            .map(|v| v.to_str().unwrap().to_owned());
        let mut incoming = request.into_inner();
        // Do not send response headers until the first request arrives. This catches
        // transports that buffer the entire request or wait for headers before sending.
        let first = incoming
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("missing initial declaration"))?;
        let events = self.events.clone();
        events
            .send(Some(first))
            .map_err(|_| Status::cancelled("test finished"))?;
        tokio::spawn(async move {
            while let Ok(Some(request)) = incoming.message().await {
                if events.send(Some(request)).is_err() {
                    return;
                }
            }
            let _ = events.send(None);
        });
        let replies = self
            .replies
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| Status::internal("only one stream expected"))?;
        Ok(RpcResponse::new(ReceiverStream::new(replies)))
    }
}

struct Harness {
    client: ChannelWriterClient,
    replies: Option<mpsc::Sender<std::result::Result<proto::WriteStreamResponse, Status>>>,
    events: mpsc::UnboundedReceiver<Option<WriteStreamRequest>>,
    auth: Arc<Mutex<Option<String>>>,
    shutdown: Option<oneshot::Sender<()>>,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel(16);
        let (events_tx, events) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let auth = Arc::new(Mutex::new(None));
        let service = FakeService {
            replies: Mutex::new(Some(rx)),
            events: events_tx,
            auth: auth.clone(),
        };
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(NominalChannelWriterServiceServer::new(service))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let client = NominalClient::builder("test-token")
            .base_url(url)
            .build()
            .unwrap()
            .channel_writer();
        Self {
            client,
            replies: Some(tx),
            events,
            auth,
            shutdown: Some(shutdown_tx),
            server,
        }
    }

    async fn reply(&self, response: Response) {
        self.replies
            .as_ref()
            .unwrap()
            .send(Ok(proto::WriteStreamResponse {
                response: Some(response),
            }))
            .await
            .unwrap();
    }

    async fn next_request(&mut self) -> WriteStreamRequest {
        tokio::time::timeout(Duration::from_secs(5), self.events.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    async fn open(&mut self) -> WriteStream {
        self.reply(Response::SchemasDeclared(proto::SchemasDeclared {
            message_id: 1,
        }))
        .await;
        let stream = self.client.write_stream(vec![schema()]).await.unwrap();
        assert_eq!(self.next_request().await, declaration(1, vec![schema()]));
        stream
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.server.abort();
    }
}

fn schema() -> Schema {
    Schema {
        schema_id: 7,
        dataset_rid: "ri.catalog.main.dataset.00000000-0000-0000-0000-000000000001".into(),
        channel: "temperature".into(),
        r#type: proto::ChannelType::Double as i32,
        ..Default::default()
    }
}

fn data() -> ChannelData {
    ChannelData {
        schema_id: 7,
        timestamps: vec![Timestamp {
            seconds: 123,
            nanos: 456,
        }],
        data: Some(proto::channel_data::Data::Values(proto::Values {
            values: Some(proto::values::Values::Doubles(proto::Doubles {
                values: vec![42.125],
            })),
        })),
    }
}

fn accepted(id: u64) -> Response {
    Response::Accepted(proto::Accepted {
        message_ids: vec![id],
    })
}

#[tokio::test]
async fn authenticated_stream_sends_exact_declaration_and_chunk_without_unary_buffering() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    assert_eq!(h.auth.lock().unwrap().as_deref(), Some("Bearer test-token"));
    h.reply(accepted(2)).await;
    stream.write_chunk(vec![data()]).await.unwrap();
    assert_eq!(
        h.next_request().await,
        WriteStreamRequest {
            request: Some(Request::Chunk(WriteChunk {
                message_id: 2,
                channels: vec![data()]
            }))
        }
    );
}

#[tokio::test]
async fn additional_declarations_use_distinct_message_ids() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    let extra = Schema {
        schema_id: 8,
        ..schema()
    };
    h.reply(Response::SchemasDeclared(proto::SchemasDeclared {
        message_id: 2,
    }))
    .await;
    stream.declare_schemas(vec![extra.clone()]).await.unwrap();
    assert_eq!(h.next_request().await, declaration(2, vec![extra]));
    h.reply(accepted(3)).await;
    stream.write_chunk(vec![data()]).await.unwrap();
    assert_eq!(
        h.next_request().await.request,
        Some(Request::Chunk(WriteChunk {
            message_id: 3,
            channels: vec![data()]
        }))
    );
}

#[tokio::test]
async fn heartbeats_do_not_acknowledge_an_operation() {
    let mut h = Harness::start().await;
    h.reply(Response::Accepted(proto::Accepted {
        message_ids: vec![],
    }))
    .await;
    let mut stream = h.open().await;
    h.reply(Response::Accepted(proto::Accepted {
        message_ids: vec![],
    }))
    .await;
    h.reply(accepted(2)).await;
    stream.write_chunk(vec![data()]).await.unwrap();
    h.next_request().await;
}

#[tokio::test]
async fn rejection_preserves_rich_status_and_stream_remains_usable() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    let status = wire::google::rpc::Status {
        code: Code::ResourceExhausted as i32,
        message: "quota".into(),
        details: vec![prost_types::Any {
            type_url: "type.googleapis.com/google.rpc.RetryInfo".into(),
            value: vec![10, 2, 8, 1],
        }],
    };
    h.reply(Response::Rejected(proto::Rejected {
        message_id: 2,
        status: Some(status.clone()),
    }))
    .await;
    let Err(WriteStreamError::Rejected(error)) = stream.write_chunk(vec![data()]).await else {
        panic!("expected per-message rejection")
    };
    assert_eq!(error.code(), Code::ResourceExhausted);
    assert_eq!(error.message(), "quota");
    assert_eq!(error.details(), status.encode_to_vec());
    h.next_request().await;
    h.reply(accepted(3)).await;
    stream.write_chunk(vec![data()]).await.unwrap();
    assert_eq!(
        h.next_request().await.request,
        Some(Request::Chunk(WriteChunk {
            message_id: 3,
            channels: vec![data()]
        }))
    );
}

#[tokio::test]
async fn initial_declaration_rejection_is_returned_instead_of_opening_session() {
    let mut h = Harness::start().await;
    h.reply(Response::Rejected(proto::Rejected {
        message_id: 1,
        status: Some(wire::google::rpc::Status {
            code: 3,
            message: "schema invalid".into(),
            details: vec![],
        }),
    }))
    .await;
    assert!(
        matches!(h.client.write_stream(vec![schema()]).await, Err(WriteStreamError::Rejected(status)) if status.code()==Code::InvalidArgument)
    );
    assert_eq!(h.next_request().await, declaration(1, vec![schema()]));
}

#[tokio::test]
async fn terminal_rpc_status_is_not_retried_and_closes_session() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    h.replies
        .as_ref()
        .unwrap()
        .send(Err(Status::unavailable("rotated")))
        .await
        .unwrap();
    assert!(
        matches!(stream.write_chunk(vec![data()]).await, Err(WriteStreamError::Rpc(status)) if status.code()==Code::Unavailable && status.message()=="rotated")
    );
    assert!(matches!(
        stream.write_chunk(vec![data()]).await,
        Err(WriteStreamError::Closed)
    ));
}

#[tokio::test]
async fn malformed_responses_close_session() {
    let cases = [
        None,
        Some(accepted(999)),
        Some(Response::Accepted(proto::Accepted {
            message_ids: vec![2, 3],
        })),
        Some(Response::SchemasDeclared(proto::SchemasDeclared {
            message_id: 2,
        })),
        Some(Response::Rejected(proto::Rejected {
            message_id: 999,
            status: None,
        })),
        Some(Response::Rejected(proto::Rejected {
            message_id: 2,
            status: None,
        })),
        Some(Response::Rejected(proto::Rejected {
            message_id: 2,
            status: Some(wire::google::rpc::Status {
                code: 0,
                ..Default::default()
            }),
        })),
        Some(Response::Rejected(proto::Rejected {
            message_id: 2,
            status: Some(wire::google::rpc::Status {
                code: 17,
                ..Default::default()
            }),
        })),
    ];
    for response in cases {
        let mut h = Harness::start().await;
        let mut stream = h.open().await;
        h.replies
            .as_ref()
            .unwrap()
            .send(Ok(proto::WriteStreamResponse { response }))
            .await
            .unwrap();
        assert!(matches!(
            stream.write_chunk(vec![data()]).await,
            Err(WriteStreamError::Protocol(_))
        ));
        assert!(matches!(
            stream.write_chunk(vec![data()]).await,
            Err(WriteStreamError::Closed)
        ));
    }
}

#[tokio::test]
async fn cancellation_closes_session_instead_of_misassigning_a_late_ack() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    let mut operation = Box::pin(stream.write_chunk(vec![data()]));
    tokio::select! {
        _ = h.next_request() => {},
        result = &mut operation => panic!("operation completed without ack: {result:?}"),
    }
    drop(operation);
    assert!(matches!(
        stream.write_chunk(vec![data()]).await,
        Err(WriteStreamError::Closed)
    ));
}

#[tokio::test]
async fn missing_ack_times_out_and_closes_session() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    stream.options.response_timeout = Duration::from_millis(10);
    assert!(matches!(
        stream.write_chunk(vec![data()]).await,
        Err(WriteStreamError::Timeout)
    ));
    assert!(matches!(
        stream.write_chunk(vec![data()]).await,
        Err(WriteStreamError::Closed)
    ));
}

#[tokio::test]
async fn clean_eof_before_ack_is_an_error() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    drop(h.replies.take());
    assert!(matches!(
        stream.write_chunk(vec![data()]).await,
        Err(WriteStreamError::Protocol(_)) | Err(WriteStreamError::Closed)
    ));
    assert!(matches!(
        stream.write_chunk(vec![data()]).await,
        Err(WriteStreamError::Closed)
    ));
}

#[tokio::test]
async fn finish_half_closes_then_waits_for_server_completion() {
    let mut h = Harness::start().await;
    let stream = h.open().await;
    let finish = tokio::spawn(stream.finish());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), h.events.recv())
            .await
            .unwrap()
            .unwrap(),
        None
    );
    assert!(!finish.is_finished());
    h.reply(Response::Accepted(proto::Accepted {
        message_ids: vec![],
    }))
    .await;
    drop(h.replies.take());
    finish.await.unwrap().unwrap();
}

#[tokio::test]
async fn finish_rejects_unexpected_nonheartbeat_response() {
    let mut h = Harness::start().await;
    let stream = h.open().await;
    h.reply(accepted(999)).await;
    assert!(matches!(
        stream.finish().await,
        Err(WriteStreamError::Protocol(_))
    ));
}

#[tokio::test]
async fn finish_times_out_if_server_does_not_complete() {
    let mut h = Harness::start().await;
    let mut stream = h.open().await;
    stream.options.response_timeout = Duration::from_millis(10);
    assert!(matches!(
        stream.finish().await,
        Err(WriteStreamError::Timeout)
    ));
}

#[tokio::test]
async fn invalid_timeout_fails_before_opening_transport() {
    let h = Harness::start().await;
    assert!(matches!(
        h.client
            .write_stream_with_options(
                vec![schema()],
                WriteStreamOptions {
                    response_timeout: Duration::ZERO
                }
            )
            .await,
        Err(WriteStreamError::InvalidTimeout)
    ));
}

#[test]
fn message_size_limit_checks_the_encoded_envelope() {
    let request = declaration(
        1,
        vec![Schema {
            channel: "x".repeat(MAX_MESSAGE_BYTES),
            ..schema()
        }],
    );
    assert!(matches!(
        check_size(&request),
        Err(WriteStreamError::MessageTooLarge)
    ));
    assert!(check_size(&declaration(1, vec![schema()])).is_ok());
}
