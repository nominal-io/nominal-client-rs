//! PKCS#11 session management and object discovery.

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
use zeroize::Zeroizing;

use super::PKCS11_MODULE_ENV_VAR;

/// OID for id-ce-extKeyUsage (2.5.29.37).
const EKU_EXTENSION_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.37");

/// OID for id-kp-clientAuth (RFC 5280 §4.2.1.12).
const CLIENT_AUTH_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.2");

/// OID for id-PIV-cardAuth (NIST SP 800-73, 2.16.840.1.101.3.6.8).
///
/// This EKU marks the PIV Card Authentication certificate (slot 9E), whose key
/// is provisioned with a PIN policy of `NEVER`. The 9E cert also carries
/// id-kp-clientAuth, so it is rejected as a *leaf* candidate in preference to
/// the PIV Authentication certificate (slot 9A) when both are present.
const PIV_CARD_AUTH_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.6.8");

/// OID for the NIST P-256 named curve (secp256r1 / prime256v1).
const P256_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

/// OID for the NIST P-384 named curve (secp384r1).
const P384_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");

/// A discovered client-authentication certificate and the data needed to use it.
pub(super) struct DiscoveredCert {
    /// The leaf certificate presented during the handshake. CACs store only
    /// leaf certificates; the server is expected to hold the issuing CAs.
    pub(super) cert: CertificateDer<'static>,
    /// How to locate the leaf's private key on the token.
    pub(super) key: KeyLocator,
}

/// Identifiers used to find the private key that pairs with a certificate.
pub(super) struct KeyLocator {
    /// The certificate's `CKA_ID`. PKCS#11 says a cert and its key SHOULD share
    /// this value; most middleware honors it.
    pub(super) id: Vec<u8>,
    /// The certificate's `CKA_LABEL`, if any — a fallback for middleware that
    /// does not pair `CKA_ID`s.
    pub(super) label: Option<Vec<u8>>,
}

/// Open a read-only session on `slot` and log the user in, prompting for the
/// PIN on the terminal.
///
/// Tokens with `CKF_PROTECTED_AUTHENTICATION_PATH` (e.g. ActivClient) drive PIN
/// entry through their own UI instead. The card's try-counter is re-read before
/// each terminal prompt so a retry never spends the card's final attempt.
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
        .is_some_and(|info| info.protected_authentication_path());

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

    // Software cap on prompts. The real safeguard against locking the card is
    // the per-attempt check of the token's hardware try-counter below; this
    // bound just stops an endless prompt loop if the flags are unavailable.
    const MAX_ATTEMPTS: u32 = 3;

    let mut prompted = false;
    for _ in 0..MAX_ATTEMPTS {
        // Re-read the card's PIN state before every attempt so a wrong guess is
        // never driven into a lock. Flags update after each failed C_Login.
        let token_info = pkcs11.get_token_info(slot).ok();
        if token_info.as_ref().is_some_and(|i| i.user_pin_locked()) {
            return Err(tls_static(
                "Smartcard is locked after too many failed attempts; \
                 contact your administrator to reset the card",
            ));
        }
        let final_try = token_info.as_ref().is_some_and(|i| i.user_pin_final_try());
        let count_low = token_info.as_ref().is_some_and(|i| i.user_pin_count_low());

        // Never let the retry loop spend the card's last try. If we have
        // already prompted this run and the card is now down to its final
        // attempt, stop *before* attempting and make the user re-run
        // deliberately — auto-retrying here would lock the card. A card that is
        // already on its final try when we start (prompted == false) still gets
        // one informed attempt, with a warning in the prompt.
        if final_try && prompted {
            return Err(tls_static(
                "Incorrect PIN; the card is now down to its final attempt. \
                 Not retrying automatically to avoid locking it — re-run and \
                 enter the correct PIN.",
            ));
        }

        let prompt = pin_prompt(card_label.as_deref(), prompted, final_try, count_low);
        prompted = true;

        // Hold the PIN in a zeroizing buffer so the plaintext is wiped on every
        // exit path, then copy it into an exact-size Box<str> for AuthPin
        // (itself a zeroizing SecretString). Copying via `Box::from(&str)`
        // avoids `String::into_boxed_str`'s shrink-to-fit, which can reallocate
        // and leak the original PIN buffer unzeroized.
        let pin = Zeroizing::new(
            rpassword::prompt_password(&prompt).map_err(tls_err("Failed to read PIN"))?,
        );
        let auth_pin = AuthPin::new(Box::<str>::from(pin.as_str()));

        match session.login(UserType::User, Some(&auth_pin)) {
            Ok(()) | Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {
                return Ok(session);
            }

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

            Err(CryptokiError::Pkcs11(RvError::PinIncorrect | RvError::PinLenRange, _)) => {
                // Loop to re-prompt. The next iteration re-reads the card's
                // try-counter and bails before spending the final attempt.
                continue;
            }

            Err(e) => {
                return Err(Error::Tls {
                    details: format!("C_Login failed: {e}"),
                });
            }
        }
    }

    Err(tls_static("incorrect PIN after too many attempts"))
}

/// Build the PIN prompt, sourcing any "attempts remaining" warning from the
/// card's real try-counter flags rather than a fabricated software count.
fn pin_prompt(label: Option<&str>, is_retry: bool, final_try: bool, count_low: bool) -> String {
    let target = match label {
        Some(label) => format!("PIN for {label}"),
        None => "smartcard PIN".to_string(),
    };
    let prefix = if is_retry { "Incorrect PIN. " } else { "" };
    let warning = if final_try {
        " (WARNING: final attempt before the card locks)"
    } else if count_low {
        " (warning: few attempts remain before the card locks)"
    } else {
        ""
    };
    format!("{prefix}Enter {target}{warning}: ")
}

pub(super) fn tls_err<E: std::fmt::Display>(context: &'static str) -> impl FnOnce(E) -> Error {
    move |e| Error::Tls {
        details: format!("{context}: {e}"),
    }
}

fn tls_static(msg: &'static str) -> Error {
    Error::Tls {
        details: msg.into(),
    }
}

/// A certificate read off a single token slot, with the fields needed for leaf
/// selection.
struct SlotCert {
    der: CertificateDer<'static>,
    key_id: Option<Vec<u8>>,
    label: Option<Vec<u8>>,
    /// Carries id-kp-clientAuth — usable for TLS client authentication.
    client_auth: bool,
    /// Carries id-PIV-cardAuth — the 9E Card Authentication cert.
    card_auth: bool,
}

/// Scan all token slots for a TLS client-authentication certificate.
///
/// Sessions are opened without login since certificate objects on PIV cards are
/// public and readable unauthenticated. The PIN is only required later when
/// opening the signing session.
///
/// On a standard PIV/CAC card several certificates carry id-kp-clientAuth (the
/// 9A PIV Authentication cert and the 9E Card Authentication cert at minimum),
/// so finding more than one is normal, not an error. Selection prefers the PIV
/// Authentication cert by excluding 9E (id-PIV-cardAuth) candidates; remaining
/// ties break on the smallest `CKA_ID` for determinism.
///
/// The same physical card exposed over more than one reader/slot (contact +
/// contactless) is deduplicated by token serial number, so it is not mistaken
/// for two inserted cards. An error is only returned when no usable certificate
/// is found, or when client-auth certificates from two genuinely different
/// cards are present at once.
pub(super) fn discover_piv_cert(pkcs11: &Pkcs11, slots: &[Slot]) -> Result<(Slot, DiscoveredCert)> {
    // (slot, token serial, discovered cert) for the first card we accept.
    let mut found: Option<(Slot, String, DiscoveredCert)> = None;

    for &slot in slots {
        let serial = pkcs11
            .get_token_info(slot)
            .ok()
            .map(|info| info.serial_number().trim().to_string())
            .unwrap_or_default();

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

        let slot_certs: Vec<SlotCert> = handles
            .into_iter()
            .filter_map(|handle| read_slot_cert(&session, handle))
            .collect();

        let Some(leaf) = select_leaf(&slot_certs) else {
            continue;
        };

        let discovered = DiscoveredCert {
            cert: leaf.der.clone(),
            key: KeyLocator {
                // `select_leaf` only returns candidates that have a CKA_ID.
                id: leaf.key_id.clone().unwrap_or_default(),
                label: leaf.label.clone(),
            },
        };

        match &found {
            Some((_, prev_serial, _)) if same_physical_card(prev_serial, &serial) => {
                tracing::debug!(slot = ?slot, "same card on another slot; keeping first match");
            }
            Some((_, _, prev)) if prev.cert.as_ref() == discovered.cert.as_ref() => {
                tracing::debug!(slot = ?slot, "identical certificate on another slot; ignoring");
            }
            Some(_) => {
                return Err(Error::Tls {
                    details: "client-auth certificates found on two different smartcards; \
                         only one smartcard should be inserted at a time"
                        .into(),
                });
            }
            None => found = Some((slot, serial, discovered)),
        }
    }

    found
        .map(|(slot, _, d)| (slot, d))
        .ok_or_else(|| Error::Tls {
            details: format!(
                "no certificate with id-kp-clientAuth EKU found on any token slot; \
             ensure the card is inserted and middleware is installed, or set \
             {PKCS11_MODULE_ENV_VAR} to override the module path"
            ),
        })
}

/// Read and classify one certificate object. Returns `None` (and logs) when the
/// object is missing required attributes or cannot be parsed.
fn read_slot_cert(session: &Session, handle: ObjectHandle) -> Option<SlotCert> {
    let attrs = match session.get_attributes(
        handle,
        &[
            AttributeType::Value,
            AttributeType::Id,
            AttributeType::Label,
        ],
    ) {
        Ok(a) => a,
        Err(e) => {
            tracing::debug!(error = ?e, "skipping cert object: C_GetAttributeValue failed");
            return None;
        }
    };

    let mut value = None;
    let mut key_id = None;
    let mut label = None;
    for attr in attrs {
        match attr {
            Attribute::Value(v) => value = Some(v),
            Attribute::Id(id) => key_id = Some(id),
            Attribute::Label(l) => label = Some(l),
            _ => {}
        }
    }

    let Some(value) = value else {
        tracing::debug!("skipping cert object: missing CKA_VALUE");
        return None;
    };

    let parsed = match Certificate::from_der(&value) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = ?e, "skipping cert object: failed to parse DER");
            return None;
        }
    };

    let eku = classify_eku(&parsed);

    Some(SlotCert {
        der: CertificateDer::from(value),
        key_id,
        label,
        client_auth: eku.client_auth,
        card_auth: eku.card_auth,
    })
}

/// Classify a parsed certificate's Extended Key Usage.
struct EkuClass {
    client_auth: bool,
    card_auth: bool,
}

fn classify_eku(cert: &Certificate) -> EkuClass {
    let mut class = EkuClass {
        client_auth: false,
        card_auth: false,
    };

    let extensions = cert
        .tbs_certificate
        .extensions
        .as_deref()
        .unwrap_or_default();

    for ext in extensions {
        if ext.extn_id != EKU_EXTENSION_OID {
            continue;
        }
        let Ok(eku) = ExtendedKeyUsage::from_der(ext.extn_value.as_bytes()) else {
            tracing::debug!("failed to decode Extended Key Usage extension; skipping");
            continue;
        };
        class.client_auth = eku.0.contains(&CLIENT_AUTH_OID);
        class.card_auth = eku.0.contains(&PIV_CARD_AUTH_OID);
    }

    class
}

/// Select the leaf certificate to present from one slot's certificates.
///
/// Considers only client-auth certs that carry a `CKA_ID` (required to find the
/// matching private key). Prefers the PIV Authentication certificate over the
/// Card Authentication (9E) cert by dropping card-auth candidates whenever
/// another candidate exists, then breaks ties on the smallest `CKA_ID` for a
/// deterministic result. Returns `None` when no usable candidate exists on the
/// slot.
fn select_leaf(certs: &[SlotCert]) -> Option<&SlotCert> {
    let eligible: Vec<&SlotCert> = certs
        .iter()
        .filter(|c| c.client_auth && c.key_id.is_some())
        .collect();

    let any_piv_auth = eligible.iter().any(|c| !c.card_auth);
    let pool: Vec<&SlotCert> = eligible
        .into_iter()
        .filter(|c| !c.card_auth || !any_piv_auth)
        .collect();

    let best = pool
        .iter()
        .min_by(|a, b| a.key_id.cmp(&b.key_id))
        .copied()?;

    if pool.len() > 1 {
        tracing::warn!(
            chosen_cka_id = ?best.key_id,
            "multiple equally-eligible client-auth certificates on one slot; \
             selected the lowest CKA_ID"
        );
    }

    Some(best)
}

/// True when two slots clearly belong to the same physical card (matching,
/// non-empty token serial numbers — e.g. a card's contact and contactless
/// interfaces).
fn same_physical_card(serial_a: &str, serial_b: &str) -> bool {
    !serial_a.is_empty() && serial_a == serial_b
}

/// Find the private key paired with the certificate located by `locator`.
///
/// Tries, in order: the cert's `CKA_ID` (the PKCS#11 SHOULD-pairing), the cert's
/// `CKA_LABEL` (middleware that does not pair IDs), and finally the sole private
/// key on the token. This tolerates middleware (e.g. some ActivClient setups)
/// that assigns different `CKA_ID`s to a cert and its key.
pub(super) fn find_key_handle(session: &Session, locator: &KeyLocator) -> Result<ObjectHandle> {
    if let Some(handle) = first_private_key(
        session,
        &[
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Id(locator.id.clone()),
        ],
    )? {
        return Ok(handle);
    }

    if let Some(label) = &locator.label {
        if let Some(handle) = first_private_key(
            session,
            &[
                Attribute::Class(ObjectClass::PRIVATE_KEY),
                Attribute::Label(label.clone()),
            ],
        )? {
            tracing::debug!("matched private key by CKA_LABEL (CKA_ID did not pair)");
            return Ok(handle);
        }
    }

    // Last resort: a single-key token unambiguously identifies the key.
    let all = session
        .find_objects(&[Attribute::Class(ObjectClass::PRIVATE_KEY)])
        .map_err(tls_err("C_FindObjects (private key) failed"))?;
    match all.as_slice() {
        [only] => {
            tracing::debug!("matched the token's only private key (CKA_ID/CKA_LABEL did not pair)");
            Ok(*only)
        }
        [] => Err(tls_static(
            "no private key found on the token; the certificate's key may not \
             be present or readable",
        )),
        _ => Err(Error::Tls {
            details: format!(
                "could not match the certificate to a private key: no key with \
                 CKA_ID {:02x?}, and the token holds multiple private keys",
                locator.id
            ),
        }),
    }
}

fn first_private_key(session: &Session, template: &[Attribute]) -> Result<Option<ObjectHandle>> {
    Ok(session
        .find_objects(template)
        .map_err(tls_err("C_FindObjects (private key) failed"))?
        .into_iter()
        .next())
}

/// Read the `CKA_KEY_TYPE` of the private key `handle`, plus its
/// `CKA_EC_PARAMS` (the curve OID) when the key is EC.
///
/// Takes an already-resolved handle so the caller's [`find_key_handle`] result
/// is reused rather than searching the token a second time.
pub(super) fn probe_key(
    session: &Session,
    handle: ObjectHandle,
) -> Result<(KeyType, Option<Vec<u8>>)> {
    let attrs = session
        .get_attributes(handle, &[AttributeType::KeyType])
        .map_err(|e| Error::Tls {
            details: format!("C_GetAttributeValue (key type) failed: {e}"),
        })?;
    let key_type = attrs
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
        })?;

    // The curve only matters for EC keys; querying CKA_EC_PARAMS on an RSA key
    // can return CKR_ATTRIBUTE_TYPE_INVALID on some tokens, so gate on the type.
    let ec_params = if key_type == KeyType::EC {
        let attrs = session
            .get_attributes(handle, &[AttributeType::EcParams])
            .map_err(|e| Error::Tls {
                details: format!("C_GetAttributeValue (EC params) failed: {e}"),
            })?;
        attrs.into_iter().find_map(|a| {
            if let Attribute::EcParams(p) = a {
                Some(p)
            } else {
                None
            }
        })
    } else {
        None
    };

    Ok((key_type, ec_params))
}

/// Return the rustls signature schemes and algorithm family for a key.
///
/// For RSA, both PSS and PKCS#1 schemes are supported. Only set membership
/// matters: the signer picks the first scheme the *server* offers that appears
/// in this list, and rustls itself excludes PKCS#1 from TLS 1.3 negotiation.
///
/// For EC, only the single scheme matching the key's actual curve (decoded from
/// `ec_params`, the `CKA_EC_PARAMS` value) is advertised. Advertising a scheme
/// for the wrong curve (e.g. ECDSA_NISTP384 for a P-256 key) makes rustls sign
/// with a mismatched mechanism, producing a signature the server rejects under
/// RFC 8446 §4.2.3 and that fails our raw→DER conversion.
pub(super) fn schemes_for_key_type(
    key_type: KeyType,
    ec_params: Option<&[u8]>,
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
        KeyType::EC => {
            let params = ec_params.ok_or_else(|| Error::Tls {
                details: "EC key is missing CKA_EC_PARAMS; cannot determine its \
                          curve for TLS signature scheme selection"
                    .into(),
            })?;
            let scheme = match curve_from_ec_params(params) {
                Some(Curve::P256) => SignatureScheme::ECDSA_NISTP256_SHA256,
                Some(Curve::P384) => SignatureScheme::ECDSA_NISTP384_SHA384,
                None => {
                    return Err(Error::Tls {
                        details: format!(
                            "unsupported EC curve (CKA_EC_PARAMS = {params:02x?}); \
                             only NIST P-256 and P-384 are supported"
                        ),
                    });
                }
            };
            Ok((vec![scheme], SignatureAlgorithm::ECDSA))
        }
        _ => Err(Error::Tls {
            details: format!("unsupported PKCS#11 key type: {key_type:?}"),
        }),
    }
}

/// A supported NIST curve.
#[derive(Debug, PartialEq, Eq)]
enum Curve {
    P256,
    P384,
}

/// Identify the curve from a `CKA_EC_PARAMS` value (an `ECParameters`
/// namedCurve `OBJECT IDENTIFIER`).
fn curve_from_ec_params(params: &[u8]) -> Option<Curve> {
    match ObjectIdentifier::from_der(params).ok()? {
        oid if oid == P256_OID => Some(Curve::P256),
        oid if oid == P384_OID => Some(Curve::P384),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DER `CKA_EC_PARAMS` (namedCurve OID) for NIST P-256 (1.2.840.10045.3.1.7).
    const EC_PARAMS_P256: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
    /// DER `CKA_EC_PARAMS` (namedCurve OID) for NIST P-384 (1.3.132.0.34).
    const EC_PARAMS_P384: &[u8] = &[0x06, 0x05, 0x2B, 0x81, 0x04, 0x00, 0x22];

    fn slot_cert(client_auth: bool, card_auth: bool, key_id: &[u8]) -> SlotCert {
        SlotCert {
            der: CertificateDer::from(vec![0x30]),
            key_id: Some(key_id.to_vec()),
            label: None,
            client_auth,
            card_auth,
        }
    }

    #[test]
    fn rsa_algorithm_family_is_rsa() {
        let (_, alg) = schemes_for_key_type(KeyType::RSA, None).unwrap();
        assert_eq!(alg, SignatureAlgorithm::RSA);
    }

    #[test]
    fn rsa_includes_all_required_tls13_pss_variants() {
        let (schemes, _) = schemes_for_key_type(KeyType::RSA, None).unwrap();
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
    fn rsa_does_not_include_ecdsa_schemes() {
        let (schemes, _) = schemes_for_key_type(KeyType::RSA, None).unwrap();
        assert!(
            !schemes.contains(&SignatureScheme::ECDSA_NISTP256_SHA256),
            "RSA key must not advertise ECDSA"
        );
    }

    #[test]
    fn ec_algorithm_family_is_ecdsa() {
        let (_, alg) = schemes_for_key_type(KeyType::EC, Some(EC_PARAMS_P256)).unwrap();
        assert_eq!(alg, SignatureAlgorithm::ECDSA);
    }

    #[test]
    fn ec_p256_advertises_only_p256() {
        let (schemes, _) = schemes_for_key_type(KeyType::EC, Some(EC_PARAMS_P256)).unwrap();
        assert_eq!(schemes, vec![SignatureScheme::ECDSA_NISTP256_SHA256]);
        assert!(
            !schemes.contains(&SignatureScheme::ECDSA_NISTP384_SHA384),
            "a P-256 key must not advertise the P-384 scheme"
        );
    }

    #[test]
    fn ec_p384_advertises_only_p384() {
        let (schemes, _) = schemes_for_key_type(KeyType::EC, Some(EC_PARAMS_P384)).unwrap();
        assert_eq!(schemes, vec![SignatureScheme::ECDSA_NISTP384_SHA384]);
        assert!(
            !schemes.contains(&SignatureScheme::ECDSA_NISTP256_SHA256),
            "a P-384 key must not advertise the P-256 scheme"
        );
    }

    #[test]
    fn ec_does_not_include_rsa_schemes() {
        let (schemes, _) = schemes_for_key_type(KeyType::EC, Some(EC_PARAMS_P256)).unwrap();
        assert!(
            !schemes.contains(&SignatureScheme::RSA_PSS_SHA256),
            "ECDSA key must not advertise RSA"
        );
        assert!(!schemes.contains(&SignatureScheme::RSA_PKCS1_SHA256));
    }

    #[test]
    fn ec_missing_params_returns_error() {
        let result = schemes_for_key_type(KeyType::EC, None);
        assert!(result.is_err(), "EC key without curve params must error");
    }

    #[test]
    fn ec_unsupported_curve_returns_error() {
        // OID 1.3.101.112 (Ed25519) — not a supported NIST P-curve.
        let unknown = [0x06u8, 0x03, 0x2B, 0x65, 0x70];
        let result = schemes_for_key_type(KeyType::EC, Some(&unknown));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unsupported EC curve"),
            "error should mention the unsupported curve, got: {msg}"
        );
    }

    #[test]
    fn unsupported_key_type_returns_tls_error() {
        let result = schemes_for_key_type(KeyType::AES, None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unsupported"),
            "error should mention 'unsupported', got: {msg}"
        );
    }

    #[test]
    fn curve_decoded_from_named_curve_oid() {
        assert_eq!(curve_from_ec_params(EC_PARAMS_P256), Some(Curve::P256));
        assert_eq!(curve_from_ec_params(EC_PARAMS_P384), Some(Curve::P384));
    }

    #[test]
    fn curve_unknown_oid_returns_none() {
        // OID 1.3.101.112 (Ed25519) — not a supported NIST P-curve.
        let unknown = [0x06u8, 0x03, 0x2B, 0x65, 0x70];
        assert_eq!(curve_from_ec_params(&unknown), None);
    }

    #[test]
    fn select_leaf_prefers_piv_auth_over_card_auth() {
        // 9E card-auth cert has the lower CKA_ID but must lose to the 9A
        // PIV-auth cert when both are present.
        let certs = vec![
            slot_cert(true, true, &[0x01]),
            slot_cert(true, false, &[0x04]),
        ];
        let leaf = select_leaf(&certs).unwrap();
        assert_eq!(leaf.key_id.as_deref(), Some(&[0x04][..]));
    }

    #[test]
    fn select_leaf_breaks_ties_on_smallest_id() {
        let certs = vec![
            slot_cert(true, false, &[0x04]),
            slot_cert(true, false, &[0x01]),
        ];
        let leaf = select_leaf(&certs).unwrap();
        assert_eq!(leaf.key_id.as_deref(), Some(&[0x01][..]));
    }

    #[test]
    fn select_leaf_falls_back_to_card_auth_when_only_option() {
        let certs = vec![slot_cert(true, true, &[0x04])];
        assert!(select_leaf(&certs).is_some());
    }

    #[test]
    fn select_leaf_ignores_non_client_auth_certs() {
        let certs = vec![slot_cert(false, false, &[0x01])];
        assert!(select_leaf(&certs).is_none());
        assert!(select_leaf(&[]).is_none());
    }
}
