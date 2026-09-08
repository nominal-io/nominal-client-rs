use super::{ingest_proto, proto};
use crate::core::{Timestamp, WaitOptions};
use crate::{Error, Result};
use chrono::{DateTime, Utc};

#[derive(Debug, thiserror::Error)]
pub enum ExtractorError {
    #[error("invalid extractor registration: {0}")]
    InvalidRegistration(String),
    #[error("extractor and image must belong to the same workspace and extractor")]
    ImageMismatch,
    #[error("image {rid} is not ready ({status:?})")]
    NotReady {
        rid: String,
        status: ContainerImageStatus,
    },
    #[error("image {rid} failed processing")]
    ImageFailed { rid: String },
    #[error("timed out waiting for image {rid}")]
    Timeout { rid: String },
    #[error("image registration failed after upload to {object_path}: {source}")]
    Registration {
        object_path: String,
        #[source]
        source: Box<Error>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerImageStatus {
    Pending,
    Ready,
    Failed,
    Unknown(i32),
}
impl ContainerImageStatus {
    pub(crate) fn from_proto(v: i32) -> Self {
        match v {
            1 => Self::Pending,
            2 => Self::Ready,
            3 => Self::Failed,
            v => Self::Unknown(v),
        }
    }
    pub(crate) fn into_proto(self) -> i32 {
        match self {
            Self::Pending => 1,
            Self::Ready => 2,
            Self::Failed => 3,
            Self::Unknown(v) => v,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutputFormat {
    Parquet,
    Csv,
    ParquetTar,
    AvroStream,
    JsonL,
    Manifest,
    Unknown(i32),
}
impl FileOutputFormat {
    fn from_proto(v: i32) -> Self {
        match v {
            1 => Self::Parquet,
            2 => Self::Csv,
            3 => Self::ParquetTar,
            4 => Self::AvroStream,
            5 => Self::JsonL,
            6 => Self::Manifest,
            v => Self::Unknown(v),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterableOutputFormat {
    Parquet,
    Csv,
    AvroStream,
    Manifest,
}
impl RegisterableOutputFormat {
    pub(crate) fn into_proto(self) -> i32 {
        match self {
            Self::Parquet => 1,
            Self::Csv => 2,
            Self::AvroStream => 4,
            Self::Manifest => 6,
        }
    }
}
#[derive(Debug, Clone)]
pub enum Activation {
    RequireReady,
    Wait(WaitOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExtractionInput {
    name: String,
    environment_variable: String,
    description: Option<String>,
    suffixes: Vec<String>,
    required: Option<bool>,
}
impl FileExtractionInput {
    pub fn new(name: impl Into<String>, env: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            environment_variable: env.into(),
            description: None,
            suffixes: vec![],
            required: None,
        }
    }
    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = Some(v.into());
        self
    }
    pub fn suffix(mut self, v: impl Into<String>) -> Self {
        self.suffixes.push(v.into());
        self
    }
    pub fn required(mut self, v: bool) -> Self {
        self.required = Some(v);
        self
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn environment_variable(&self) -> &str {
        &self.environment_variable
    }
    pub fn description_text(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn suffixes(&self) -> &[String] {
        &self.suffixes
    }
    pub fn is_required(&self) -> bool {
        self.required.unwrap_or(false)
    }
    pub(crate) fn into_proto(self) -> proto::FileExtractionInput {
        proto::FileExtractionInput {
            name: self.name,
            environment_variable: self.environment_variable,
            description: self.description,
            required: self.required,
            file_filters: self
                .suffixes
                .into_iter()
                .map(|suffix| proto::FileFilter {
                    filter: Some(proto::file_filter::Filter::Suffix(proto::FileSuffix {
                        suffix,
                    })),
                })
                .collect(),
        }
    }
    pub(crate) fn from_proto(v: proto::FileExtractionInput) -> Result<Self> {
        Ok(Self {
            name: v.name,
            environment_variable: v.environment_variable,
            description: v.description,
            required: v.required,
            suffixes: v
                .file_filters
                .into_iter()
                .map(|f| match f.filter {
                    Some(proto::file_filter::Filter::Suffix(s)) => Ok(s.suffix),
                    None => Err(Error::UnexpectedResponse {
                        field: "file_filter.filter",
                    }),
                })
                .collect::<Result<_>>()?,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExtractionParameter {
    name: String,
    environment_variable: String,
    description: Option<String>,
    required: Option<bool>,
}
impl FileExtractionParameter {
    pub fn new(name: impl Into<String>, env: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            environment_variable: env.into(),
            description: None,
            required: None,
        }
    }
    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = Some(v.into());
        self
    }
    pub fn required(mut self, v: bool) -> Self {
        self.required = Some(v);
        self
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn environment_variable(&self) -> &str {
        &self.environment_variable
    }
    pub fn description_text(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn is_required(&self) -> bool {
        self.required.unwrap_or(false)
    }
    pub(crate) fn into_proto(self) -> proto::FileExtractionParameter {
        proto::FileExtractionParameter {
            name: self.name,
            environment_variable: self.environment_variable,
            description: self.description,
            required: self.required,
        }
    }
    pub(crate) fn from_proto(v: proto::FileExtractionParameter) -> Self {
        Self {
            name: v.name,
            environment_variable: v.environment_variable,
            description: v.description,
            required: v.required,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ContainerImage {
    rid: String,
    workspace_rid: String,
    extractor_rid: String,
    tag: String,
    size_bytes: Option<i64>,
    created_at: Option<DateTime<Utc>>,
    status: ContainerImageStatus,
    inputs: Vec<FileExtractionInput>,
    parameters: Vec<FileExtractionParameter>,
    format: FileOutputFormat,
    timestamp: Option<Timestamp>,
}
impl ContainerImage {
    pub fn rid(&self) -> &str {
        &self.rid
    }
    pub fn workspace_rid(&self) -> &str {
        &self.workspace_rid
    }
    pub fn extractor_rid(&self) -> &str {
        &self.extractor_rid
    }
    pub fn tag(&self) -> &str {
        &self.tag
    }
    pub fn size_bytes(&self) -> Option<i64> {
        self.size_bytes
    }
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }
    pub fn status(&self) -> ContainerImageStatus {
        self.status
    }
    pub fn inputs(&self) -> &[FileExtractionInput] {
        &self.inputs
    }
    pub fn parameters(&self) -> &[FileExtractionParameter] {
        &self.parameters
    }
    pub fn file_output_format(&self) -> FileOutputFormat {
        self.format
    }
    pub fn output_format(&self) -> FileOutputFormat {
        self.format
    }
    pub fn default_timestamp(&self) -> Option<&Timestamp> {
        self.timestamp.as_ref()
    }
    pub(crate) fn from_proto(v: proto::ContainerImage, workspace_rid: String) -> Result<Self> {
        Ok(Self {
            rid: v.rid,
            workspace_rid,
            extractor_rid: v.extractor_rid,
            tag: v.tag,
            size_bytes: v.size_bytes,
            created_at: v
                .created_at
                .map(|t| {
                    DateTime::from_timestamp(t.seconds, t.nanos as u32).ok_or(
                        Error::UnexpectedResponse {
                            field: "image.created_at",
                        },
                    )
                })
                .transpose()?,
            status: ContainerImageStatus::from_proto(v.status),
            inputs: v
                .inputs
                .into_iter()
                .map(FileExtractionInput::from_proto)
                .collect::<Result<_>>()?,
            parameters: v
                .parameters
                .into_iter()
                .map(FileExtractionParameter::from_proto)
                .collect(),
            format: FileOutputFormat::from_proto(v.file_output_format),
            timestamp: v
                .default_timestamp_metadata
                .map(Timestamp::from_registry_proto)
                .transpose()?,
        })
    }
}
#[derive(Debug, Clone)]
pub struct ContainerizedExtractor {
    rid: String,
    workspace_rid: String,
    name: String,
    description: Option<String>,
    archived: bool,
    created_at: Option<DateTime<Utc>>,
    active_image: Option<ContainerImage>,
}
impl ContainerizedExtractor {
    pub fn rid(&self) -> &str {
        &self.rid
    }
    pub fn workspace_rid(&self) -> &str {
        &self.workspace_rid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn is_archived(&self) -> bool {
        self.archived
    }
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }
    pub fn active_container_image(&self) -> Option<&ContainerImage> {
        self.active_image.as_ref()
    }
    pub(crate) fn from_proto(v: ingest_proto::ContainerizedExtractor) -> Result<Self> {
        Ok(Self {
            rid: v.rid,
            active_image: v
                .active_container_image
                .map(|i| ContainerImage::from_proto(i, v.workspace_rid.clone()))
                .transpose()?,
            workspace_rid: v.workspace_rid,
            name: v.name,
            description: v.description,
            archived: v.is_archived,
            created_at: v
                .created_at
                .map(|t| {
                    DateTime::from_timestamp(t.seconds, t.nanos as u32).ok_or(
                        Error::UnexpectedResponse {
                            field: "extractor.created_at",
                        },
                    )
                })
                .transpose()?,
        })
    }
}
#[derive(Debug, Clone)]
pub struct ExtractorCreate {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}
impl ExtractorCreate {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }
    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = Some(v.into());
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct ExtractorUpdate {
    name: Option<String>,
    description: Option<String>,
    archived: Option<bool>,
}
impl ExtractorUpdate {
    pub fn name(mut self, v: impl Into<String>) -> Self {
        self.name = Some(v.into());
        self
    }
    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = Some(v.into());
        self
    }
    pub fn archived(mut self, v: bool) -> Self {
        self.archived = Some(v);
        self
    }
    pub(crate) fn into_request(
        self,
        rid: &str,
        workspace: &str,
    ) -> ingest_proto::UpdateContainerizedExtractorRequest {
        ingest_proto::UpdateContainerizedExtractorRequest {
            rid: rid.into(),
            workspace_rid: workspace.into(),
            name: self.name,
            description: self.description,
            is_archived: self.archived,
            active_container_image_rid: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ImageRegistration {
    pub(crate) tag: String,
    pub(crate) format: RegisterableOutputFormat,
    pub(crate) timestamp: Timestamp,
    pub(crate) inputs: Vec<FileExtractionInput>,
    pub(crate) parameters: Vec<FileExtractionParameter>,
}
impl ImageRegistration {
    pub fn new(
        tag: impl Into<String>,
        format: RegisterableOutputFormat,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            tag: tag.into(),
            format,
            timestamp,
            inputs: vec![],
            parameters: vec![],
        }
    }
    pub fn input(mut self, v: FileExtractionInput) -> Self {
        self.inputs.push(v);
        self
    }
    pub fn parameter(mut self, v: FileExtractionParameter) -> Self {
        self.parameters.push(v);
        self
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.timestamp.to_registry_proto().series_name.is_empty()
            || self.inputs.is_empty()
            || self
                .inputs
                .iter()
                .any(|i| i.name.is_empty() || i.environment_variable.is_empty())
            || self
                .parameters
                .iter()
                .any(|p| p.name.is_empty() || p.environment_variable.is_empty())
            || self.tag.is_empty()
        {
            return Err(ExtractorError::InvalidRegistration(
                "tag and input contract must be nonempty".into(),
            )
            .into());
        }
        Ok(())
    }
}
