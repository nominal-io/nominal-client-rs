use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub profiles: HashMap<String, Profile>,
    pub version: u32,
}

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub base_url: String,
    pub token: String,
    pub workspace_rid: Option<String>,
}

impl Config {
    pub fn from_file(path: Option<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.unwrap_or_else(|| {
            let home = std::env::var("HOME").expect("Failed to get HOME environment variable");
            PathBuf::from(format!("{}/.config/nominal/config.yml", home))
        });
        let contents = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }

    pub fn get_profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }
}
