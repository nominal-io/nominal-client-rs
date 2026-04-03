use crate::core::{
    rid::{parse_rid, rid_to_string},
    utils::api_base_url_to_app_base_url,
};
use crate::{Error, Result};
use conjure_http::client::AsyncService;
use nominal_api::scout::asset::api::UpdateAssetRequest;
use nominal_api::scout::assets::AssetServiceAsyncClient;

use super::NominalClient;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    pub fn properties(mut self, value: HashMap<String, String>) -> Self {
        self.properties = Some(value);
        self
    }

    #[must_use]
    pub fn labels(mut self, value: Vec<String>) -> Self {
        self.labels = Some(value);
        self
    }

    pub(crate) fn into_request(self) -> nominal_api::scout::asset::api::UpdateAssetRequest {
        let AssetUpdate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut request_builder = UpdateAssetRequest::builder();

        if let Some(n) = name {
            request_builder = request_builder.title(n);
        }
        if let Some(d) = description {
            request_builder = request_builder.description(d);
        }
        if let Some(p) = properties {
            let props = p
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect::<BTreeMap<_, _>>();
            request_builder = request_builder.properties(props);
        }
        if let Some(l) = labels {
            let labels_set = l.into_iter().map(|s| s.into()).collect::<BTreeSet<_>>();
            request_builder = request_builder.labels(labels_set);
        }

        request_builder.build()
    }
}

/// Represents an asset in Nominal.
///
/// Assets are the top-level organizational unit in Nominal, containing datasets, videos,
/// connections, and attachments related to a specific test, flight, or analysis.
#[derive(Debug, Clone)]
pub struct Asset {
    /// The resource identifier (RID) for this asset
    rid: String,

    /// The display name of the asset
    name: String,

    /// Optional description of the asset
    description: Option<String>,

    /// Key-value properties for custom metadata
    properties: HashMap<String, String>,

    /// Labels for categorizing and filtering assets
    labels: Vec<String>,

    /// Creation timestamp
    created_at: DateTime<Utc>,

    /// Reference to the client for API calls
    client: NominalClient,
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

    /// Update asset metadata.
    ///
    /// Only the metadata passed in will be replaced, the rest will remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal_client::AssetUpdate;
    /// # async fn example(mut asset: nominal_client::Asset) -> nominal_client::Result<()> {
    /// asset.update(
    ///     AssetUpdate::default()
    ///         .name("New Name")
    ///         .labels(vec!["label1".to_string(), "label2".to_string()]),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update(&mut self, update: AssetUpdate) -> Result<()> {
        let request = update.into_request();
        let service = AssetServiceAsyncClient::new(self.client.service_client());

        let rid = parse_rid(&self.rid)?;

        let response = service
            .update_asset(self.client.bearer_token(), &rid, &request)
            .await
            .map_err(Error::from)?;

        *self = Self::from_conjure(&self.client, response);

        Ok(())
    }

    /// Get the URL to view this asset in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        let app_base_url = api_base_url_to_app_base_url(self.client.base_url());
        format!("{}/assets/{}", app_base_url, self.rid)
    }

    /// Archive this asset.
    ///
    /// Archived assets are not deleted, but are hidden from the UI.
    pub async fn archive(&self) -> Result<()> {
        let service = AssetServiceAsyncClient::new(self.client.service_client());

        let rid = parse_rid(&self.rid)?;

        service
            .archive(self.client.bearer_token(), &rid, None)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Unarchive this asset, allowing it to be viewed in the UI.
    pub async fn unarchive(&self) -> Result<()> {
        let service = AssetServiceAsyncClient::new(self.client.service_client());

        let rid = parse_rid(&self.rid)?;

        service
            .unarchive(self.client.bearer_token(), &rid, None)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Internal method to construct an Asset from the Conjure API type.
    pub(crate) fn from_conjure(
        client: &NominalClient,
        asset: nominal_api::scout::asset::api::Asset,
    ) -> Self {
        let properties = asset
            .properties()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let labels = asset.labels().iter().map(|l| l.to_string()).collect();

        let description = asset
            .description()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Self {
            rid: rid_to_string(asset.rid()),
            name: asset.title().to_string(),
            description,
            properties,
            labels,
            created_at: asset.created_at().to_utc(),
            client: client.clone(),
        }
    }
}
