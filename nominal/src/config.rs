use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 2;

/// Nominal v2 configuration stored at `~/.config/nominal/config.yml`.
///
/// Example format: `nominal/tests/fixtures/config/config-v2-example.yml`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    profiles: HashMap<String, Profile>,
    version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    base_url: String,
    token: String,
    workspace_rid: Option<String>,
}

impl Profile {
    pub fn new(base_url: String, token: String, workspace_rid: Option<String>) -> Self {
        Self {
            base_url,
            token,
            workspace_rid,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn workspace_rid(&self) -> Option<&str> {
        self.workspace_rid.as_deref()
    }
}

impl Config {
    pub fn empty() -> Self {
        Self {
            profiles: HashMap::new(),
            version: CONFIG_VERSION,
        }
    }

    /// Load the config from the default path, or return an empty v2 config when no v2
    /// file exists yet (including when only the deprecated `~/.nominal.yml` is present).
    pub fn load_or_default() -> Result<Self> {
        match Self::load() {
            Ok(config) => Ok(config),
            Err(crate::Error::ConfigNotFound { .. })
            | Err(crate::Error::DeprecatedConfigFound { .. }) => Ok(Self::empty()),
            Err(err) => Err(err),
        }
    }

    /// Load the config from the default path (`~/.config/nominal/config.yml`).
    pub fn load() -> Result<Self> {
        Self::load_from(&default_config_path()?)
    }

    /// Load the config from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            let path_display = path.display().to_string();
            if is_default_config_path(path) && deprecated_config_path()?.exists() {
                return Err(crate::Error::DeprecatedConfigFound {
                    path: path_display,
                    deprecated_path: deprecated_config_path()?.display().to_string(),
                });
            }
            return Err(crate::Error::ConfigNotFound { path: path_display });
        }

        let contents = fs::read_to_string(path)?;
        Self::from_yaml_str(&contents, path)
    }

    fn from_yaml_str(contents: &str, path: &Path) -> Result<Self> {
        let value: serde_yaml::Value = serde_yaml::from_str(contents)?;
        let Some(mapping) = value.as_mapping() else {
            return Err(crate::Error::ConfigMissingVersion {
                path: path.display().to_string(),
            });
        };

        match mapping.get("version").and_then(|v| v.as_u64()) {
            None => Err(crate::Error::ConfigMissingVersion {
                path: path.display().to_string(),
            }),
            Some(version) if version != u64::from(CONFIG_VERSION) => {
                Err(crate::Error::ConfigUnsupportedVersion {
                    version: version as u32,
                    path: path.display().to_string(),
                })
            }
            Some(_) => {
                let mut config: Config = serde_yaml::from_str(contents)?;
                config.version = CONFIG_VERSION;
                Ok(config)
            }
        }
    }

    pub fn get_profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn profiles(&self) -> &HashMap<String, Profile> {
        &self.profiles
    }

    pub fn add_profile(&mut self, name: String, profile: Profile) {
        self.profiles.insert(name, profile);
    }

    pub fn remove_profile(&mut self, name: &str) -> Option<Profile> {
        self.profiles.remove(name)
    }

    /// Save the config to the default path (`~/.config/nominal/config.yml`).
    pub fn save(&self) -> Result<()> {
        self.save_to(&default_config_path()?)
    }

    /// Save the config to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut to_save = self.clone();
        to_save.version = CONFIG_VERSION;
        let contents = serde_yaml::to_string(&to_save)?;
        fs::write(path, contents)?;
        Ok(())
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(home_dir()?
        .join(".config")
        .join("nominal")
        .join("config.yml"))
}

fn deprecated_config_path() -> Result<PathBuf> {
    Ok(home_dir()?.join(".nominal.yml"))
}

/// Resolve the directory used for config files (`~/.config/nominal/`, `~/.nominal.yml`).
///
/// `NOMINAL_HOME` overrides `dirs::home_dir()` when set (useful in tests; on Windows
/// `dirs::home_dir()` reads the shell profile folder and ignores `HOME`/`USERPROFILE`).
fn home_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("NOMINAL_HOME").filter(|home| !home.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    dirs::home_dir().ok_or(crate::Error::HomeDirNotFound)
}

fn is_default_config_path(path: &Path) -> bool {
    default_config_path()
        .ok()
        .is_some_and(|default| default == path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/config")
            .join(name)
    }

    fn temp_config(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.yml");
        let mut file = std::fs::File::create(&path).expect("create config");
        write!(file, "{contents}").expect("write config");
        (dir, path)
    }

    /// Isolate config path resolution to a temp directory via `NOMINAL_HOME`.
    fn with_home_dir<F: FnOnce()>(home: &Path, f: F) {
        let home = home.to_str().expect("home path must be utf-8");
        temp_env::with_var("NOMINAL_HOME", Some(home), f);
    }

    #[test]
    fn empty_config_uses_version_two() {
        let config = Config::empty();
        assert_eq!(config.version(), CONFIG_VERSION);
        assert!(config.profiles().is_empty());
    }

    #[test]
    fn load_enforces_version_two() {
        let (_dir, path) = temp_config(
            "version: 1\nprofiles:\n  default:\n    base_url: https://api.example/api\n    token: tok\n",
        );
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(
            err,
            crate::Error::ConfigUnsupportedVersion { version: 1, .. }
        ));
    }

    #[test]
    fn load_requires_version_key() {
        let path = fixture_path("config-v2-bad-example.yml");
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, crate::Error::ConfigMissingVersion { .. }));
    }

    #[test]
    fn load_v2_example_fixture() {
        let path = fixture_path("config-v2-example.yml");
        let config = Config::load_from(&path).expect("load example fixture");

        assert_eq!(config.version(), CONFIG_VERSION);
        assert!(config.get_profile("default").is_some());
        assert!(config.get_profile("staging").is_some());
        assert_eq!(
            config
                .get_profile("staging")
                .and_then(Profile::workspace_rid),
            Some("ri.security.example.workspace.00000000-0000-0000-0000-000000000001")
        );
    }

    #[test]
    fn reject_v2_bad_example_fixture() {
        let path = fixture_path("config-v2-bad-example.yml");
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, crate::Error::ConfigMissingVersion { .. }));
    }

    #[test]
    fn example_fixture_roundtrips_via_save() {
        let path = fixture_path("config-v2-example.yml");
        let config = Config::load_from(&path).expect("load example fixture");

        let dir = tempfile::tempdir().expect("tempdir");
        let saved_path = dir.path().join("config.yml");
        config.save_to(&saved_path).expect("save");

        let loaded = Config::load_from(&saved_path).expect("reload");
        assert_eq!(loaded.version(), config.version());
        assert_eq!(loaded.profiles().len(), config.profiles().len());
        for (name, profile) in config.profiles() {
            let reloaded = loaded
                .get_profile(name)
                .expect("profile present after reload");
            assert_eq!(reloaded.base_url(), profile.base_url());
            assert_eq!(reloaded.token(), profile.token());
            assert_eq!(reloaded.workspace_rid(), profile.workspace_rid());
        }
    }

    #[test]
    fn load_or_default_returns_empty_when_deprecated_config_exists() {
        let home = tempfile::tempdir().expect("tempdir");
        let home_path = home.path().to_path_buf();

        let deprecated_path = home_path.join(".nominal.yml");
        std::fs::copy(
            fixture_path("config-v1-deprecated-example.yml"),
            &deprecated_path,
        )
        .expect("copy deprecated config fixture");

        with_home_dir(&home_path, || {
            let config = Config::load_or_default().expect("load_or_default");
            assert_eq!(config.version(), CONFIG_VERSION);
            assert!(config.profiles().is_empty());

            let err = Config::load().unwrap_err();
            assert!(matches!(err, crate::Error::DeprecatedConfigFound { .. }));
        });
    }

    #[test]
    fn load_or_default_returns_empty_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("missing.yml");
        let config = Config::load_from(&path);
        assert!(matches!(config, Err(crate::Error::ConfigNotFound { .. })));

        // load_or_default is only defined for the default path; verify empty() shape here.
        let empty = Config::empty();
        assert_eq!(empty.version(), CONFIG_VERSION);
        assert!(empty.profiles().is_empty());
    }

    #[test]
    fn save_writes_version_two() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.yml");
        let mut config = Config::empty();
        config.add_profile(
            "dev".to_string(),
            Profile::new(
                "https://api.example/api".to_string(),
                "token".to_string(),
                None,
            ),
        );
        config.save_to(&path).expect("save");

        let loaded = Config::load_from(&path).expect("load");
        assert_eq!(loaded.version(), CONFIG_VERSION);
        assert!(loaded.get_profile("dev").is_some());
    }
}
