use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::Stream;
use nominal_api::clients::scout::assets::{AsyncAssetService, AsyncAssetServiceClient};
use nominal_api::objects::api::{Label, PropertyName, PropertyValue, SetOperator};
use nominal_api::objects::scout::asset::api::{
    AddDataScopesToAssetRequest, AssetSortField, AssetSortOptions, CreateAssetDataScope,
    CreateAssetRequest, SearchAssetsQuery, SearchAssetsRequest, SearchAssetsResponse, SortKey,
    UpdateAssetRequest,
};
use nominal_api::objects::scout::rids::api::{LabelsFilter, PropertiesFilter};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::core::datasource::DataSource;
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
    data_sources: HashMap<String, DataSource>,
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

    /// Data sources attached to this asset, keyed by scope name.
    pub fn data_sources(&self) -> &HashMap<String, DataSource> {
        &self.data_sources
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Get the URL to view this asset in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/assets/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_conjure(
        asset: nominal_api::objects::scout::asset::api::Asset,
        app_base_url: &str,
    ) -> Self {
        let data_sources = asset
            .data_scopes()
            .iter()
            .filter_map(|scope| {
                DataSource::from_conjure(scope.data_source())
                    .map(|ds| (scope.data_scope_name().to_string(), ds))
            })
            .collect();
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
            data_sources,
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

/// Parameters for creating a new asset.
#[derive(Debug, Clone)]
pub struct AssetCreate {
    name: String,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
}

impl AssetCreate {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            properties: None,
            labels: None,
        }
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

    pub(crate) fn into_request(self, workspace_rid: Option<&str>) -> Result<CreateAssetRequest> {
        use nominal_api::objects::api::rids::WorkspaceRid;

        let AssetCreate {
            name,
            description,
            properties,
            labels,
        } = self;

        let mut b = CreateAssetRequest::builder().title(name);

        if let Some(d) = description {
            b = b.description(d);
        }
        if let Some(p) = properties {
            b = b.properties(
                p.into_iter()
                    .map(|(k, v)| (PropertyName(k), PropertyValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if let Some(l) = labels {
            b = b.labels(l.into_iter().map(Label).collect::<BTreeSet<_>>());
        }
        if let Some(wid) = workspace_rid {
            b = b.workspace(parse_rid::<WorkspaceRid>(wid)?);
        }

        Ok(b.build())
    }
}

/// A query for searching assets, which can be composed into a tree with [`and`](AssetQuery::and) and [`or`](AssetQuery::or).
#[derive(Debug, Clone)]
pub enum AssetQuery {
    /// Fuzzy full-text search against title and description.
    SearchText(String),
    /// Case-insensitive substring match against the asset name.
    SubstringMatch(String),
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

    pub fn substring_match(text: impl Into<String>) -> Self {
        Self::SubstringMatch(text.into())
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

    pub(crate) fn collect_substring_matches(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_substring_matches_into(&mut out);
        out
    }

    fn collect_substring_matches_into(&self, out: &mut Vec<String>) {
        match self {
            Self::SubstringMatch(s) => out.push(s.clone()),
            Self::And(qs) => qs
                .iter()
                .for_each(|q| q.collect_substring_matches_into(out)),
            _ => {}
        }
    }

    fn into_conjure(self) -> SearchAssetsQuery {
        match self {
            Self::SearchText(s) => SearchAssetsQuery::SearchText(s),
            Self::SubstringMatch(s) => SearchAssetsQuery::ExactSubstring(s),
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
            Self::And(qs) => {
                SearchAssetsQuery::And(qs.into_iter().map(Self::into_conjure).collect())
            }
            Self::Or(qs) => SearchAssetsQuery::Or(qs.into_iter().map(Self::into_conjure).collect()),
        }
    }
}

/// Client for asset collection operations (list, get).
pub struct AssetsClient {
    service: AsyncAssetServiceClient<Client>,
    token: BearerToken,
    workspace_rid: Option<String>,
    app_base_url: String,
}

impl AssetsClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<String>,
        app_base_url: String,
    ) -> Self {
        Self {
            service: AsyncAssetServiceClient::new(client, runtime),
            token,
            workspace_rid,
            app_base_url,
        }
    }

    /// Create a new asset.
    pub async fn create(&self, create: AssetCreate) -> Result<Asset> {
        let request = create.into_request(self.workspace_rid.as_deref())?;
        let response = self
            .service
            .create_asset(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(Asset::from_conjure(response, &self.app_base_url))
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
            .map(|(k, v)| {
                (
                    rid_to_string(&k),
                    Asset::from_conjure(v, &self.app_base_url),
                )
            })
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
            |resp: &SearchAssetsResponse| resp.next_page_token().cloned(),
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
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::AssetQuery;
    /// let assets = client.assets()
    ///     .search(AssetQuery::and([
    ///         AssetQuery::label("production"),
    ///         AssetQuery::property("vehicle", "rocket"),
    ///     ]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search(&self, query: AssetQuery) -> Result<Vec<Asset>> {
        let substrings = query.collect_substring_matches();
        let assets: Vec<Asset> = self.search_stream(query).try_collect().await?;
        Ok(assets
            .into_iter()
            .filter(|a| crate::core::utils::name_matches_all(a.name(), &substrings))
            .collect())
    }

    /// Update asset metadata. Returns the updated asset.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal::core::AssetUpdate;
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
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

    /// Attach data sources to an asset under the given scope names.
    ///
    /// Scope names should be stable across assets of the same type, since
    /// checklists and templates use them to reference data sources.
    /// Returns the updated asset.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::DataSource;
    /// client.assets().add_data_sources("ri.scout.cerulean-staging.asset.<uuid>", [
    ///     ("flight-data", DataSource::dataset("ri.catalog.cerulean-staging.dataset.<uuid>")),
    ///     ("cockpit-cam", DataSource::video("ri.catalog.cerulean-staging.video.<uuid>")),
    /// ]).await?;
    /// # Ok(()) }
    /// ```
    pub async fn add_data_sources<I, N>(&self, rid: &str, sources: I) -> Result<Asset>
    where
        I: IntoIterator<Item = (N, DataSource)>,
        N: Into<String>,
    {
        let scopes = sources
            .into_iter()
            .map(|(name, ds)| {
                ds.into_conjure().map(|conjure_ds| {
                    CreateAssetDataScope::builder()
                        .data_scope_name(name.into().into())
                        .data_source(conjure_ds)
                        .build()
                })
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let request = AddDataScopesToAssetRequest::builder()
            .data_scopes(scopes)
            .build();
        let asset_rid = parse_rid(rid)?;
        let response = self
            .service
            .add_data_scopes_to_asset(&self.token, &asset_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(Asset::from_conjure(response, &self.app_base_url))
    }

    /// Attach a dataset to an asset under the given scope name. See [`add_data_sources`](Self::add_data_sources).
    pub async fn add_dataset(&self, rid: &str, name: &str, dataset_rid: &str) -> Result<Asset> {
        self.add_data_sources(rid, [(name, DataSource::dataset(dataset_rid))])
            .await
    }

    /// Attach a video to an asset under the given scope name. See [`add_data_sources`](Self::add_data_sources).
    pub async fn add_video(&self, rid: &str, name: &str, video_rid: &str) -> Result<Asset> {
        self.add_data_sources(rid, [(name, DataSource::video(video_rid))])
            .await
    }

    /// Attach a connection to an asset under the given scope name. See [`add_data_sources`](Self::add_data_sources).
    pub async fn add_connection(
        &self,
        rid: &str,
        name: &str,
        connection_rid: &str,
    ) -> Result<Asset> {
        self.add_data_sources(rid, [(name, DataSource::connection(connection_rid))])
            .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use nominal_api::objects::scout::asset::api::SearchAssetsQuery;

    // --- AssetQuery::into_conjure ---

    #[test]
    fn query_search_text() {
        let q = AssetQuery::search_text("hello");
        assert_eq!(
            q.into_conjure(),
            SearchAssetsQuery::SearchText("hello".into())
        );
    }

    #[test]
    fn query_substring_match() {
        let q = AssetQuery::substring_match("foo");
        assert_eq!(
            q.into_conjure(),
            SearchAssetsQuery::ExactSubstring("foo".into())
        );
    }

    #[test]
    fn query_label() {
        let q = AssetQuery::label("my-label");
        let SearchAssetsQuery::Labels(f) = q.into_conjure() else {
            panic!("expected Labels variant");
        };
        assert_eq!(
            f.labels(),
            [nominal_api::objects::api::Label("my-label".into())]
        );
    }

    #[test]
    fn query_property() {
        let q = AssetQuery::property("key", "val");
        let SearchAssetsQuery::Properties(f) = q.into_conjure() else {
            panic!("expected Properties variant");
        };
        assert_eq!(
            f.name(),
            &nominal_api::objects::api::PropertyName("key".into())
        );
        assert_eq!(
            f.values(),
            [nominal_api::objects::api::PropertyValue("val".into())]
        );
    }

    #[test]
    fn query_and_flattens_children() {
        let q = AssetQuery::and([AssetQuery::search_text("a"), AssetQuery::search_text("b")]);
        let SearchAssetsQuery::And(children) = q.into_conjure() else {
            panic!("expected And variant");
        };
        assert_eq!(children.len(), 2);
        assert_eq!(children[0], SearchAssetsQuery::SearchText("a".into()));
        assert_eq!(children[1], SearchAssetsQuery::SearchText("b".into()));
    }

    #[test]
    fn query_or_flattens_children() {
        let q = AssetQuery::or([AssetQuery::label("x"), AssetQuery::label("y")]);
        let SearchAssetsQuery::Or(children) = q.into_conjure() else {
            panic!("expected Or variant");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn query_nested_and_or() {
        let q = AssetQuery::and([
            AssetQuery::label("prod"),
            AssetQuery::or([
                AssetQuery::property("env", "us"),
                AssetQuery::property("env", "eu"),
            ]),
        ]);
        let SearchAssetsQuery::And(children) = q.into_conjure() else {
            panic!("expected And variant");
        };
        assert!(matches!(children[0], SearchAssetsQuery::Labels(_)));
        assert!(matches!(children[1], SearchAssetsQuery::Or(_)));
    }

    // --- AssetUpdate::into_request ---

    #[test]
    fn update_empty_request_has_no_fields() {
        let req = AssetUpdate::new().into_request();
        assert!(req.title().is_none());
        assert!(req.description().is_none());
        assert!(req.properties().is_none());
        assert!(req.labels().is_none());
    }

    #[test]
    fn update_name_only() {
        let req = AssetUpdate::new().name("New Name").into_request();
        assert_eq!(req.title(), Some("New Name"));
        assert!(req.description().is_none());
    }

    #[test]
    fn update_description_only() {
        let req = AssetUpdate::new().description("desc").into_request();
        assert!(req.title().is_none());
        assert_eq!(req.description(), Some("desc"));
    }

    #[test]
    fn update_properties_converted_correctly() {
        let req = AssetUpdate::new()
            .properties([("k1", "v1"), ("k2", "v2")])
            .into_request();
        let props = req.properties().expect("properties should be set");
        assert_eq!(props.len(), 2);
        assert_eq!(
            props.get(&nominal_api::objects::api::PropertyName("k1".into())),
            Some(&nominal_api::objects::api::PropertyValue("v1".into()))
        );
        assert_eq!(
            props.get(&nominal_api::objects::api::PropertyName("k2".into())),
            Some(&nominal_api::objects::api::PropertyValue("v2".into()))
        );
    }

    #[test]
    fn update_labels_converted_and_deduplicated() {
        let req = AssetUpdate::new()
            .labels(["tag1", "tag2", "tag1"])
            .into_request();
        let labels = req.labels().expect("labels should be set");
        // BTreeSet deduplicates
        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&nominal_api::objects::api::Label("tag1".into())));
        assert!(labels.contains(&nominal_api::objects::api::Label("tag2".into())));
    }

    #[test]
    fn update_all_fields() {
        let req = AssetUpdate::new()
            .name("name")
            .description("desc")
            .properties([("k", "v")])
            .labels(["t"])
            .into_request();
        assert_eq!(req.title(), Some("name"));
        assert_eq!(req.description(), Some("desc"));
        assert!(req.properties().is_some());
        assert!(req.labels().is_some());
    }
}
