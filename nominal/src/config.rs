use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for CAC / PIV smartcard client-certificate authentication.
///
/// Stored under the `smartcard` key in a profile's YAML entry.
/// The PKCS#11 implementation is loaded at runtime by calling
/// `nominal::core::smartcard::load_pkcs11_backend` (provided in a later PR).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SmartcardConfig {
    /// Path to the PKCS#11 module shared library.
    ///
    /// Common values:
    /// - Linux: `/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so`
    /// - macOS: `/Library/OpenSC/lib/opensc-pkcs11.so`
    /// - Windows: `C:\Windows\System32\opensc-pkcs11.dll`
    pub pkcs11_module: PathBuf,

    /// Optional SHA-256 fingerprint (lowercase hex, no colons) of the PIV
    /// Authentication certificate. When `None`, the first certificate whose
    /// Extended Key Usage contains the PIV Authentication OID is selected.
    pub cert_fingerprint_sha256: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    profiles: HashMap<String, Profile>,
    version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    base_url: String,
    token: String,
    workspace_rid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smartcard: Option<SmartcardConfig>,
}

impl Profile {
    pub fn new(base_url: String, token: String, workspace_rid: Option<String>) -> Self {
        Self {
            base_url,
            token,
            workspace_rid,
            smartcard: None,
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
    /// Load the config from the default path (`~/.config/nominal/config.yml`).
    pub fn load() -> Result<Self> {
        Self::load_from(&default_config_path()?)
    }

    /// Load the config from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
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

    /// Save the config to the default path (`~/.config/nominal/config.yml`).
    pub fn save(&self) -> Result<()> {
        self.save_to(&default_config_path()?)
    }

    /// Save the config to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = serde_yaml::to_string(self)?;
        fs::write(path, contents)?;
        Ok(())
    }
}

fn default_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDirNotFound)?;
    Ok(home.join(".config").join("nominal").join("config.yml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smartcard_config_roundtrips_through_yaml() {
        let cfg = SmartcardConfig {
            pkcs11_module: PathBuf::from("/usr/lib/opensc-pkcs11.so"),
            cert_fingerprint_sha256: Some("abcd1234".to_string()),
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let roundtripped: SmartcardConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(roundtripped.pkcs11_module, cfg.pkcs11_module);
        assert_eq!(
            roundtripped.cert_fingerprint_sha256,
            cfg.cert_fingerprint_sha256
        );
    }

    #[test]
    fn smartcard_config_optional_fingerprint_roundtrips() {
        let cfg = SmartcardConfig {
            pkcs11_module: PathBuf::from("/usr/lib/opensc-pkcs11.so"),
            cert_fingerprint_sha256: None,
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let roundtripped: SmartcardConfig = serde_yaml::from_str(&yaml).unwrap();
        assert!(roundtripped.cert_fingerprint_sha256.is_none());
    }

    #[test]
    fn profile_without_smartcard_roundtrips() {
        let profile = Profile::new(
            "https://api.example.com".into(),
            "mytoken".into(),
            Some("ri.workspace..123".into()),
        );
        let yaml = serde_yaml::to_string(&profile).unwrap();
        assert!(
            !yaml.contains("smartcard"),
            "smartcard field should be absent when None"
        );
        let roundtripped: Profile = serde_yaml::from_str(&yaml).unwrap();
        assert!(roundtripped.smartcard.is_none());
    }

    #[test]
    fn profile_with_smartcard_roundtrips() {
        let mut profile = Profile::new("https://api.example.com".into(), "mytoken".into(), None);
        profile.smartcard = Some(SmartcardConfig {
            pkcs11_module: PathBuf::from("/usr/lib/opensc-pkcs11.so"),
            cert_fingerprint_sha256: None,
        });
        let yaml = serde_yaml::to_string(&profile).unwrap();
        assert!(
            yaml.contains("smartcard"),
            "smartcard field should be present"
        );
        let roundtripped: Profile = serde_yaml::from_str(&yaml).unwrap();
        assert!(roundtripped.smartcard.is_some());
        assert_eq!(
            roundtripped.smartcard.unwrap().pkcs11_module,
            PathBuf::from("/usr/lib/opensc-pkcs11.so")
        );
    }
}
