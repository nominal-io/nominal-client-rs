use super::*;
use prost::Message as _;
use proto::nominal_channel_writer_service_server::{
    NominalChannelWriterService, NominalChannelWriterServiceServer,
};
use proto::write_stream_request::Request as Message;
use proto::write_stream_response::Response as Reply;
use std::pin::pin;
use tokio::net::TcpListener;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Code, Request, Response, Status, Streaming};

mod pool;
mod recovery;

type WireReply = std::result::Result<proto::WriteStreamResponse, Status>;
struct FakeService {
    sessions: mpsc::UnboundedSender<Session>,
}

#[tonic::async_trait]
impl NominalChannelWriterService for FakeService {
    async fn write(
        &self,
        _: Request<proto::WriteRequest>,
    ) -> std::result::Result<Response<proto::WriteResponse>, Status> {
        Err(Status::unimplemented("unary v2 writes are unsupported"))
    }
    type WriteStreamStream = ReceiverStream<WireReply>;
    async fn write_stream(
        &self,
        request: Request<Streaming<proto::WriteStreamRequest>>,
    ) -> std::result::Result<Response<Self::WriteStreamStream>, Status> {
        let auth = request
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let mut incoming = request.into_inner();
        // Intentionally withhold response headers until the first declaration arrives.
        let first = incoming
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("missing declaration"))?;
        let (requests, receiver) = mpsc::unbounded_channel();
        let (replies, response) = mpsc::channel(32);
        self.sessions
            .send(Session {
                first,
                auth,
                requests: receiver,
                replies: Some(replies),
            })
            .map_err(|_| Status::cancelled("test ended"))?;
        tokio::spawn(async move {
            while let Ok(Some(request)) = incoming.message().await {
                if requests.send(Some(request)).is_err() {
                    return;
                }
            }
            let _ = requests.send(None);
        });
        Ok(Response::new(ReceiverStream::new(response)))
    }
}

struct Session {
    first: proto::WriteStreamRequest,
    auth: String,
    requests: mpsc::UnboundedReceiver<Option<proto::WriteStreamRequest>>,
    replies: Option<mpsc::Sender<WireReply>>,
}
impl Session {
    async fn reply(&self, reply: Reply) {
        self.replies
            .as_ref()
            .unwrap()
            .send(Ok(proto::WriteStreamResponse {
                response: Some(reply),
            }))
            .await
            .unwrap();
    }
    async fn ack(&self, ids: Vec<u64>) {
        self.reply(Reply::Accepted(proto::Accepted { message_ids: ids }))
            .await;
    }
    async fn declared(&self, request: &proto::WriteStreamRequest) {
        self.reply(Reply::SchemasDeclared(proto::SchemasDeclared {
            message_id: request_id(request),
        }))
        .await;
    }
    async fn next(&mut self) -> proto::WriteStreamRequest {
        tokio::time::timeout(Duration::from_secs(5), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }
    async fn fail(&self, status: Status) {
        self.replies
            .as_ref()
            .unwrap()
            .send(Err(status))
            .await
            .unwrap();
    }
}

struct Harness {
    client: ChannelWriterClient,
    sessions: mpsc::UnboundedReceiver<Session>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        let (sender, sessions) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(NominalChannelWriterServiceServer::new(FakeService {
                    sessions: sender,
                }))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let client = crate::NominalClient::builder("test-token")
            .base_url(url)
            .build()
            .unwrap()
            .channel_writer();
        Self {
            client,
            sessions,
            server,
        }
    }
    async fn next_session(&mut self) -> Session {
        tokio::time::timeout(Duration::from_secs(5), self.sessions.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn open(&mut self, options: WriteStreamOptions) -> (WriteStream, Session) {
        self.open_schemas(vec![schema(1)], options).await
    }
    async fn open_schemas(
        &mut self,
        schemas: Vec<proto::Schema>,
        options: WriteStreamOptions,
    ) -> (WriteStream, Session) {
        let client = self.client.clone();
        let initial = schemas.clone();
        let opening =
            tokio::spawn(async move { client.write_stream_with_options(schemas, options).await });
        let session = self.next_session().await;
        assert_eq!(session.first, connection::declaration(1, initial));
        assert_eq!(session.auth, "Bearer test-token");
        session.declared(&session.first).await;
        (opening.await.unwrap().unwrap(), session)
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}
fn options() -> WriteStreamOptions {
    WriteStreamOptions {
        initial_backoff: Duration::ZERO,
        max_backoff: Duration::ZERO,
        response_timeout: Duration::from_secs(5),
        ..Default::default()
    }
}
fn schema(id: u32) -> proto::Schema {
    proto::Schema {
        schema_id: id,
        dataset_rid: "ri.catalog.main.dataset.00000000-0000-0000-0000-000000000001".into(),
        channel: format!("temperature_{id}"),
        r#type: proto::ChannelType::Double as i32,
        ..Default::default()
    }
}
fn data() -> Vec<proto::ChannelData> {
    vec![proto::ChannelData {
        schema_id: 1,
        timestamps: vec![Timestamp {
            seconds: 123,
            nanos: 456,
        }],
        data: Some(proto::channel_data::Data::Values(proto::Values {
            values: Some(proto::values::Values::Doubles(proto::Doubles {
                values: vec![42.125],
            })),
        })),
    }]
}

#[tokio::test]
async fn sends_full_window_before_any_ack_and_matches_batched_out_of_order_acks() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    let b = stream.enqueue_chunk(data()).await.unwrap();
    let c = stream.enqueue_chunk(data()).await.unwrap();
    for receipt in [&a, &b, &c] {
        assert_eq!(
            s.next().await.request,
            Some(Message::Chunk(proto::WriteChunk {
                message_id: receipt.message_id(),
                channels: data()
            }))
        );
    }
    s.ack(vec![c.message_id(), a.message_id()]).await;
    c.acknowledged().await.unwrap();
    a.acknowledged().await.unwrap();
    let id = b.message_id();
    let mut waiting = pin!(b.acknowledged());
    assert!(futures::poll!(&mut waiting).is_pending());
    s.ack(vec![id]).await;
    waiting.await.unwrap();
    assert_eq!(
        stream.shared.messages.available_permits(),
        options().max_in_flight_messages
    );
    assert_eq!(
        stream.shared.bytes.available_permits(),
        options().max_in_flight_bytes
    );
}

#[tokio::test]
async fn concurrent_write_futures_share_one_wire_stream() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let writes =
        async { futures::try_join!(stream.write_chunk(data()), stream.write_chunk(data())) };
    let server = async {
        let a = s.next().await;
        let b = s.next().await;
        s.ack(vec![request_id(&b), request_id(&a)]).await;
    };
    let (result, _) = tokio::join!(writes, server);
    result.unwrap();
    assert!(h.sessions.try_recv().is_err());
}

#[tokio::test]
async fn count_window_blocks_until_ack_releases_a_slot() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h
        .open(WriteStreamOptions {
            max_in_flight_messages: 2,
            ..options()
        })
        .await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    let b = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    s.next().await;
    let mut third = pin!(stream.enqueue_chunk(data()));
    assert!(futures::poll!(&mut third).is_pending());
    s.ack(vec![a.message_id()]).await;
    a.acknowledged().await.unwrap();
    let c = third.await.unwrap();
    assert_eq!(request_id(&s.next().await), c.message_id());
    s.ack(vec![b.message_id(), c.message_id()]).await;
    b.acknowledged().await.unwrap();
    c.acknowledged().await.unwrap();
}

#[tokio::test]
async fn byte_window_blocks_independently_of_count_window() {
    let mut h = Harness::start().await;
    let size = proto::WriteStreamRequest {
        request: Some(Message::Chunk(proto::WriteChunk {
            message_id: 2,
            channels: data(),
        })),
    }
    .encoded_len();
    let (stream, mut s) = h
        .open(WriteStreamOptions {
            max_in_flight_bytes: size,
            ..options()
        })
        .await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    let mut second = pin!(stream.enqueue_chunk(data()));
    assert!(futures::poll!(&mut second).is_pending());
    s.ack(vec![a.message_id()]).await;
    a.acknowledged().await.unwrap();
    let b = second.await.unwrap();
    s.next().await;
    s.ack(vec![b.message_id()]).await;
    b.acknowledged().await.unwrap();
    assert_eq!(stream.shared.bytes.available_permits(), size);
}

#[tokio::test]
async fn reconnect_restores_schemas_and_replays_only_unacknowledged_chunks() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h.open(options()).await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    let b = stream.enqueue_chunk(data()).await.unwrap();
    first.next().await;
    let unanswered = first.next().await;
    first.ack(vec![a.message_id()]).await;
    a.acknowledged().await.unwrap();
    first.fail(Status::unavailable("rotation")).await;
    let mut second = h.next_session().await;
    assert_eq!(second.first, connection::declaration(4, vec![schema(1)]));
    assert!(second.requests.try_recv().is_err());
    second.declared(&second.first).await;
    assert_eq!(second.next().await, unanswered);
    second.ack(vec![b.message_id()]).await;
    b.acknowledged().await.unwrap();
    assert!(second.requests.try_recv().is_err());
}

#[tokio::test]
async fn timeout_reconnects_and_replays_same_message_id() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    let request = first.next().await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    let mut second = h.next_session().await;
    second.declared(&second.first).await;
    assert_eq!(second.next().await, request);
    second.ack(vec![receipt.message_id()]).await;
    receipt.acknowledged().await.unwrap();
}

#[tokio::test]
async fn retry_exhaustion_fails_all_receipts_and_releases_window() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h
        .open(WriteStreamOptions {
            max_retries: 1,
            ..options()
        })
        .await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    let b = stream.enqueue_chunk(data()).await.unwrap();
    first.next().await;
    first.next().await;
    first.fail(Status::unavailable("first failure")).await;
    let mut second = h.next_session().await;
    second.declared(&second.first).await;
    second.next().await;
    second.next().await;
    second.fail(Status::unavailable("second failure")).await;
    assert!(matches!(
        a.acknowledged().await,
        Err(WriteStreamError::RetryExhausted)
    ));
    assert!(matches!(
        b.acknowledged().await,
        Err(WriteStreamError::RetryExhausted)
    ));
    assert_eq!(
        stream.shared.bytes.available_permits(),
        options().max_in_flight_bytes
    );
    assert!(matches!(
        stream.enqueue_chunk(data()).await,
        Err(WriteStreamError::Closed)
    ));
}

#[tokio::test]
async fn permanent_authentication_error_is_not_retried() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    s.fail(Status::permission_denied("revoked")).await;
    assert!(
        matches!(receipt.acknowledged().await, Err(WriteStreamError::Rpc(status)) if status.code()==Code::PermissionDenied)
    );
    assert!(matches!(
        stream.enqueue_chunk(data()).await,
        Err(WriteStreamError::Closed)
    ));
    assert!(h.sessions.try_recv().is_err());
}

#[tokio::test]
async fn dropped_receipt_does_not_discard_write_or_release_its_slot() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h
        .open(WriteStreamOptions {
            max_in_flight_messages: 1,
            ..options()
        })
        .await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    let id = receipt.message_id();
    drop(receipt);
    let request = first.next().await;
    assert_eq!(stream.shared.messages.available_permits(), 0);
    first.fail(Status::unavailable("reconnect")).await;
    let mut second = h.next_session().await;
    second.declared(&second.first).await;
    assert_eq!(second.next().await, request);
    second.ack(vec![id]).await;
    let next = stream.enqueue_chunk(data()).await.unwrap();
    second.next().await;
    second.ack(vec![next.message_id()]).await;
    next.acknowledged().await.unwrap();
}

#[tokio::test]
async fn rejected_message_preserves_details_and_does_not_block_other_acks() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let a = stream.enqueue_chunk(data()).await.unwrap();
    let b = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    s.next().await;
    let status = wire::google::rpc::Status {
        code: Code::ResourceExhausted as i32,
        message: "quota".into(),
        details: vec![prost_types::Any {
            type_url: "type.googleapis.com/google.rpc.RetryInfo".into(),
            value: vec![10, 2, 8, 1],
        }],
    };
    s.reply(Reply::Rejected(proto::Rejected {
        message_id: a.message_id(),
        status: Some(status.clone()),
    }))
    .await;
    let Err(WriteStreamError::Rejected(error)) = a.acknowledged().await else {
        panic!("expected rejection")
    };
    assert_eq!(error.code(), Code::ResourceExhausted);
    assert_eq!(error.details(), status.encode_to_vec());
    s.ack(vec![b.message_id()]).await;
    b.acknowledged().await.unwrap();
    assert_eq!(
        stream.shared.messages.available_permits(),
        options().max_in_flight_messages
    );
}

#[tokio::test]
async fn accepted_additional_schemas_are_restored_before_pending_writes() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h.open(options()).await;
    let declaring = stream.declare_schemas(vec![schema(2)]);
    let server = async {
        let declaration = first.next().await;
        assert_eq!(declaration, connection::declaration(2, vec![schema(2)]));
        first.declared(&declaration).await;
    };
    let (result, _) = tokio::join!(declaring, server);
    result.unwrap();
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    let request = first.next().await;
    first.fail(Status::unavailable("reconnect")).await;
    let mut second = h.next_session().await;
    assert_eq!(
        second.first,
        connection::declaration(4, vec![schema(1), schema(2)])
    );
    second.declared(&second.first).await;
    assert_eq!(second.next().await, request);
    second.ack(vec![receipt.message_id()]).await;
    receipt.acknowledged().await.unwrap();
}

#[tokio::test]
async fn unanswered_schema_declaration_is_replayed_not_prematurely_installed() {
    let mut h = Harness::start().await;
    let (stream, mut first) = h.open(options()).await;
    let declaring = stream.declare_schemas(vec![schema(2)]);
    let server = async {
        let declaration = first.next().await;
        first.fail(Status::unavailable("reconnect")).await;
        let mut second = h.next_session().await;
        assert_eq!(second.first, connection::declaration(3, vec![schema(1)]));
        second.declared(&second.first).await;
        assert_eq!(second.next().await, declaration);
        second.declared(&declaration).await;
    };
    let (result, _) = tokio::join!(declaring, server);
    result.unwrap();
}

#[tokio::test]
async fn malformed_responses_do_not_partially_complete_messages() {
    let cases = [
        Some(Reply::Accepted(proto::Accepted {
            message_ids: vec![2, 999],
        })),
        Some(Reply::Accepted(proto::Accepted {
            message_ids: vec![2, 2],
        })),
        None,
        Some(Reply::SchemasDeclared(proto::SchemasDeclared {
            message_id: 2,
        })),
        Some(Reply::Rejected(proto::Rejected {
            message_id: 999,
            status: None,
        })),
        Some(Reply::Rejected(proto::Rejected {
            message_id: 2,
            status: None,
        })),
        Some(Reply::Rejected(proto::Rejected {
            message_id: 2,
            status: Some(wire::google::rpc::Status {
                code: 0,
                ..Default::default()
            }),
        })),
        Some(Reply::Rejected(proto::Rejected {
            message_id: 2,
            status: Some(wire::google::rpc::Status {
                code: 17,
                ..Default::default()
            }),
        })),
    ];
    for response in cases {
        let mut h = Harness::start().await;
        let (stream, mut s) = h.open(options()).await;
        let receipt = stream.enqueue_chunk(data()).await.unwrap();
        s.next().await;
        s.replies
            .as_ref()
            .unwrap()
            .send(Ok(proto::WriteStreamResponse { response }))
            .await
            .unwrap();
        assert!(matches!(
            receipt.acknowledged().await,
            Err(WriteStreamError::Protocol(_))
        ));
        assert!(h.sessions.try_recv().is_err());
    }
}

#[tokio::test]
async fn heartbeat_does_not_release_a_slot_or_reset_ack_deadline() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    s.ack(vec![]).await;
    assert_eq!(
        stream.shared.messages.available_permits(),
        options().max_in_flight_messages - 1
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    let mut next = h.next_session().await;
    next.declared(&next.first).await;
    next.next().await;
    next.ack(vec![receipt.message_id()]).await;
    receipt.acknowledged().await.unwrap();
}

#[tokio::test]
async fn finish_drains_unawaited_receipts_then_half_closes() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    let finish = tokio::spawn(stream.finish());
    assert!(!finish.is_finished());
    s.ack(vec![receipt.message_id()]).await;
    receipt.acknowledged().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), s.requests.recv())
            .await
            .unwrap()
            .unwrap(),
        None
    );
    drop(s.replies.take());
    finish.await.unwrap().unwrap();
}

#[tokio::test]
async fn finish_reports_rejection_even_when_receipt_is_dropped() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    let id = receipt.message_id();
    drop(receipt);
    s.next().await;
    let finish = tokio::spawn(stream.finish());
    s.reply(Reply::Rejected(proto::Rejected {
        message_id: id,
        status: Some(wire::google::rpc::Status {
            code: 3,
            message: "invalid".into(),
            details: vec![],
        }),
    }))
    .await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), s.requests.recv())
            .await
            .unwrap()
            .unwrap(),
        None
    );
    drop(s.replies.take());
    assert!(matches!(
        finish.await.unwrap(),
        Err(WriteStreamError::Rejected(_))
    ));
}

#[tokio::test]
async fn invalid_limits_and_oversized_messages_fail_without_waiting_for_capacity() {
    let mut h = Harness::start().await;
    for invalid in [
        WriteStreamOptions {
            max_in_flight_messages: 0,
            ..options()
        },
        WriteStreamOptions {
            max_in_flight_bytes: 0,
            ..options()
        },
        WriteStreamOptions {
            response_timeout: Duration::ZERO,
            ..options()
        },
    ] {
        assert!(matches!(
            h.client
                .write_stream_with_options(vec![schema(1)], invalid)
                .await,
            Err(WriteStreamError::InvalidOptions(_))
        ));
    }
    let (stream, _) = h
        .open(WriteStreamOptions {
            max_in_flight_bytes: 1,
            ..options()
        })
        .await;
    assert!(matches!(
        stream.enqueue_chunk(data()).await,
        Err(WriteStreamError::MessageBudgetExceeded)
    ));
    assert_eq!(
        stream.shared.messages.available_permits(),
        options().max_in_flight_messages
    );
}

#[tokio::test]
async fn dropping_stream_cancels_pending_receipts() {
    let mut h = Harness::start().await;
    let (stream, mut s) = h.open(options()).await;
    let receipt = stream.enqueue_chunk(data()).await.unwrap();
    s.next().await;
    drop(stream);
    assert!(matches!(
        receipt.acknowledged().await,
        Err(WriteStreamError::Closed)
    ));
}
