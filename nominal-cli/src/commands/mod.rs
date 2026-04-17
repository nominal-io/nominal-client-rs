pub mod api;
pub mod asset;
pub mod config;
pub mod connection;
pub mod dataset;
pub mod endpoint;
pub mod grpc;
pub mod ingest;
pub mod user;
pub mod video;

use anyhow::Context;
use nominal::core::NominalClient;
use nominal::{Config, Profile};
use once_cell::sync::OnceCell;
use prost_reflect::DescriptorPool;

static DESCRIPTOR_POOL: OnceCell<DescriptorPool> = OnceCell::new();

pub(crate) fn descriptor_pool() -> &'static DescriptorPool {
    DESCRIPTOR_POOL.get_or_init(|| {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/nominal_descriptor.bin"));
        DescriptorPool::decode(bytes.as_ref()).expect("failed to decode proto descriptor")
    })
}

pub(crate) fn load_profile(profile_name: &str) -> anyhow::Result<Profile> {
    let config = Config::from_file(None).context("Failed to load config")?;
    config
        .get_profile(profile_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile_name}' not found"))
}

pub(crate) fn load_client(profile_name: &str) -> anyhow::Result<NominalClient> {
    let profile = load_profile(profile_name)?;
    NominalClient::from_profile_config(&profile)
        .with_context(|| format!("Failed to create client for profile '{profile_name}'"))
}
