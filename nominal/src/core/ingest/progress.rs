use std::sync::Arc;

/// Events emitted during a multipart upload.
#[derive(Debug, Clone)]
pub(crate) enum UploadEvent {
    /// Emitted once, after the upload has been initiated and total_parts is known.
    Started { total_bytes: u64, total_parts: u32 },
    /// Emitted each time a part is successfully uploaded to object storage.
    PartCompleted { part_number: u32, bytes: u64 },
    /// Emitted once, after the multipart upload has been finalized.
    Completed { s3_path: String },
}

/// A callback invoked for each [`UploadEvent`]. Callbacks may be invoked from
/// any task and must be cheap — emit to a channel or bounded buffer and do
/// heavy work elsewhere.
pub(crate) type ProgressCallback = Arc<dyn Fn(UploadEvent) + Send + Sync + 'static>;
