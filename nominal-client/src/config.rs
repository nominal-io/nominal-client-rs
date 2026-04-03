use crate::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    profiles: HashMap<String, Profile>,
    version: u32,
}

#[derive(Debug, Deserialize)]
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
    pub fn from_file(path: Option<PathBuf>) -> Result<Self> {
        let path = match path {
            Some(path) => path,
            None => {
                let home = dirs::home_dir().ok_or(crate::Error::HomeDirNotFound)?;
                home.join(".config").join("nominal").join("config.yml")
            }
        };
        let contents = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&contents)?;
        Ok(config)
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
}
