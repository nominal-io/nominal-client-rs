use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use nominal_api::objects::api::{
    Channel, ColumnName, Empty, McapChannelLocator, McapChannelTopic,
    TagName, TagValue, Timestamp as ApiTimestamp,
};
use nominal_api::objects::ingest::api::{
    AvroStreamOpts, ChannelPrefix, CsvOpts, DataflashOpts,
    DatasetIngestTarget as ApiDatasetIngestTarget, ExistingDatasetIngestDestination,
    ExistingVideoIngestDestination, IngestSource, JournalJsonOpts, LogTime, McapChannels,
    McapProtobufTimeseriesOpts, McapTimestampType, ParquetOpts, S3IngestSource,
    VideoIngestTarget as ApiVideoIngestTarget, VideoOpts,
};
use nominal_api::objects::scout::video::api::{
    McapTimestampManifest, NoTimestampManifest, VideoFileTimestampManifest,
};

use crate::core::catalog::{DatasetCreate, VideoCreate};
use crate::core::ingest::progress::{ProgressCallback, UploadEvent};
use crate::core::ingest::timestamp::Timestamp;
use crate::core::rid::parse_rid;
use crate::{Error, Result};

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
    pub(crate) fn into_api(self, workspace_rid: Option<&str>) -> Result<ApiDatasetIngestTarget> {
        Ok(match self {
            DatasetTarget::Existing(rid) => ApiDatasetIngestTarget::Existing(
                ExistingDatasetIngestDestination::new(parse_rid(&rid)?),
            ),
            DatasetTarget::New(create) => {
                ApiDatasetIngestTarget::New(create.into_new_ingest_destination(workspace_rid)?)
            }
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
    pub fn additional_file_tag(mut self, tag: impl Into<String>, value: impl Into<String>) -> Self {
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
    pub fn additional_file_tag(mut self, tag: impl Into<String>, value: impl Into<String>) -> Self {
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

/// Configuration for ingesting an MCAP file's protobuf timeseries data into a
/// dataset. MCAP video tracks are a separate path and not yet wired up.
#[derive(Debug, Clone, Default)]
pub struct McapIngest {
    include_topics: Vec<String>,
    exclude_topics: Vec<String>,
    additional_file_tags: BTreeMap<String, String>,
    ignore_invalid_topics: Option<bool>,
    pub(crate) upload_options: UploadOptions,
}

impl McapIngest {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest only the given topic (call repeatedly to include multiple).
    /// Mutually exclusive with [`Self::exclude_topic`].
    #[must_use]
    pub fn include_topic(mut self, topic: impl Into<String>) -> Self {
        self.include_topics.push(topic.into());
        self
    }

    /// Skip the given topic during ingest (call repeatedly to exclude
    /// multiple). Mutually exclusive with [`Self::include_topic`].
    #[must_use]
    pub fn exclude_topic(mut self, topic: impl Into<String>) -> Self {
        self.exclude_topics.push(topic.into());
        self
    }

    /// Apply a fixed tag value to every row in this file.
    #[must_use]
    pub fn additional_file_tag(mut self, tag: impl Into<String>, value: impl Into<String>) -> Self {
        self.additional_file_tags.insert(tag.into(), value.into());
        self
    }

    /// If `true`, skip invalid MCAP topics instead of failing the whole
    /// ingest. Defaults to the server-side default (false).
    #[must_use]
    pub fn ignore_invalid_topics(mut self, value: bool) -> Self {
        self.ignore_invalid_topics = Some(value);
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
    ) -> Result<McapProtobufTimeseriesOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));

        let channel_filter = match (
            self.include_topics.is_empty(),
            self.exclude_topics.is_empty(),
        ) {
            (true, true) => McapChannels::All(Empty::new()),
            (false, true) => McapChannels::Include(
                self.include_topics
                    .into_iter()
                    .map(topic_locator)
                    .collect(),
            ),
            (true, false) => McapChannels::Exclude(
                self.exclude_topics
                    .into_iter()
                    .map(topic_locator)
                    .collect(),
            ),
            (false, false) => {
                return Err(Error::Ingest {
                    details: "mcap ingest cannot set both include_topic and exclude_topic".into(),
                });
            }
        };

        let mut b = McapProtobufTimeseriesOpts::builder()
            .source(source)
            .target(target)
            .channel_filter(channel_filter)
            .timestamp_type(McapTimestampType::LogTime(LogTime::new()));
        if let Some(ignore) = self.ignore_invalid_topics {
            b = b.ignore_invalid_topics(ignore);
        }
        if !self.additional_file_tags.is_empty() {
            b = b.additional_file_tags(
                self.additional_file_tags
                    .into_iter()
                    .map(|(k, v)| (TagName(k), TagValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        Ok(b.build())
    }
}

fn topic_locator(topic: String) -> McapChannelLocator {
    McapChannelLocator::Topic(McapChannelTopic(topic))
}

/// Configuration for ingesting a journald JSON (`.jsonl` / `.jsonl.gz`) file
/// into a dataset.
#[derive(Debug, Clone, Default)]
pub struct JournalJsonIngest {
    channel: Option<String>,
    pub(crate) upload_options: UploadOptions,
}

impl JournalJsonIngest {
    pub fn new() -> Self {
        Self::default()
    }

    /// Name of the channel the log lines should land in. Defaults to `logs`
    /// server-side when unset.
    #[must_use]
    pub fn channel(mut self, name: impl Into<String>) -> Self {
        self.channel = Some(name.into());
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
    ) -> Result<JournalJsonOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));

        let mut b = JournalJsonOpts::builder().source(source).target(target);
        if let Some(ch) = self.channel {
            b = b.channel(Channel(ch));
        }
        Ok(b.build())
    }
}

/// Configuration for ingesting a Nominal Avro-stream (`.avro`) file into a
/// dataset. The Avro record schema is fixed; see the server-side docs.
#[derive(Debug, Clone, Default)]
pub struct AvroStreamIngest {
    pub(crate) upload_options: UploadOptions,
}

impl AvroStreamIngest {
    pub fn new() -> Self {
        Self::default()
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
    ) -> Result<AvroStreamOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));
        Ok(AvroStreamOpts::builder()
            .source(source)
            .target(target)
            .build())
    }
}

/// Configuration for ingesting an ArduPilot DataFlash (`.bin`) file into a
/// dataset.
#[derive(Debug, Clone, Default)]
pub struct DataflashIngest {
    additional_file_tags: BTreeMap<String, String>,
    pub(crate) upload_options: UploadOptions,
}

impl DataflashIngest {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn additional_file_tag(mut self, tag: impl Into<String>, value: impl Into<String>) -> Self {
        self.additional_file_tags.insert(tag.into(), value.into());
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
    ) -> Result<DataflashOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));

        let mut b = DataflashOpts::builder().source(source).target(target);
        if !self.additional_file_tags.is_empty() {
            b = b.additional_file_tags(
                self.additional_file_tags
                    .into_iter()
                    .map(|(k, v)| (TagName(k), TagValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        Ok(b.build())
    }
}

/// Where a video ingest should land. Either an existing video resource (by
/// RID) or a new one, created atomically with the ingest.
///
/// `&str` / `String` convert to [`VideoTarget::Existing`]; [`VideoCreate`]
/// converts to [`VideoTarget::New`].
#[derive(Debug, Clone)]
pub enum VideoTarget {
    Existing(String),
    New(VideoCreate),
}

impl From<String> for VideoTarget {
    fn from(rid: String) -> Self {
        Self::Existing(rid)
    }
}

impl From<&str> for VideoTarget {
    fn from(rid: &str) -> Self {
        Self::Existing(rid.to_string())
    }
}

impl From<&String> for VideoTarget {
    fn from(rid: &String) -> Self {
        Self::Existing(rid.clone())
    }
}

impl From<VideoCreate> for VideoTarget {
    fn from(create: VideoCreate) -> Self {
        Self::New(create)
    }
}

impl VideoTarget {
    pub(crate) fn into_api(self, workspace_rid: Option<&str>) -> Result<ApiVideoIngestTarget> {
        Ok(match self {
            VideoTarget::Existing(rid) => ApiVideoIngestTarget::Existing(
                ExistingVideoIngestDestination::new(parse_rid(&rid)?),
            ),
            VideoTarget::New(create) => {
                ApiVideoIngestTarget::New(create.into_new_ingest_destination(workspace_rid)?)
            }
        })
    }
}

/// How the timestamps for a video ingest are derived.
#[derive(Debug, Clone)]
enum VideoManifest {
    /// The first frame is at this absolute UTC timestamp; subsequent frames
    /// are spaced by the video file's own metadata.
    StartingAt(DateTime<Utc>),
    /// The video is one stream inside an MCAP file, on this topic. Per-frame
    /// timestamps come from the MCAP log times.
    McapTopic(String),
}

/// Configuration for ingesting a video into an existing or new video resource.
#[derive(Debug, Clone)]
pub struct VideoIngest {
    manifest: VideoManifest,
    pub(crate) upload_options: UploadOptions,
}

impl VideoIngest {
    /// Ingest a standalone video file (`.mp4` / `.mkv` / `.avi` / `.ts`) whose
    /// first frame is at `start`.
    pub fn starting_at(start: DateTime<Utc>) -> Self {
        Self {
            manifest: VideoManifest::StartingAt(start),
            upload_options: UploadOptions::default(),
        }
    }

    /// Extract the video stream on `topic` from an MCAP file. Each upload may
    /// only target a single MCAP topic.
    pub fn mcap_topic(topic: impl Into<String>) -> Self {
        Self {
            manifest: VideoManifest::McapTopic(topic.into()),
            upload_options: UploadOptions::default(),
        }
    }

    #[must_use]
    pub fn upload_options(mut self, options: UploadOptions) -> Self {
        self.upload_options = options;
        self
    }

    /// True if this ingest targets an MCAP video stream rather than a plain
    /// video file. Used by the upload path to pick the right MIME type.
    pub(crate) fn is_mcap(&self) -> bool {
        matches!(self.manifest, VideoManifest::McapTopic(_))
    }

    pub(crate) fn into_opts(
        self,
        target: VideoTarget,
        workspace_rid: Option<&str>,
        s3_path: String,
    ) -> Result<VideoOpts> {
        let target = target.into_api(workspace_rid)?;
        let source = IngestSource::S3(S3IngestSource::new(s3_path));
        let manifest = match self.manifest {
            VideoManifest::StartingAt(dt) => VideoFileTimestampManifest::NoManifest(
                NoTimestampManifest::builder()
                    .starting_timestamp(api_timestamp_from_datetime(dt)?)
                    .build(),
            ),
            VideoManifest::McapTopic(topic) => VideoFileTimestampManifest::Mcap(
                McapTimestampManifest::builder()
                    .mcap_channel_locator(McapChannelLocator::Topic(McapChannelTopic(topic)))
                    .build(),
            ),
        };
        Ok(VideoOpts::builder()
            .source(source)
            .target(target)
            .timestamp_manifest(manifest)
            .build())
    }
}

fn api_timestamp_from_datetime(dt: DateTime<Utc>) -> Result<ApiTimestamp> {
    let seconds = dt.timestamp();
    let nanos = i64::from(dt.timestamp_subsec_nanos());
    let seconds_safe = conjure_object::SafeLong::try_from(seconds).map_err(|_| Error::Ingest {
        details: format!("video starting timestamp seconds out of range: {seconds}"),
    })?;
    let nanos_safe = conjure_object::SafeLong::try_from(nanos).map_err(|_| Error::Ingest {
        details: format!("video starting timestamp nanos out of range: {nanos}"),
    })?;
    Ok(ApiTimestamp::new(seconds_safe, nanos_safe))
}
