use super::CatalogClient;
use crate::core::{WaitOptions, rid::parse_rid};
use crate::{Error, Result};
use chrono::{DateTime, Utc};
use nominal_api::clients::scout::catalog::AsyncCatalogService;
use nominal_api::objects::{api::IngestStatusV2, scout::catalog::DatasetFile as ApiDatasetFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetFileStatus {
    Success,
    InProgress,
    Failed,
    DeletionInProgress,
    Deleted,
    Queued,
    Parsing,
    Ingesting,
    Unknown(String),
}
impl DatasetFileStatus {
    pub fn is_complete(&self) -> bool {
        matches!(
            self,
            Self::Success | Self::DeletionInProgress | Self::Deleted
        )
    }
}
/// Immutable catalog file snapshot; this is distinct from a File Store resource.
#[derive(Debug, Clone)]
pub struct DatasetFile {
    api: ApiDatasetFile,
    id: String,
    status: DatasetFileStatus,
    error: Option<String>,
}
impl DatasetFile {
    pub fn rid(&self) -> &str {
        &self.id
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn dataset_rid(&self) -> &str {
        self.api.dataset_rid().0.as_str()
    }
    pub fn name(&self) -> &str {
        self.api.name()
    }
    pub fn ingest_status(&self) -> &DatasetFileStatus {
        &self.status
    }
    pub fn uploaded_at(&self) -> DateTime<Utc> {
        self.api.uploaded_at()
    }
    pub fn ingested_at(&self) -> Option<DateTime<Utc>> {
        self.api.ingested_at()
    }
    pub fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.api.deleted_at()
    }
    pub fn ingest_error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn timestamp_channel(&self) -> Option<&str> {
        self.api.timestamp_metadata().map(|t| t.series_name())
    }
    pub fn file_tags(&self) -> Option<std::collections::BTreeMap<String, String>> {
        self.api.ingest_tag_metadata().map(|m| {
            m.additional_file_tags()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
    }
    pub fn tag_columns(&self) -> Option<std::collections::BTreeMap<String, String>> {
        self.api.ingest_tag_metadata().map(|m| {
            m.tag_columns()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
    }
    pub(crate) fn from_conjure(api: ApiDatasetFile) -> Self {
        let status = match api.ingest_status() {
            IngestStatusV2::Success(_) => DatasetFileStatus::Success,
            IngestStatusV2::InProgress(_) => DatasetFileStatus::InProgress,
            IngestStatusV2::Error(_) => DatasetFileStatus::Failed,
            IngestStatusV2::DeletionInProgress(_) => DatasetFileStatus::DeletionInProgress,
            IngestStatusV2::Deleted(_) => DatasetFileStatus::Deleted,
            IngestStatusV2::Queued(_) => DatasetFileStatus::Queued,
            IngestStatusV2::Parsing(_) => DatasetFileStatus::Parsing,
            IngestStatusV2::Ingesting(_) => DatasetFileStatus::Ingesting,
            IngestStatusV2::Unknown(value) => DatasetFileStatus::Unknown(format!("{value:?}")),
        };
        let error = match api.ingest_status() {
            IngestStatusV2::Error(e) => Some(format!("{} ({})", e.message(), e.error_type())),
            _ => None,
        };
        Self {
            id: api.id().to_string(),
            api,
            status,
            error,
        }
    }
}
impl CatalogClient {
    pub async fn wait_for_dataset_files(
        &self,
        mut files: Vec<DatasetFile>,
        options: WaitOptions,
    ) -> Result<Vec<DatasetFile>> {
        options.validate()?;
        let started = tokio::time::Instant::now();
        loop {
            let mut pending = Vec::new();
            for file in &mut files {
                if file.status.is_complete() {
                    continue;
                }
                match file.status {
                    DatasetFileStatus::Failed | DatasetFileStatus::Unknown(_) => {
                        return Err(Error::Ingest {
                            details: format!(
                                "dataset file {} failed: {}",
                                file.rid(),
                                file.ingest_error().unwrap_or("unknown status")
                            ),
                        });
                    }
                    _ => {}
                }
                pending.push(file.id.clone());
            }
            if pending.is_empty() {
                return Ok(files);
            }
            if options
                .timeout_duration()
                .is_some_and(|timeout| started.elapsed() >= timeout)
            {
                return Err(Error::Ingest {
                    details: format!("timed out waiting for dataset files {}", pending.join(", ")),
                });
            }
            for file in &mut files {
                if !pending.contains(&file.id) {
                    continue;
                }
                let dataset_rid = parse_rid(file.dataset_rid())?;
                *file = DatasetFile::from_conjure(
                    self.catalog_service
                        .get_dataset_file(&self.token, &dataset_rid, file.api.id())
                        .await?,
                );
            }
            let delay = options
                .timeout_duration()
                .map(|t| {
                    t.saturating_sub(started.elapsed())
                        .min(options.poll_interval())
                })
                .unwrap_or(options.poll_interval());
            tokio::time::sleep(delay).await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dataset_file_completion_requires_success() {
        assert!(DatasetFileStatus::Success.is_complete());
        assert!(DatasetFileStatus::Deleted.is_complete());
        assert!(!DatasetFileStatus::Failed.is_complete());
        assert!(!DatasetFileStatus::Unknown("future".into()).is_complete());
    }
}
