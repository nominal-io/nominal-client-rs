use std::path::Path;

use nominal::Error;

/// Resolve the active profile name from CLI flag or environment variable.
pub fn resolve_profile(flag: Option<&str>) -> Result<String, Error> {
    if let Some(name) = flag {
        return Ok(name.to_string());
    }

    if let Ok(name) = std::env::var("NOMINAL_PROFILE") {
        return Ok(name);
    }

    Err(Error::EnvVarNotSet {
        name: "NOMINAL_PROFILE",
    })
}

pub fn display_config_path(path: &Path) -> String {
    // `~/` is only a meaningful shorthand on Unix shells.
    if cfg!(unix) {
        if let Some(home) = dirs::home_dir() {
            if path.starts_with(&home) {
                return format!("~/{}", path.strip_prefix(&home).unwrap().display());
            }
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_takes_precedence() {
        let resolved = resolve_profile(Some("flag")).unwrap();
        assert_eq!(resolved, "flag");
    }

    #[test]
    fn env_var_used_when_flag_missing() {
        temp_env::with_var("NOMINAL_PROFILE", Some("env-profile"), || {
            let resolved = resolve_profile(None).unwrap();
            assert_eq!(resolved, "env-profile");
        });
    }

    #[test]
    fn errors_when_no_profile_source() {
        temp_env::with_var("NOMINAL_PROFILE", None::<&str>, || {
            let err = resolve_profile(None).unwrap_err();
            assert!(matches!(err, Error::EnvVarNotSet { .. }));
        });
    }

    #[test]
    fn display_config_path_rewrites_home() {
        let home = dirs::home_dir().expect("home");
        let path = home.join(".config/nominal/config.yml");
        if cfg!(unix) {
            assert_eq!(display_config_path(&path), "~/.config/nominal/config.yml");
        } else {
            assert_eq!(display_config_path(&path), path.display().to_string());
        }
    }
}
