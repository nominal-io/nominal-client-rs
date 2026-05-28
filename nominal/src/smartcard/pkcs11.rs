// PKCS#11 session management and object discovery.

use crate::{Error, Result};
use cryptoki::context::Pkcs11;
use cryptoki::error::{Error as CryptokiError, RvError};
use cryptoki::object::{
    Attribute, AttributeType, CertificateType, KeyType, ObjectClass, ObjectHandle,
};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use rustls::SignatureAlgorithm;
use rustls::SignatureScheme;
use rustls::pki_types::CertificateDer;
use x509_cert::Certificate;
use x509_cert::der::Decode;
use x509_cert::der::asn1::ObjectIdentifier;
use x509_cert::ext::pkix::ExtendedKeyUsage;

/// OID for id-ce-extKeyUsage (2.5.29.37).
const EKU_EXTENSION_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.37");

/// OID for id-kp-clientAuth (RFC 5280 §4.2.1.12).
const CLIENT_AUTH_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.2");

pub(super) fn open_session(pkcs11: &Pkcs11, slot: Slot) -> Result<Session> {
    let session = pkcs11
        .open_ro_session(slot)
        .map_err(tls_err("C_OpenSession failed"))?;

    let token_info = pkcs11.get_token_info(slot).ok();

    let card_label = token_info
        .as_ref()
        .map(|info| info.label().trim().to_string())
        .filter(|s| !s.is_empty());

    // CKF_PROTECTED_AUTHENTICATION_PATH: the middleware manages PIN entry via its own UI
    // (e.g. ActivClient PIN dialog, Windows Hello). Passing a PIN string would fail with
    // CKR_ARGUMENTS_BAD; instead call C_Login with no PIN and let the middleware handle it.
    let protected_auth_path = token_info
        .as_ref()
        .map(|info| info.protected_authentication_path())
        .unwrap_or(false);

    if protected_auth_path {
        match session.login(UserType::User, None) {
            Ok(()) | Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {
                return Ok(session);
            }
            Err(CryptokiError::Pkcs11(RvError::PinLocked, _)) => {
                return Err(tls_static(
                    "Smartcard is locked after too many failed attempts; \
                     contact your administrator to reset the card",
                ));
            }
            Err(e) => {
                return Err(Error::Tls {
                    details: format!("C_Login (protected auth path) failed: {e}"),
                });
            }
        }
    }

    const MAX_ATTEMPTS: u32 = 3;

    for attempt in 1..=MAX_ATTEMPTS {
        let prompt = pin_prompt(&card_label, attempt, MAX_ATTEMPTS);

        let pin = rpassword::prompt_password(&prompt).map_err(tls_err("Failed to read PIN"))?;

        match session.login(UserType::User, Some(&AuthPin::new(pin.into_boxed_str()))) {
            Ok(()) => return Ok(session),

            Err(CryptokiError::Pkcs11(RvError::PinLocked, _)) => {
                return Err(tls_static(
                    "Smartcard is locked after too many failed attempts; \
                     contact your administrator to reset the card",
                ));
            }

            Err(CryptokiError::Pkcs11(RvError::PinExpired, _)) => {
                return Err(tls_static(
                    "Smartcard PIN has expired; contact your administrator \
                     to set a new PIN before connecting",
                ));
            }

            Err(CryptokiError::Pkcs11(RvError::PinIncorrect | RvError::PinLenRange, _))
                if attempt < MAX_ATTEMPTS =>
            {
                continue;
            }

            Err(CryptokiError::Pkcs11(RvError::PinIncorrect | RvError::PinLenRange, _)) => {
                return Err(tls_static("incorrect PIN after too many attempts"));
            }

            Err(e) => {
                return Err(Error::Tls {
                    details: format!("C_Login failed: {e}"),
                });
            }
        }
    }

    Err(tls_static("PIN authentication failed"))
}

fn pin_prompt(label: &Option<String>, attempt: u32, max_attempts: u32) -> String {
    if attempt == 1 {
        match label {
            Some(label) => format!("Enter PIN for {label}: "),
            None => "Enter smartcard PIN: ".to_string(),
        }
    } else {
        let remaining = max_attempts - attempt + 1;
        format!(
            "Incorrect PIN, {remaining} attempt{} remaining: ",
            if remaining == 1 { "" } else { "s" }
        )
    }
}

fn tls_err<E: std::fmt::Display>(context: &'static str) -> impl FnOnce(E) -> Error {
    move |e| Error::Tls {
        details: format!("{context}: {e}"),
    }
}

fn tls_static(msg: &'static str) -> Error {
    Error::Tls {
        details: msg.into(),
    }
}

/// Scan all token slots for a certificate carrying an id-kp-clientAuth EKU.
///
/// Sessions are opened without login since certificate objects on PIV cards are
/// public and readable unauthenticated. The PIN is only required later when
/// opening the signing session.
///
/// Returns `(slot, cert_der, key_id)` where `key_id` is the `CKA_ID` of the
/// matching certificate, which is used to locate the corresponding private key.
/// Deriving `key_id` from the cert avoids hardcoding OpenSC's `CKA_ID = 0x01`
/// convention, which does not apply to ActivClient and other middleware.
///
/// Returns an error if no certificate is found, if multiple clientAuth
/// certificates exist on a single slot, or if clientAuth certificates appear on
/// more than one slot simultaneously (two cards inserted at once).
pub(super) fn discover_piv_cert(
    pkcs11: &Pkcs11,
    slots: &[Slot],
) -> Result<(Slot, CertificateDer<'static>, Vec<u8>)> {
    let mut found: Option<(Slot, CertificateDer<'static>, Vec<u8>)> = None;

    for &slot in slots {
        let session = match pkcs11.open_ro_session(slot) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(slot = ?slot, error = ?e, "skipping slot: failed to open session");
                continue;
            }
        };

        let handles = match session.find_objects(&[
            Attribute::Class(ObjectClass::CERTIFICATE),
            Attribute::CertificateType(CertificateType::X_509),
        ]) {
            Ok(h) => h,
            Err(e) => {
                tracing::debug!(slot = ?slot, error = ?e, "skipping slot: C_FindObjects failed");
                continue;
            }
        };

        // Collect every cert on this slot that carries id-kp-clientAuth.
        let mut slot_matches: Vec<(CertificateDer<'static>, Vec<u8>)> = Vec::new();

        for handle in handles {
            let attrs = match session
                .get_attributes(handle, &[AttributeType::Value, AttributeType::Id])
            {
                Ok(a) => a,
                Err(e) => {
                    tracing::debug!(error = ?e, "skipping cert object: C_GetAttributeValue failed");
                    continue;
                }
            };

            let cert_bytes = attrs.iter().find_map(|a| {
                if let Attribute::Value(v) = a {
                    Some(v.clone())
                } else {
                    None
                }
            });
            let key_id = attrs.iter().find_map(|a| {
                if let Attribute::Id(id) = a {
                    Some(id.clone())
                } else {
                    None
                }
            });

            let (Some(cert_bytes), Some(key_id)) = (cert_bytes, key_id) else {
                tracing::debug!("skipping cert object: missing CKA_VALUE or CKA_ID");
                continue;
            };

            if check_client_auth_eku(&cert_bytes).is_ok() {
                slot_matches.push((CertificateDer::from(cert_bytes), key_id));
            }
        }

        match slot_matches.len() {
            0 => continue,
            1 => {
                if found.is_some() {
                    return Err(Error::Tls {
                        details: "client-auth certificates found on multiple token slots; \
                                  only one smartcard should be inserted at a time"
                            .into(),
                    });
                }
                let (cert, key_id) = slot_matches.into_iter().next().unwrap();
                found = Some((slot, cert, key_id));
            }
            n => {
                return Err(Error::Tls {
                    details: format!(
                        "{n} certificates with id-kp-clientAuth EKU found on a single token slot; \
                         use {env} to select a specific PKCS#11 module or contact support",
                        env = super::PKCS11_MODULE_ENV_VAR
                    ),
                });
            }
        }
    }

    found.ok_or_else(|| Error::Tls {
        details: format!(
            "no certificate with id-kp-clientAuth EKU found on any token slot; \
             ensure the card is inserted and middleware is installed, or set {} \
             to override the module path",
            super::PKCS11_MODULE_ENV_VAR
        ),
    })
}

/// Return an error if the certificate does not contain id-kp-clientAuth in its
/// Extended Key Usage extension.
fn check_client_auth_eku(cert_der: &[u8]) -> Result<()> {
    let cert = Certificate::from_der(cert_der).map_err(|e| Error::Tls {
        details: format!("failed to parse certificate: {e}"),
    })?;

    let extensions = cert
        .tbs_certificate
        .extensions
        .as_deref()
        .unwrap_or_default();

    for ext in extensions {
        if ext.extn_id == EKU_EXTENSION_OID {
            let eku =
                ExtendedKeyUsage::from_der(ext.extn_value.as_bytes()).map_err(|e| Error::Tls {
                    details: format!("failed to decode Extended Key Usage extension: {e}"),
                })?;
            if eku.0.iter().any(|oid| *oid == CLIENT_AUTH_OID) {
                return Ok(());
            }
            return Err(Error::Tls {
                details: "certificate does not include id-kp-clientAuth in its \
                          Extended Key Usage; this certificate cannot be used for TLS \
                          client authentication"
                    .into(),
            });
        }
    }

    Err(Error::Tls {
        details: "certificate has no Extended Key Usage extension; \
                  expected id-kp-clientAuth for TLS client authentication"
            .into(),
    })
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
/// PSS is listed before PKCS#1 so TLS 1.3 servers negotiate PSS while TLS 1.2
/// servers that only support PKCS#1 still find a match.
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

    #[test]
    fn rsa_algorithm_family_is_rsa() {
        let (_, alg) = schemes_for_key_type(KeyType::RSA).unwrap();
        assert_eq!(alg, SignatureAlgorithm::RSA);
    }

    #[test]
    fn rsa_includes_all_required_tls13_pss_variants() {
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
        let result = schemes_for_key_type(KeyType::AES);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unsupported"),
            "error should mention 'unsupported', got: {msg}"
        );
    }
}
