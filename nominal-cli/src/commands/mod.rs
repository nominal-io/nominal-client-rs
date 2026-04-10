pub mod asset;
pub mod config;
pub mod user;

use anyhow::Context;
use nominal::{Config, NominalClient};

pub(crate) fn load_client(profile_name: &str) -> anyhow::Result<NominalClient> {
    let config = Config::from_file(None).context("Failed to load config")?;

    let profile = config
        .get_profile(profile_name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile_name}' not found"))?;

    NominalClient::from_profile_config(profile)
        .with_context(|| format!("Failed to create client for profile '{profile_name}'"))
}
