//! rustls signing bridge: `SigningKey` + `Signer` backed by a PKCS#11 private key.

use std::sync::{Arc, Mutex};

use cryptoki::mechanism::Mechanism;
use cryptoki::mechanism::MechanismType;
use cryptoki::mechanism::rsa::{PkcsMgfType, PkcsPssParams};
use cryptoki::object::ObjectHandle;
use cryptoki::session::Session;
use rustls::SignatureAlgorithm;
use rustls::SignatureScheme;
use rustls::sign::{Signer, SigningKey};

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
        // C_Sign on a hardware token is synchronous and can take 100–500 ms.
        // This runs synchronously inside rustls's `Signer` callback, on
        // whatever runtime thread is driving the handshake, and the whole
        // operation is serialized behind the session mutex because a single
        // PKCS#11 session cannot have two signing operations in flight at once.
        //
        // Consequences the caller should be aware of:
        //   * On a current-thread runtime, the in-progress sign blocks the
        //     entire reactor until the token responds.
        //   * On a multi-threaded runtime, each concurrent handshake blocks its
        //     own worker thread, and they serialize on this mutex — so many
        //     simultaneous new connections can see added latency.
        // The block happens at most once per handshake. We deliberately do NOT
        // use tokio::task::block_in_place: it panics on a current-thread
        // runtime, and this is a public library whose consumers control the
        // runtime flavor. Offloading to a dedicated signing thread is the
        // natural next step if connection-setup throughput becomes a concern.
        let session = self
            .session
            .lock()
            .map_err(|e| rustls::Error::General(e.to_string()))?;

        sign_message(&session, self.key_handle, self.scheme, message)
            .map_err(|e| rustls::Error::General(e.to_string()))
    }

    fn scheme(&self) -> SignatureScheme {
        self.scheme
    }
}

// --- Scheme selection ----------------------------------------------------

/// Return the first element of `offered` that appears in `supported`.
/// Iterates `offered` first so the server's preference order is respected.
fn first_supported(
    offered: &[SignatureScheme],
    supported: &[SignatureScheme],
) -> Option<SignatureScheme> {
    offered.iter().find(|s| supported.contains(s)).copied()
}

// --- Signing logic -------------------------------------------------------

/// Post-processing applied to the raw bytes a PKCS#11 sign returns.
enum SigPost {
    /// RSA: the token already emits the encoded signature.
    Raw,
    /// ECDSA P-256: the token emits raw `r || s`; convert to DER.
    EcdsaP256,
    /// ECDSA P-384: the token emits raw `r || s`; convert to DER.
    EcdsaP384,
}

impl SigPost {
    fn finish(&self, raw: Vec<u8>) -> Result<Vec<u8>> {
        match self {
            SigPost::Raw => Ok(raw),
            SigPost::EcdsaP256 => ecdsa_p256_raw_to_der(&raw),
            SigPost::EcdsaP384 => ecdsa_p384_raw_to_der(&raw),
        }
    }
}

/// Map a TLS signature scheme to its PKCS#11 mechanism and post-processing.
///
/// All mechanisms are compound (hash-and-sign): the token hashes the full
/// message internally, matching rustls handing over the unhashed message.
fn mechanism_for_scheme(scheme: SignatureScheme) -> Result<(Mechanism<'static>, SigPost)> {
    use SignatureScheme as S;
    Ok(match scheme {
        S::RSA_PKCS1_SHA256 => (Mechanism::Sha256RsaPkcs, SigPost::Raw),
        S::RSA_PKCS1_SHA384 => (Mechanism::Sha384RsaPkcs, SigPost::Raw),
        S::RSA_PKCS1_SHA512 => (Mechanism::Sha512RsaPkcs, SigPost::Raw),
        S::RSA_PSS_SHA256 => (
            Mechanism::Sha256RsaPkcsPss(pss_params(MechanismType::SHA256)),
            SigPost::Raw,
        ),
        S::RSA_PSS_SHA384 => (
            Mechanism::Sha384RsaPkcsPss(pss_params(MechanismType::SHA384)),
            SigPost::Raw,
        ),
        S::RSA_PSS_SHA512 => (
            Mechanism::Sha512RsaPkcsPss(pss_params(MechanismType::SHA512)),
            SigPost::Raw,
        ),
        S::ECDSA_NISTP256_SHA256 => (Mechanism::EcdsaSha256, SigPost::EcdsaP256),
        S::ECDSA_NISTP384_SHA384 => (Mechanism::EcdsaSha384, SigPost::EcdsaP384),
        other => {
            return Err(Error::Tls {
                details: format!("unsupported SignatureScheme: {other:?}"),
            });
        }
    })
}

/// RSA-PSS parameters for `hash`, with the salt length pinned to the hash's
/// output length (RFC 8017 §9.1 recommendation; mandated by TLS 1.3).
///
/// Keying the MGF and salt length off the hash here keeps the three callers
/// from drifting — a mismatched salt length would silently produce a malformed
/// signature for just one hash size.
fn pss_params(hash: MechanismType) -> PkcsPssParams {
    let (mgf, s_len) = if hash == MechanismType::SHA256 {
        (PkcsMgfType::MGF1_SHA256, 32.into())
    } else if hash == MechanismType::SHA384 {
        (PkcsMgfType::MGF1_SHA384, 48.into())
    } else {
        // SHA-512 — the only remaining hash mechanism_for_scheme passes here.
        (PkcsMgfType::MGF1_SHA512, 64.into())
    };
    PkcsPssParams {
        hash_alg: hash,
        mgf,
        s_len,
    }
}

/// Sign `message` with the token, converting the result for rustls.
fn sign_message(
    session: &Session,
    key: ObjectHandle,
    scheme: SignatureScheme,
    message: &[u8],
) -> Result<Vec<u8>> {
    let (mechanism, post) = mechanism_for_scheme(scheme)?;
    let raw = session
        .sign(&mechanism, key, message)
        .map_err(|e| Error::Tls {
            details: format!("PKCS#11 C_Sign failed: {e}"),
        })?;
    post.finish(raw)
}

// --- ECDSA DER conversion ------------------------------------------------

/// Convert a raw PKCS#11 P-256 ECDSA signature (`r || s`, 64 bytes) to DER.
fn ecdsa_p256_raw_to_der(raw: &[u8]) -> Result<Vec<u8>> {
    let sig = p256::ecdsa::Signature::from_slice(raw).map_err(|e| Error::Tls {
        details: format!("invalid P-256 ECDSA signature from PKCS#11: {e}"),
    })?;
    Ok(sig.to_der().as_bytes().to_vec())
}

/// Convert a raw PKCS#11 P-384 ECDSA signature (`r || s`, 96 bytes) to DER.
fn ecdsa_p384_raw_to_der(raw: &[u8]) -> Result<Vec<u8>> {
    let sig = p384::ecdsa::Signature::from_slice(raw).map_err(|e| Error::Tls {
        details: format!("invalid P-384 ECDSA signature from PKCS#11: {e}"),
    })?;
    Ok(sig.to_der().as_bytes().to_vec())
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

    #[test]
    fn p256_valid_raw_produces_valid_der() {
        let mut raw = [0x12u8; 64];
        raw[32..].copy_from_slice(&[0x34u8; 32]);

        let der = ecdsa_p256_raw_to_der(&raw).unwrap();

        assert_eq!(der[0], 0x30, "output must start with DER SEQUENCE tag");
        let rt = p256::ecdsa::Signature::from_der(&der).unwrap();
        assert_eq!(rt.to_bytes()[..], raw[..]);
    }

    #[test]
    fn p384_valid_raw_produces_valid_der() {
        let mut raw = [0x12u8; 96];
        raw[48..].copy_from_slice(&[0x34u8; 48]);

        let der = ecdsa_p384_raw_to_der(&raw).unwrap();

        assert_eq!(der[0], 0x30, "output must start with DER SEQUENCE tag");
        let rt = p384::ecdsa::Signature::from_der(&der).unwrap();
        assert_eq!(rt.to_bytes()[..], raw[..]);
    }

    #[test]
    fn p256_wrong_length_is_rejected() {
        assert!(ecdsa_p256_raw_to_der(&[0x01u8; 63]).is_err());
        assert!(ecdsa_p256_raw_to_der(&[0x01u8; 65]).is_err());
    }

    #[test]
    fn p384_wrong_length_is_rejected() {
        assert!(ecdsa_p384_raw_to_der(&[0x01u8; 95]).is_err());
        assert!(ecdsa_p384_raw_to_der(&[0x01u8; 97]).is_err());
    }

    #[test]
    fn p256_zero_r_is_rejected() {
        let mut raw = [0x01u8; 64];
        raw[..32].fill(0x00);
        assert!(ecdsa_p256_raw_to_der(&raw).is_err());
    }

    #[test]
    fn p256_zero_s_is_rejected() {
        let mut raw = [0x01u8; 64];
        raw[32..].fill(0x00);
        assert!(ecdsa_p256_raw_to_der(&raw).is_err());
    }
}
