#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod config;
pub mod core;
pub mod error;
#[cfg(feature = "smartcard")]
#[cfg_attr(docsrs, doc(cfg(feature = "smartcard")))]
pub mod smartcard;

pub use config::{Config, Profile};
pub use core::{NominalClient, NominalClientBuilder};
pub use error::{Error, Result};
pub use nominal_streaming as streaming;
pub use rustls::client::ResolvesClientCert;
