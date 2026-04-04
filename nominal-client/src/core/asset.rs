use chrono::{DateTime, Utc};
use conjure_http::client::AsyncService;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::scout::asset::api::{
    AssetSortOptions, SearchAssetsQuery, SearchAssetsRequest, SortField, SortKey,
    UpdateAssetRequest,
};
use nominal_api::scout::assets::AssetServiceAsyncClient;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::core::rid::{parse_rid, rid_to_string};
use crate::{Error, Result};

/// Represents an asset in Nominal.
///
/// Assets are the top-level organizational unit in Nominal, containing datasets, videos,
/// connections, and attachments related to a specific test, flight, or analysis.
#[derive(Debug, Clone)]
pub struct Asset {
    rid: String,
    name: String,
    description: Option<String>,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    created_at: DateTime<Utc>,
    app_base_url: String,
}

impl Asset {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Get the URL to view this asset in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/assets/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_conjure(
        asset: nominal_api::scout::asset::api::Asset,
        app_base_url: &str,
    ) -> Self {
        Self {
            rid: rid_to_string(asset.rid()),
            name: asset.title().to_string(),
            description: asset
                .description()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            properties: asset
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: asset.labels().iter().map(|l| l.to_string()).collect(),
            created_at: asset.created_at().to_utc(),
            app_base_url: app_base_url.to_string(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct AssetUpdate {
    name: Option<String>,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
}

impl AssetUpdate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn name(mut self, value: impl Into<String>) -> Self {
        self.name = Some(value.into());
        self
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn properties<I, K, V>(mut self, value: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.properties = Some(
            value
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    #[must_use]
    pub fn labels<I>(mut self, value: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.labels = Some(value.into_iter().map(Into::into).collect());
        self
    }

    pub(crate) fn into_request(self) -> UpdateAssetRequest {
        let AssetUpdate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut b = UpdateAssetRequest::builder();
        if let Some(n) = name {
            b = b.title(n);
        }
        if let Some(d) = description {
            b = b.description(d);
        }
        if let Some(p) = properties {
            b = b.properties(
                p.into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if let Some(l) = labels {
            b = b.labels(l.into_iter().map(|s| s.into()).collect::<BTreeSet<_>>());
        }
        b.build()
    }
}

/// Client for asset collection operations (list, get).
pub struct AssetsClient {
    service: AssetServiceAsyncClient<Client>,
    token: BearerToken,
    app_base_url: String,
}

impl AssetsClient {
    pub(crate) fn new(client: Client, token: BearerToken, app_base_url: String) -> Self {
        Self {
            service: AssetServiceAsyncClient::new(client),
            token,
            app_base_url,
        }
    }

    /// Get an asset by RID.
    pub async fn get(&self, rid: &str) -> Result<Asset> {
        let parsed = parse_rid(rid)?;
        let rid_set = std::collections::BTreeSet::from([parsed]);
        let response = self
            .service
            .get_assets(&self.token, &rid_set)
            .await
            .map_err(Error::from)?;

        let asset = response
            .into_iter()
            .next()
            .ok_or(Error::NotFound {
                resource: "asset with given RID",
            })?
            .1;

        Ok(Asset::from_conjure(asset, &self.app_base_url))
    }

    /// List assets, sorted by creation date descending.
    pub async fn list(&self) -> Result<Vec<Asset>> {
        let request = SearchAssetsRequest::new(
            AssetSortOptions::builder()
                .is_descending(true)
                .sort_key(SortKey::Field(SortField::CreatedAt))
                .build(),
            SearchAssetsQuery::SearchText("".to_string()),
        );
        let response = self
            .service
            .search_assets(&self.token, &request)
            .await
            .map_err(Error::from)?;

        Ok(response
            .results()
            .iter()
            .map(|a| Asset::from_conjure(a.clone(), &self.app_base_url))
            .collect())
    }

    /// Update asset metadata. Returns the updated asset.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal_client::AssetUpdate;
    /// # async fn example(client: nominal_client::NominalClient) -> nominal_client::Result<()> {
    /// let asset = client.assets()
    ///     .update("ri.scout.cerulean-staging.asset.<uuid>", AssetUpdate::new().name("New Name").labels(["tag1", "tag2"]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update(&self, rid: &str, update: AssetUpdate) -> Result<Asset> {
        let request = update.into_request();
        let asset_rid = parse_rid(rid)?;
        let response = self
            .service
            .update_asset(&self.token, &asset_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(Asset::from_conjure(response, &self.app_base_url))
    }

    /// Archive an asset. Archived assets are hidden from the UI but not deleted.
    pub async fn archive(&self, rid: &str) -> Result<()> {
        let asset_rid = parse_rid(rid)?;
        self.service
            .archive(&self.token, &asset_rid, None)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive an asset, restoring its visibility in the UI.
    pub async fn unarchive(&self, rid: &str) -> Result<()> {
        let asset_rid = parse_rid(rid)?;
        self.service
            .unarchive(&self.token, &asset_rid, None)
            .await
            .map_err(Error::from)?;
        Ok(())
    }
}
