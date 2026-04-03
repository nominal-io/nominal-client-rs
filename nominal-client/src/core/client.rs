use super::{Asset, Run};
use crate::config::Profile;
use crate::core::rid::parse_rid;
use crate::{Error, Result};
use conjure_http::client::AsyncService;
use conjure_object::BearerToken;
use conjure_runtime::{Agent, Client, UserAgent};
use nominal_api::scout::RunServiceAsyncClient;
use nominal_api::scout::asset::api::{
    AssetSortOptions, SearchAssetsQuery, SearchAssetsRequest, SortField, SortKey,
};
use nominal_api::scout::assets::AssetServiceAsyncClient;

#[derive(Clone)]
pub struct NominalClient {
    client: Client,
    token: BearerToken,
    workspace_rid: Option<String>,
    base_url: String,
}

impl NominalClient {
    pub fn new(base_url: String, token: String, workspace_rid: Option<String>) -> Result<Self> {
        let bearer_token =
            BearerToken::new(&token).map_err(|e| Error::InvalidBearerToken(e.to_string()))?;
        let client = create_client(&base_url)?;
        Ok(NominalClient {
            client,
            token: bearer_token,
            workspace_rid,
            base_url,
        })
    }

    pub fn from_profile(profile: &Profile) -> Result<Self> {
        NominalClient::new(
            profile.base_url.clone(),
            profile.token.clone(),
            profile.workspace_rid.clone(),
        )
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn workspace_rid(&self) -> Option<&str> {
        self.workspace_rid.as_deref()
    }

    pub(crate) fn service_client(&self) -> Client {
        self.client.clone()
    }

    pub(crate) fn bearer_token(&self) -> &BearerToken {
        &self.token
    }

    /// Get an asset by RID
    pub async fn get_asset(&self, rid: &str) -> Result<Asset> {
        let service = AssetServiceAsyncClient::new(self.client.clone());
        let rid = parse_rid(rid)?;
        let rid_set = std::collections::BTreeSet::from([rid]);
        let response = service
            .get_assets(&self.token, &rid_set)
            .await
            .map_err(Error::from)?;

        let asset = response
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound("asset with given RID".to_string()))?
            .1;

        Ok(Asset::from_conjure(self, asset))
    }

    /// List/search assets
    pub async fn list_assets(&self) -> Result<Vec<Asset>> {
        let service = AssetServiceAsyncClient::new(self.client.clone());
        let request = SearchAssetsRequest::new(
            AssetSortOptions::builder()
                .is_descending(true)
                .sort_key(SortKey::Field(SortField::CreatedAt))
                .build(),
            SearchAssetsQuery::SearchText("".to_string()),
        );
        let response = service
            .search_assets(&self.token, &request)
            .await
            .map_err(Error::from)?;

        Ok(response
            .results()
            .iter()
            .map(|asset| Asset::from_conjure(self, asset.clone()))
            .collect())
    }

    /// Get a run by RID
    pub async fn get_run(&self, rid: &str) -> Result<Run> {
        let service = RunServiceAsyncClient::new(self.client.clone());
        let run_rid = parse_rid(rid)?;

        let response = service
            .get_run(&self.token, &run_rid)
            .await
            .map_err(Error::from)?;

        Ok(Run::from_conjure(self, response))
    }

    /// List/search runs
    pub async fn list_runs(&self) -> Result<Vec<Run>> {
        use nominal_api::scout::run::api::{
            SearchQuery, SearchRunsRequest, SortField, SortKey, SortOptions,
        };

        let service = RunServiceAsyncClient::new(self.client.clone());
        let request = SearchRunsRequest::new(
            SortOptions::builder()
                .is_descending(true)
                .sort_key(SortKey::Field(SortField::CreatedAt))
                .build(),
            100, // page_size
            SearchQuery::SearchText("".to_string()),
        );

        let response = service
            .search_runs(&self.token, &request)
            .await
            .map_err(Error::from)?;

        Ok(response
            .results()
            .iter()
            .map(|run| Run::from_conjure(self, run.clone()))
            .collect::<Vec<_>>())
    }
}

fn create_client(url: &str) -> Result<Client> {
    let uri = url
        .try_into()
        .map_err(|e| Error::InvalidServiceUrl(format!("{e:?}")))?;

    Client::builder()
        .service("nom-cli-rs")
        .user_agent(UserAgent::new(Agent::new("nom-cli-rs", "0.0")))
        .uri(uri)
        .build()
        .map_err(Error::from)
}
