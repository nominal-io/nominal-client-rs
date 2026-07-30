use crate::core::datetime::NominalDateTimeError;
use crate::core::rid::RidConversionError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(feature = "drives")]
#[derive(Debug, Error)]
#[error("transport error")]
pub struct TransportError(#[source] Box<dyn std::error::Error + Send + Sync + 'static>);

#[cfg(feature = "drives")]
impl TransportError {
    pub(crate) fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Box::new(error))
    }
}

#[cfg(feature = "drives")]
impl TryFrom<tonic::Status> for TransportError {
    type Error = tonic::Status;

    fn try_from(status: tonic::Status) -> std::result::Result<Self, Self::Error> {
        match status.code() {
            tonic::Code::Cancelled
            | tonic::Code::Unknown
            | tonic::Code::DeadlineExceeded
            | tonic::Code::Internal
            | tonic::Code::Unavailable
            | tonic::Code::DataLoss => Ok(Self::new(status)),
            _ => Err(status),
        }
    }
}

#[cfg(feature = "drives")]
#[derive(Debug, Error)]
#[error("unexpected error")]
pub struct UnexpectedError(#[source] Box<dyn std::error::Error + Send + Sync + 'static>);

#[cfg(feature = "drives")]
impl UnexpectedError {
    fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Box::new(error))
    }
}

#[cfg(feature = "drives")]
#[derive(Debug, Error)]
pub enum FileStoreError {
    #[error("server response missing required field: {field}")]
    MissingResponseField { field: &'static str },

    #[error("file store operation failed ({code}): {message}")]
    ChangeFailed { code: String, message: String },
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not determine home directory")]
    HomeDirNotFound,

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Conjure error: {details}")]
    Conjure {
        details: String,
        status: Option<u16>,
    },

    #[error("Workspace not provided, but there is no default workspace for the user.")]
    NoDefaultWorkspace,

    #[error("RID conversion error: invalid RID '{rid}': {reason}")]
    Rid { rid: String, reason: String },

    #[error("seconds_since_epoch out of range: {0}")]
    TimestampSecondsOutOfRange(i64),

    #[error("offset_nanoseconds out of range: {0}")]
    TimestampNanosOutOfRange(i64),

    #[error("invalid timestamp: seconds={seconds}, nanos={nanos}")]
    InvalidTimestamp { seconds: i64, nanos: i64 },

    #[error("invalid bearer token: {reason}")]
    InvalidBearerToken { reason: String },

    #[error("invalid service URL '{url}': {reason}")]
    InvalidServiceUrl { url: String, reason: String },

    #[error("profile '{name}' not found in config")]
    ProfileNotFound { name: String },

    #[error(
        "no config file found at {path}: create with `nomctl config profile add` or `nomctl config init`"
    )]
    ConfigNotFound { path: String },

    #[error("missing 'version' key in config file: {path}")]
    ConfigMissingVersion { path: String },

    #[error("unsupported config version: {version} (expected 2)")]
    ConfigUnsupportedVersion { version: u32, path: String },

    #[error("environment variable '{name}' is not set")]
    EnvVarNotSet { name: &'static str },

    #[error("resource not found: {resource}")]
    NotFound { resource: &'static str },

    #[error("channel data type missing from server response for channel '{channel}'")]
    MissingChannelDataType { channel: String },

    #[error("unsupported channel data type for metadata upsert: {data_type}")]
    UnsupportedChannelDataType { data_type: String },

    #[error("multipart upload failed: {details}")]
    Upload { details: String },

    #[error("ingest error: {details}")]
    Ingest { details: String },

    #[cfg(feature = "drives")]
    #[error(transparent)]
    Transport(#[from] TransportError),

    #[cfg(feature = "drives")]
    #[error(transparent)]
    Unexpected(UnexpectedError),

    #[cfg(feature = "drives")]
    #[error(
        "workspace RID required for this operation: set workspace_rid on the profile or client builder"
    )]
    WorkspaceRequired,

    #[cfg(feature = "drives")]
    #[error(transparent)]
    FileStore(#[from] FileStoreError),
}

#[cfg(feature = "drives")]
impl From<tonic::Status> for Error {
    fn from(status: tonic::Status) -> Self {
        match TransportError::try_from(status) {
            Ok(error) => Self::Transport(error),
            Err(status) => Self::Unexpected(UnexpectedError::new(status)),
        }
    }
}

impl From<RidConversionError> for Error {
    fn from(value: RidConversionError) -> Self {
        Self::Rid {
            rid: value.rid().to_string(),
            reason: value.reason().to_string(),
        }
    }
}

impl From<NominalDateTimeError> for Error {
    fn from(value: NominalDateTimeError) -> Self {
        match value {
            NominalDateTimeError::SecondsOutOfRange(v) => Self::TimestampSecondsOutOfRange(v),
            NominalDateTimeError::NanosOutOfRange(v) => Self::TimestampNanosOutOfRange(v),
            NominalDateTimeError::InvalidTimestamp { seconds, nanos } => {
                Self::InvalidTimestamp { seconds, nanos }
            }
        }
    }
}

impl From<conjure_error::Error> for Error {
    fn from(value: conjure_error::Error) -> Self {
        let status = value
            .cause()
            .downcast_ref::<conjure_runtime::errors::RemoteError>()
            .map(|remote| remote.status().as_u16());
        Self::Conjure {
            details: format!("{value:?}"),
            status,
        }
    }
}

impl Error {
    pub fn http_status(&self) -> Option<u16> {
        match self {
            Self::Conjure { status, .. } => *status,
            _ => None,
        }
    }
}

#[cfg(all(test, feature = "drives"))]
mod tests {
    use super::*;

    #[test]
    fn classifies_transient_grpc_statuses_as_transport_errors() {
        assert!(matches!(
            Error::from(tonic::Status::unavailable("network unavailable")),
            Error::Transport(_)
        ));
    }

    #[test]
    fn classifies_non_transport_grpc_statuses_as_unexpected_errors() {
        assert!(matches!(
            Error::from(tonic::Status::not_found("missing")),
            Error::Unexpected(_)
        ));
    }
}
