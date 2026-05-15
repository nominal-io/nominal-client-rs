use std::sync::Arc;

use conjure_runtime::crypto::ring_crypto_provider;
use rustls::client::ResolvesClientCert;
use rustls::pki_types::CertificateDer;
use rustls::sign::{CertifiedKey, Signer, SigningKey};
use rustls::{ClientConfig, SignatureAlgorithm, SignatureScheme};

use crate::{Error, Result};

/// Abstract interface over a token's signing and certificate operations.
///
/// Implement this trait to provide a PKCS#11-backed token (e.g. an OpenSC
/// session against a CAC or a SoftHSM2 slot). The concrete implementation
/// using the `cryptoki` crate is provided in a subsequent PR.
///
/// All methods are called at most once during client construction except
/// `sign_raw`, which is called once per TLS handshake.
pub trait TokenBackend: Send + Sync + 'static {
    /// DER-encoded certificate chain for the client certificate, leaf first.
    fn cert_chain(&self) -> Vec<CertificateDer<'static>>;

    /// Signature schemes this token supports, ordered by preference.
    ///
    /// For RSA-2048 PIV auth certs a reasonable list is:
    /// `[RSA_PSS_SHA256, RSA_PSS_SHA384, RSA_PSS_SHA512, RSA_PKCS1_SHA256, ...]`.
    fn supported_schemes(&self) -> Vec<SignatureScheme>;

    /// Signature algorithm family (`RSA`, `ECDSA`, …).
    fn algorithm(&self) -> SignatureAlgorithm;

    /// Sign `message` and return the raw signature bytes.
    ///
    /// `scheme` identifies the hash and padding to use.  rustls passes the
    /// full pre-image (not a digest) for all schemes; the token must hash
    /// internally before signing.
    fn sign_raw(&self, scheme: SignatureScheme, message: &[u8]) -> Result<Vec<u8>>;
}

// --- rustls signing bridge -----------------------------------------------

struct SmartcardSigner {
    backend: Arc<dyn TokenBackend>,
    scheme: SignatureScheme,
}

impl std::fmt::Debug for SmartcardSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartcardSigner")
            .field("scheme", &self.scheme)
            .finish_non_exhaustive()
    }
}

impl Signer for SmartcardSigner {
    fn sign(&self, message: &[u8]) -> std::result::Result<Vec<u8>, rustls::Error> {
        self.backend
            .sign_raw(self.scheme, message)
            .map_err(|e| rustls::Error::General(e.to_string()))
    }

    fn scheme(&self) -> SignatureScheme {
        self.scheme
    }
}

struct SmartcardSigningKey {
    backend: Arc<dyn TokenBackend>,
}

impl std::fmt::Debug for SmartcardSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartcardSigningKey")
            .finish_non_exhaustive()
    }
}

impl SigningKey for SmartcardSigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        let supported = self.backend.supported_schemes();
        offered
            .iter()
            .find(|s| supported.contains(s))
            .map(|&scheme| {
                Box::new(SmartcardSigner {
                    backend: self.backend.clone(),
                    scheme,
                }) as Box<dyn Signer>
            })
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        self.backend.algorithm()
    }
}

// --- rustls certificate resolver -----------------------------------------

/// A rustls client-certificate resolver backed by a [`TokenBackend`].
///
/// Presents the same certified key for every TLS handshake, ignoring the
/// server's acceptable-issuer hints.  This is correct for CAC deployments
/// where the server is configured to accept any DoD-rooted certificate; the
/// server validates the chain after the handshake.
#[derive(Debug)]
pub struct SmartcardCertResolver {
    key: Arc<CertifiedKey>,
}

impl SmartcardCertResolver {
    /// Build a resolver from `backend`.  Calls `backend.cert_chain()` eagerly
    /// so cert-not-found errors surface at client-construction time.
    pub fn new(backend: Arc<dyn TokenBackend>) -> Self {
        let signing_key = Arc::new(SmartcardSigningKey {
            backend: backend.clone(),
        });
        let cert_chain = backend.cert_chain();
        let certified_key = Arc::new(CertifiedKey::new(cert_chain, signing_key));
        Self { key: certified_key }
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

// --- TLS config builders -------------------------------------------------

/// Build a `rustls::ClientConfig` for the S3 multipart-upload reqwest client.
///
/// Uses the WebPKI trust roots for server-certificate verification (suitable
/// for standard AWS S3 endpoints) plus `resolver` for client-cert presentation.
pub(crate) fn build_s3_tls_config(resolver: Arc<dyn ResolvesClientCert>) -> Result<ClientConfig> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder_with_provider(ring_crypto_provider().clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Smartcard {
            details: format!("TLS protocol-version config: {e}"),
        })?
        .with_root_certificates(root_store)
        .with_client_cert_resolver(resolver);
    Ok(config)
}

/// Build an `Arc<rustls::ClientConfig>` directly from a [`TokenBackend`].
///
/// Intended for callers that need a preconfigured TLS handle outside of
/// [`NominalClientBuilder`] — for example, integration tests against a
/// SoftHSM2 instance.  Uses the WebPKI trust roots for server-cert
/// verification.
pub fn build_rustls_config(backend: Arc<dyn TokenBackend>) -> Result<Arc<ClientConfig>> {
    let resolver: Arc<dyn ResolvesClientCert> = Arc::new(SmartcardCertResolver::new(backend));
    build_s3_tls_config(resolver).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Mock backend --------------------------------------------------------

    /// Minimal `TokenBackend` for unit tests.
    ///
    /// `sign_raw` returns the message bytes reversed — a deterministic,
    /// detectable output that doesn't require actual crypto.
    struct MockBackend {
        cert: Vec<CertificateDer<'static>>,
        schemes: Vec<SignatureScheme>,
        algorithm: SignatureAlgorithm,
        sign_error: bool,
    }

    impl MockBackend {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                // Minimal DER SEQUENCE: tag 0x30, length 0x03, three zero bytes.
                // rustls stores this opaquely and only parses it during a real
                // TLS handshake, so a stub is sufficient for unit tests.
                cert: vec![CertificateDer::from(vec![0x30, 0x03, 0x00, 0x00, 0x00])],
                schemes: vec![
                    SignatureScheme::RSA_PSS_SHA256,
                    SignatureScheme::RSA_PSS_SHA384,
                    SignatureScheme::RSA_PKCS1_SHA256,
                ],
                algorithm: SignatureAlgorithm::RSA,
                sign_error: false,
            })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                cert: vec![],
                schemes: vec![SignatureScheme::RSA_PSS_SHA256],
                algorithm: SignatureAlgorithm::RSA,
                sign_error: true,
            })
        }
    }

    impl TokenBackend for MockBackend {
        fn cert_chain(&self) -> Vec<CertificateDer<'static>> {
            self.cert.clone()
        }

        fn supported_schemes(&self) -> Vec<SignatureScheme> {
            self.schemes.clone()
        }

        fn algorithm(&self) -> SignatureAlgorithm {
            self.algorithm
        }

        fn sign_raw(&self, _scheme: SignatureScheme, message: &[u8]) -> Result<Vec<u8>> {
            if self.sign_error {
                return Err(crate::Error::Smartcard {
                    details: "mock signing error".into(),
                });
            }
            // Reverse bytes: detectable + deterministic without real crypto.
            Ok(message.iter().rev().cloned().collect())
        }
    }

    // --- SmartcardCertResolver tests -----------------------------------------

    #[test]
    fn resolver_reports_has_certs() {
        let resolver = SmartcardCertResolver::new(MockBackend::new());
        assert!(resolver.has_certs());
    }

    #[test]
    fn resolver_returns_key_with_empty_hints() {
        let resolver = SmartcardCertResolver::new(MockBackend::new());
        assert!(resolver.resolve(&[], &[]).is_some());
    }

    #[test]
    fn resolver_returns_key_regardless_of_issuer_hints() {
        let resolver = SmartcardCertResolver::new(MockBackend::new());
        let hint = b"CN=DoD Root CA 3";
        let key = resolver.resolve(&[hint.as_slice()], &[SignatureScheme::RSA_PSS_SHA256]);
        assert!(key.is_some());
    }

    #[test]
    fn resolver_cert_chain_matches_backend() {
        let backend = MockBackend::new();
        let resolver = SmartcardCertResolver::new(backend.clone());
        let key = resolver.resolve(&[], &[]).unwrap();
        assert_eq!(key.cert, backend.cert_chain());
    }

    // --- SmartcardSigningKey tests --------------------------------------------

    #[test]
    fn signing_key_picks_first_matching_scheme() {
        let key = SmartcardSigningKey {
            backend: MockBackend::new(),
        };
        // Offer ECDSA first (not supported), then RSA_PSS_SHA256 (supported).
        let offered = [
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
        ];
        let signer = key.choose_scheme(&offered).expect("should find a scheme");
        assert_eq!(signer.scheme(), SignatureScheme::RSA_PSS_SHA256);
    }

    #[test]
    fn signing_key_prefers_offered_order_within_supported_set() {
        let key = SmartcardSigningKey {
            backend: MockBackend::new(),
        };
        // Both RSA_PSS_SHA384 and RSA_PSS_SHA256 are offered and supported;
        // choose_scheme iterates the offered slice so SHA384 comes first.
        let offered = [
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
        ];
        let signer = key.choose_scheme(&offered).expect("should find a scheme");
        assert_eq!(signer.scheme(), SignatureScheme::RSA_PSS_SHA384);
    }

    #[test]
    fn signing_key_returns_none_when_no_scheme_overlaps() {
        let key = SmartcardSigningKey {
            backend: MockBackend::new(),
        };
        let offered = [
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
        ];
        assert!(key.choose_scheme(&offered).is_none());
    }

    #[test]
    fn signing_key_algorithm_matches_backend() {
        let key = SmartcardSigningKey {
            backend: MockBackend::new(),
        };
        assert_eq!(key.algorithm(), SignatureAlgorithm::RSA);
    }

    // --- SmartcardSigner tests -----------------------------------------------

    #[test]
    fn signer_delegates_to_backend_and_returns_result() {
        let signer = SmartcardSigner {
            backend: MockBackend::new(),
            scheme: SignatureScheme::RSA_PSS_SHA256,
        };
        let message = b"hello CAC";
        let sig = signer.sign(message).expect("sign should succeed");
        let expected: Vec<u8> = message.iter().rev().cloned().collect();
        assert_eq!(sig, expected);
    }

    #[test]
    fn signer_exposes_its_scheme() {
        let signer = SmartcardSigner {
            backend: MockBackend::new(),
            scheme: SignatureScheme::RSA_PKCS1_SHA256,
        };
        assert_eq!(signer.scheme(), SignatureScheme::RSA_PKCS1_SHA256);
    }

    #[test]
    fn signer_maps_backend_error_to_rustls_general_error() {
        let signer = SmartcardSigner {
            backend: MockBackend::failing(),
            scheme: SignatureScheme::RSA_PSS_SHA256,
        };
        let err = signer.sign(b"anything").unwrap_err();
        assert!(
            matches!(err, rustls::Error::General(_)),
            "expected rustls::Error::General, got {err:?}",
        );
    }

    // --- TLS config builder tests --------------------------------------------

    #[test]
    fn build_s3_tls_config_returns_ok() {
        let resolver: Arc<dyn ResolvesClientCert> =
            Arc::new(SmartcardCertResolver::new(MockBackend::new()));
        assert!(build_s3_tls_config(resolver).is_ok());
    }

    #[test]
    fn build_rustls_config_returns_arc_client_config() {
        assert!(build_rustls_config(MockBackend::new()).is_ok());
    }

    #[test]
    fn build_rustls_config_resolver_has_certs() {
        // Verify the embedded resolver will signal to rustls that a client cert
        // is available, so rustls sends it during the CertificateRequest phase.
        let config = build_rustls_config(MockBackend::new()).unwrap();
        assert!(config.client_auth_cert_resolver.has_certs());
    }
}
