use clap::{Subcommand, error::ErrorKind};
use nominal_client::{Config, Profile};
use std::path::PathBuf;

#[derive(Subcommand, Clone)]
pub enum ConfigCommands {
    /// Profile management
    Profile {
        #[command(subcommand)]
        profile_command: ProfileCommands,
    },
}

#[derive(Subcommand, Clone)]
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

pub fn handle(cmd: ConfigCommands, config_path: Option<PathBuf>) -> Result<(), clap::Error> {
    match cmd {
        ConfigCommands::Profile { profile_command } => match profile_command {
            ProfileCommands::Add {
                name,
                url,
                token,
                workspace_rid,
            } => {
                let mut config = Config::from_file(config_path.clone()).map_err(|e| {
                    super::clap_error(ErrorKind::Io, format!("Failed to load config: {e}"))
                })?;
                config.add_profile(name.clone(), Profile::new(url, token, workspace_rid));
                // TODO: Save config (not implemented yet)
                println!("Profile '{}' added.", name);
            }
            ProfileCommands::Remove { name } => {
                let mut config = Config::from_file(config_path.clone()).map_err(|e| {
                    super::clap_error(ErrorKind::Io, format!("Failed to load config: {e}"))
                })?;
                config.remove_profile(&name).ok_or_else(|| {
                    super::clap_error(
                        ErrorKind::InvalidValue,
                        format!("Profile '{}' not found", name),
                    )
                })?;
                println!("Profile '{}' removed.", name);
                // TODO: Save config (not implemented yet)
            }
        },
    }

    Ok(())
}
