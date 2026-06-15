use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::{Stream, TryStreamExt};
use nominal_api::clients::scout::{AsyncNotebookService, AsyncNotebookServiceClient};
use nominal_api::objects::api::{Label, PropertyName, PropertyValue, SetOperator};
use nominal_api::objects::scout::notebook::api::{
    AssetsFilter, CreateNotebookRequest, NotebookDataScope, NotebookMetadata, RunsFilter,
    SearchNotebooksQuery, SearchNotebooksRequest, SearchNotebooksResponse,
};
use nominal_api::objects::scout::rids::api::{LabelsFilter, PropertiesFilter};
use nominal_api::objects::scout::workbookcommon::api::UnifiedWorkbookContent;

use crate::core::rid::{parse_rid, rid_to_string};
use crate::core::template::Template;
use crate::core::utils::paginate_stream;
use crate::{Error, Result};

/// Represents a workbook in Nominal.
///
/// Workbooks are saved analysis views attached to an asset or run, visualizing
/// time-series data through panels, charts, and tables.
#[derive(Debug, Clone)]
pub struct Workbook {
    rid: String,
    name: String,
    description: Option<String>,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    data_scope: WorkbookDataScope,
    created_at: DateTime<Utc>,
    app_base_url: String,
}

/// A workbook's data scope: either a set of assets or a set of runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbookDataScope {
    Assets(Vec<String>),
    Runs(Vec<String>),
}

impl WorkbookDataScope {
    pub fn assets<I>(rids: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Self::Assets(rids.into_iter().map(Into::into).collect())
    }

    pub fn runs<I>(rids: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Self::Runs(rids.into_iter().map(Into::into).collect())
    }

    pub(crate) fn from_conjure(scope: &NotebookDataScope) -> Self {
        match scope {
            NotebookDataScope::AssetRids(rids) => {
                Self::Assets(rids.iter().map(rid_to_string).collect())
            }
            NotebookDataScope::RunRids(rids) => {
                Self::Runs(rids.iter().map(rid_to_string).collect())
            }
            _ => Self::Assets(Vec::new()),
        }
    }

    pub(crate) fn into_conjure(self) -> Result<NotebookDataScope> {
        Ok(match self {
            Self::Assets(rids) => NotebookDataScope::AssetRids(
                rids.iter()
                    .map(|r| parse_rid(r).map_err(Error::from))
                    .collect::<Result<BTreeSet<_>>>()?,
            ),
            Self::Runs(rids) => NotebookDataScope::RunRids(
                rids.iter()
                    .map(|r| parse_rid(r).map_err(Error::from))
                    .collect::<Result<BTreeSet<_>>>()?,
            ),
        })
    }
}

impl Workbook {
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

    /// The workbook's data scope.
    pub fn data_scope(&self) -> &WorkbookDataScope {
        &self.data_scope
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Get the URL to view this workbook in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/workbooks/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_metadata(
        rid: String,
        metadata: &NotebookMetadata,
        app_base_url: &str,
    ) -> Self {
        let data_scope = WorkbookDataScope::from_conjure(metadata.data_scope());
        let description = if metadata.description().is_empty() {
            None
        } else {
            Some(metadata.description().to_string())
        };
        Self {
            rid,
            name: metadata.title().to_string(),
            description,
            properties: metadata
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: metadata.labels().iter().map(|l| l.to_string()).collect(),
            data_scope,
            created_at: metadata.created_at().to_utc(),
            app_base_url: app_base_url.to_string(),
        }
    }

    pub(crate) fn from_conjure(
        notebook: nominal_api::objects::scout::notebook::api::Notebook,
        app_base_url: &str,
    ) -> Self {
        Self::from_metadata(
            rid_to_string(notebook.rid()),
            notebook.metadata(),
            app_base_url,
        )
    }
}

/// Parameters for creating a workbook from a template.
#[derive(Debug, Default, Clone)]
pub struct WorkbookCreate {
    title: Option<String>,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
}

impl WorkbookCreate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
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
}

/// A query for searching workbooks, composable with [`and`](WorkbookQuery::and) and [`or`](WorkbookQuery::or).
#[derive(Debug, Clone)]
pub enum WorkbookQuery {
    /// Fuzzy full-text search against title and description.
    SearchText(String),
    /// Filter by label.
    Label(String),
    /// Filter by property key and value.
    Property(String, String),
    /// Filter to workbooks attached to a given asset.
    AssetRid(String),
    /// Filter to workbooks attached to a given run.
    RunRid(String),
    /// All sub-queries must match.
    And(Vec<WorkbookQuery>),
    /// At least one sub-query must match.
    Or(Vec<WorkbookQuery>),
}

impl WorkbookQuery {
    /// Fuzzy full-text search against title and description.
    pub fn search_text(text: impl Into<String>) -> Self {
        Self::SearchText(text.into())
    }

    /// Filter by label.
    pub fn label(label: impl Into<String>) -> Self {
        Self::Label(label.into())
    }

    /// Filter by property key and value.
    pub fn property(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Property(key.into(), value.into())
    }

    /// Filter to workbooks attached to a given asset.
    pub fn asset_rid(rid: impl Into<String>) -> Self {
        Self::AssetRid(rid.into())
    }

    /// Filter to workbooks attached to a given run.
    pub fn run_rid(rid: impl Into<String>) -> Self {
        Self::RunRid(rid.into())
    }

    /// All sub-queries must match.
    pub fn and(queries: impl IntoIterator<Item = WorkbookQuery>) -> Self {
        Self::And(queries.into_iter().collect())
    }

    /// At least one sub-query must match.
    pub fn or(queries: impl IntoIterator<Item = WorkbookQuery>) -> Self {
        Self::Or(queries.into_iter().collect())
    }

    fn into_conjure(self) -> Result<SearchNotebooksQuery> {
        Ok(match self {
            Self::SearchText(s) => SearchNotebooksQuery::SearchText(s),
            Self::Label(l) => SearchNotebooksQuery::Labels(
                LabelsFilter::builder()
                    .operator(SetOperator::Or)
                    .extend_labels([Label(l)])
                    .build(),
            ),
            Self::Property(k, v) => SearchNotebooksQuery::Properties(
                PropertiesFilter::builder()
                    .name(PropertyName(k))
                    .extend_values([PropertyValue(v)])
                    .build(),
            ),
            Self::AssetRid(r) => SearchNotebooksQuery::AssetRids(
                AssetsFilter::builder()
                    .operator(SetOperator::Or)
                    .extend_assets([parse_rid(&r)?])
                    .build(),
            ),
            Self::RunRid(r) => SearchNotebooksQuery::RunRids(
                RunsFilter::builder()
                    .operator(SetOperator::Or)
                    .extend_runs([parse_rid(&r)?])
                    .build(),
            ),
            Self::And(qs) => SearchNotebooksQuery::And(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<Result<Vec<_>>>()?,
            ),
            Self::Or(qs) => SearchNotebooksQuery::Or(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<Result<Vec<_>>>()?,
            ),
        })
    }
}

/// Client for workbook collection operations (get, search).
pub struct WorkbooksClient {
    service: AsyncNotebookServiceClient<Client>,
    token: BearerToken,
    workspace_rid: Option<String>,
    app_base_url: String,
}

impl WorkbooksClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<String>,
        app_base_url: String,
    ) -> Self {
        Self {
            service: AsyncNotebookServiceClient::new(client, runtime),
            token,
            workspace_rid,
            app_base_url,
        }
    }

    /// Create a new workbook from a template, attached to the given asset or run.
    ///
    /// The workbook reuses the template's layout and content as-is. The title and
    /// description default to the template's values when not overridden.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::{WorkbookCreate, WorkbookDataScope};
    /// let template = client.templates().get("ri.scout.cerulean-staging.template.<uuid>").await?;
    /// let workbook = client.workbooks().create_from_template(
    ///     &template,
    ///     WorkbookDataScope::assets(["ri.scout.cerulean-staging.asset.<uuid>"]),
    ///     WorkbookCreate::new().properties([("source_template_rid", template.rid())]),
    /// ).await?;
    /// # Ok(()) }
    /// ```
    pub async fn create_from_template(
        &self,
        template: &Template,
        scope: WorkbookDataScope,
        create: WorkbookCreate,
    ) -> Result<Workbook> {
        use nominal_api::objects::api::rids::WorkspaceRid;

        let data_scope = scope.into_conjure()?;
        let WorkbookCreate {
            title,
            description,
            properties,
            labels,
        } = create;

        let title = title.unwrap_or_else(|| template.title().to_string());
        let description = description
            .or_else(|| template.description().map(str::to_string))
            .unwrap_or_default();

        let mut b = CreateNotebookRequest::builder()
            .title(title)
            .description(description)
            .is_draft(false)
            .state_as_json("{}")
            .layout(template.layout().clone())
            .data_scope(data_scope)
            .content_v2(UnifiedWorkbookContent::Workbook(template.content().clone()));

        if let Some(l) = labels {
            b = b.labels(l.into_iter().map(Label).collect::<BTreeSet<_>>());
        }
        if let Some(p) = properties {
            b = b.properties(
                p.into_iter()
                    .map(|(k, v)| (PropertyName(k), PropertyValue(v)))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        if let Some(wid) = self.workspace_rid.as_deref() {
            b = b.workspace(parse_rid::<WorkspaceRid>(wid)?);
        }

        let response = self
            .service
            .create(&self.token, &b.build())
            .await
            .map_err(Error::from)?;
        Ok(Workbook::from_conjure(response, &self.app_base_url))
    }

    /// Get a workbook by RID.
    pub async fn get(&self, rid: &str) -> Result<Workbook> {
        let notebook_rid = parse_rid(rid)?;
        let response = self
            .service
            .get(&self.token, &notebook_rid, None)
            .await
            .map_err(Error::from)?;
        Ok(Workbook::from_conjure(response, &self.app_base_url))
    }

    /// Get multiple workbooks by RID.
    ///
    /// Returns a map from RID string to Workbook. RIDs not found in Nominal are omitted.
    pub async fn get_batch<I, S>(&self, rids: I) -> Result<HashMap<String, Workbook>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let rid_set = rids
            .into_iter()
            .map(|s| parse_rid(s.as_ref()).map_err(Error::from))
            .collect::<Result<BTreeSet<_>>>()?;
        let response = self
            .service
            .batch_get(&self.token, &rid_set)
            .await
            .map_err(Error::from)?;
        Ok(response
            .into_iter()
            .map(|n| {
                let rid = rid_to_string(n.rid());
                (rid, Workbook::from_conjure(n, &self.app_base_url))
            })
            .collect())
    }

    fn search_stream(&self, query: SearchNotebooksQuery) -> impl Stream<Item = Result<Workbook>> {
        let service = self.service.clone();
        let token = self.token.clone();
        let app_base_url = self.app_base_url.clone();
        paginate_stream(
            move |page_token| {
                SearchNotebooksRequest::builder()
                    .query(query.clone())
                    .show_drafts(true)
                    .next_page_token(page_token)
                    .build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move { service.search(&token, &req).await.map_err(Error::from) }
            },
            |resp: &SearchNotebooksResponse| resp.next_page_token().cloned(),
            move |resp| {
                resp.results()
                    .iter()
                    .map(|n| {
                        Workbook::from_metadata(rid_to_string(n.rid()), n.metadata(), &app_base_url)
                    })
                    .collect()
            },
        )
    }

    /// Search workbooks with a query, collecting all pages eagerly.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::WorkbookQuery;
    /// let workbooks = client.workbooks()
    ///     .search(WorkbookQuery::and([
    ///         WorkbookQuery::asset_rid("ri.scout.cerulean-staging.asset.<uuid>"),
    ///         WorkbookQuery::property("source_template_rid", "ri.scout.cerulean-staging.template.<uuid>"),
    ///     ]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search(&self, query: WorkbookQuery) -> Result<Vec<Workbook>> {
        let conjure_query = query.into_conjure()?;
        self.search_stream(conjure_query).try_collect().await
    }

    /// Archive a workbook. Archived workbooks are hidden from the UI but not deleted.
    pub async fn archive(&self, rid: &str) -> Result<()> {
        let notebook_rid = parse_rid(rid)?;
        self.service
            .archive(&self.token, &notebook_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive a workbook, restoring its visibility in the UI.
    pub async fn unarchive(&self, rid: &str) -> Result<()> {
        let notebook_rid = parse_rid(rid)?;
        self.service
            .unarchive(&self.token, &notebook_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSET_RID: &str = "ri.scout.cerulean-staging.asset.00000000-0000-0000-0000-000000000001";

    // --- WorkbookQuery::into_conjure ---

    #[test]
    fn query_search_text() {
        let q = WorkbookQuery::search_text("hello");
        assert_eq!(
            q.into_conjure().unwrap(),
            SearchNotebooksQuery::SearchText("hello".into())
        );
    }

    #[test]
    fn query_label() {
        let q = WorkbookQuery::label("my-label");
        let SearchNotebooksQuery::Labels(f) = q.into_conjure().unwrap() else {
            panic!("expected Labels variant");
        };
        assert_eq!(f.labels(), [Label("my-label".into())]);
    }

    #[test]
    fn query_property() {
        let q = WorkbookQuery::property("key", "val");
        let SearchNotebooksQuery::Properties(f) = q.into_conjure().unwrap() else {
            panic!("expected Properties variant");
        };
        assert_eq!(f.name(), &PropertyName("key".into()));
        assert_eq!(f.values(), [PropertyValue("val".into())]);
    }

    #[test]
    fn query_asset_rid() {
        let q = WorkbookQuery::asset_rid(ASSET_RID);
        let SearchNotebooksQuery::AssetRids(f) = q.into_conjure().unwrap() else {
            panic!("expected AssetRids variant");
        };
        assert_eq!(f.assets().len(), 1);
    }

    #[test]
    fn query_asset_rid_invalid_errors() {
        let q = WorkbookQuery::asset_rid("not a rid");
        assert!(q.into_conjure().is_err());
    }

    #[test]
    fn query_run_rid() {
        let q = WorkbookQuery::run_rid(
            "ri.scout.cerulean-staging.run.00000000-0000-0000-0000-000000000002",
        );
        let SearchNotebooksQuery::RunRids(f) = q.into_conjure().unwrap() else {
            panic!("expected RunRids variant");
        };
        assert_eq!(f.runs().len(), 1);
    }

    #[test]
    fn query_and_flattens_children() {
        let q = WorkbookQuery::and([WorkbookQuery::search_text("a"), WorkbookQuery::label("b")]);
        let SearchNotebooksQuery::And(children) = q.into_conjure().unwrap() else {
            panic!("expected And variant");
        };
        assert_eq!(children.len(), 2);
        assert!(matches!(children[0], SearchNotebooksQuery::SearchText(_)));
        assert!(matches!(children[1], SearchNotebooksQuery::Labels(_)));
    }

    #[test]
    fn query_or_flattens_children() {
        let q = WorkbookQuery::or([WorkbookQuery::label("x"), WorkbookQuery::label("y")]);
        let SearchNotebooksQuery::Or(children) = q.into_conjure().unwrap() else {
            panic!("expected Or variant");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn query_nested_and_or() {
        let q = WorkbookQuery::and([
            WorkbookQuery::asset_rid(ASSET_RID),
            WorkbookQuery::or([
                WorkbookQuery::property("k", "v1"),
                WorkbookQuery::property("k", "v2"),
            ]),
        ]);
        let SearchNotebooksQuery::And(children) = q.into_conjure().unwrap() else {
            panic!("expected And variant");
        };
        assert!(matches!(children[0], SearchNotebooksQuery::AssetRids(_)));
        assert!(matches!(children[1], SearchNotebooksQuery::Or(_)));
    }
}
