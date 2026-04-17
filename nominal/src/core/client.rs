use conjure_object::BearerToken;
use conjure_runtime::{Agent, Client, UserAgent};

use crate::config::{Config, Profile};
use crate::core::{
    asset::AssetsClient, catalog::CatalogClient, ingest::IngestClient, run::RunsClient,
    user::UsersClient, utils::api_base_url_to_app_base_url,
};
use crate::{Error, Result};

#[derive(Clone)]
pub struct NominalClient {
    client: Client,
    token: BearerToken,
    workspace_rid: Option<String>,
    base_url: String,
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
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
        workspace_rid: Option<String>,
    ) -> Result<Self> {
        let base_url = base_url.into();
        let token = token.into();
        let bearer_token = create_bearer_token(&token)?;
        let client = create_client(&base_url)?;
        Ok(Self {
            client,
            token: bearer_token,
            workspace_rid,
            base_url,
        })
    }

    pub fn from_profile(name: &str) -> Result<Self> {
        let config = Config::from_file(None)?;
        let profile = config
            .get_profile(name)
            .ok_or_else(|| Error::ProfileNotFound { name: name.to_string() })?;
        Self::from_profile_config(profile)
    }

    pub fn from_profile_config(profile: &Profile) -> Result<Self> {
        Self::new(
            profile.base_url(),
            profile.token(),
            profile.workspace_rid().map(ToString::to_string),
        )
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
            self.token.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access asset operations.
    pub fn assets(&self) -> AssetsClient {
        AssetsClient::new(
            self.client.clone(),
            self.token.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access user operations.
    pub fn users(&self) -> UsersClient {
        UsersClient::new(self.client.clone(), self.token.clone())
    }

    /// Access catalog operations: datasets, videos, and connections.
    pub fn catalog(&self) -> CatalogClient {
        CatalogClient::new(
            self.client.clone(),
            self.token.clone(),
            self.workspace_rid.clone(),
            api_base_url_to_app_base_url(&self.base_url),
        )
    }

    /// Access ingest operations: uploading files and triggering ingest jobs.
    pub fn ingest(&self) -> IngestClient {
        IngestClient::new(
            self.client.clone(),
            self.token.clone(),
            self.workspace_rid.clone(),
        )
    }
}

fn create_bearer_token(token: &str) -> Result<BearerToken> {
    BearerToken::new(token).map_err(|e| Error::InvalidBearerToken {
        reason: e.to_string(),
    })
}

fn create_client(url: &str) -> Result<Client> {
    let uri = url.try_into().map_err(|e| Error::InvalidServiceUrl {
        url: url.to_string(),
        reason: format!("{e:?}"),
    })?;

    Client::builder()
        .service("nom-cli-rs")
        .user_agent(UserAgent::new(Agent::new("nom-cli-rs", "0.0")))
        .uri(uri)
        .build()
        .map_err(Error::from)
}
