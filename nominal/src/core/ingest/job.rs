use nominal_api::ingest::api::{
    IngestJob as ApiIngestJob, IngestJobStatus as ApiIngestJobStatus, IngestType as ApiIngestType,
};

/// The lifecycle status of an ingest job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestJobStatus {
    Submitted,
    Queued,
    InProgress,
    Completed,
    Failed,
    Cancelled,
    /// A status returned by the server that this client does not recognize.
    /// Treated as a terminal failure by the client's wait loop.
    Unknown(String),
}

impl IngestJobStatus {
    /// Whether the job has reached a terminal state (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown(_)
        )
    }
}

impl From<&ApiIngestJobStatus> for IngestJobStatus {
    fn from(s: &ApiIngestJobStatus) -> Self {
        match s {
            ApiIngestJobStatus::Submitted => Self::Submitted,
            ApiIngestJobStatus::Queued => Self::Queued,
            ApiIngestJobStatus::InProgress => Self::InProgress,
            ApiIngestJobStatus::Completed => Self::Completed,
            ApiIngestJobStatus::Failed => Self::Failed,
            ApiIngestJobStatus::Cancelled => Self::Cancelled,
            ApiIngestJobStatus::Unknown(u) => Self::Unknown(u.to_string()),
        }
    }
}

/// The kind of data produced by an ingest job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestType {
    Tabular,
    Mcap,
    Dataflash,
    JournalJson,
    Containerized,
    Video,
    AvroStream,
    /// An ingest type returned by the server that this client does not recognize.
    Unknown(String),
}

impl From<&ApiIngestType> for IngestType {
    fn from(t: &ApiIngestType) -> Self {
        match t {
            ApiIngestType::Tabular => Self::Tabular,
            ApiIngestType::Mcap => Self::Mcap,
            ApiIngestType::Dataflash => Self::Dataflash,
            ApiIngestType::JournalJson => Self::JournalJson,
            ApiIngestType::Containerized => Self::Containerized,
            ApiIngestType::Video => Self::Video,
            ApiIngestType::AvroStream => Self::AvroStream,
            ApiIngestType::Unknown(u) => Self::Unknown(u.to_string()),
        }
    }
}

/// A snapshot of an ingest job's server-side state.
#[derive(Debug, Clone)]
pub struct IngestJob {
    rid: String,
    status: IngestJobStatus,
    origin_files: Vec<String>,
    ingest_type: IngestType,
}

impl IngestJob {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn status(&self) -> &IngestJobStatus {
        &self.status
    }

    /// Source files this job is ingesting, if reported by the server.
    pub fn origin_files(&self) -> &[String] {
        &self.origin_files
    }

    pub fn ingest_type(&self) -> &IngestType {
        &self.ingest_type
    }

    pub(crate) fn from_conjure(job: ApiIngestJob) -> Self {
        let rid = job.ingest_job_rid().to_string();
        let status = IngestJobStatus::from(job.status());
        let ingest_type = IngestType::from(job.ingest_type());
        let origin_files = job
            .origin_files()
            .map(|files| files.to_vec())
            .unwrap_or_default();
        Self {
            rid,
            status,
            origin_files,
            ingest_type,
        }
    }
}
