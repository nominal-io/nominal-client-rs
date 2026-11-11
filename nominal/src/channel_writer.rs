//! Bounded, pipelined native gRPC writes with automatic reconnect and replay.
//!
//! Concurrent `write_chunk` futures share a configured message/byte window.
//! A background task reads acknowledgments concurrently with sending. It retains each
//! request until its terminal response; transient failures and acknowledgment timeouts
//! reconnect, redeclare installed schemas, and replay only unanswered requests.
//!
//! Delivery is **at least once**: replay may duplicate data. Cancelling a write future
//! does not cancel a write already admitted. Dropping the stream cancels its background task;
//! use [`WriteStream::finish`] to drain pending writes and half-close gracefully.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use prost::Message;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

use crate::core::grpc::GrpcConnection;

mod actor;
mod connection;
mod pool;
pub use pool::{WriteStreamPool, WriteStreamPoolOptions};
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
/// All v2 wire types, including numeric, string, array, struct and log values.
pub use wire::nominal::channel_writer::v2 as proto;

#[cfg(test)]
mod tests;

const MAX_MESSAGE_BYTES: usize = 50 * 1024 * 1024;
type Result<T> = std::result::Result<T, WriteStreamError>;

/// Limits apply to retained, unacknowledged protobuf messages, not transport buffers.
#[derive(Clone, Copy, Debug)]
pub struct WriteStreamOptions {
    /// Maximum number of queued or sent messages awaiting a terminal response.
    pub max_in_flight_messages: usize,
    /// Maximum total encoded size of retained messages. Transport encoding adds overhead.
    pub max_in_flight_bytes: usize,
    /// Time from handing a message to the transport until acknowledgment; also bounds opening/draining.
    pub response_timeout: Duration,
    /// Maximum replays of any individual message (in addition to its first attempt).
    pub max_retries: u32,
    /// Full-jitter exponential reconnect backoff, capped at `max_backoff`.
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for WriteStreamOptions {
    fn default() -> Self {
        Self {
            max_in_flight_messages: 16,
            max_in_flight_bytes: 64 * 1024 * 1024,
            response_timeout: Duration::from_secs(30),
            max_retries: 3,
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum WriteStreamError {
    #[error("stream terminated: {0}")]
    Rpc(#[source] Box<tonic::Status>),
    /// Terminal response for one message; other pending writes continue.
    #[error("message rejected (partial effects are possible): {0}")]
    Rejected(#[source] Box<tonic::Status>),
    #[error("stream operation timed out")]
    Timeout,
    #[error("retry budget exhausted; unanswered writes may have partial effects")]
    RetryExhausted,
    #[error("stream is closed")]
    Closed,
    #[error("invalid stream response: {0}")]
    Protocol(&'static str),
    #[error("stream message exceeds the 50 MiB limit")]
    MessageTooLarge,
    #[error("message exceeds max_in_flight_bytes")]
    MessageBudgetExceeded,
    #[error("invalid streaming options: {0}")]
    InvalidOptions(&'static str),
    #[error("invalid schema registry: {0}")]
    InvalidSchemas(&'static str),
}

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

    /// Install initial schemas before returning a pipelined stream.
    pub async fn write_stream(&self, schemas: Vec<proto::Schema>) -> Result<WriteStream> {
        self.write_stream_with_options(schemas, WriteStreamOptions::default())
            .await
    }

    pub async fn write_stream_with_options(
        &self,
        schemas: Vec<proto::Schema>,
        options: WriteStreamOptions,
    ) -> Result<WriteStream> {
        self.open_stream(schemas, options, Arc::new(AtomicU64::new(1)), None)
            .await
    }

    async fn open_stream(
        &self,
        schemas: Vec<proto::Schema>,
        options: WriteStreamOptions,
        ids: Arc<AtomicU64>,
        pool: Option<Arc<pool::Budget>>,
    ) -> Result<WriteStream> {
        options.validate()?;
        let connection =
            connection::open_with_retry(&self.connection, &schemas, &ids, options).await?;
        let (commands, receiver) = mpsc::channel(options.max_in_flight_messages);
        let shared = Arc::new(Shared {
            pool,
            commands,
            ids,
            messages: Arc::new(Semaphore::new(options.max_in_flight_messages)),
            bytes: Arc::new(Semaphore::new(options.max_in_flight_bytes)),
            closed: AtomicBool::new(false),
            options,
        });
        let actor = actor::Writer::new(
            self.connection.clone(),
            connection,
            schemas,
            shared.clone(),
            receiver,
        );
        Ok(WriteStream {
            shared,
            task: tokio::spawn(actor.run()),
        })
    }
}

impl WriteStreamOptions {
    fn validate(self) -> Result<()> {
        if self.max_in_flight_messages == 0 || self.max_in_flight_messages > Semaphore::MAX_PERMITS
        {
            return Err(WriteStreamError::InvalidOptions(
                "max_in_flight_messages must be positive and fit a semaphore",
            ));
        }
        if self.max_in_flight_bytes == 0
            || self.max_in_flight_bytes > u32::MAX as usize
            || self.max_in_flight_bytes > Semaphore::MAX_PERMITS
        {
            return Err(WriteStreamError::InvalidOptions(
                "max_in_flight_bytes must fit a positive u32 and semaphore",
            ));
        }
        if self.response_timeout.is_zero() || self.initial_backoff > self.max_backoff {
            return Err(WriteStreamError::InvalidOptions(
                "positive response_timeout and initial_backoff <= max_backoff required",
            ));
        }
        Ok(())
    }
}

struct Shared {
    pool: Option<Arc<pool::Budget>>,
    commands: mpsc::Sender<Command>,
    ids: Arc<AtomicU64>,
    messages: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
    closed: AtomicBool,
    options: WriteStreamOptions,
}

impl Shared {
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.messages.close();
        self.bytes.close();
    }
}

/// Share by reference (or `Arc`) to submit concurrently. The actor owns the wire stream.
pub struct WriteStream {
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

// Internal completion handle. Dropping it does not discard the retained write.
struct WriteReceipt {
    #[cfg(test)]
    message_id: u64,
    result: oneshot::Receiver<Result<()>>,
}

impl WriteReceipt {
    #[cfg(test)]
    fn message_id(&self) -> u64 {
        self.message_id
    }
    async fn acknowledged(self) -> Result<()> {
        self.result.await.unwrap_or(Err(WriteStreamError::Closed))
    }
}

impl WriteStream {
    // Admission and completion are separate internally so the actor can pipeline writes.
    async fn enqueue_chunk(&self, channels: Vec<proto::ChannelData>) -> Result<WriteReceipt> {
        let id = next_id(&self.shared.ids)?;
        self.submit(proto::WriteStreamRequest {
            request: Some(proto::write_stream_request::Request::Chunk(
                proto::WriteChunk {
                    message_id: id,
                    channels,
                },
            )),
        })
        .await
    }

    /// Wait for window capacity, then send and await this message's terminal response.
    /// Poll multiple write futures concurrently to pipeline chunks on one stream.
    /// Cancelling after admission does not cancel the retained write or its retries.
    pub async fn write_chunk(&self, channels: Vec<proto::ChannelData>) -> Result<()> {
        self.enqueue_chunk(channels).await?.acknowledged().await
    }

    /// Wait for acknowledgment before using new schema IDs in chunks.
    pub async fn declare_schemas(&self, schemas: Vec<proto::Schema>) -> Result<()> {
        let id = next_id(&self.shared.ids)?;
        self.submit(connection::declaration(id, schemas))
            .await?
            .acknowledged()
            .await
    }

    async fn submit(&self, request: proto::WriteStreamRequest) -> Result<WriteReceipt> {
        self.submit_pooled(request, None).await
    }

    async fn submit_pooled(
        &self,
        request: proto::WriteStreamRequest,
        pool: Option<pool::Permits>,
    ) -> Result<WriteReceipt> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(WriteStreamError::Closed);
        }
        let size = request.encoded_len();
        if size > MAX_MESSAGE_BYTES {
            return Err(WriteStreamError::MessageTooLarge);
        }
        if size > self.shared.options.max_in_flight_bytes {
            return Err(WriteStreamError::MessageBudgetExceeded);
        }
        let messages = self
            .shared
            .messages
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| WriteStreamError::Closed)?;
        let bytes = self
            .shared
            .bytes
            .clone()
            .acquire_many_owned(size as u32)
            .await
            .map_err(|_| WriteStreamError::Closed)?;
        self.send(request, Admission { messages, bytes }, pool)
            .await
    }

    async fn send(
        &self,
        request: proto::WriteStreamRequest,
        admission: Admission,
        pool: Option<pool::Permits>,
    ) -> Result<WriteReceipt> {
        #[cfg(test)]
        let id = request_id(&request);
        if let Some(budget) = &self.shared.pool {
            budget.check()?;
        }
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(WriteStreamError::Closed);
        }
        let (reply, result) = oneshot::channel();
        self.shared
            .commands
            .send(Command::Submit(Pending {
                request,
                reply,
                _messages: admission.messages,
                _bytes: admission.bytes,
                _pool: pool,
                sent_at: None,
                replays: 0,
            }))
            .await
            .map_err(|_| WriteStreamError::Closed)?;
        Ok(WriteReceipt {
            #[cfg(test)]
            message_id: id,
            result,
        })
    }

    /// Stop admission, await all terminal responses (including retries), then half-close.
    /// Also reports the first per-message rejection, even if its write future was cancelled.
    pub async fn finish(self) -> Result<()> {
        self.shared.close();
        let (reply, result) = oneshot::channel();
        self.shared
            .commands
            .send(Command::Finish(reply))
            .await
            .map_err(|_| WriteStreamError::Closed)?;
        result.await.unwrap_or(Err(WriteStreamError::Closed))
    }
}

impl Drop for WriteStream {
    fn drop(&mut self) {
        self.shared.close();
        self.task.abort();
    }
}

enum Command {
    Submit(Pending),
    Finish(oneshot::Sender<Result<()>>),
}

struct Admission {
    messages: OwnedSemaphorePermit,
    bytes: OwnedSemaphorePermit,
}

struct Pending {
    request: proto::WriteStreamRequest,
    reply: oneshot::Sender<Result<()>>,
    _messages: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
    // Declared after local permits so pool waiters wake after worker capacity is released.
    _pool: Option<pool::Permits>,
    sent_at: Option<tokio::time::Instant>,
    replays: u32,
}

fn next_id(ids: &AtomicU64) -> Result<u64> {
    ids.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| WriteStreamError::Protocol("message IDs exhausted"))
}

fn request_id(request: &proto::WriteStreamRequest) -> u64 {
    match request
        .request
        .as_ref()
        .expect("SDK requests always have a variant")
    {
        proto::write_stream_request::Request::DeclareSchemas(declaration) => declaration.message_id,
        proto::write_stream_request::Request::Chunk(chunk) => chunk.message_id,
    }
}
