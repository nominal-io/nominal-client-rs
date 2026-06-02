use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    profiles: HashMap<String, Profile>,
    version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    base_url: String,
    token: String,
    workspace_rid: String,
}

impl Profile {
    pub fn new(base_url: String, token: String, workspace_rid: String) -> Self {
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

    pub fn workspace_rid(&self) -> &str {
        &self.workspace_rid
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            profiles: HashMap::new(),
            version: CONFIG_VERSION,
        }
    }

    /// Load the config from the default path (`~/.config/nominal/config.yml`).
    pub fn load() -> Result<Self> {
        Self::load_from(&default_config_path()?)
    }

    /// Load the config from the default path, or return an empty v2 config if it does not exist.
    pub fn load_or_default() -> Result<Self> {
        match Self::load() {
            Ok(config) => Ok(config),
            Err(crate::Error::Io(err)) if err.kind() == ErrorKind::NotFound => Ok(Self::new()),
            Err(err) => Err(err),
        }
    }

    /// Load the config from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&contents)?;
        if config.version != CONFIG_VERSION {
            return Err(crate::Error::UnsupportedConfigVersion {
                version: config.version,
            });
        }
        config.validate_profiles()?;
        Ok(config)
    }

    /// Return the default config path (`~/.config/nominal/config.yml`).
    pub fn default_path() -> Result<PathBuf> {
        default_config_path()
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
        self.validate_profiles()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = serde_yaml::to_string(self)?;
        fs::write(path, contents)?;
        Ok(())
    }

    fn validate_profiles(&self) -> Result<()> {
        for (name, profile) in &self.profiles {
            if profile.workspace_rid.trim().is_empty() {
                return Err(crate::Error::MissingProfileWorkspace { name: name.clone() });
            }
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

fn default_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDirNotFound)?;
    Ok(home.join(".config").join("nominal").join("config.yml"))
}
