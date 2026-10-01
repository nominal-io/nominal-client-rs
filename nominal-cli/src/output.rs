use nominal::User;
use serde::Serialize;

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

pub fn emit<T: Serialize>(value: &T, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string(value)?)
    } else {
        print_human(&serde_json::to_value(value)?, 0)
    }
    Ok(())
}
fn print_human(value: &serde_json::Value, indent: usize) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                match value {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        println!("{}{}:", " ".repeat(indent), key);
                        print_human(value, indent + 2);
                    }
                    serde_json::Value::String(text) => {
                        println!("{}{}: {}", " ".repeat(indent), key, text)
                    }
                    value => println!("{}{}: {}", " ".repeat(indent), key, value),
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                print_human(value, indent);
                if value.is_object() {
                    println!();
                }
            }
        }
        serde_json::Value::String(text) => println!("{}{}", " ".repeat(indent), text),
        value => println!("{}{}", " ".repeat(indent), value),
    }
}
