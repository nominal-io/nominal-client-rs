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

fn load_config() -> anyhow::Result<Config> {
    Config::from_file(None).context("Failed to load config")
}

fn save_config(config: Config) -> anyhow::Result<()> {
    config.to_file(None).context("Failed to save config")
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
                let mut config = load_config()?;
                config.add_profile(name.clone(), Profile::new(url, token, workspace_rid));
                save_config(config)?;
                println!("Profile '{name}' added.");
            }
            ProfileCommands::Remove { name } => {
                let mut config = load_config()?;
                config
                    .remove_profile(&name)
                    .ok_or_else(|| anyhow::anyhow!("Profile '{name}' not found"))?;
                save_config(config)?;
                println!("Profile '{name}' removed.");
            }
        },
    }

    Ok(())
}
