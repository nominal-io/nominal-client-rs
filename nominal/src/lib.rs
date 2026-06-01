pub mod config;
pub mod core;
pub mod error;

pub use config::{
    CONFIG_VERSION, Config, DeprecatedConfig, Profile, default_config_path, deprecated_config_path,
};
pub use core::{NominalClient, NominalClientBuilder};
pub use error::{Error, Result};
pub use nominal_streaming as streaming;
