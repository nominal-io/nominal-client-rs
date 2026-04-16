pub mod api;
pub mod asset;
pub mod config;
pub mod endpoint;
pub mod grpc;
pub mod user;

use anyhow::Context;
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

fn resolve_profile(flag: Option<&str>) -> anyhow::Result<String> {
    flag.map(|p| Ok(p.to_string())).unwrap_or_else(|| std::env::var("NOMINAL_PROFILE").map_err(|_| anyhow::anyhow!("no profile specified: use --profile or set NOMINAL_PROFILE")))
}

pub(crate) fn load_profile(flag: Option<&str>) -> anyhow::Result<Profile> {
    let profile_name = resolve_profile(flag)?;
    let config = Config::from_file(None).context("Failed to load config")?;
    config
        .get_profile(&profile_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile_name}' not found"))
}

