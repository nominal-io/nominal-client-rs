use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use nominal_api::api::{ColumnName, TagName, TagValue};
use nominal_api::ingest::api::{
    ChannelPrefix, CsvOpts, DatasetIngestTarget as ApiDatasetIngestTarget,
    ExistingDatasetIngestDestination, IngestSource, ParquetOpts, S3IngestSource,
};

use crate::Result;
use crate::core::catalog::DatasetCreate;
use crate::core::ingest::progress::{ProgressCallback, UploadEvent};
use crate::core::ingest::timestamp::Timestamp;
use crate::core::rid::parse_rid;

/// Where an ingest should land. Either an existing dataset (by RID) or a new
/// dataset, which will be created atomically alongside the ingest — a failed
/// ingest leaves no dataset behind.
///
/// `&str` / `String` convert to [`DatasetTarget::Existing`]; [`DatasetCreate`]
/// converts to [`DatasetTarget::New`], so callers can usually pass either
/// directly without naming the enum.
#[derive(Debug, Clone)]
pub enum DatasetTarget {
    Existing(String),
    New(DatasetCreate),
}

impl From<String> for DatasetTarget {
    fn from(rid: String) -> Self {
        Self::Existing(rid)
    }
}

impl From<&str> for DatasetTarget {
    fn from(rid: &str) -> Self {
        Self::Existing(rid.to_string())
    }
}

impl From<&String> for DatasetTarget {
    fn from(rid: &String) -> Self {
        Self::Existing(rid.clone())
    }
}

impl From<DatasetCreate> for DatasetTarget {
    fn from(create: DatasetCreate) -> Self {
        Self::New(create)
    }
}

impl DatasetTarget {
    pub(crate) fn into_api(
        self,
        workspace_rid: Option<&str>,
    ) -> Result<ApiDatasetIngestTarget> {
        Ok(match self {
            DatasetTarget::Existing(rid) => {
                ApiDatasetIngestTarget::Existing(ExistingDatasetIngestDestination::new(
                    parse_rid(&rid)?,
                ))
            }
            DatasetTarget::New(create) => ApiDatasetIngestTarget::New(
                create.into_new_ingest_destination(workspace_rid)?,
            ),
        })
    }
}

pub(crate) const DEFAULT_CHUNK_SIZE: usize = 64 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_CONCURRENCY: usize = 8;
pub(crate) const DEFAULT_MAX_RETRIES: usize = 3;

/// Knobs for the network-side of a multipart upload.
#[derive(Clone)]
pub struct UploadOptions {
    pub(crate) chunk_size: usize,
    pub(crate) max_concurrency: usize,
    pub(crate) max_retries: usize,
    pub(crate) progress: Option<ProgressCallback>,
}

impl std::fmt::Debug for UploadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UploadOptions")
            .field("chunk_size", &self.chunk_size)
            .field("max_concurrency", &self.max_concurrency)
            .field("max_retries", &self.max_retries)
            .field("progress", &self.progress.as_ref().map(|_| "<callback>"))
            .finish()
    }
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            max_retries: DEFAULT_MAX_RETRIES,
            progress: None,
        }
    }
}

impl UploadOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Size of each uploaded part, in bytes. Defaults to 64 MiB. Must be at
    /// least 5 MiB except for the final part (S3 minimum).
    #[must_use]
    pub fn chunk_size(mut self, bytes: usize) -> Self {
        self.chunk_size = bytes;
        self
    }

    /// Maximum number of parts uploaded concurrently. Defaults to 8.
    #[must_use]
    pub fn max_concurrency(mut self, n: usize) -> Self {
        self.max_concurrency = n;
        self
    }

    /// Maximum number of retries per part on failure. Defaults to 3.
    #[must_use]
    pub fn max_retries(mut self, n: usize) -> Self {
        self.max_retries = n;
        self
    }

    /// Register a callback that receives progress events during the upload.
    ///
    /// The callback runs from arbitrary tasks; keep it cheap. Forward events
    /// through a channel or atomic counter if you need to do real work.
    #[must_use]
    pub fn on_progress<F>(mut self, f: F) -> Self
    where
        F: Fn(UploadEvent) + Send + Sync + 'static,
    {
        self.progress = Some(Arc::new(f));
        self
    }
}

/// Configuration for ingesting a CSV file into an existing dataset.
#[derive(Debug, Clone)]
pub struct CsvIngest {
    timestamp: Timestamp,
    channel_prefix: Option<String>,
    tag_columns: BTreeMap<String, String>,
    additional_file_tags: BTreeMap<String, String>,
    exclude_columns: BTreeSet<String>,
    pub(crate) upload_options: UploadOptions,
}

impl CsvIngest {
    pub fn new(timestamp: Timestamp) -> Self {
        Self {
            timestamp,
            channel_prefix: None,
            tag_columns: BTreeMap::new(),
            additional_file_tags: BTreeMap::new(),
            exclude_columns: BTreeSet::new(),
            upload_options: UploadOptions::default(),
        }
    }

    /// Prefix every channel name in this file with the given string.
    #[must_use]
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.channel_prefix = Some(prefix.into());
        self
    }

    /// Derive the given tag name's value from the named column.
    #[must_use]
    pub fn tag_column(mut self, tag: impl Into<String>, column: impl Into<String>) -> Self {
        self.tag_columns.insert(tag.into(), column.into());
        self
    }

    /// Apply a fixed tag value to every row in this file.
    #[must_use]
    pub fn additional_file_tag(
        mut self,
        tag: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.additional_file_tags.insert(tag.into(), value.into());
        self
    }

    /// Exclude the named column from ingestion.
    #[must_use]
    pub fn exclude_column(mut self, column: impl Into<String>) -> Self {
        self.exclude_columns.insert(column.into());
        self
    }

    /// Override upload behavior (chunk size, concurrency, retries, progress).
    #[must_use]
    pub fn upload_options(mut self, options: UploadOptions) -> Self {
        self.upload_options = options;
        self
    }

    pub(crate) fn into_opts(
        self,
        target: DatasetTarget,
        workspace_rid: Option<&str>,
        s3_path: String,
    ) -> Result<CsvOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));

        let mut b = CsvOpts::builder()
            .source(source)
            .target(target)
            .timestamp_metadata(self.timestamp.into_conjure());
        if let Some(prefix) = self.channel_prefix {
            b = b.channel_prefix(ChannelPrefix(Some(prefix)));
        }
        if !self.tag_columns.is_empty() {
            b = b.tag_columns(
                self.tag_columns
                    .into_iter()
                    .map(|(k, v)| (TagName(k), ColumnName(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if !self.additional_file_tags.is_empty() {
            b = b.additional_file_tags(
                self.additional_file_tags
                    .into_iter()
                    .map(|(k, v)| (TagName(k), TagValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if !self.exclude_columns.is_empty() {
            b = b.exclude_columns(self.exclude_columns.into_iter().map(ColumnName));
        }
        Ok(b.build())
    }
}

/// Configuration for ingesting a Parquet file into an existing dataset.
#[derive(Debug, Clone)]
pub struct ParquetIngest {
    timestamp: Timestamp,
    channel_prefix: Option<String>,
    tag_columns: BTreeMap<String, String>,
    additional_file_tags: BTreeMap<String, String>,
    exclude_columns: BTreeSet<String>,
    is_archive: Option<bool>,
    pub(crate) upload_options: UploadOptions,
}

impl ParquetIngest {
    pub fn new(timestamp: Timestamp) -> Self {
        Self {
            timestamp,
            channel_prefix: None,
            tag_columns: BTreeMap::new(),
            additional_file_tags: BTreeMap::new(),
            exclude_columns: BTreeSet::new(),
            is_archive: None,
            upload_options: UploadOptions::default(),
        }
    }

    #[must_use]
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.channel_prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub fn tag_column(mut self, tag: impl Into<String>, column: impl Into<String>) -> Self {
        self.tag_columns.insert(tag.into(), column.into());
        self
    }

    #[must_use]
    pub fn additional_file_tag(
        mut self,
        tag: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.additional_file_tags.insert(tag.into(), value.into());
        self
    }

    #[must_use]
    pub fn exclude_column(mut self, column: impl Into<String>) -> Self {
        self.exclude_columns.insert(column.into());
        self
    }

    /// Mark the file as an archive (.tar, .tar.gz, .zip) whose .parquet
    /// entries will be extracted and ingested.
    #[must_use]
    pub fn is_archive(mut self, is_archive: bool) -> Self {
        self.is_archive = Some(is_archive);
        self
    }

    #[must_use]
    pub fn upload_options(mut self, options: UploadOptions) -> Self {
        self.upload_options = options;
        self
    }

    pub(crate) fn into_opts(
        self,
        target: DatasetTarget,
        workspace_rid: Option<&str>,
        s3_path: String,
    ) -> Result<ParquetOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));

        let mut b = ParquetOpts::builder()
            .source(source)
            .target(target)
            .timestamp_metadata(self.timestamp.into_conjure());
        if let Some(prefix) = self.channel_prefix {
            b = b.channel_prefix(ChannelPrefix(Some(prefix)));
        }
        if !self.tag_columns.is_empty() {
            b = b.tag_columns(
                self.tag_columns
                    .into_iter()
                    .map(|(k, v)| (TagName(k), ColumnName(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if !self.additional_file_tags.is_empty() {
            b = b.additional_file_tags(
                self.additional_file_tags
                    .into_iter()
                    .map(|(k, v)| (TagName(k), TagValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if let Some(is_archive) = self.is_archive {
            b = b.is_archive(is_archive);
        }
        if !self.exclude_columns.is_empty() {
            b = b.exclude_columns(self.exclude_columns.into_iter().map(ColumnName));
        }
        Ok(b.build())
    }
}

