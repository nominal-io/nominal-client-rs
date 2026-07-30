use nominal::User;

use crate::validate::ValidationError;

const MANUAL_WORKSPACE_HINT: &str = "You can enter a workspace RID manually instead.";

/// Warn that workspaces could not be listed, tailoring the message to the cause.
pub fn print_workspace_fetch_warning(err: &nominal::Error) {
    match err.http_status() {
        Some(401) => {
            eprintln!(
                "Could not list workspaces: the token may be invalid. {MANUAL_WORKSPACE_HINT}"
            )
        }
        Some(403) => {
            eprintln!("Could not list workspaces: not authorized. {MANUAL_WORKSPACE_HINT}")
        }
        Some(status) => {
            eprintln!("Could not list workspaces (status {status}). {MANUAL_WORKSPACE_HINT}")
        }
        None => {
            eprintln!("Could not reach the API to list workspaces. {MANUAL_WORKSPACE_HINT}")
        }
    }
}

/// Inform the user that no workspaces are available for the account.
pub fn print_no_workspaces_found() {
    eprintln!("No workspaces found for this account. {MANUAL_WORKSPACE_HINT}");
}

pub fn print_profile_added_success(profile_name: &str, user: Option<&User>, config_path: &str) {
    if let Some(user) = user {
        println!("Authenticated as {}.", user.email());
    }
    println!("Profile '{profile_name}' saved to {config_path}.");
    println!(
        "Use this profile with `nomctl --profile {profile_name}` or `export NOMINAL_PROFILE={profile_name}`."
    );
}

pub fn print_validation_error(err: &ValidationError) {
    eprintln!("{err}");
    eprintln!("Failed to authenticate. See above for details.");
}
