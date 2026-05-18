// rustls signing bridge: SigningKey + Signer backed by a PKCS#11 private key.

use std::sync::{Arc, Mutex};

use cryptoki::mechanism::Mechanism;
use cryptoki::mechanism::MechanismType;
use cryptoki::mechanism::rsa::{PkcsMgfType, PkcsPssParams};
use cryptoki::object::ObjectHandle;
use cryptoki::session::Session;
use rustls::SignatureAlgorithm;
use rustls::SignatureScheme;
use rustls::sign::{Signer, SigningKey};

use super::der::ecdsa_raw_to_der;
use crate::{Error, Result};

// --- SigningKey ----------------------------------------------------------

pub(super) struct Pkcs11SigningKey {
    pub(super) session: Arc<Mutex<Session>>,
    pub(super) key_handle: ObjectHandle,
    pub(super) schemes: Vec<SignatureScheme>,
    pub(super) algorithm: SignatureAlgorithm,
}

impl std::fmt::Debug for Pkcs11SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkcs11SigningKey")
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

impl SigningKey for Pkcs11SigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        let scheme = first_supported(offered, &self.schemes)?;
        Some(Box::new(Pkcs11Signer {
            session: self.session.clone(),
            key_handle: self.key_handle,
            scheme,
        }))
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }
}

// --- Signer --------------------------------------------------------------

struct Pkcs11Signer {
    session: Arc<Mutex<Session>>,
    key_handle: ObjectHandle,
    scheme: SignatureScheme,
}

impl std::fmt::Debug for Pkcs11Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkcs11Signer")
            .field("scheme", &self.scheme)
            .finish_non_exhaustive()
    }
}

impl Signer for Pkcs11Signer {
    fn sign(&self, message: &[u8]) -> std::result::Result<Vec<u8>, rustls::Error> {
        let session = self
            .session
            .lock()
            .map_err(|e| rustls::Error::General(e.to_string()))?;
        sign_with_scheme(&session, self.key_handle, self.scheme, message)
            .map_err(|e| rustls::Error::General(e.to_string()))
    }

    fn scheme(&self) -> SignatureScheme {
        self.scheme
    }
}

// --- Scheme selection ----------------------------------------------------

/// Return the first element of `offered` that appears in `supported`.
/// Iterates `offered` first so the server's preference order is respected.
pub(super) fn first_supported(
    offered: &[SignatureScheme],
    supported: &[SignatureScheme],
) -> Option<SignatureScheme> {
    offered.iter().find(|s| supported.contains(s)).copied()
}

// --- Signing logic -------------------------------------------------------

fn sign_with_scheme(
    session: &Session,
    key: ObjectHandle,
    scheme: SignatureScheme,
    message: &[u8],
) -> Result<Vec<u8>> {
    match scheme {
        // RSA PKCS#1 — compound mechanisms: the token hashes and signs the
        // full message, so no hashing is needed here.
        SignatureScheme::RSA_PKCS1_SHA256 => session
            .sign(&Mechanism::Sha256RsaPkcs, key, message)
            .map_err(pkcs11_err),
        SignatureScheme::RSA_PKCS1_SHA384 => session
            .sign(&Mechanism::Sha384RsaPkcs, key, message)
            .map_err(pkcs11_err),
        SignatureScheme::RSA_PKCS1_SHA512 => session
            .sign(&Mechanism::Sha512RsaPkcs, key, message)
            .map_err(pkcs11_err),

        // RSA-PSS — compound mechanisms. The PSS parameters specify the hash
        // algorithm and MGF so the token can perform the full operation.
        // Salt length is set to the hash output length, which is the
        // recommended value per RFC 8017 §9.1.1 and what TLS 1.3 mandates.
        SignatureScheme::RSA_PSS_SHA256 => {
            let params = PkcsPssParams {
                hash_alg: MechanismType::SHA256,
                mgf: PkcsMgfType::MGF1_SHA256,
                s_len: 32.into(),
            };
            session
                .sign(&Mechanism::Sha256RsaPkcsPss(params), key, message)
                .map_err(pkcs11_err)
        }
        SignatureScheme::RSA_PSS_SHA384 => {
            let params = PkcsPssParams {
                hash_alg: MechanismType::SHA384,
                mgf: PkcsMgfType::MGF1_SHA384,
                s_len: 48.into(),
            };
            session
                .sign(&Mechanism::Sha384RsaPkcsPss(params), key, message)
                .map_err(pkcs11_err)
        }
        SignatureScheme::RSA_PSS_SHA512 => {
            let params = PkcsPssParams {
                hash_alg: MechanismType::SHA512,
                mgf: PkcsMgfType::MGF1_SHA512,
                s_len: 64.into(),
            };
            session
                .sign(&Mechanism::Sha512RsaPkcsPss(params), key, message)
                .map_err(pkcs11_err)
        }

        // ECDSA — compound mechanisms: the token hashes internally.
        // PKCS#11 returns raw r || s bytes; TLS requires a DER SEQUENCE.
        SignatureScheme::ECDSA_NISTP256_SHA256 => session
            .sign(&Mechanism::EcdsaSha256, key, message)
            .map(|raw| ecdsa_raw_to_der(&raw))
            .map_err(pkcs11_err),
        SignatureScheme::ECDSA_NISTP384_SHA384 => session
            .sign(&Mechanism::EcdsaSha384, key, message)
            .map(|raw| ecdsa_raw_to_der(&raw))
            .map_err(pkcs11_err),

        _ => Err(Error::Tls {
            details: format!("unsupported SignatureScheme: {scheme:?}"),
        }),
    }
}

fn pkcs11_err(e: cryptoki::error::Error) -> Error {
    Error::Tls {
        details: format!("PKCS#11 error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_first_offered_scheme_that_is_supported() {
        let supported = vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
        ];
        // Server offers PKCS#1 first — it should be chosen over PSS.
        let offered = [
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
        ];
        assert_eq!(
            first_supported(&offered, &supported),
            Some(SignatureScheme::RSA_PKCS1_SHA256),
            "server preference (PKCS#1 first) must be respected"
        );
    }

    #[test]
    fn picks_pss_when_offered_first() {
        let supported = vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
        ];
        let offered = [
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
        ];
        assert_eq!(
            first_supported(&offered, &supported),
            Some(SignatureScheme::RSA_PSS_SHA256),
        );
    }

    #[test]
    fn skips_unsupported_schemes_before_finding_match() {
        let supported = vec![SignatureScheme::RSA_PSS_SHA256];
        let offered = [
            SignatureScheme::ECDSA_NISTP256_SHA256, // not supported
            SignatureScheme::ED25519,               // not supported
            SignatureScheme::RSA_PSS_SHA256,        // supported
        ];
        assert_eq!(
            first_supported(&offered, &supported),
            Some(SignatureScheme::RSA_PSS_SHA256),
        );
    }

    #[test]
    fn returns_none_when_sets_are_disjoint() {
        let supported = vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
        ];
        let offered = [
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
        ];
        assert!(
            first_supported(&offered, &supported).is_none(),
            "no overlap must return None"
        );
    }

    #[test]
    fn returns_none_for_empty_offered() {
        let supported = vec![SignatureScheme::RSA_PSS_SHA256];
        assert!(first_supported(&[], &supported).is_none());
    }

    #[test]
    fn returns_none_for_empty_supported() {
        let offered = [SignatureScheme::RSA_PSS_SHA256];
        assert!(first_supported(&offered, &[]).is_none());
    }

    #[test]
    fn returns_none_when_both_empty() {
        assert!(first_supported(&[], &[]).is_none());
    }

    #[test]
    fn single_element_match() {
        let supported = vec![SignatureScheme::RSA_PSS_SHA256];
        let offered = [SignatureScheme::RSA_PSS_SHA256];
        assert_eq!(
            first_supported(&offered, &supported),
            Some(SignatureScheme::RSA_PSS_SHA256),
        );
    }
}
