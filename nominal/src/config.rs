use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_VERSION: u32 = 2;
const DEPRECATED_CONFIG_FILENAME: &str = ".nominal.yml";

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    version: u32,
    profiles: HashMap<String, Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_profile: Option<String>,
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
            version: CONFIG_VERSION,
            profiles: HashMap::new(),
            default_profile: None,
        }
    }

    /// Load the config from the default path (`~/.config/nominal/config.yml`).
    pub fn load() -> Result<Self> {
        Self::load_from(&default_config_path()?)
    }

    /// Load the config from the default path, or return an empty version 2 config
    /// if the file has not been created yet.
    pub fn load_or_default() -> Result<Self> {
        Self::load_from_or_default(&default_config_path()?)
    }

    /// Load the config from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        Self::load_from_with_deprecated_path(path, default_deprecated_config_path(path))
    }

    /// Load the config from an explicit path, or return an empty version 2 config
    /// if the file has not been created yet.
    pub fn load_from_or_default(path: &Path) -> Result<Self> {
        match Self::load_from(path) {
            Ok(config) => Ok(config),
            Err(crate::Error::ConfigNotFound { .. }) => Ok(Self::empty()),
            Err(error) => Err(error),
        }
    }

    fn load_from_with_deprecated_path(
        path: &Path,
        deprecated_path: Option<PathBuf>,
    ) -> Result<Self> {
        if !path.try_exists()? {
            if let Some(deprecated_path) = deprecated_path {
                if deprecated_path.try_exists()? {
                    return Err(crate::Error::DeprecatedConfigFound {
                        path: path.display().to_string(),
                        deprecated_path: deprecated_path.display().to_string(),
                    });
                }
            }

            return Err(crate::Error::ConfigNotFound {
                path: path.display().to_string(),
            });
        }

        let contents = fs::read_to_string(path)?;
        let raw_config: RawConfig = serde_yaml::from_str(&contents)?;
        let path = path.display().to_string();
        let version = raw_config
            .version
            .ok_or_else(|| crate::Error::MissingConfigKey {
                path: path.clone(),
                key: "version",
            })?;
        if version != CONFIG_VERSION {
            return Err(crate::Error::UnsupportedConfigVersion {
                path,
                expected: CONFIG_VERSION,
                found: version,
            });
        }

        let profiles = raw_config
            .profiles
            .ok_or_else(|| crate::Error::MissingConfigKey {
                path,
                key: "profiles",
            })?;

        Ok(Self {
            version: CONFIG_VERSION,
            profiles,
            default_profile: raw_config.default_profile,
        })
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

    pub fn default_profile(&self) -> Option<&str> {
        self.default_profile.as_deref()
    }

    pub fn set_default_profile(&mut self, name: impl Into<String>) {
        self.default_profile = Some(name.into());
    }

    pub fn clear_default_profile(&mut self) {
        self.default_profile = None;
    }

    pub fn add_profile(&mut self, name: String, profile: Profile) {
        if self.default_profile.is_none() {
            self.default_profile = Some(name.clone());
        }
        self.profiles.insert(name, profile);
    }

    pub fn remove_profile(&mut self, name: &str) -> Option<Profile> {
        if self.default_profile.as_deref() == Some(name) {
            self.default_profile = None;
        }
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
        let contents = serde_yaml::to_string(&SerializableConfig {
            version: CONFIG_VERSION,
            profiles: &self.profiles,
            default_profile: self.default_profile.as_deref(),
        })?;
        fs::write(path, contents)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    version: Option<u32>,
    profiles: Option<HashMap<String, Profile>>,
    default_profile: Option<String>,
}

#[derive(Serialize)]
struct SerializableConfig<'a> {
    version: u32,
    profiles: &'a HashMap<String, Profile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_profile: Option<&'a str>,
}

fn default_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDirNotFound)?;
    Ok(home.join(".config").join("nominal").join("config.yml"))
}

fn deprecated_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDirNotFound)?;
    Ok(home.join(DEPRECATED_CONFIG_FILENAME))
}

fn default_deprecated_config_path(path: &Path) -> Option<PathBuf> {
    if default_config_path().is_ok_and(|default_path| default_path == path) {
        deprecated_config_path().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_from_or_default_returns_empty_config_when_file_is_missing() {
        let path = temp_path("missing-config").join("config.yml");

        let config = Config::load_from_or_default(&path).expect("config should load");

        assert_eq!(config.version(), CONFIG_VERSION);
        assert!(config.profiles().is_empty());
        assert_eq!(config.default_profile(), None);
    }

    #[test]
    fn load_from_reports_deprecated_config_when_present() {
        let dir = temp_path("deprecated-config");
        fs::create_dir_all(&dir).expect("temp dir should be created");
        let config_path = dir.join("config.yml");
        let deprecated_path = dir.join(DEPRECATED_CONFIG_FILENAME);
        fs::write(&deprecated_path, "token: old-token\n")
            .expect("deprecated config should be written");

        let error =
            Config::load_from_with_deprecated_path(&config_path, Some(deprecated_path.clone()))
                .expect_err("deprecated config should block default config creation");

        match error {
            Error::DeprecatedConfigFound {
                path,
                deprecated_path: actual_deprecated_path,
            } => {
                assert_eq!(path, config_path.display().to_string());
                assert_eq!(
                    actual_deprecated_path,
                    deprecated_path.display().to_string()
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_from_requires_version_key() {
        let path = write_config("missing-version", "profiles: {}\n");

        let error = Config::load_from(&path).expect_err("missing version should fail");

        match error {
            Error::MissingConfigKey {
                path: actual_path,
                key,
            } => {
                assert_eq!(actual_path, path.display().to_string());
                assert_eq!(key, "version");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_from_requires_profiles_key() {
        let path = write_config("missing-profiles", "version: 2\n");

        let error = Config::load_from(&path).expect_err("missing profiles should fail");

        match error {
            Error::MissingConfigKey {
                path: actual_path,
                key,
            } => {
                assert_eq!(actual_path, path.display().to_string());
                assert_eq!(key, "profiles");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_from_requires_version_two() {
        let path = write_config("unsupported-version", "version: 1\nprofiles: {}\n");

        let error = Config::load_from(&path).expect_err("unsupported version should fail");

        match error {
            Error::UnsupportedConfigVersion {
                path: actual_path,
                expected,
                found,
            } => {
                assert_eq!(actual_path, path.display().to_string());
                assert_eq!(expected, CONFIG_VERSION);
                assert_eq!(found, 1);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn load_from_reads_default_profile() {
        let path = write_config(
            "default-profile",
            r#"version: 2
profiles:
  staging:
    base_url: https://api-staging.gov.nominal.io/api
    token: nominal_api_key_staging
default_profile: staging
"#,
        );

        let config = Config::load_from(&path).expect("config should load");

        assert_eq!(config.default_profile(), Some("staging"));
        assert!(config.get_profile("staging").is_some());
    }

    #[test]
    fn add_profile_sets_default_profile_once_and_remove_clears_it() {
        let mut config = Config::empty();

        config.add_profile("default".to_string(), profile("nominal_api_key_default"));
        config.add_profile("staging".to_string(), profile("nominal_api_key_staging"));

        assert_eq!(config.default_profile(), Some("default"));
        assert!(config.remove_profile("default").is_some());
        assert_eq!(config.default_profile(), None);
        assert!(config.get_profile("staging").is_some());
    }

    #[test]
    fn save_to_always_writes_version_two() {
        let mut profiles = HashMap::new();
        profiles.insert("default".to_string(), profile("nominal_api_key_default"));
        let config = Config {
            version: 1,
            profiles,
            default_profile: Some("default".to_string()),
        };
        let path = temp_path("save-version").join("config.yml");

        config.save_to(&path).expect("config should save");

        let contents = fs::read_to_string(path).expect("saved config should be readable");
        assert!(contents.contains("version: 2"));
        assert!(contents.contains("default_profile: default"));
    }

    fn profile(token: &str) -> Profile {
        Profile::new(
            "https://api.gov.nominal.io/api".to_string(),
            token.to_string(),
            None,
        )
    }

    fn write_config(name: &str, contents: &str) -> PathBuf {
        let path = temp_path(name).join("config.yml");
        fs::create_dir_all(path.parent().expect("config path should have parent"))
            .expect("temp dir should be created");
        fs::write(&path, contents).expect("config should be written");
        path
    }

    fn temp_path(name: &str) -> PathBuf {
        let timestamp_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nominal-config-{name}-{}-{timestamp_nanos}",
            std::process::id()
        ))
    }
}
