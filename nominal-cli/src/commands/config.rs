use anyhow::Context;
use clap::Subcommand;
use nominal::{Config, Profile};
use std::path::{Path, PathBuf};

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
        #[arg(short, long, required = true)]
        workspace_rid: String,
    },
    /// List profiles
    List,
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
                let mut config = Config::load_or_default().context("Failed to load config")?;
                config.add_profile(name.clone(), Profile::new(url, token, workspace_rid));
                config.save().context("Failed to save config")?;
                println!("Profile '{name}' added.");
            }
            ProfileCommands::List => {
                let config = Config::load_or_default().context("Failed to load config")?;
                let config_path = default_config_path_label()?;
                let mut profiles: Vec<_> = config.profiles().iter().collect();
                profiles.sort_by_key(|(name, _)| *name);

                if profiles.is_empty() {
                    println!("No profiles found in `{config_path}`");
                    return Ok(());
                }

                println!("Profiles from `{config_path}`:\n");
                for (profile_name, profile) in profiles {
                    print!(
                        "- {profile_name} ({base_url}",
                        base_url = profile.base_url()
                    );
                    if profile.token().is_empty() {
                        print!(", missing token");
                    }
                    print!(
                        ", workspace {workspace_rid}",
                        workspace_rid = profile.workspace_rid()
                    );
                    println!(")");
                }
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

fn default_config_path_label() -> anyhow::Result<String> {
    Ok(abbreviate_home(&Config::default_path()?))
}

fn abbreviate_home(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.display().to_string();
    };
    let home = PathBuf::from(home);
    match path.strip_prefix(&home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}
