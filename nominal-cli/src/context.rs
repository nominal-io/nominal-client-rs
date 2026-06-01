use std::path::Path;

use nominal::{Config, Error};

/// Resolve the active profile name from CLI flag, environment, and config defaults.
pub fn resolve_profile(flag: Option<&str>, config: &Config) -> Result<String, Error> {
    if let Some(name) = flag {
        return Ok(name.to_string());
    }

    if let Ok(name) = std::env::var("NOMINAL_PROFILE") {
        return Ok(name);
    }

    if let Some(name) = config.default_profile() {
        return Ok(name.to_string());
    }

    Err(Error::EnvVarNotSet {
        name: "NOMINAL_PROFILE",
    })
}

pub fn display_config_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if path.starts_with(&home) {
            return format!("~/{}", path.strip_prefix(&home).unwrap().display());
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nominal::Config;

    #[test]
    fn flag_takes_precedence() {
        let config = Config::empty();
        let resolved = resolve_profile(Some("flag"), &config).unwrap();
        assert_eq!(resolved, "flag");
    }

    #[test]
    fn env_var_used_when_flag_missing() {
        let config = Config::empty();
        temp_env::with_var("NOMINAL_PROFILE", Some("env-profile"), || {
            let resolved = resolve_profile(None, &config).unwrap();
            assert_eq!(resolved, "env-profile");
        });
    }

    #[test]
    fn default_profile_used_when_flag_and_env_missing() {
        let mut config = Config::empty();
        config.set_default_profile(Some("default".to_string()));
        temp_env::with_var("NOMINAL_PROFILE", None::<&str>, || {
            let resolved = resolve_profile(None, &config).unwrap();
            assert_eq!(resolved, "default");
        });
    }

    #[test]
    fn errors_when_no_profile_source() {
        let config = Config::empty();
        temp_env::with_var("NOMINAL_PROFILE", None::<&str>, || {
            let err = resolve_profile(None, &config).unwrap_err();
            assert!(matches!(err, Error::EnvVarNotSet { .. }));
        });
    }

    #[test]
    fn display_config_path_rewrites_home() {
        let home = dirs::home_dir().expect("home");
        let path = home.join(".config/nominal/config.yml");
        assert_eq!(display_config_path(&path), "~/.config/nominal/config.yml");
    }
}
