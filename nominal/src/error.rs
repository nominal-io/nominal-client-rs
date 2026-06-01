use crate::core::datetime::NominalDateTimeError;
use crate::core::rid::RidConversionError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

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
    Conjure { details: String },

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
        "no config file found at {path}: create with `nom config profile add` or `nom config init`"
    )]
    ConfigNotFound { path: String },

    #[error(
        "no config file found at {path}: deprecated config file {deprecated_path} found. migrate with `nom config migrate`"
    )]
    DeprecatedConfigFound {
        path: String,
        deprecated_path: String,
    },

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
        Self::Conjure {
            details: format!("{value:?}"),
        }
    }
}
