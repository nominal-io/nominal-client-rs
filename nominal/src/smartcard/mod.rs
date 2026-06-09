//! PKCS#11-backed client-certificate resolver for CAC/PIV mTLS.
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use nominal::smartcard::SmartcardCertResolver;
//!
//! # fn main() -> nominal::Result<()> {
//! let resolver = SmartcardCertResolver::new()?;
//!
//! let client = nominal::NominalClient::builder("api-token")
//!     .client_cert_resolver(Arc::new(resolver))
//!     .build()?;
//! # Ok(())
//! # }
//! ```

mod pkcs11;
mod signing;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use rustls::SignatureScheme;
use rustls::client::ResolvesClientCert;
use rustls::sign::CertifiedKey;

use pkcs11::{
    discover_piv_cert, find_key_handle, open_session, probe_key, schemes_for_key_type, tls_err,
};
use signing::Pkcs11SigningKey;

use crate::{Error, Result};

/// Environment variable that overrides the PKCS#11 module path.
///
/// Set to the absolute path of the PKCS#11 shared library when the default
/// OpenSC paths do not apply (e.g. non-standard installs or ActivClient).
pub const PKCS11_MODULE_ENV_VAR: &str = "NOMINAL_PKCS11_MODULE";

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
    /// The module path is resolved in order:
    /// 1. `NOMINAL_PKCS11_MODULE` environment variable, if set.
    /// 2. Platform-specific OpenSC default paths.
    ///
    /// Scans all token slots for a PIV Authentication certificate (slot 9A),
    /// then prompts for PIN once. The session is kept open so no further PIN
    /// prompts occur during TLS handshakes.
    ///
    /// Note: some middleware times out idle sessions; if `sign()` starts
    /// returning errors after a long idle period, the session may need to be
    /// re-opened by constructing a new resolver.
    pub fn new() -> Result<Self> {
        let module_path = discover_module_path()?;

        let pkcs11 = Pkcs11::new(&module_path).map_err(|e| Error::Tls {
            details: format!("failed to load PKCS#11 module {module_path:?}: {e}"),
        })?;

        pkcs11
            .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
            .map_err(tls_err("C_Initialize failed"))?;

        let slots = pkcs11
            .get_slots_with_token()
            .map_err(tls_err("C_GetSlotList failed"))?;

        let (slot, cert_der, key_id) = discover_piv_cert(&pkcs11, &slots)?;

        let session = open_session(&pkcs11, slot)?;
        let key_handle = find_key_handle(&session, &key_id)?;
        let (key_type, ec_params) = probe_key(&session, key_handle)?;
        let session = Arc::new(Mutex::new(session));

        let (schemes, algorithm) = schemes_for_key_type(key_type, ec_params.as_deref())?;

        let signing_key: Arc<dyn rustls::sign::SigningKey> = Arc::new(Pkcs11SigningKey {
            session,
            key_handle,
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

/// Resolve the PKCS#11 module path.
///
/// Checks `NOMINAL_PKCS11_MODULE` first, then walks platform-specific OpenSC
/// default paths. Returns an error if no module is found.
fn discover_module_path() -> Result<PathBuf> {
    if let Ok(env_val) = std::env::var(PKCS11_MODULE_ENV_VAR) {
        let path = PathBuf::from(&env_val);
        if path.exists() {
            return Ok(path);
        }
        return Err(Error::Tls {
            details: format!(
                "PKCS#11 module path from {PKCS11_MODULE_ENV_VAR} does not exist: {path:?}"
            ),
        });
    }

    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/Library/OpenSC/lib/opensc-pkcs11.so",
        "/opt/homebrew/lib/opensc-pkcs11.so",
        "/usr/local/lib/opensc-pkcs11.so",
    ];
    #[cfg(target_os = "linux")]
    let candidates: &[&str] = &[
        "/usr/lib64/opensc-pkcs11.so",
        "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so",
        "/usr/lib/aarch64-linux-gnu/opensc-pkcs11.so",
        "/usr/lib/opensc-pkcs11.so",
    ];
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        r"C:\Program Files\OpenSC Project\OpenSC\pkcs11\opensc-pkcs11.dll",
        r"C:\Program Files (x86)\OpenSC Project\OpenSC\pkcs11\opensc-pkcs11.dll",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let candidates: &[&str] = &[];

    for &candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(Error::Tls {
        details: format!(
            "could not find an OpenSC PKCS#11 module; install OpenSC or set \
             {PKCS11_MODULE_ENV_VAR} to the module path"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // These tests mutate the shared, process-global PKCS11_MODULE_ENV_VAR. cargo
    // runs tests on multiple threads by default, so without serialization they
    // race (one test's remove_var clears the var the other just set, and the
    // fall-through then resolves a real OpenSC install if one is present). Lock
    // this mutex for the whole get/set/remove window in every env-touching test.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_var_set_to_nonexistent_path_returns_error() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: the ENV_LOCK guard serializes all access to this env var
        // across the test binary, so no other thread reads/writes it here.
        unsafe { std::env::set_var(PKCS11_MODULE_ENV_VAR, "/nonexistent/path/opensc.so") };
        let result = discover_module_path();
        unsafe { std::env::remove_var(PKCS11_MODULE_ENV_VAR) };
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("does not exist"), "got: {msg}");
    }

    #[test]
    fn env_var_set_to_existing_path_is_returned() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let existing = std::env::current_exe().unwrap();
        // SAFETY: the ENV_LOCK guard serializes all access to this env var
        // across the test binary, so no other thread reads/writes it here.
        unsafe { std::env::set_var(PKCS11_MODULE_ENV_VAR, &existing) };
        let result = discover_module_path();
        unsafe { std::env::remove_var(PKCS11_MODULE_ENV_VAR) };
        assert_eq!(result.unwrap(), existing);
    }
}
