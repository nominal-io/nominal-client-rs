use crate::core::datetime::NominalDateTimeError;
use crate::core::rid::RidConversionError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not determine home directory")]
    HomeDirNotFound,

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Conjure error: {0}")]
    Conjure(String),

    #[error("RID conversion error: {0}")]
    Rid(String),

    #[error("timestamp conversion error: {0}")]
    Timestamp(String),

    #[error("invalid bearer token: {0}")]
    InvalidBearerToken(String),

    #[error("invalid service URL: {0}")]
    InvalidServiceUrl(String),

    #[error("resource not found: {0}")]
    NotFound(String),
}

impl From<RidConversionError> for Error {
    fn from(value: RidConversionError) -> Self {
        Self::Rid(value.to_string())
    }
}

impl From<NominalDateTimeError> for Error {
    fn from(value: NominalDateTimeError) -> Self {
        Self::Timestamp(value.to_string())
    }
}

impl From<conjure_error::Error> for Error {
    fn from(value: conjure_error::Error) -> Self {
        Self::Conjure(format!("{value:?}"))
    }
}
