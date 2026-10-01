use crate::core::Timestamp;
use crate::extractor::timestamp::output_timestamp;
use crate::extractor::{Error, Result, environment::Environment};
use chrono::{DateTime, Utc};
use nominal_api::objects::ingest::manifest::ExtractorManifest;
use nominal_api::objects::{
    api::{Channel, Timestamp as VideoTimestamp},
    ingest::manifest::{
        ManifestIngestType, ManifestOutput, ManifestVideoOutput, VideoTimestampManifest,
    },
    scout::video::api::{NoTimestampManifest, ScaleParameter},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};
/// CSV or Parquet channel, tag and timestamp overrides. Defaults inherit job settings.
#[derive(Clone, Debug, Default)]
pub struct TabularOptions {
    timestamp: Option<Timestamp>,
    prefix: Option<String>,
    tags: BTreeMap<String, String>,
}
impl TabularOptions {
    /// Use the job timestamp settings with no prefix or tag columns.
    pub fn new() -> Self {
        Self::default()
    }
    /// Override timestamps with numeric epoch or relative settings. Registration accepts
    /// seconds through nanoseconds and requires an explicit offset for relative timestamps.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }
    /// Prepend this prefix to ingested channel names.
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
    /// Use values from `column` as the tag named `key`; repeated keys replace the mapping.
    pub fn tag_column(mut self, key: impl Into<String>, column: impl Into<String>) -> Self {
        self.tags.insert(key.into(), column.into());
        self
    }
}
/// Avro stream timestamp and channel overrides. Defaults inherit job settings.
#[derive(Clone, Debug, Default)]
pub struct AvroStreamOptions {
    timestamp: Option<Timestamp>,
    prefix: Option<String>,
}
impl AvroStreamOptions {
    /// Use the job timestamp settings without a channel prefix.
    pub fn new() -> Self {
        Self::default()
    }
    /// Override numeric timestamps; the series must be `timestamps`. Registration accepts
    /// seconds through nanoseconds and requires an explicit offset for relative timestamps.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }
    /// Prepend this prefix to ingested channel names.
    pub fn channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
}
/// Journal JSON timestamp overrides. Defaults inherit job settings.
#[derive(Clone, Debug, Default)]
pub struct JournalJsonOptions {
    timestamp: Option<Timestamp>,
}
impl JournalJsonOptions {
    /// Use the job timestamp settings.
    pub fn new() -> Self {
        Self::default()
    }
    /// Override timestamps with numeric epoch or relative settings. Registration accepts
    /// seconds through nanoseconds and requires an explicit offset for relative timestamps.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }
}
/// Video timing settings. Supply a start time or an existing frame timestamp sidecar.
#[derive(Clone, Debug)]
pub struct VideoOptions {
    timing: VideoTiming,
    scale: Option<VideoScale>,
}
#[derive(Clone, Debug)]
enum VideoTiming {
    Start(DateTime<Utc>),
    Frames(PathBuf),
}
#[derive(Clone, Debug)]
enum VideoScale {
    End(DateTime<Utc>),
    Rate(f64),
    Factor(f64),
}
impl VideoOptions {
    /// Anchor the video's encoded timeline at this instant.
    pub fn starting_at(at: DateTime<Utc>) -> Self {
        Self {
            timing: VideoTiming::Start(at),
            scale: None,
        }
    }
    /// Read absolute epoch nanoseconds from a JSON list relative to the output directory.
    pub fn frame_timestamps(path: impl Into<PathBuf>) -> Self {
        Self {
            timing: VideoTiming::Frames(path.into()),
            scale: None,
        }
    }
    /// Scale a start-based timeline to end at this instant; replaces any prior scale.
    pub fn ending_at(mut self, at: DateTime<Utc>) -> Self {
        self.scale = Some(VideoScale::End(at));
        self
    }
    /// Interpret a start-based timeline with this frame rate; replaces any prior scale.
    pub fn frame_rate(mut self, rate: f64) -> Self {
        self.scale = Some(VideoScale::Rate(rate));
        self
    }
    /// Multiply a start-based timeline by this factor; replaces any prior scale.
    pub fn scale_factor(mut self, factor: f64) -> Self {
        self.scale = Some(VideoScale::Factor(factor));
        self
    }
}
/// Inputs, parameters and output declarations passed to an extractor callback.
///
/// Write files under [`Self::output_dir`], then register them with an `add_*` method.
/// Registration records paths and settings; it does not parse the output contents.
/// The runner publishes the manifest after the callback succeeds.
pub struct ManifestContext {
    env: Environment,
    output_dir: PathBuf,
    declared_paths: BTreeSet<PathBuf>,
    outputs: Vec<ManifestOutput>,
    video_outputs: Vec<ManifestVideoOutput>,
}
impl ManifestContext {
    pub(crate) fn new(env: BTreeMap<String, String>) -> Result<Self> {
        let env = Environment::new(env)?;
        let output_dir = std::fs::canonicalize(
            env.value("OUTPUT_DIR")
                .ok_or_else(|| Error::MissingEnvironment("OUTPUT_DIR".into()))?,
        )?;
        Ok(Self {
            env,
            output_dir,
            declared_paths: BTreeSet::new(),
            outputs: vec![],
            video_outputs: vec![],
        })
    }
    /// Register a CSV or Parquet file relative to the output directory.
    /// Supports gzip suffixes. Returns its absolute local path. Invalid paths or
    /// timestamp overrides leave the declarations unchanged.
    pub fn add_tabular(
        &mut self,
        path: impl AsRef<Path>,
        options: TabularOptions,
    ) -> Result<PathBuf> {
        let timestamp = options.timestamp.map(output_timestamp).transpose()?;
        self.record(
            path.as_ref(),
            &[".csv", ".csv.gz", ".parquet", ".parquet.gz"],
            |relative| {
                ManifestOutput::builder()
                    .ingest_type(ManifestIngestType::Tabular)
                    .relative_path(relative)
                    .tag_columns(options.tags)
                    .channel_prefix(options.prefix)
                    .timestamp_metadata(timestamp)
                    .build()
            },
        )
    }
    /// Register an Avro stream relative to the output directory, optionally gzip compressed.
    /// Numeric timestamp overrides must name `timestamps`. Returns its absolute local path.
    pub fn add_avro_stream(
        &mut self,
        path: impl AsRef<Path>,
        options: AvroStreamOptions,
    ) -> Result<PathBuf> {
        let timestamp = options.timestamp.map(output_timestamp).transpose()?;
        if timestamp
            .as_ref()
            .is_some_and(|timestamp| timestamp.series_name() != "timestamps")
        {
            return Err(Error::InvalidOutput(
                "Avro stream timestamp overrides must name timestamps".into(),
            ));
        }
        self.record(path.as_ref(), &[".avro", ".avro.gz"], |relative| {
            ManifestOutput::builder()
                .ingest_type(ManifestIngestType::AvroStream)
                .relative_path(relative)
                .channel_prefix(options.prefix)
                .timestamp_metadata(timestamp)
                .build()
        })
    }
    /// Register journal JSON (`.jsonl` or `.jsonl.gz`) relative to the output directory.
    /// Returns its absolute local path. Timestamp overrides are validated before registration.
    pub fn add_journal_json(
        &mut self,
        path: impl AsRef<Path>,
        options: JournalJsonOptions,
    ) -> Result<PathBuf> {
        let timestamp = options.timestamp.map(output_timestamp).transpose()?;
        self.record(path.as_ref(), &[".jsonl", ".jsonl.gz"], |relative| {
            ManifestOutput::builder()
                .ingest_type(ManifestIngestType::JsonL)
                .relative_path(relative)
                .timestamp_metadata(timestamp)
                .build()
        })
    }
    /// Register a video relative to the output directory with a nonempty channel name.
    /// Returns its absolute local path. Frame sidecars must contain a nonempty JSON list
    /// of signed epoch nanoseconds. Timeline scaling requires start-based timing and
    /// finite values. Invalid settings or paths leave declarations unchanged.
    pub fn add_video(
        &mut self,
        path: impl AsRef<Path>,
        channel: impl Into<String>,
        options: VideoOptions,
    ) -> Result<PathBuf> {
        let channel = channel.into();
        if channel.is_empty() {
            return Err(Error::InvalidOutput("video channel is empty".into()));
        }
        let (path, relative) = self.resolve_relative(path.as_ref())?;
        check_extension(&path, &[".avi", ".m2ts", ".mkv", ".mp4", ".ts"])?;
        let mut sidecar = None;
        let timing = match options.timing {
            VideoTiming::Start(at) => {
                let scale = options
                    .scale
                    .map(|scale| match scale {
                        VideoScale::End(at) => {
                            video_timestamp(at).map(ScaleParameter::EndingTimestamp)
                        }
                        VideoScale::Rate(value) if value.is_finite() => {
                            Ok(ScaleParameter::TrueFrameRate(value))
                        }
                        VideoScale::Factor(value) if value.is_finite() => {
                            Ok(ScaleParameter::ScaleFactor(value))
                        }
                        _ => Err(Error::InvalidOutput("video scale must be finite".into())),
                    })
                    .transpose()?;
                VideoTimestampManifest::NoManifest(
                    NoTimestampManifest::builder()
                        .starting_timestamp(video_timestamp(at)?)
                        .scale_parameter(scale)
                        .build(),
                )
            }
            VideoTiming::Frames(frame_path) => {
                if options.scale.is_some() {
                    return Err(Error::InvalidOutput(
                        "video timeline scaling cannot be combined with frame timestamps".into(),
                    ));
                }
                let (frame_path, name) = self.resolve_relative(&frame_path)?;
                let frames: Vec<i64> = serde_json::from_reader(std::fs::File::open(frame_path)?)?;
                if frames.is_empty() {
                    return Err(Error::InvalidOutput("frame timestamps are empty".into()));
                }
                sidecar = Some(name.clone());
                VideoTimestampManifest::FrameTimestampsRelativePath(name)
            }
        };
        tracing::debug!(path = %relative, channel = %channel, "declared video output");
        self.video_outputs.push(ManifestVideoOutput::new(
            relative.clone(),
            Channel(channel),
            timing,
        ));
        self.declared_paths.insert(self.output_dir.join(relative));
        if let Some(sidecar) = sidecar {
            self.declared_paths.insert(self.output_dir.join(sidecar));
        }
        Ok(path)
    }

    fn record(
        &mut self,
        path: &Path,
        extensions: &[&str],
        declaration: impl FnOnce(String) -> ManifestOutput,
    ) -> Result<PathBuf> {
        let (path, relative) = self.resolve_relative(path)?;
        check_extension(&path, extensions)?;
        let output = declaration(relative.clone());
        tracing::debug!(path = %relative, ingest_type = ?output.ingest_type(), "declared output");
        self.outputs.push(output);
        self.declared_paths.insert(self.output_dir.join(relative));
        Ok(path)
    }

    /// Write a new frame timestamp sidecar at an explicit path relative to the output directory.
    /// Pass this path to [`VideoOptions::frame_timestamps`] when declaring a video. Values are
    /// signed nanoseconds since the Unix epoch. The list must be nonempty, the parent
    /// directory must exist inside the output directory, and an existing file is never
    /// overwritten. Failed writes remove the partial file. The sidecar is declared only
    /// when referenced by [`Self::add_video`].
    pub fn write_frame_timestamps(
        &self,
        relative_path: impl AsRef<Path>,
        frames: &[i64],
    ) -> Result<()> {
        let relative_path = relative_path.as_ref();
        if frames.is_empty() {
            return Err(Error::InvalidOutput("frame timestamps are empty".into()));
        }
        if relative_path.is_absolute() {
            return Err(Error::InvalidOutput(
                "sidecar paths must be relative to OUTPUT_DIR".into(),
            ));
        }
        let filename = relative_path
            .file_name()
            .ok_or_else(|| Error::InvalidOutput("sidecar filename is missing".into()))?;
        let parent = std::fs::canonicalize(
            self.output_dir
                .join(relative_path.parent().unwrap_or(Path::new(""))),
        )?;
        if !parent.starts_with(&self.output_dir) {
            return Err(Error::InvalidOutput(
                "sidecar parent is outside output directory".into(),
            ));
        }
        let path = parent.join(filename);
        if path == self.output_dir.join("manifest.json") {
            return Err(Error::InvalidOutput("manifest.json is reserved".into()));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::SidecarCollision(path.clone())
                } else {
                    Error::Io(error)
                }
            })?;
        let written = (|| -> Result<()> {
            serde_json::to_writer(&mut file, frames)?;
            file.flush()?;
            self.resolve_relative(relative_path)?;
            Ok(())
        })();
        match written {
            Ok(()) => Ok(()),
            Err(error) => {
                drop(file);
                let _ = std::fs::remove_file(path);
                Err(error)
            }
        }
    }

    fn resolve_relative(&self, relative: &Path) -> Result<(PathBuf, String)> {
        if relative.is_absolute() {
            return Err(Error::InvalidOutput(
                "manifest paths must be relative to OUTPUT_DIR".into(),
            ));
        }
        let path = self.output_dir.join(relative);
        let resolved = std::fs::canonicalize(&path)?;
        if !resolved.is_file() {
            return Err(Error::InvalidOutput(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let relative = resolved.strip_prefix(&self.output_dir).map_err(|_| {
            Error::InvalidOutput(format!("{} is outside output directory", path.display()))
        })?;
        let relative = manifest_path(relative)?;
        if relative == "manifest.json" {
            return Err(Error::InvalidOutput("manifest.json is reserved".into()));
        }
        // Validate the supplied extension, but record the canonical relative path.
        Ok((path, relative))
    }
    /// Return the current declarations as JSON without publishing a manifest file.
    /// Sidecars created by [`Self::write_frame_timestamps`] already exist on disk.
    /// Returns an error if serialization fails.
    pub fn build_manifest(&self) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(self.manifest())?)
    }
    fn manifest(&self) -> ExtractorManifest {
        ExtractorManifest::builder()
            .outputs(self.outputs.clone())
            .video_outputs((!self.video_outputs.is_empty()).then(|| self.video_outputs.clone()))
            .build()
    }
    pub(crate) fn finalize(&self) -> Result<()> {
        if self.outputs.is_empty() && self.video_outputs.is_empty() {
            return Err(Error::EmptyOutputs);
        }
        if let Err(error) = self.warn_strays(&self.output_dir) {
            tracing::warn!(%error, "could not scan for undeclared output files");
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.output_dir)?;
        serde_json::to_writer(&mut temp, &self.manifest())?;
        temp.flush()?;
        temp.persist(self.output_dir.join("manifest.json"))
            .map_err(|error| error.error)?;
        tracing::info!(
            path = %self.output_dir.join("manifest.json").display(),
            outputs = self.outputs.len(),
            video_outputs = self.video_outputs.len(),
            "wrote extractor manifest"
        );
        Ok(())
    }

    fn warn_strays(&self, directory: &Path) -> Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                self.warn_strays(&path)?;
            } else if path.is_file() && !self.declared_paths.contains(&path) {
                let relative = path
                    .strip_prefix(&self.output_dir)
                    .expect("walk under output directory");
                tracing::warn!(path = %relative.display(), "undeclared output file will not be ingested");
            }
        }
        Ok(())
    }

    /// Return registered input paths in registration order.
    /// Without registration metadata, list files in `NOMINAL_EXTRACTOR_INPUT_DIR`
    /// (default `/input`) in sorted order. A missing directory gives an empty list;
    /// directory read failures return an error. Registered paths are not checked here.
    pub fn inputs(&self) -> Result<Vec<PathBuf>> {
        self.env.inputs()
    }
    /// Look up an input by its registered name or environment variable.
    /// Without registration metadata, read the path from the named environment variable.
    /// Returns an error for an unknown name; the returned path is not checked for existence.
    pub fn input(&self, name: &str) -> Result<PathBuf> {
        self.env.input(name)
    }
    /// Return the only input, or an error if there are zero or multiple inputs.
    /// Uses the same discovery rules as [`Self::inputs`].
    pub fn sole_input(&self) -> Result<PathBuf> {
        let mut files = self.inputs()?;
        if files.len() != 1 {
            return Err(Error::SoleInput(files.len()));
        }
        Ok(files.remove(0))
    }
    /// Return the canonical output directory supplied through `OUTPUT_DIR`.
    /// Write output files here before registering them.
    pub fn output_dir(&self) -> &std::path::Path {
        &self.output_dir
    }
    /// Parse a parameter by its registered name or environment variable.
    /// Returns an error if the name is unregistered, the value is absent, or parsing fails.
    /// Without registration metadata, the name is used directly as an environment variable.
    pub fn param<T: std::str::FromStr>(&self, name: &str) -> Result<T>
    where
        T::Err: std::fmt::Display,
    {
        self.optional_param(name)?
            .ok_or_else(|| Error::MissingParameter(name.into()))
    }
    /// Parse a parameter when its environment variable is present.
    /// An absent value returns `None`; an unregistered name or invalid value returns an error.
    /// An empty string is present and is passed to the requested type's parser.
    /// Without registration metadata, the name is used directly as an environment variable.
    pub fn optional_param<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>>
    where
        T::Err: std::fmt::Display,
    {
        self.env.optional_param(name)
    }
    /// Return the ingest job RID supplied by Nominal, if present and nonempty.
    pub fn ingest_job_rid(&self) -> Option<&str> {
        self.env.value("_NOMINAL_INGEST_JOB_RID")
    }
    /// Return the dataset RID supplied by Nominal, if present and nonempty.
    pub fn dataset_rid(&self) -> Option<&str> {
        self.env.value("_NOMINAL_DATASET_RID")
    }
    /// Return the additional tags supplied for the job, or an empty map when absent.
    pub fn additional_tags(&self) -> &std::collections::BTreeMap<String, String> {
        &self.env.tags
    }
    /// Return the job timestamp settings as the same type used by client ingestion.
    /// Unsupported server settings return an error; absent metadata returns `None`.
    pub fn job_timestamp_metadata(&self) -> Result<Option<Timestamp>> {
        self.env
            .timestamp
            .clone()
            .map(crate::extractor::timestamp::job_timestamp)
            .transpose()
    }
}

fn video_timestamp(at: DateTime<Utc>) -> Result<VideoTimestamp> {
    if at.timestamp_subsec_nanos() >= 1_000_000_000 {
        return Err(Error::InvalidOutput(
            "video timestamp nanoseconds must be between 0 and 999999999".into(),
        ));
    }
    // Chrono's supported seconds and normalized nanoseconds fit Conjure SafeLong.
    Ok(VideoTimestamp::new(
        at.timestamp()
            .try_into()
            .expect("chrono seconds fit SafeLong"),
        i64::from(at.timestamp_subsec_nanos())
            .try_into()
            .expect("subsecond nanos fit SafeLong"),
    ))
}

/// Convert separators between components, never characters inside a filename.
fn manifest_path(path: &Path) -> Result<String> {
    path.components()
        .map(|component| {
            component.as_os_str().to_str().ok_or_else(|| {
                Error::InvalidOutput("output filename cannot be represented as UTF-8".into())
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}
fn check_extension(path: &Path, allowed: &[&str]) -> Result<()> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if allowed.iter().any(|ext| name.ends_with(ext)) {
        Ok(())
    } else {
        Err(Error::InvalidOutput(format!(
            "unsupported extension: {}",
            path.display()
        )))
    }
}
