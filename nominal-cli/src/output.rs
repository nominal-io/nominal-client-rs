use nominal::User;

const DEFAULT_MASKED_TOKEN: &str = "(empty)";

/// Mask a token for display, keeping the first and last four characters when possible.
pub fn mask_token(token: &str) -> String {
    if token.is_empty() {
        return DEFAULT_MASKED_TOKEN.to_string();
    }
    if token.len() <= 8 {
        return "*".repeat(token.len());
    }
    format!("{}...{}", &token[..4], &token[token.len() - 4..])
}

pub fn print_profile_added_success(
    profile_name: &str,
    token: &str,
    user: Option<&User>,
    config_path: &str,
    default_profile: bool,
) {
    if let Some(user) = user {
        println!("Authenticated as {}.", user.email());
    }
    println!("Profile '{profile_name}' saved to {config_path}.");
    println!("Token: {}", mask_token(token));
    if default_profile {
        println!("Set as default profile.");
    }
    println!(
        "Use this profile with `nom --profile {profile_name}` or `export NOMINAL_PROFILE={profile_name}`."
    );
}

pub fn print_validation_error(err: &nominal::ValidationError) {
    eprintln!("{err}");
    eprintln!("Failed to authenticate. See above for details.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_token_short_and_long() {
        assert_eq!(mask_token(""), "(empty)");
        assert_eq!(mask_token("abcd"), "****");
        assert_eq!(mask_token("123456789"), "1234...6789");
    }
}
