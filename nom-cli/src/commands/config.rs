use nominal_client::{Config, Profile};
use clap::Subcommand;
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

pub fn handle(cmd: ConfigCommands, config_path: Option<PathBuf>) {
    match cmd {
        ConfigCommands::Profile { profile_command } => match profile_command {
            ProfileCommands::Add {
                name,
                url,
                token,
                workspace_rid,
            } => {
                let mut config =
                    Config::from_file(config_path.clone()).expect("Failed to load config");
                config.profiles.insert(
                    name.clone(),
                    Profile {
                        base_url: url,
                        token,
                        workspace_rid,
                    },
                );
                // Save config (not implemented yet)
                println!("Profile '{}' added.", name);
            }
            ProfileCommands::Remove { name } => {
                let mut config =
                    Config::from_file(config_path.clone()).expect("Failed to load config");
                config.profiles.remove(&name);
                // Save config (not implemented yet)
                println!("Profile '{}' removed.", name);
            }
        },
    }
}
