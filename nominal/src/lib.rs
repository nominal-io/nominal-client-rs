pub mod config;
pub mod core;
pub mod error;
pub mod validate;

pub use config::{
    CONFIG_VERSION, Config, DeprecatedConfig, Profile, default_config_path, deprecated_config_path,
};
pub use core::{NominalClient, NominalClientBuilder, User};
pub use error::{Error, Result};
pub use nominal_streaming as streaming;
pub use validate::{AUTH_DOCS_LINK, ValidationError, validate_profile};
