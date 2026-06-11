use anyhow::{Context, bail};
use clap::{ArgAction, Subcommand};
use inquire::{Confirm, Text};
use nominal::{Config, Error, Profile, default_config_path, validate_profile};

use crate::context::display_config_path;
use crate::output::{print_profile_added_success, print_validation_error};

const DEFAULT_BASE_URL: &str = "https://api.gov.nominal.io/api";

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Interactive first-run setup
    Init,
    /// Profile management
    Profile {
        #[command(subcommand)]
        profile_command: ProfileCommands,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommands {
    /// Add or update a profile
    Add {
        name: String,
        #[arg(short, long, alias = "base-url")]
        url: String,
        #[arg(short, long)]
        token: String,
        #[arg(short, long)]
        workspace_rid: Option<String>,
        /// Skip the authentication check that runs before saving. Useful in
        /// CI or air-gapped environments where the API is unreachable.
        #[arg(long = "no-validate", action = ArgAction::SetFalse)]
        validate: bool,
    },
    /// Remove a profile
    Remove { name: String },
    /// List configured profiles
    List,
    /// Show details for one profile
    Show { name: String },
}

pub async fn handle(cmd: ConfigCommands) -> anyhow::Result<()> {
    match cmd {
        ConfigCommands::Init => handle_init().await,
        ConfigCommands::Profile { profile_command } => match profile_command {
            ProfileCommands::Add {
                name,
                url,
                token,
                workspace_rid,
                validate,
            } => add_profile(&name, &url, &token, workspace_rid.as_deref(), validate).await,
            ProfileCommands::Remove { name } => remove_profile(&name),
            ProfileCommands::List => list_profiles(),
            ProfileCommands::Show { name } => show_profile(&name),
        },
    }
}

async fn add_profile(
    name: &str,
    url: &str,
    token: &str,
    workspace_rid: Option<&str>,
    validate: bool,
) -> anyhow::Result<()> {
    let user = if validate {
        Some(
            validate_profile(url, token, workspace_rid)
                .await
                .map_err(map_validation_error)?,
        )
    } else {
        None
    };

    let mut config = Config::load_or_default().context("Failed to load config")?;
    config.add_profile(
        name.to_string(),
        Profile::new(
            url.to_string(),
            token.to_string(),
            workspace_rid.map(ToString::to_string),
        ),
    );
    config.save().context("Failed to save config")?;

    let config_path = display_config_path(&default_config_path()?);
    print_profile_added_success(name, user.as_ref(), &config_path);
    Ok(())
}

fn remove_profile(name: &str) -> anyhow::Result<()> {
    let mut config = Config::load().map_err(map_config_error)?;
    config
        .remove_profile(name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{name}' not found"))?;
    config.save().context("Failed to save config")?;
    println!("Profile '{name}' removed.");
    Ok(())
}

fn list_profiles() -> anyhow::Result<()> {
    let config = Config::load_or_default().context("Failed to load config")?;
    let config_path = display_config_path(&default_config_path()?);

    if config.profiles().is_empty() {
        println!("No profiles found in `{config_path}`");
        return Ok(());
    }

    println!("Profiles from `{config_path}`:\n");
    for (profile_name, profile) in config.profiles() {
        let mut tags: Vec<&str> = Vec::new();
        if profile.workspace_rid().is_some() {
            tags.push("workspace");
        }
        if profile.token().is_empty() {
            tags.push("missing token");
        }

        if tags.is_empty() {
            println!("  {profile_name}  {}", profile.base_url());
        } else {
            println!(
                "  {profile_name}  {}  [{}]",
                profile.base_url(),
                tags.join(", ")
            );
        }
    }

    Ok(())
}

fn show_profile(name: &str) -> anyhow::Result<()> {
    let config = Config::load().map_err(map_config_error)?;
    let profile = config
        .get_profile(name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{name}' not found"))?;

    let config_path = display_config_path(&default_config_path()?);
    println!("Profile '{name}' from `{config_path}`:");
    println!("  base_url: {}", profile.base_url());

    if let Some(workspace_rid) = profile.workspace_rid() {
        println!("  workspace_rid: {workspace_rid}");
    }

    Ok(())
}

async fn handle_init() -> anyhow::Result<()> {
    let name = Text::new("Profile name:")
        .with_help_message("A short name for this profile connection")
        .prompt()
        .context("Failed to read profile name")?;

    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("Profile name cannot be empty");
    }

    let url = Text::new("API base URL:")
        .with_default(DEFAULT_BASE_URL)
        .with_help_message("Press Enter to use the default, or paste your organization's API URL")
        .prompt()
        .context("Failed to read base URL")?;

    let token = Text::new("API token or bearer token:")
        .with_help_message(&format!(
            "See {} for instructions to generate a token",
            nominal::AUTH_DOCS_LINK
        ))
        .prompt()
        .context("Failed to read token")?;

    let workspace_rid = if Confirm::new("Add a workspace RID?:")
        .with_default(false)
        .with_help_message("Only needed if your organization uses multiple workspaces")
        .prompt()
        .context("Failed to read workspace prompt")?
    {
        Some(
            Text::new("Workspace RID:")
                .with_help_message("Find this in the Nominal app under Settings > Workspaces")
                .prompt()
                .context("Failed to read workspace RID")?,
        )
    } else {
        None
    };

    add_profile(&name, &url, &token, workspace_rid.as_deref(), true).await
}

fn map_config_error(err: Error) -> anyhow::Error {
    anyhow::Error::new(err)
}

fn map_validation_error(err: Error) -> anyhow::Error {
    if let Error::Validation(validation) = err {
        print_validation_error(&validation);
        anyhow::Error::new(validation)
    } else {
        anyhow::Error::new(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nominal::ValidationError;

    #[test]
    fn default_base_url_is_gov_api() {
        assert_eq!(DEFAULT_BASE_URL, "https://api.gov.nominal.io/api");
    }

    #[test]
    fn validation_error_maps_to_anyhow() {
        let err = map_validation_error(Error::Validation(ValidationError::InvalidToken));
        assert!(err.to_string().contains("authorization token"));
    }
}
