/// Failures while reading the container environment or declaring and writing outputs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Job timestamp metadata uses an encoding or unit this client does not support.
    #[error("unsupported job timestamp metadata: {0}")]
    UnsupportedJobTimestamp(String),
    /// Registered metadata could not be decoded; the callback has not run.
    #[error("invalid environment metadata in {variable}: {source}")]
    Metadata {
        /// Environment key containing invalid metadata.
        variable: String,
        /// Underlying JSON decoding error.
        source: serde_json::Error,
    },
    /// A required runtime environment value, such as `OUTPUT_DIR`, is absent.
    #[error("missing environment variable {0}")]
    MissingEnvironment(String),
    /// The requested input name is unregistered or has no local environment value.
    #[error("unknown or missing input {0}")]
    Input(String),
    /// `sole_input` found a different number of inputs; contains the actual count.
    #[error("expected exactly one input, found {0}")]
    SoleInput(usize),
    /// A parameter name does not appear in supplied registration metadata.
    #[error("unknown parameter {0}")]
    UnknownParameter(String),
    /// A parameter requested with `param` has no value.
    #[error("required parameter {0} is absent")]
    MissingParameter(String),
    /// A parameter value cannot be parsed into the requested Rust type.
    #[error("cannot parse parameter {name}: {message}")]
    ParseParameter {
        /// Requested parameter name.
        name: String,
        /// Error reported by the value parser.
        message: String,
    },
    /// An output path, extension, or format-specific setting is invalid.
    #[error("invalid output: {0}")]
    InvalidOutput(String),
    /// The callback succeeded without declaring any outputs.
    #[error("extractor produced no outputs")]
    EmptyOutputs,
    /// The registered output format is incompatible with manifest authoring.
    #[error("expected manifest output format, got {0}")]
    FormatMismatch(String),
    /// A generated video timestamp path already exists and was not overwritten.
    #[error("timestamp sidecar already exists: {0}")]
    SidecarCollision(std::path::PathBuf),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Output metadata could not be serialized as JSON.
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    /// The callback returned an error; the runner did not publish a new manifest.
    #[error("extractor function failed: {source}")]
    Author {
        /// Original error returned by author code.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
