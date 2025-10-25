use super::{Asset, Run};
use crate::config::Profile;
use conjure_http::client::AsyncService;
use conjure_object::{BearerToken, ResourceIdentifier};
use conjure_runtime::{Agent, Client, UserAgent};
use nominal_api::scout::RunServiceAsyncClient;
use nominal_api::scout::asset::api::{
    AssetSortOptions, SearchAssetsQuery, SearchAssetsRequest, SortField, SortKey,
};
use nominal_api::scout::assets::AssetServiceAsyncClient;

#[derive(Clone)]
pub struct NominalClient {
    pub client: Client,
    pub token: BearerToken,
    pub workspace_rid: Option<String>,
    base_url: String,
}

impl NominalClient {
    pub fn new(
        base_url: String,
        token: String,
        workspace_rid: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let bearer_token = BearerToken::new(&token).unwrap();
        let client = create_client(&base_url).unwrap();
        Ok(NominalClient {
            client,
            token: bearer_token,
            workspace_rid,
            base_url,
        })
    }

    pub fn from_profile(profile: &Profile) -> Result<Self, Box<dyn std::error::Error>> {
        NominalClient::new(
            profile.base_url.clone(),
            profile.token.clone(),
            profile.workspace_rid.clone(),
        )
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get an asset by RID
    pub async fn get_asset(&self, rid: &str) -> Result<Asset, Box<dyn std::error::Error>> {
        let service = AssetServiceAsyncClient::new(self.client.clone());
        let resource_id =
            ResourceIdentifier::new(rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let asset_rid: nominal_api::scout::rids::api::AssetRid = resource_id.into();
        let rid_set = std::collections::BTreeSet::from([asset_rid]);
        let response = service
            .get_assets(&self.token, &rid_set)
            .await
            .map_err(|e| format!("Failed to get assets: {:?}", e))?;

        let asset = response
            .into_iter()
            .next()
            .ok_or("No asset found with that RID")?
            .1;

        Ok(Asset::from_conjure(self, asset))
    }

    /// List/search assets
    pub async fn list_assets(&self) -> Result<Vec<Asset>, Box<dyn std::error::Error>> {
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
            .map_err(|e| format!("Failed to search assets: {:?}", e))?;

        Ok(response
            .results()
            .iter()
            .map(|asset| Asset::from_conjure(self, asset.clone()))
            .collect())
    }

    /// Get a run by RID
    pub async fn get_run(&self, rid: &str) -> Result<Run, Box<dyn std::error::Error>> {
        let service = RunServiceAsyncClient::new(self.client.clone());
        let resource_id =
            ResourceIdentifier::new(rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid: nominal_api::scout::run::api::RunRid = resource_id.into();

        let response = service
            .get_run(&self.token, &run_rid)
            .await
            .map_err(|e| format!("Failed to get run: {:?}", e))?;

        Ok(Run::from_conjure(self, response))
    }

    /// List/search runs
    pub async fn list_runs(&self) -> Result<Vec<Run>, Box<dyn std::error::Error>> {
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
            .map_err(|e| format!("Failed to search runs: {:?}", e))?;

        Ok(response
            .results()
            .iter()
            .map(|run| Run::from_conjure(self, run.clone()))
            .collect())
    }
}

fn create_client(url: &str) -> Result<Client, conjure_error::Error> {
    Client::builder()
        .service("nom-cli-rs")
        .user_agent(UserAgent::new(Agent::new("nom-cli-rs", "0.0")))
        .uri(url.try_into().unwrap())
        .build()
}
