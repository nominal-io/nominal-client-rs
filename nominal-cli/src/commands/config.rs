use anyhow::{Context, bail};
use clap::Subcommand;
use inquire::{Confirm, Text};
use nominal::{Config, DeprecatedConfig, Error, Profile, default_config_path, validate_profile};

use crate::context::display_config_path;
use crate::output::{print_profile_added_success, print_validation_error};

const DEFAULT_BASE_URL: &str = "https://api.gov.nominal.io/api";

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Interactive first-run setup
    Init,
    /// Migrate deprecated config at ~/.nominal.yml
    Migrate,
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
        /// Validate authentication parameters before saving
        #[arg(long, default_value_t = true)]
        validate: bool,
        /// Skip validation before saving
        #[arg(long, default_value_t = false)]
        no_validate: bool,
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
        ConfigCommands::Migrate => handle_migrate().await,
        ConfigCommands::Profile { profile_command } => match profile_command {
            ProfileCommands::Add {
                name,
                url,
                token,
                workspace_rid,
                validate,
                no_validate,
            } => {
                add_profile(
                    &name,
                    &url,
                    &token,
                    workspace_rid.as_deref(),
                    validate && !no_validate,
                )
                .await
            }
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
    let set_default = config.profiles().is_empty() && config.default_profile().is_none();
    config.add_profile(
        name.to_string(),
        Profile::new(
            url.to_string(),
            token.to_string(),
            workspace_rid.map(ToString::to_string),
        ),
    );
    if set_default {
        config.set_default_profile(Some(name.to_string()));
    }
    config.save().context("Failed to save config")?;

    let config_path = display_config_path(&default_config_path()?);
    print_profile_added_success(name, token, user.as_ref(), &config_path, set_default);
    Ok(())
}

fn remove_profile(name: &str) -> anyhow::Result<()> {
    let mut config = Config::load().map_err(map_config_error)?;
    config
        .remove_profile(name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{name}' not found"))?;
    if config.default_profile() == Some(name) {
        config.set_default_profile(None);
    }
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
        print!("- {profile_name} ({})", profile.base_url());
        if profile.token().is_empty() {
            print!(", missing token");
        }
        if profile.workspace_rid().is_some() {
            print!(", in workspace");
        }
        if config.default_profile() == Some(profile_name.as_str()) {
            print!(", default");
        }
        println!(")");
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
    println!("  token: {}", crate::output::mask_token(profile.token()));
    if let Some(workspace_rid) = profile.workspace_rid() {
        println!("  workspace_rid: {workspace_rid}");
    }
    if config.default_profile() == Some(name) {
        println!("  default: true");
    }

    Ok(())
}

async fn handle_init() -> anyhow::Result<()> {
    let name = Text::new("Profile name")
        .with_help_message("Used with --profile or NOMINAL_PROFILE")
        .prompt()
        .context("Failed to read profile name")?;

    let url = Text::new("API base URL")
        .with_default(DEFAULT_BASE_URL)
        .prompt()
        .context("Failed to read base URL")?;

    let token = Text::new("API token or bearer token")
        .with_help_message(&format!("See {} for instructions", nominal::AUTH_DOCS_LINK))
        .prompt()
        .context("Failed to read token")?;

    let workspace_rid = if Confirm::new("Add a workspace RID?")
        .with_default(false)
        .prompt()
        .context("Failed to read workspace prompt")?
    {
        Some(
            Text::new("Workspace RID")
                .prompt()
                .context("Failed to read workspace RID")?,
        )
    } else {
        None
    };

    add_profile(&name, &url, &token, workspace_rid.as_deref(), true).await
}

async fn handle_migrate() -> anyhow::Result<()> {
    let deprecated = DeprecatedConfig::load()
        .map_err(|err| anyhow::anyhow!("Failed to load deprecated config: {err:#}"))?;

    if deprecated.environments().is_empty() {
        bail!("Deprecated config contains no environments to migrate");
    }

    let mut profiles = Config::load_or_default().context("Failed to load config")?;
    let mut migrated = 0usize;

    for (host, token) in deprecated.environments() {
        let add = Confirm::new(&format!("Add profile for {host}?"))
            .with_default(true)
            .prompt()
            .context("Failed to read migration prompt")?;
        if !add {
            continue;
        }

        let default_url = if host.starts_with("http") {
            host.clone()
        } else {
            format!("https://{host}")
        };

        let name = Text::new("Profile name")
            .with_help_message("Used with --profile or NOMINAL_PROFILE")
            .prompt()
            .context("Failed to read profile name")?;

        let url = Text::new("API base URL")
            .with_default(&default_url)
            .prompt()
            .context("Failed to read base URL")?;

        let new_token = Text::new("Token")
            .with_default(token)
            .prompt()
            .context("Failed to read token")?;

        let workspace_rid = if Confirm::new("Add workspace RID?")
            .with_default(false)
            .prompt()
            .context("Failed to read workspace prompt")?
        {
            Some(
                Text::new("Workspace RID")
                    .prompt()
                    .context("Failed to read workspace RID")?,
            )
        } else {
            None
        };

        let validate = Confirm::new("Validate authentication?")
            .with_default(true)
            .prompt()
            .context("Failed to read validation prompt")?;

        if validate {
            validate_profile(&url, &new_token, workspace_rid.as_deref())
                .await
                .map_err(map_validation_error)?;
        }

        profiles.add_profile(
            name.clone(),
            Profile::new(url, new_token, workspace_rid.map(|value| value.to_string())),
        );
        if profiles.default_profile().is_none() {
            profiles.set_default_profile(Some(name));
        }
        migrated += 1;
    }

    if migrated == 0 {
        bail!("No profiles migrated");
    }

    profiles.save().context("Failed to save config")?;
    let config_path = display_config_path(&default_config_path()?);
    println!("Migrated {migrated} profile(s) to {config_path}.");
    Ok(())
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
