use nominal_api::objects::ingest::api::{
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
    PointCloud,
    Multi,
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
            ApiIngestType::PointCloud => Self::PointCloud,
            ApiIngestType::Multi => Self::Multi,
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
    dataset_rid: Option<String>,
    produced_file_count: Option<i32>,
    created_by_rid: Option<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    start_time: Option<chrono::DateTime<chrono::Utc>>,
    end_time: Option<chrono::DateTime<chrono::Utc>>,
    app_base_url: String,
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

    pub fn dataset_rid(&self) -> Option<&str> {
        self.dataset_rid.as_deref()
    }
    pub fn produced_file_count(&self) -> Option<i32> {
        self.produced_file_count
    }
    pub fn created_by_rid(&self) -> Option<&str> {
        self.created_by_rid.as_deref()
    }
    pub fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.created_at
    }
    pub fn start_time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.start_time
    }
    pub fn end_time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.end_time
    }
    pub fn nominal_url(&self) -> String {
        format!(
            "{}/ingestion/{}",
            self.app_base_url.trim_end_matches('/'),
            self.rid
        )
    }
    pub(crate) fn with_app_base_url(mut self, url: &str) -> Self {
        self.app_base_url = url.into();
        self
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
            dataset_rid: job.dataset_rid().map(ToString::to_string),
            produced_file_count: job.produced_file_count(),
            created_by_rid: job.created_by_rid().map(ToString::to_string),
            created_at: job.created_at(),
            start_time: job.start_time(),
            end_time: job.end_time(),
            app_base_url: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_metadata_preserves_optional_fields() {
        let api = serde_json::from_value(serde_json::json!({
            "ingestJobRid":"ri.ingest.main.job.test", "status":"COMPLETED",
            "createdBy":"00000000-0000-0000-0000-000000000000",
            "orgUuid":"00000000-0000-0000-0000-000000000000", "ingestType":"MULTI",
            "datasetRid":"ri.catalog.main.dataset.test", "producedFileCount":2,
            "createdAt":"2026-01-01T00:00:00Z"
        }))
        .unwrap();
        let job = IngestJob::from_conjure(api);
        assert_eq!(job.dataset_rid(), Some("ri.catalog.main.dataset.test"));
        assert_eq!(job.produced_file_count(), Some(2));
        assert!(job.created_at().is_some());
        assert!(job.start_time().is_none());
    }
}
