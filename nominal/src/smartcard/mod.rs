//! PKCS#11-backed client-certificate resolver for CAC/PIV mTLS.
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use nominal::smartcard::{SmartcardCertResolver, SmartcardConfig};
//!
//! # fn main() -> nominal::Result<()> {
//! let resolver = SmartcardCertResolver::new(SmartcardConfig {
//!     module_path: "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so".into(),
//!     slot_index: None,
//! })?;
//!
//! let client = nominal::NominalClient::builder("api-token")
//!     .client_cert_resolver(Arc::new(resolver))
//!     .build()?;
//! # Ok(())
//! # }
//! ```

mod der;
mod pkcs11;
mod signing;

use std::path::PathBuf;
use std::sync::Arc;

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use rustls::SignatureScheme;
use rustls::client::ResolvesClientCert;
use rustls::sign::CertifiedKey;

use pkcs11::{find_certificate, open_session, probe_key_type, schemes_for_key_type};
use signing::Pkcs11SigningKey;

use crate::{Error, Result};

/// Configuration for [`SmartcardCertResolver`].
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct SmartcardConfig {
    /// Path to the PKCS#11 shared library (`.so` / `.dylib` / `.dll`).
    ///
    /// Common paths:
    /// - Linux: `/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so`
    /// - macOS: `/Library/OpenSC/lib/opensc-pkcs11.so`
    /// - Windows: `C:\Windows\System32\opensc-pkcs11.dll`
    pub module_path: PathBuf,

    /// Zero-based slot index. `None` selects the first available slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_index: Option<usize>,
}

/// rustls client-certificate resolver backed by a PKCS#11 token (CAC/PIV).
///
/// Pass to [`NominalClientBuilder::client_cert_resolver`].
///
/// [`NominalClientBuilder::client_cert_resolver`]: crate::NominalClientBuilder::client_cert_resolver
pub struct SmartcardCertResolver {
    key: Arc<CertifiedKey>,
}

impl std::fmt::Debug for SmartcardCertResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartcardCertResolver")
            .finish_non_exhaustive()
    }
}

impl SmartcardCertResolver {
    /// Load the PKCS#11 module and prepare the signing context.
    ///
    /// Opens a session once to locate the certificate and confirm the private key
    /// is present, then closes it. A new session is opened per TLS handshake.
    pub fn new(config: SmartcardConfig) -> Result<Self> {
        let pkcs11 = Pkcs11::new(&config.module_path).map_err(|e| Error::Tls {
            details: format!(
                "failed to load PKCS#11 module {:?}: {e}",
                config.module_path
            ),
        })?;

        pkcs11
            .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
            .map_err(|e| Error::Tls {
                details: format!("C_Initialize failed: {e}"),
            })?;

        let slots = pkcs11.get_slots_with_token().map_err(|e| Error::Tls {
            details: format!("C_GetSlotList failed: {e}"),
        })?;

        let slot = match config.slot_index {
            Some(i) => slots.get(i).copied().ok_or_else(|| Error::Tls {
                details: format!(
                    "slot_index {i} is out of range ({} token slots found)",
                    slots.len()
                ),
            })?,
            None => slots.into_iter().next().ok_or_else(|| Error::Tls {
                details: "no PKCS#11 slot with a token found".into(),
            })?,
        };

        let session = open_session(&pkcs11, slot)?;
        let (cert_der, key_id) = find_certificate(&session)?;
        let key_type = probe_key_type(&session, &key_id)?;
        drop(session);

        let (schemes, algorithm) = schemes_for_key_type(key_type)?;

        let signing_key: Arc<dyn rustls::sign::SigningKey> = Arc::new(Pkcs11SigningKey {
            pkcs11: Arc::new(pkcs11),
            slot,
            key_id,
            schemes,
            algorithm,
        });

        let certified_key = Arc::new(CertifiedKey::new(vec![cert_der], signing_key));
        Ok(Self { key: certified_key })
    }
}

impl ResolvesClientCert for SmartcardCertResolver {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        Some(self.key.clone())
    }

    fn has_certs(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_slot_index_absent_when_none() {
        let cfg = SmartcardConfig {
            module_path: "/usr/lib/opensc-pkcs11.so".into(),
            slot_index: None,
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        assert!(!yaml.contains("slot_index"), "absent slot must not appear");
    }

    #[test]
    fn config_roundtrips_all_fields_through_yaml() {
        let cfg = SmartcardConfig {
            module_path: "/usr/lib/opensc-pkcs11.so".into(),
            slot_index: Some(2),
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let rt: SmartcardConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(rt.module_path, cfg.module_path);
        assert_eq!(rt.slot_index, cfg.slot_index);
    }

    #[test]
    fn config_deserializes_with_module_path_only() {
        let cfg: SmartcardConfig =
            serde_yaml::from_str("module_path: /usr/lib/opensc-pkcs11.so\n").unwrap();
        assert_eq!(
            cfg.module_path.to_str().unwrap(),
            "/usr/lib/opensc-pkcs11.so"
        );
        assert!(cfg.slot_index.is_none());
    }

    #[test]
    fn config_preserves_module_path_with_spaces() {
        let cfg = SmartcardConfig {
            module_path: "/path/with spaces/opensc.so".into(),
            slot_index: None,
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let rt: SmartcardConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(rt.module_path, cfg.module_path);
    }
}
