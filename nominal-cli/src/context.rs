use std::path::Path;

use nominal::Error;

/// Resolve the active profile name from CLI flag or environment variable.
pub fn resolve_profile(flag: Option<&str>) -> Result<String, Error> {
    resolve_profile_with(flag, || std::env::var("NOMINAL_PROFILE").ok())
}

fn resolve_profile_with(
    flag: Option<&str>,
    env_lookup: impl FnOnce() -> Option<String>,
) -> Result<String, Error> {
    if let Some(name) = flag {
        return Ok(name.to_string());
    }

    if let Some(name) = env_lookup() {
        return Ok(name);
    }

    Err(Error::EnvVarNotSet {
        name: "NOMINAL_PROFILE",
    })
}

pub fn display_config_path(path: &Path) -> String {
    display_config_path_with(path, dirs::home_dir())
}

fn display_config_path_with(path: &Path, home: Option<std::path::PathBuf>) -> String {
    if cfg!(unix) {
        if let Some(home) = home {
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
        let resolved = resolve_profile_with(Some("flag"), || Some("ignored".to_string()));
        assert_eq!(resolved.unwrap(), "flag");
    }

    #[test]
    fn env_var_used_when_flag_missing() {
        let resolved = resolve_profile_with(None, || Some("env-profile".to_string()));
        assert_eq!(resolved.unwrap(), "env-profile");
    }

    #[test]
    fn errors_when_no_profile_source() {
        let err = resolve_profile_with(None, || None).unwrap_err();
        assert!(matches!(err, Error::EnvVarNotSet { .. }));
    }

    #[test]
    fn display_config_path_rewrites_home() {
        let home = Path::new("/home/testuser").to_path_buf();
        let path = home.join(".config/nominal/config.yml");
        if cfg!(unix) {
            assert_eq!(
                display_config_path_with(&path, Some(home)),
                "~/.config/nominal/config.yml"
            );
        } else {
            assert_eq!(
                display_config_path_with(&path, Some(home)),
                path.display().to_string()
            );
        }
    }
}
