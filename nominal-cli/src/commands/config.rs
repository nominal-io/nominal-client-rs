use anyhow::Context;
use clap::Subcommand;
use nominal::{Config, Profile};

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Profile management
    Profile {
        #[command(subcommand)]
        profile_command: ProfileCommands,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommands {
    /// Add a profile
    Add {
        name: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        token: String,
        #[arg(short, long)]
        workspace_rid: Option<String>,
    },
    /// Remove a profile
    Remove { name: String },
}

pub fn handle(cmd: ConfigCommands) -> anyhow::Result<()> {
    match cmd {
        ConfigCommands::Profile { profile_command } => match profile_command {
            ProfileCommands::Add {
                name,
                url,
                token,
                workspace_rid,
            } => {
                let mut config = Config::load().context("Failed to load config")?;
                config.add_profile(name.clone(), Profile::new(url, token, workspace_rid));
                config.save().context("Failed to save config")?;
                println!("Profile '{name}' added.");
            }
            ProfileCommands::Remove { name } => {
                let mut config = Config::load().context("Failed to load config")?;
                config
                    .remove_profile(&name)
                    .ok_or_else(|| anyhow::anyhow!("Profile '{name}' not found"))?;
                config.save().context("Failed to save config")?;
                println!("Profile '{name}' removed.");
            }
        },
    }

    Ok(())
}
