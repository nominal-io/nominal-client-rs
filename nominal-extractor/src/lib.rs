//! Experimental, synchronous runtime for containerized Nominal extractors.
//! No credentials, network client, Docker, or async executor are required.
mod environment;
mod error;
mod manifest;
mod paths;
mod runner;
mod single;
mod timestamp;
pub use error::Error;
pub use manifest::*;
pub use runner::*;
pub use single::SingleFileContext;
pub use timestamp::{
    AbsoluteTimestampType, CustomTimestampFormat, EpochTimeUnit, JobTimestampMetadata,
    JobTimestampType, NumericTimeUnit, NumericTimestamp, RelativeTimestamp,
};
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T> = std::result::Result<T, Error>;
pub type ExtractResult = std::result::Result<(), BoxError>;
