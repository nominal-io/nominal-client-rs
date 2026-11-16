//! Fixed stream membership, shared admission limits, and connection-local schema barriers.
use std::sync::{Mutex, atomic::AtomicU64};

use tokio::sync::{Notify, RwLock};

use super::*;

/// Pool-wide limits do not multiply when `stream_count` increases.
#[derive(Clone, Copy, Debug)]
pub struct WriteStreamPoolOptions {
    /// Number of streams to open eagerly. There is no adaptive scaling.
    pub stream_count: usize,
    /// Maximum retained messages across every stream, including queued writes.
    pub max_pending_messages: usize,
    /// Maximum retained encoded payload bytes across every stream.
    pub max_pending_bytes: usize,
    /// Per-stream windows, deadlines and reconnect/replay policy.
    pub stream_options: WriteStreamOptions,
}

impl Default for WriteStreamPoolOptions {
    fn default() -> Self {
        Self {
            stream_count: 2,
            max_pending_messages: 32,
            max_pending_bytes: 64 * 1024 * 1024,
            stream_options: WriteStreamOptions::default(),
        }
    }
}

impl ChannelWriterClient {
    /// Open every stream and acknowledge initial schemas before returning the pool.
    /// Failure/cancellation while opening drops all streams already created.
    pub async fn write_stream_pool(
        &self,
        schemas: Vec<proto::Schema>,
        options: WriteStreamPoolOptions,
    ) -> Result<WriteStreamPool> {
        options.validate()?;
        connection::validate_schemas(&schemas)?;
        let budget = Arc::new(Budget::new(options));
        let ids = Arc::new(AtomicU64::new(1));
        let mut streams = Vec::new();
        for _ in 0..options.stream_count {
            streams.push(
                self.open_stream(
                    schemas.clone(),
                    options.stream_options,
                    ids.clone(),
                    Some(budget.clone()),
                )
                .await?,
            );
        }
        budget.check()?;
        Ok(WriteStreamPool {
            streams,
            budget,
            next: Mutex::new(0),
            schemas: RwLock::new(()),
        })
    }
}

impl WriteStreamPoolOptions {
    fn validate(self) -> Result<()> {
        if self.stream_count == 0 {
            return Err(WriteStreamError::InvalidOptions(
                "stream_count must be positive",
            ));
        }
        self.stream_options.validate()?;
        WriteStreamOptions {
            max_in_flight_messages: self.max_pending_messages,
            max_in_flight_bytes: self.max_pending_bytes,
            ..self.stream_options
        }
        .validate()
    }
}

/// A fixed pool with the same public chunk-writing API as an individual stream.
///
/// Concurrent write futures choose a least-loaded stream with available count/byte
/// capacity; ties rotate. Each write belongs to one worker for its entire retry life.
/// There is no cross-stream ordering or automatic expansion on server overload.
/// Dropping the pool cancels its workers; `finish` drains every worker.
pub struct WriteStreamPool {
    streams: Vec<WriteStream>,
    budget: Arc<Budget>,
    next: Mutex<usize>,
    schemas: RwLock<()>,
}

impl WriteStreamPool {
    /// Wait for shared and per-stream capacity, then await this write's terminal response.
    /// Cancelling after admission does not discard the retained write or its global permits.
    pub async fn write_chunk(&self, channels: Vec<proto::ChannelData>) -> Result<()> {
        let schema_guard = self.schemas.read().await;
        self.budget.check()?;
        let id = next_id(&self.streams[0].shared.ids)?;
        let request = proto::WriteStreamRequest {
            request: Some(proto::write_stream_request::Request::Chunk(
                proto::WriteChunk {
                    message_id: id,
                    channels,
                },
            )),
        };
        let size = self.budget.check_size(&request)?;
        let global = self.budget.acquire(size).await?;
        let (worker, admission) = self.reserve_worker(size).await?;
        let receipt = self.streams[worker]
            .send(request, admission, Some(global))
            .await?;
        drop(schema_guard);
        receipt.acknowledged().await
    }

    async fn reserve_worker(&self, size: usize) -> Result<(usize, Admission)> {
        loop {
            // Register before inspecting capacity so releases cannot be missed.
            let changed = self.budget.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.budget.check()?;
            if let Some(reservation) = self.try_reserve(size) {
                return Ok(reservation);
            }
            changed.await;
        }
    }

    fn try_reserve(&self, size: usize) -> Option<(usize, Admission)> {
        let mut next = self.next.lock().unwrap();
        let options = self.budget.options.stream_options;
        let mut candidates = (0..self.streams.len())
            .map(|offset| (*next + offset) % self.streams.len())
            .collect::<Vec<_>>();
        candidates.sort_by_key(|i| {
            options.max_in_flight_messages - self.streams[*i].shared.messages.available_permits()
        });
        for index in candidates {
            let worker = &self.streams[index].shared;
            if worker.closed.load(Ordering::Acquire) {
                continue;
            }
            let Ok(messages) = worker.messages.clone().try_acquire_owned() else {
                continue;
            };
            let Ok(bytes) = worker.bytes.clone().try_acquire_many_owned(size as u32) else {
                continue;
            };
            *next = (index + 1) % self.streams.len();
            return Some((index, Admission { messages, bytes }));
        }
        None
    }

    /// Install new schema IDs on every worker before admitting any more chunks.
    /// If any declaration fails or this operation is cancelled mid-broadcast, close
    /// admission: immutable schema declarations cannot be rolled back across streams.
    pub async fn declare_schemas(&self, schemas: Vec<proto::Schema>) -> Result<()> {
        let _guard = self.schemas.write().await;
        self.budget.check()?;
        let mut broadcast = SchemaBroadcast {
            budget: self.budget.clone(),
            complete: false,
        };
        let result = futures::future::try_join_all(self.streams.iter().map(|worker| async {
            let request = connection::declaration(next_id(&worker.shared.ids)?, schemas.clone());
            let global = self
                .budget
                .acquire(self.budget.check_size(&request)?)
                .await?;
            worker
                .submit_pooled(request, Some(global))
                .await?
                .acknowledged()
                .await
        }))
        .await;
        broadcast.complete = true;
        if let Err(error) = result {
            self.budget.fail(error.clone());
            return Err(error);
        }
        Ok(())
    }

    /// Stop admission and drain/half-close every worker, even if another worker fails.
    /// The first pool failure or per-message rejection is returned after all drains.
    pub async fn finish(self) -> Result<()> {
        self.budget.close();
        let results =
            futures::future::join_all(self.streams.into_iter().map(WriteStream::finish)).await;
        if let Some(error) = self.budget.error() {
            return Err(error);
        }
        results.into_iter().collect::<Result<Vec<_>>>().map(|_| ())
    }
}

pub(super) struct Budget {
    options: WriteStreamPoolOptions,
    messages: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
    closed: AtomicBool,
    failure: Mutex<Option<WriteStreamError>>,
    changed: Notify,
}

impl Budget {
    fn new(options: WriteStreamPoolOptions) -> Self {
        Self {
            options,
            messages: Arc::new(Semaphore::new(options.max_pending_messages)),
            bytes: Arc::new(Semaphore::new(options.max_pending_bytes)),
            closed: AtomicBool::new(false),
            failure: Mutex::new(None),
            changed: Notify::new(),
        }
    }
    fn error(&self) -> Option<WriteStreamError> {
        self.failure.lock().unwrap().clone()
    }
    pub(super) fn check(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            Err(self.error().unwrap_or(WriteStreamError::Closed))
        } else {
            Ok(())
        }
    }
    fn check_size(&self, request: &proto::WriteStreamRequest) -> Result<usize> {
        let size = request.encoded_len();
        if size > MAX_MESSAGE_BYTES {
            return Err(WriteStreamError::MessageTooLarge);
        }
        if size > self.options.max_pending_bytes
            || size > self.options.stream_options.max_in_flight_bytes
        {
            return Err(WriteStreamError::MessageBudgetExceeded);
        }
        Ok(size)
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.messages.close();
        self.bytes.close();
        self.changed.notify_waiters();
    }
    pub(super) fn fail(&self, error: WriteStreamError) {
        self.failure.lock().unwrap().get_or_insert(error);
        self.close();
    }
    async fn acquire(self: &Arc<Self>, size: usize) -> Result<Permits> {
        let messages = self
            .messages
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| self.error().unwrap_or(WriteStreamError::Closed))?;
        let bytes = self
            .bytes
            .clone()
            .acquire_many_owned(size as u32)
            .await
            .map_err(|_| self.error().unwrap_or(WriteStreamError::Closed))?;
        self.check()?;
        Ok(Permits {
            messages: Some(messages),
            bytes: Some(bytes),
            budget: self.clone(),
        })
    }
}

pub(super) struct Permits {
    messages: Option<OwnedSemaphorePermit>,
    bytes: Option<OwnedSemaphorePermit>,
    budget: Arc<Budget>,
}
impl Drop for Permits {
    fn drop(&mut self) {
        drop(self.messages.take());
        drop(self.bytes.take());
        self.budget.changed.notify_waiters();
    }
}

struct SchemaBroadcast {
    budget: Arc<Budget>,
    complete: bool,
}
impl Drop for SchemaBroadcast {
    fn drop(&mut self) {
        if !self.complete {
            self.budget.fail(WriteStreamError::Protocol(
                "pool schema declaration cancelled before all streams acknowledged",
            ));
        }
    }
}
