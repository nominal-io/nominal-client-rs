// PKCS#11 session management and object discovery.
//
// All functions here interact directly with the PKCS#11 C API through the
// cryptoki crate. No rustls types appear here; signing concerns live in
// signing.rs instead.

use cryptoki::context::Pkcs11;
use cryptoki::object::{Attribute, AttributeType, KeyType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use rustls::SignatureAlgorithm;
use rustls::SignatureScheme;
use rustls::pki_types::CertificateDer;
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Open a read-only PKCS#11 session on `slot`, prompting the user interactively
/// for their PIN. The PIN is used immediately for `C_Login` and never stored.
pub(super) fn open_session(pkcs11: &Pkcs11, slot: Slot) -> Result<Session> {
    let session = pkcs11.open_ro_session(slot).map_err(|e| Error::Tls {
        details: format!("C_OpenSession failed: {e}"),
    })?;
    let pin = rpassword::prompt_password("Enter smartcard PIN: ").map_err(|e| Error::Tls {
        details: format!("failed to read PIN: {e}"),
    })?;
    session
        .login(UserType::User, Some(&AuthPin::new(pin.into_boxed_str())))
        .map_err(|e| Error::Tls {
            details: format!("C_Login failed: {e}"),
        })?;
    Ok(session)
}

/// Find a certificate on the token, returning its DER bytes and `CKA_ID`.
///
/// When `fingerprint` is `Some`, the certificate whose SHA-256 fingerprint
/// (lowercase hex, no colons) matches is selected; an error is returned if no
/// match is found.
///
/// When `fingerprint` is `None`, the first certificate object that has a
/// corresponding private key is returned. CAC cards typically order slots as
/// PIV Authentication (9A), Card Authentication, Digital Signature, Key
/// Management — so the first match is almost always the correct one for mTLS.
pub(super) fn find_certificate(
    session: &Session,
    fingerprint: Option<&str>,
) -> Result<(CertificateDer<'static>, Vec<u8>)> {
    let handles = session
        .find_objects(&[Attribute::Class(ObjectClass::CERTIFICATE)])
        .map_err(|e| Error::Tls {
            details: format!("C_FindObjects (certificate) failed: {e}"),
        })?;

    if handles.is_empty() {
        return Err(Error::Tls {
            details: "no certificate objects found on PKCS#11 token".into(),
        });
    }

    for handle in handles {
        let attrs = session
            .get_attributes(handle, &[AttributeType::Value, AttributeType::Id])
            .map_err(|e| Error::Tls {
                details: format!("C_GetAttributeValue (certificate) failed: {e}"),
            })?;

        let cert_bytes = attrs.iter().find_map(|a| {
            if let Attribute::Value(v) = a {
                Some(v.clone())
            } else {
                None
            }
        });
        let key_id = attrs.iter().find_map(|a| {
            if let Attribute::Id(v) = a {
                Some(v.clone())
            } else {
                None
            }
        });

        let (Some(cert_bytes), Some(key_id)) = (cert_bytes, key_id) else {
            continue;
        };

        if let Some(fp) = fingerprint {
            let actual: String = Sha256::digest(&cert_bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if actual != fp {
                continue;
            }
            return Ok((CertificateDer::from(cert_bytes), key_id));
        }

        if key_exists(session, &key_id) {
            return Ok((CertificateDer::from(cert_bytes), key_id));
        }
    }

    Err(Error::Tls {
        details: match fingerprint {
            Some(fp) => format!("no certificate with fingerprint {fp} found on token"),
            None => "no certificate with a corresponding private key found on token".into(),
        },
    })
}

/// Return `true` if a private key with the given `CKA_ID` exists on the token.
fn key_exists(session: &Session, key_id: &[u8]) -> bool {
    session
        .find_objects(&[
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Id(key_id.to_vec()),
        ])
        .ok()
        .is_some_and(|v| !v.is_empty())
}

/// Find the private key whose `CKA_ID` matches `key_id` and return its handle.
pub(super) fn find_key_handle(session: &Session, key_id: &[u8]) -> Result<ObjectHandle> {
    session
        .find_objects(&[
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Id(key_id.to_vec()),
        ])
        .map_err(|e| Error::Tls {
            details: format!("C_FindObjects (private key) failed: {e}"),
        })?
        .into_iter()
        .next()
        .ok_or_else(|| Error::Tls {
            details: format!("private key with CKA_ID {key_id:02x?} not found on token"),
        })
}

/// Read the `CKA_KEY_TYPE` attribute of the private key identified by `key_id`.
pub(super) fn probe_key_type(session: &Session, key_id: &[u8]) -> Result<KeyType> {
    let handle = find_key_handle(session, key_id)?;
    let attrs = session
        .get_attributes(handle, &[AttributeType::KeyType])
        .map_err(|e| Error::Tls {
            details: format!("C_GetAttributeValue (key type) failed: {e}"),
        })?;
    attrs
        .iter()
        .find_map(|a| {
            if let Attribute::KeyType(kt) = a {
                Some(*kt)
            } else {
                None
            }
        })
        .ok_or_else(|| Error::Tls {
            details: "CKA_KEY_TYPE attribute missing from private key".into(),
        })
}

/// Return the rustls signature schemes and algorithm family for `key_type`.
///
/// For RSA, PSS variants are listed before PKCS#1 because TLS 1.3 requires PSS
/// and prohibits PKCS#1. Listing PSS first means a TLS 1.3 server will always
/// negotiate PSS, while a TLS 1.2 server that only supports PKCS#1 will still
/// find a match.
pub(super) fn schemes_for_key_type(
    key_type: KeyType,
) -> Result<(Vec<SignatureScheme>, SignatureAlgorithm)> {
    match key_type {
        KeyType::RSA => Ok((
            vec![
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
            ],
            SignatureAlgorithm::RSA,
        )),
        KeyType::EC => Ok((
            vec![
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
            ],
            SignatureAlgorithm::ECDSA,
        )),
        _ => Err(Error::Tls {
            details: format!("unsupported PKCS#11 key type: {key_type:?}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- schemes_for_key_type ----------------------------------------------
    //
    // These tests verify the advertised scheme list, which determines what
    // TLS versions and hash algorithms are available. Mistakes here directly
    // affect which TLS handshakes succeed.

    #[test]
    fn rsa_algorithm_family_is_rsa() {
        let (_, alg) = schemes_for_key_type(KeyType::RSA).unwrap();
        assert_eq!(alg, SignatureAlgorithm::RSA);
    }

    #[test]
    fn rsa_includes_all_required_tls13_pss_variants() {
        // TLS 1.3 (RFC 8446 §4.2.3) requires these three PSS schemes.
        let (schemes, _) = schemes_for_key_type(KeyType::RSA).unwrap();
        assert!(
            schemes.contains(&SignatureScheme::RSA_PSS_SHA256),
            "RSA_PSS_SHA256 required"
        );
        assert!(
            schemes.contains(&SignatureScheme::RSA_PSS_SHA384),
            "RSA_PSS_SHA384 required"
        );
        assert!(
            schemes.contains(&SignatureScheme::RSA_PSS_SHA512),
            "RSA_PSS_SHA512 required"
        );
    }

    #[test]
    fn rsa_pss_listed_before_pkcs1_for_tls13_preference() {
        // TLS 1.3 forbids PKCS#1 v1.5 for handshake signatures. Listing PSS
        // first ensures it is negotiated when both sides support it.
        let (schemes, _) = schemes_for_key_type(KeyType::RSA).unwrap();
        let pss_pos = schemes
            .iter()
            .position(|s| *s == SignatureScheme::RSA_PSS_SHA256)
            .expect("RSA_PSS_SHA256 must be present");
        let pkcs1_pos = schemes
            .iter()
            .position(|s| *s == SignatureScheme::RSA_PKCS1_SHA256)
            .expect("RSA_PKCS1_SHA256 must be present");
        assert!(
            pss_pos < pkcs1_pos,
            "PSS (pos {pss_pos}) must come before PKCS#1 (pos {pkcs1_pos})"
        );
    }

    #[test]
    fn rsa_does_not_include_ecdsa_schemes() {
        let (schemes, _) = schemes_for_key_type(KeyType::RSA).unwrap();
        assert!(
            !schemes.contains(&SignatureScheme::ECDSA_NISTP256_SHA256),
            "RSA key must not advertise ECDSA"
        );
    }

    #[test]
    fn ec_algorithm_family_is_ecdsa() {
        let (_, alg) = schemes_for_key_type(KeyType::EC).unwrap();
        assert_eq!(alg, SignatureAlgorithm::ECDSA);
    }

    #[test]
    fn ec_includes_p256_and_p384() {
        let (schemes, _) = schemes_for_key_type(KeyType::EC).unwrap();
        assert!(schemes.contains(&SignatureScheme::ECDSA_NISTP256_SHA256));
        assert!(schemes.contains(&SignatureScheme::ECDSA_NISTP384_SHA384));
    }

    #[test]
    fn ec_does_not_include_rsa_schemes() {
        let (schemes, _) = schemes_for_key_type(KeyType::EC).unwrap();
        assert!(
            !schemes.contains(&SignatureScheme::RSA_PSS_SHA256),
            "ECDSA key must not advertise RSA"
        );
        assert!(!schemes.contains(&SignatureScheme::RSA_PKCS1_SHA256));
    }

    #[test]
    fn unsupported_key_type_returns_tls_error() {
        // AES keys are symmetric and cannot be used for TLS signatures.
        let result = schemes_for_key_type(KeyType::AES);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unsupported"),
            "error should mention 'unsupported', got: {msg}"
        );
    }
}
