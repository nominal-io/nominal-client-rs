use anyhow::{Context, bail};
use clap::{ArgAction, Subcommand};
use inquire::{Confirm, Select, Text};
use nominal::{Config, NominalClient, Profile, User, Workspace, default_config_path};

use crate::context::display_config_path;
use crate::output::{
    print_no_workspaces_found, print_profile_added_success, print_validation_error,
    print_workspace_fetch_warning,
};
use crate::validate::{AUTH_DOCS_LINK, ValidationError, is_api_unreachable, validate_profile};

const DEFAULT_BASE_URL: &str = "https://api.gov.nominal.io/api";
const MANUAL_WORKSPACE_OPTION: &str = "Enter a workspace RID manually";

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
        workspace_rid: String,
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
            } => add_profile(&name, &url, &token, Some(&workspace_rid), validate).await,
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

    save_profile(name, url, token, workspace_rid, user.as_ref())
}

/// Persist a profile to the config file and print the success message.
fn save_profile(
    name: &str,
    url: &str,
    token: &str,
    workspace_rid: Option<&str>,
    user: Option<&User>,
) -> anyhow::Result<()> {
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
    print_profile_added_success(name, user, &config_path);
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
            AUTH_DOCS_LINK
        ))
        .prompt()
        .context("Failed to read token")?;

    let token = token.trim().to_string();
    if token.is_empty() {
        bail!(
            "API token cannot be empty. See {} for how to generate one.",
            AUTH_DOCS_LINK
        );
    }

    let workspace_rid = select_workspace(&url, &token).await?;

    // Validate against the API, but stay usable when the API is unreachable:
    // offer to save without validation rather than blocking setup entirely.
    match validate_profile(&url, &token, Some(&workspace_rid)).await {
        Ok(user) => save_profile(&name, &url, &token, Some(&workspace_rid), Some(&user)),
        Err(err) if is_api_unreachable(&err) => {
            eprintln!("{err}");
            let proceed = Confirm::new("Save this profile anyway, without validation?")
                .with_default(false)
                .prompt()
                .context("Failed to read confirmation")?;
            if proceed {
                save_profile(&name, &url, &token, Some(&workspace_rid), None)
            } else {
                bail!("Aborted. Re-run `nomctl config init` once the API is reachable.");
            }
        }
        Err(err) => Err(map_validation_error(err)),
    }
}

/// Prompt for a workspace, fetching the user's accessible workspaces so they can
/// pick from a list. Always falls back to manual RID entry when the list can't
/// be fetched (network outage, invalid token) or the user prefers to type it.
async fn select_workspace(url: &str, token: &str) -> anyhow::Result<String> {
    match fetch_workspaces(url, token).await {
        Ok(workspaces) if !workspaces.is_empty() => {
            let mut options: Vec<String> = workspaces
                .iter()
                .map(|w| workspace_label(w.display_name(), w.rid()))
                .collect();
            options.push(MANUAL_WORKSPACE_OPTION.to_string());

            let selection = Select::new("Select a workspace:", options)
                .raw_prompt()
                .context("Failed to read workspace selection")?;

            if selection.index == workspaces.len() {
                prompt_workspace_rid()
            } else {
                Ok(workspaces[selection.index].rid().to_string())
            }
        }
        Ok(_) => {
            print_no_workspaces_found();
            prompt_workspace_rid()
        }
        Err(err) => {
            print_workspace_fetch_warning(&err);
            prompt_workspace_rid()
        }
    }
}

/// Display label for a workspace in the picker, e.g. `"Flight Test (ri...)"`,
/// falling back to just the RID when the workspace has no display name.
fn workspace_label(display_name: Option<&str>, rid: &str) -> String {
    match display_name {
        Some(name) => format!("{name} ({rid})"),
        None => rid.to_string(),
    }
}

/// List the workspaces reachable with the given credentials.
async fn fetch_workspaces(url: &str, token: &str) -> Result<Vec<Workspace>, nominal::Error> {
    let client = NominalClient::builder(token).base_url(url).build()?;
    client.workspaces().list_workspaces().await
}

/// Free-text workspace RID prompt with an empty-input guard.
fn prompt_workspace_rid() -> anyhow::Result<String> {
    let workspace_rid = Text::new("Workspace RID:")
        .with_help_message("Find this in the Nominal app under Settings > Workspaces")
        .prompt()
        .context("Failed to read workspace RID")?;

    let workspace_rid = workspace_rid.trim().to_string();
    if workspace_rid.is_empty() {
        bail!(
            "Workspace RID cannot be empty. Find it in the Nominal app under Settings > Workspaces."
        );
    }

    Ok(workspace_rid)
}

fn map_config_error(err: nominal::Error) -> anyhow::Error {
    anyhow::Error::new(err)
}

fn map_validation_error(err: ValidationError) -> anyhow::Error {
    print_validation_error(&err);
    anyhow::Error::new(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_url_is_gov_api() {
        assert_eq!(DEFAULT_BASE_URL, "https://api.gov.nominal.io/api");
    }

    #[test]
    fn validation_error_maps_to_anyhow() {
        let err = map_validation_error(ValidationError::InvalidToken);
        assert!(err.to_string().contains("authorization token"));
    }

    #[test]
    fn workspace_label_formats_with_and_without_display_name() {
        assert_eq!(
            workspace_label(Some("Flight Test"), "ri.security.x.workspace.abc"),
            "Flight Test (ri.security.x.workspace.abc)"
        );
        assert_eq!(
            workspace_label(None, "ri.security.x.workspace.abc"),
            "ri.security.x.workspace.abc"
        );
    }
}
