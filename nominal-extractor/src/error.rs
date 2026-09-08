use crate::BoxError;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid environment metadata in {variable}: {source}")]
    Metadata {
        variable: String,
        source: serde_json::Error,
    },
    #[error("missing environment variable {0}")]
    MissingEnvironment(String),
    #[error("unknown or missing input {0}")]
    Input(String),
    #[error("expected exactly one input, found {0}")]
    SoleInput(usize),
    #[error("unknown parameter {0}")]
    UnknownParameter(String),
    #[error("required parameter {0} is absent")]
    MissingParameter(String),
    #[error("cannot parse parameter {name}: {message}")]
    ParseParameter { name: String, message: String },
    #[error("invalid output: {0}")]
    InvalidOutput(String),
    #[error("extractor produced no outputs")]
    EmptyOutputs,
    #[error("registered output format {0} disagrees with runner")]
    FormatMismatch(String),
    #[error("timestamp sidecar already exists: {0}")]
    SidecarCollision(std::path::PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error("extractor function failed: {source}")]
    Author { source: BoxError },
}
