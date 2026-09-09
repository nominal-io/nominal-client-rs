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
/// A dataset file and its current ingest state. This is not a File Store resource.
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
        use futures::{TryStreamExt, stream};
        use tokio::time::{Instant, sleep_until, timeout_at};

        const MAX_FILE_REFRESHES: usize = 8;
        options.validate()?;
        let deadline = options
            .timeout_duration()
            .map(|timeout| Instant::now() + timeout);
        for file in &files {
            file.check_ingest_failure()?;
        }
        while files.iter().any(|file| !file.status.is_complete()) {
            // Refresh files independently so responses can complete out of order while
            // the caller's result order stays unchanged. Dropping this stream on a
            // failure cancels the remaining read requests.
            stream::iter(
                files
                    .iter_mut()
                    .filter(|file| !file.status.is_complete())
                    .map(Ok),
            )
            .try_for_each_concurrent(MAX_FILE_REFRESHES, |file| async move {
                let dataset_rid = parse_rid(file.dataset_rid())?;
                let refresh =
                    self.catalog_service
                        .get_dataset_file(&self.token, &dataset_rid, file.api.id());
                let latest = match deadline {
                    Some(deadline) => {
                        timeout_at(deadline, refresh)
                            .await
                            .map_err(|_| Error::Ingest {
                                details: format!("timed out waiting for dataset file {}", file.id),
                            })??
                    }
                    None => refresh.await?,
                };
                *file = DatasetFile::from_conjure(latest);
                file.check_ingest_failure()
            })
            .await?;
            if files.iter().any(|file| !file.status.is_complete()) {
                let next_poll = Instant::now() + options.poll_interval();
                sleep_until(deadline.map_or(next_poll, |deadline| deadline.min(next_poll))).await;
            }
        }
        Ok(files)
    }
}

impl DatasetFile {
    fn check_ingest_failure(&self) -> Result<()> {
        if matches!(
            self.status,
            DatasetFileStatus::Failed | DatasetFileStatus::Unknown(_)
        ) {
            return Err(Error::Ingest {
                details: format!(
                    "dataset file {} failed: {}",
                    self.rid(),
                    self.ingest_error().unwrap_or("unknown status")
                ),
            });
        }
        Ok(())
    }

    /// Bounds in nanoseconds, using the coordinate system named by `bounds_timestamp_type`.
    pub fn bounds(&self) -> Option<(i128, i128)> {
        self.api.bounds().map(|b| {
            let nanos = |t: &nominal_api::objects::api::Timestamp| {
                i128::from(i64::from(t.seconds())) * 1_000_000_000
                    + i128::from(i64::from(t.nanos()))
            };
            (nanos(b.start()), nanos(b.end()))
        })
    }
    pub fn bounds_timestamp_type(&self) -> Option<String> {
        self.api.bounds().map(|b| b.type_().to_string())
    }
    pub fn file_size_bytes(&self) -> Option<i64> {
        self.api.file_size_bytes().map(i64::from)
    }
    /// Full timestamp interpretation including relative offset and custom defaults.
    /// Unrecognized server encodings are reported explicitly.
    pub fn timestamp(&self) -> Result<Option<crate::core::Timestamp>> {
        use crate::core::{TimeUnit, Timestamp};
        use nominal_api::objects::scout::catalog::{AbsoluteTimestamp as A, TimestampType as T};
        let Some(metadata) = self.api.timestamp_metadata() else {
            return Ok(None);
        };
        let unit = |u: &nominal_api::objects::api::TimeUnit| -> Result<TimeUnit> {
            use nominal_api::objects::api::TimeUnit as U;
            Ok(match u {
                U::Nanoseconds => TimeUnit::Nanoseconds,
                U::Microseconds => TimeUnit::Microseconds,
                U::Milliseconds => TimeUnit::Milliseconds,
                U::Seconds => TimeUnit::Seconds,
                U::Minutes => TimeUnit::Minutes,
                U::Hours => TimeUnit::Hours,
                U::Days => TimeUnit::Days,
                U::Unknown(_) => {
                    return Err(Error::UnexpectedResponse {
                        field: "dataset_file.timestamp.time_unit",
                    });
                }
            })
        };
        let name = metadata.series_name();
        let timestamp = match metadata.timestamp_type() {
            T::Relative(r) => {
                let timestamp = Timestamp::relative(name, unit(r.time_unit())?);
                if let Some(offset) = r.offset() {
                    timestamp.with_offset(offset)
                } else {
                    timestamp
                }
            }
            T::Absolute(a) => match a.as_ref() {
                A::Iso8601(_) => Timestamp::iso8601(name),
                A::EpochOfTimeUnit(e) => Timestamp::epoch(name, unit(e.time_unit())?),
                A::CustomFormat(c) => {
                    let mut timestamp = Timestamp::custom(name, c.format());
                    if let Some(year) = c.default_year() {
                        timestamp = timestamp.with_default_year(year);
                    }
                    if let Some(day) = c.default_day_of_year() {
                        timestamp = timestamp.with_default_day_of_year(day);
                    }
                    timestamp
                }
                A::Unknown(_) => {
                    return Err(Error::UnexpectedResponse {
                        field: "dataset_file.timestamp.absolute",
                    });
                }
            },
            T::Unknown(_) => {
                return Err(Error::UnexpectedResponse {
                    field: "dataset_file.timestamp",
                });
            }
        };
        Ok(Some(timestamp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{HttpFixture, dataset_file};
    #[tokio::test]
    async fn file_failure_is_reported_even_when_another_refresh_stalls() {
        let stalled = "00000000-0000-0000-0000-000000000001";
        let failed = "00000000-0000-0000-0000-000000000002";
        for ids in [[failed, stalled], [stalled, failed]] {
            let server = HttpFixture::new(move |request| async move {
                if request.uri().path().contains(failed) {
                    dataset_file(failed, "futureFailure")
                } else {
                    std::future::pending().await
                }
            })
            .await;
            let files = ids
                .into_iter()
                .map(|id| {
                    DatasetFile::from_conjure(
                        serde_json::from_value(dataset_file(id, "inProgress")).unwrap(),
                    )
                })
                .collect();
            let catalog = server.client.catalog();
            let error = server
                .run(catalog.wait_for_dataset_files(files, WaitOptions::default()))
                .await
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(failed) && error.contains("failed"),
                "{error}"
            );
            assert!(!error.contains("timed out"), "{error}");
        }
    }
}
