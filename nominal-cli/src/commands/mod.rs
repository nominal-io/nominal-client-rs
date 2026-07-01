pub mod api;
pub mod asset;
pub mod channel;
pub mod config;
pub mod connection;
pub mod dataset;
pub mod endpoint;
#[cfg(feature = "unstable")]
pub mod fs;
pub mod grpc;
pub mod ingest;
pub mod run;
pub mod user;
pub mod video;

use anyhow::Context;
use nominal::core::{NominalClient, NominalClientBuilder};
use nominal::{Config, Error, Profile};
use once_cell::sync::OnceCell;
use prost_reflect::DescriptorPool;

use crate::context::resolve_profile;

static DESCRIPTOR_POOL: OnceCell<DescriptorPool> = OnceCell::new();

pub(crate) fn descriptor_pool() -> &'static DescriptorPool {
    DESCRIPTOR_POOL.get_or_init(|| {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/nominal_descriptor.bin"));
        DescriptorPool::decode(bytes.as_ref()).expect("failed to decode proto descriptor")
    })
}

pub(crate) fn load_profile(flag: Option<&str>) -> anyhow::Result<Profile> {
    let profile_name = resolve_profile(flag).map_err(|err| match err {
        Error::EnvVarNotSet { .. } => {
            anyhow::anyhow!("no profile specified: pass --profile or set NOMINAL_PROFILE")
        }
        other => anyhow::Error::new(other),
    })?;
    let config = Config::load().map_err(anyhow::Error::new)?;
    config
        .get_profile(&profile_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile_name}' not found"))
}

pub(crate) fn load_client(flag: Option<&str>) -> anyhow::Result<NominalClient> {
    let profile = load_profile(flag)?;
    NominalClientBuilder::from_profile_config(&profile)
        .user_agent("nominal-cli", env!("CARGO_PKG_VERSION"))
        .build()
        .context("Failed to create client")
}
