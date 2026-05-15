use std::sync::Arc;

use conjure_http::client::ConjureRuntime;
use conjure_object::BearerToken;
use conjure_runtime::{Agent, Client, UserAgent};
use rustls::client::ResolvesClientCert;

use crate::config::{Config, Profile};
use crate::core::{
    asset::AssetsClient,
    catalog::CatalogClient,
    ingest::IngestClient,
    run::RunsClient,
    smartcard::{SmartcardCertResolver, TokenBackend},
    user::UsersClient,
    utils::api_base_url_to_app_base_url,
};
use crate::{Error, Result};

const SDK_USER_AGENT_NAME: &str = "nominal-rust";
const SDK_USER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_BASE_URL: &str = "https://api.gov.nominal.io/api";

#[derive(Clone)]
pub struct NominalClient {
    client: Client,
    runtime: Arc<ConjureRuntime>,
    token: BearerToken,
    workspace_rid: Option<String>,
    base_url: String,
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
}

impl std::fmt::Debug for NominalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NominalClient")
            .field("workspace_rid", &self.workspace_rid)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl NominalClient {
    pub fn builder(token: impl Into<String>) -> NominalClientBuilder {
        NominalClientBuilder::new(token)
    }

    pub fn from_profile(name: &str) -> Result<Self> {
        let config = Config::load()?;
        let profile = config
            .get_profile(name)
            .ok_or_else(|| Error::ProfileNotFound {
                name: name.to_string(),
            })?;
        Self::from_profile_config(profile)
    }

    /// Create a client from the profile named by the `NOMINAL_PROFILE` environment variable.
    /// Returns an error if the variable is not set.
    pub fn from_profile_env() -> Result<Self> {
        let name = std::env::var("NOMINAL_PROFILE").map_err(|_| Error::EnvVarNotSet {
            name: "NOMINAL_PROFILE",
        })?;
        Self::from_profile(&name)
    }

    pub fn from_profile_config(profile: &Profile) -> Result<Self> {
        NominalClientBuilder::from_profile_config(profile).build()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn workspace_rid(&self) -> Option<&str> {
        self.workspace_rid.as_deref()
    }

    /// Access run operations.
    pub fn runs(&self) -> RunsClient {
        RunsClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access asset operations.
    pub fn assets(&self) -> AssetsClient {
        AssetsClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access user operations.
    pub fn users(&self) -> UsersClient {
        UsersClient::new(self.client.clone(), &self.runtime, self.token.clone())
    }

    /// Access catalog operations: datasets, videos, and connections.
    pub fn catalog(&self) -> CatalogClient {
        CatalogClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access ingest operations: uploading files and triggering ingest jobs.
    pub fn ingest(&self) -> IngestClient {
        IngestClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            self.tls_resolver.clone(),
        )
    }
}

/// Builds a [`NominalClient`] when callers need to customize optional client settings.
///
/// The default base URL is `https://api.gov.nominal.io/api`, and the default
/// user agent is `nominal-rust/<crate version>`.
pub struct NominalClientBuilder {
    base_url: String,
    token: String,
    workspace_rid: Option<String>,
    user_agent: UserAgent,
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
}

impl NominalClientBuilder {
    fn new(token: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            token: token.into(),
            workspace_rid: None,
            user_agent: default_user_agent(),
            tls_resolver: None,
        }
    }

    pub fn from_profile_config(profile: &Profile) -> Self {
        Self::new(profile.token())
            .base_url(profile.base_url())
            .workspace_rid(profile.workspace_rid().map(ToString::to_string))
    }

    /// Override the default base URL.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn workspace_rid(mut self, workspace_rid: Option<String>) -> Self {
        self.workspace_rid = workspace_rid;
        self
    }

    /// Override the default `User-Agent` value.
    ///
    /// The generated header is formatted as `name/version`.
    pub fn user_agent(mut self, name: &str, version: &str) -> Self {
        self.user_agent = UserAgent::new(Agent::new(name, version));
        self
    }

    /// Enable CAC / PIV smartcard mTLS using the provided token backend.
    ///
    /// `backend` is the initialized PKCS#11 session wrapper returned by
    /// `nominal::core::smartcard::load_pkcs11_backend` (implemented in a
    /// subsequent PR). Once set, every connection — both to the Nominal API
    /// via conjure-runtime and to S3 during multipart uploads — will present
    /// the smartcard client certificate.
    pub fn smartcard_backend(mut self, backend: Arc<dyn TokenBackend>) -> Self {
        self.tls_resolver = Some(Arc::new(SmartcardCertResolver::new(backend)));
        self
    }

    pub fn build(self) -> Result<NominalClient> {
        let bearer_token = create_bearer_token(&self.token)?;
        let client = create_client(&self.base_url, self.user_agent, self.tls_resolver.clone())?;
        Ok(NominalClient {
            client,
            runtime: Arc::new(ConjureRuntime::default()),
            token: bearer_token,
            workspace_rid: self.workspace_rid,
            base_url: self.base_url,
            tls_resolver: self.tls_resolver,
        })
    }
}

fn create_bearer_token(token: &str) -> Result<BearerToken> {
    BearerToken::new(token).map_err(|e| Error::InvalidBearerToken {
        reason: e.to_string(),
    })
}

fn default_user_agent() -> UserAgent {
    UserAgent::new(Agent::new(SDK_USER_AGENT_NAME, SDK_USER_AGENT_VERSION))
}

fn create_client(
    url: &str,
    user_agent: UserAgent,
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
) -> Result<Client> {
    let uri = url.try_into().map_err(|e| Error::InvalidServiceUrl {
        url: url.to_string(),
        reason: format!("{e:?}"),
    })?;

    let mut builder = Client::builder()
        .service(SDK_USER_AGENT_NAME)
        .user_agent(user_agent)
        .uri(uri);

    if let Some(resolver) = tls_resolver {
        builder = builder.client_cert_resolver(resolver);
    }

    builder.build().map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_user_agent_uses_sdk_name_and_crate_version() {
        assert_eq!(
            default_user_agent().to_string(),
            format!("{SDK_USER_AGENT_NAME}/{SDK_USER_AGENT_VERSION}")
        );
    }

    #[test]
    fn builder_accepts_custom_user_agent() {
        let builder = NominalClientBuilder::new("token").user_agent("nominal-cli", "1.2.3");

        assert_eq!(builder.user_agent.to_string(), "nominal-cli/1.2.3");
    }

    #[test]
    fn builder_defaults_base_url() {
        let builder = NominalClientBuilder::new("token");

        assert_eq!(builder.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn builder_without_smartcard_has_no_resolver() {
        let builder = NominalClientBuilder::new("token");
        assert!(builder.tls_resolver.is_none());
    }

    #[test]
    fn builder_with_smartcard_backend_sets_resolver() {
        use crate::core::smartcard::{SmartcardCertResolver, TokenBackend};
        use rustls::pki_types::CertificateDer;
        use rustls::{SignatureAlgorithm, SignatureScheme};

        struct StubBackend;
        impl TokenBackend for StubBackend {
            fn cert_chain(&self) -> Vec<CertificateDer<'static>> {
                vec![CertificateDer::from(vec![0x30, 0x03, 0x00, 0x00, 0x00])]
            }
            fn supported_schemes(&self) -> Vec<SignatureScheme> {
                vec![SignatureScheme::RSA_PSS_SHA256]
            }
            fn algorithm(&self) -> SignatureAlgorithm {
                SignatureAlgorithm::RSA
            }
            fn sign_raw(&self, _: SignatureScheme, msg: &[u8]) -> crate::Result<Vec<u8>> {
                Ok(msg.to_vec())
            }
        }

        let builder = NominalClientBuilder::new("token").smartcard_backend(Arc::new(StubBackend));
        assert!(builder.tls_resolver.is_some());
        assert!(builder.tls_resolver.unwrap().has_certs());
    }

    #[test]
    fn builder_with_smartcard_builds_successfully() {
        use crate::core::smartcard::TokenBackend;
        use rustls::pki_types::CertificateDer;
        use rustls::{SignatureAlgorithm, SignatureScheme};

        struct StubBackend;
        impl TokenBackend for StubBackend {
            fn cert_chain(&self) -> Vec<CertificateDer<'static>> {
                vec![CertificateDer::from(vec![0x30, 0x03, 0x00, 0x00, 0x00])]
            }
            fn supported_schemes(&self) -> Vec<SignatureScheme> {
                vec![SignatureScheme::RSA_PSS_SHA256]
            }
            fn algorithm(&self) -> SignatureAlgorithm {
                SignatureAlgorithm::RSA
            }
            fn sign_raw(&self, _: SignatureScheme, msg: &[u8]) -> crate::Result<Vec<u8>> {
                Ok(msg.to_vec())
            }
        }

        // Verify that the builder wires through to conjure-runtime without
        // error. No network connection is made; the Client is constructed lazily.
        let result = NominalClientBuilder::new("validtoken123")
            .base_url("https://api.example.com/api")
            .smartcard_backend(Arc::new(StubBackend))
            .build();
        assert!(result.is_ok(), "build failed: {result:?}");
    }
}
