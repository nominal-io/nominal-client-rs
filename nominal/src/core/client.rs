use std::sync::Arc;

use conjure_http::client::ConjureRuntime;
use conjure_object::BearerToken;
use conjure_runtime::{Agent, Client, UserAgent};
use nominal_api::objects::api::rids::WorkspaceRid;
use nominal_streaming::client::ProductAgent;
use nominal_streaming::log::NominalLogStreamOpts;
use nominal_streaming::stream::NominalStreamOpts;

use crate::config::{Config, Profile};
use crate::core::rid::parse_rid;
use crate::core::{
    asset::AssetsClient, catalog::CatalogClient, checklist::ChecklistsClient, ingest::IngestClient,
    run::RunsClient, template::TemplatesClient, user::UsersClient,
    utils::api_base_url_to_app_base_url, workbook::WorkbooksClient, workspace::WorkspacesClient,
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
    workspace_rid: Option<WorkspaceRid>,
    base_url: String,
    grpc: crate::core::grpc::GrpcConnection,
    user_agent: ProductAgent,
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
        self.workspace_rid.as_ref().map(|w| w.0.as_str())
    }

    /// The product name and version this client sends at the start of its `User-Agent` header.
    pub fn user_agent(&self) -> &ProductAgent {
        &self.user_agent
    }

    /// Options for a [`crate::streaming::stream::NominalDatasetStream`] that send to this
    /// client's base URL with this client's user agent.
    pub fn stream_opts(&self) -> NominalStreamOpts {
        NominalStreamOpts::default()
            .with_base_api_url(&self.base_url)
            .with_user_agent(self.user_agent.clone())
    }

    /// Options for a [`crate::streaming::log::NominalLogStream`] that send to this client's
    /// base URL with this client's user agent.
    pub fn log_stream_opts(&self) -> NominalLogStreamOpts {
        let mut opts = NominalLogStreamOpts::default();
        opts.base_api_url = self.base_url.clone();
        opts.user_agent = self.user_agent.clone();
        opts
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

    /// Access workspace operations.
    pub fn workspaces(&self) -> WorkspacesClient {
        WorkspacesClient::new(self.client.clone(), &self.runtime, self.token.clone())
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

    /// Access template operations.
    pub fn templates(&self) -> TemplatesClient {
        TemplatesClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access checklist operations.
    pub fn checklists(&self) -> ChecklistsClient {
        ChecklistsClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access workbook operations.
    pub fn workbooks(&self) -> WorkbooksClient {
        WorkbooksClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access drive operations in the Nominal file store.
    pub fn drives(&self) -> crate::core::fs::DrivesClient {
        crate::core::fs::DrivesClient::new(&self.grpc, self.workspace_rid.clone())
    }

    /// Access file operations scoped to `drive_rid` in the Nominal file store.
    pub fn files(&self, drive_rid: impl Into<String>) -> crate::core::fs::DriveFilesClient {
        crate::core::fs::DriveFilesClient::new(
            &self.grpc,
            self.client.clone(),
            self.runtime.clone(),
            self.token.clone(),
            drive_rid,
        )
    }

    /// Access ingest operations: uploading files and triggering ingest jobs.
    pub fn ingest(&self) -> IngestClient {
        IngestClient::new(
            self.client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
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
    user_agent_name: String,
    user_agent_version: String,
}

impl NominalClientBuilder {
    fn new(token: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            token: token.into(),
            workspace_rid: None,
            user_agent_name: SDK_USER_AGENT_NAME.to_string(),
            user_agent_version: SDK_USER_AGENT_VERSION.to_string(),
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

    /// Override the default `User-Agent` value for API and streaming requests.
    ///
    /// The generated header starts with `name/version`. [`Self::build`] returns
    /// [`Error::InvalidUserAgent`] unless the name and version satisfy [`ProductAgent::new`].
    pub fn user_agent(mut self, name: &str, version: &str) -> Self {
        self.user_agent_name = name.to_string();
        self.user_agent_version = version.to_string();
        self
    }

    pub fn build(self) -> Result<NominalClient> {
        let user_agent = ProductAgent::new(&self.user_agent_name, &self.user_agent_version)
            .map_err(|e| Error::InvalidUserAgent {
                reason: e.to_string(),
            })?;
        let bearer_token = create_bearer_token(&self.token)?;
        let client = create_client(&self.base_url, &user_agent)?;
        let grpc = crate::core::grpc::GrpcConnection::connect_lazy(&self.base_url, &bearer_token)?;
        let workspace_rid = self
            .workspace_rid
            .as_deref()
            .map(parse_rid::<WorkspaceRid>)
            .transpose()?;
        Ok(NominalClient {
            client,
            runtime: Arc::new(ConjureRuntime::default()),
            token: bearer_token,
            workspace_rid,
            base_url: self.base_url,
            grpc,
            user_agent,
        })
    }
}

fn create_bearer_token(token: &str) -> Result<BearerToken> {
    BearerToken::new(token).map_err(|e| Error::InvalidBearerToken {
        reason: e.to_string(),
    })
}

fn create_client(url: &str, user_agent: &ProductAgent) -> Result<Client> {
    let uri = url.try_into().map_err(|e| Error::InvalidServiceUrl {
        url: url.to_string(),
        reason: format!("{e:?}"),
    })?;

    Client::builder()
        .service(SDK_USER_AGENT_NAME)
        .user_agent(UserAgent::new(Agent::new(
            user_agent.name(),
            user_agent.version(),
        )))
        .uri(uri)
        .build()
        .map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_user_agent_is_a_valid_product_agent() {
        let agent = ProductAgent::new(SDK_USER_AGENT_NAME, SDK_USER_AGENT_VERSION)
            .expect("SDK user agent name and crate version must be valid");

        assert_eq!(
            agent.to_string(),
            format!("{SDK_USER_AGENT_NAME}/{SDK_USER_AGENT_VERSION}")
        );
    }

    #[tokio::test]
    async fn client_defaults_to_sdk_user_agent() {
        let client = NominalClientBuilder::new("token").build().unwrap();

        assert_eq!(client.user_agent().name(), SDK_USER_AGENT_NAME);
        assert_eq!(client.user_agent().version(), SDK_USER_AGENT_VERSION);
    }

    #[tokio::test]
    async fn stream_opts_carry_client_user_agent_and_base_url() {
        let client = NominalClientBuilder::new("token")
            .base_url("https://api.example.test/api")
            .user_agent("nominal-cli", "1.2.3")
            .build()
            .unwrap();
        let expected = ProductAgent::new("nominal-cli", "1.2.3").unwrap();

        let stream_opts = client.stream_opts();
        assert_eq!(stream_opts.user_agent, expected);
        assert_eq!(stream_opts.base_api_url, "https://api.example.test/api");

        let log_stream_opts = client.log_stream_opts();
        assert_eq!(log_stream_opts.user_agent, expected);
        assert_eq!(log_stream_opts.base_api_url, "https://api.example.test/api");
    }

    #[test]
    fn build_rejects_invalid_user_agent_name() {
        let result = NominalClientBuilder::new("token")
            .user_agent("nominal_cli", "1.2.3")
            .build();

        assert!(matches!(result, Err(Error::InvalidUserAgent { .. })));
    }

    #[test]
    fn build_rejects_invalid_user_agent_version() {
        let result = NominalClientBuilder::new("token")
            .user_agent("nominal-cli", "1.2.3-beta.1")
            .build();

        assert!(matches!(result, Err(Error::InvalidUserAgent { .. })));
    }

    #[test]
    fn builder_defaults_base_url() {
        let builder = NominalClientBuilder::new("token");

        assert_eq!(builder.base_url, DEFAULT_BASE_URL);
    }
}
