use chrono::{DateTime, Utc};
use conjure_http::client::AsyncService;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::Stream;
use nominal_api::api::{Label, PropertyName, PropertyValue, SetOperator};
use nominal_api::scout::asset::api::{
    AssetSortField, AssetSortOptions, SearchAssetsQuery, SearchAssetsRequest, SortKey,
    UpdateAssetRequest,
};
use nominal_api::scout::assets::AssetServiceAsyncClient;
use nominal_api::scout::rids::api::{LabelsFilter, PropertiesFilter};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::core::rid::{parse_rid, rid_to_string};
use crate::core::utils::paginate_stream;
use crate::{Error, Result};
use futures::TryStreamExt;

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

/// A query for searching assets, which can be composed into a tree with [`and`](AssetQuery::and) and [`or`](AssetQuery::or).
#[derive(Debug, Clone)]
pub enum AssetQuery {
    /// Fuzzy full-text search against title and description.
    SearchText(String),
    /// Case-insensitive substring match of title or description.
    ExactSubstring(String),
    /// Filter by label.
    Label(String),
    /// Filter by property key and value.
    Property(String, String),
    /// All sub-queries must match.
    And(Vec<AssetQuery>),
    /// At least one sub-query must match.
    Or(Vec<AssetQuery>),
}

impl AssetQuery {
    pub fn search_text(text: impl Into<String>) -> Self {
        Self::SearchText(text.into())
    }

    pub fn exact_substring(text: impl Into<String>) -> Self {
        Self::ExactSubstring(text.into())
    }

    pub fn label(label: impl Into<String>) -> Self {
        Self::Label(label.into())
    }

    pub fn property(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Property(key.into(), value.into())
    }

    pub fn and(queries: impl IntoIterator<Item = AssetQuery>) -> Self {
        Self::And(queries.into_iter().collect())
    }

    pub fn or(queries: impl IntoIterator<Item = AssetQuery>) -> Self {
        Self::Or(queries.into_iter().collect())
    }

    fn into_conjure(self) -> SearchAssetsQuery {
        match self {
            Self::SearchText(s) => SearchAssetsQuery::SearchText(s),
            Self::ExactSubstring(s) => SearchAssetsQuery::ExactSubstring(s),
            Self::Label(l) => SearchAssetsQuery::Labels(
                LabelsFilter::builder()
                    .operator(SetOperator::Or)
                    .extend_labels([Label(l)])
                    .build(),
            ),
            Self::Property(k, v) => SearchAssetsQuery::Properties(
                PropertiesFilter::builder()
                    .name(PropertyName(k))
                    .extend_values([PropertyValue(v)])
                    .build(),
            ),
            Self::And(qs) => SearchAssetsQuery::And(qs.into_iter().map(Self::into_conjure).collect()),
            Self::Or(qs) => SearchAssetsQuery::Or(qs.into_iter().map(Self::into_conjure).collect()),
        }
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

    /// Get multiple assets by RID.
    ///
    /// Returns a map from RID string to Asset. RIDs not found in Nominal are omitted.
    pub async fn get_batch<I, S>(&self, rids: I) -> Result<HashMap<String, Asset>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let rid_set = rids
            .into_iter()
            .map(|s| parse_rid(s.as_ref()).map_err(Error::from))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let response = self
            .service
            .get_assets(&self.token, &rid_set)
            .await
            .map_err(Error::from)?;
        Ok(response
            .into_iter()
            .map(|(k, v)| (rid_to_string(&k), Asset::from_conjure(v, &self.app_base_url)))
            .collect())
    }

    fn list_stream(&self) -> impl Stream<Item = Result<Asset>> {
        self.search_stream(AssetQuery::search_text(""))
    }

    /// List assets, sorted by creation date descending.
    pub async fn list(&self) -> Result<Vec<Asset>> {
        self.list_stream().try_collect().await
    }

    fn search_stream(&self, query: AssetQuery) -> impl Stream<Item = Result<Asset>> {
        let conjure_query = query.into_conjure();
        let service = self.service.clone();
        let token = self.token.clone();
        let app_base_url = self.app_base_url.clone();
        paginate_stream(
            move |page_token| {
                SearchAssetsRequest::builder()
                    .sort(
                        AssetSortOptions::builder()
                            .is_descending(true)
                            .sort_key(SortKey::Field(AssetSortField::CreatedAt))
                            .build(),
                    )
                    .query(conjure_query.clone())
                    .next_page_token(page_token)
                    .build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move {
                    service
                        .search_assets(&token, &req)
                        .await
                        .map_err(Error::from)
                }
            },
            |resp| resp.next_page_token().cloned(),
            move |resp| {
                resp.results()
                    .iter()
                    .map(|a| Asset::from_conjure(a.clone(), &app_base_url))
                    .collect()
            },
        )
    }

    /// Search assets with a query, collecting all pages eagerly.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::NominalClient) -> nominal::Result<()> {
    /// use nominal::AssetQuery;
    /// let assets = client.assets()
    ///     .search(AssetQuery::and([
    ///         AssetQuery::label("production"),
    ///         AssetQuery::property("vehicle", "rocket"),
    ///     ]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search(&self, query: AssetQuery) -> Result<Vec<Asset>> {
        self.search_stream(query).try_collect().await
    }

    /// Update asset metadata. Returns the updated asset.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal::AssetUpdate;
    /// # async fn example(client: nominal::NominalClient) -> nominal::Result<()> {
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
