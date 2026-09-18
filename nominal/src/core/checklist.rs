use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::{BearerToken, SafeLong};
use conjure_runtime::Client;
use futures::{Stream, TryStreamExt};
use nominal_api::clients::scout::checklistexecution::api::{
    AsyncChecklistExecutionService, AsyncChecklistExecutionServiceClient,
};
use nominal_api::clients::scout::checks::api::{
    AsyncChecklistService, AsyncChecklistServiceClient,
};
use nominal_api::objects::api::rids::WorkspaceRid;
use nominal_api::objects::api::{Label, PropertyName, PropertyValue, SetOperator};
use nominal_api::objects::scout::checklistexecution::api::{
    BatchChecklistLiveStatusRequest, CheckLiveStatusResponse as ApiCheckLiveStatusResponse,
    CheckStatus as ApiCheckStatus, ChecklistLiveStatus as ApiChecklistLiveStatus,
    ChecklistLiveStatusRequest, ExecuteChecklistForAssetsRequest,
    ListStreamingChecklistForAssetRequest, ListStreamingChecklistForAssetResponse,
    StopStreamingChecklistForAssetsRequest,
};
use nominal_api::objects::scout::checks::api::{
    ArchiveChecklistsRequest, ChecklistSearchQuery, SearchChecklistsRequest,
    UnarchiveChecklistsRequest, VersionedChecklist, VersionedChecklistPage,
};
use nominal_api::objects::scout::integrations::api::{IntegrationRid, NotificationConfiguration};
use nominal_api::objects::scout::rids::api::{
    AssetRid, ChecklistRid, LabelsFilter, PropertiesFilter,
};
use nominal_api::objects::scout::run::api::Duration as ApiDuration;

use crate::core::rid::{parse_rid, rid_to_string};
use crate::core::utils::paginate_stream;
use crate::{Error, Result};

/// Represents a checklist in Nominal.
///
/// Checklists are versioned collections of checks evaluated against assets or runs.
/// Attach a checklist to a set of assets with
/// [`execute_streaming`](ChecklistsClient::execute_streaming) to run it continuously.
#[derive(Debug, Clone)]
pub struct Checklist {
    rid: String,
    title: String,
    description: Option<String>,
    commit_id: String,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    created_at: DateTime<Utc>,
    is_archived: bool,
    is_published: bool,
    app_base_url: String,
}

impl Checklist {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The commit ID identifying this version of the checklist.
    pub fn commit_id(&self) -> &str {
        &self.commit_id
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

    pub fn is_archived(&self) -> bool {
        self.is_archived
    }

    pub fn is_published(&self) -> bool {
        self.is_published
    }

    /// Get the URL to view this checklist in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/checklists/{}", self.app_base_url, self.rid)
    }

    pub(crate) fn from_versioned(vc: VersionedChecklist, app_base_url: &str) -> Self {
        let rid = rid_to_string(vc.rid());
        let metadata = vc.metadata();
        let description = if metadata.description().is_empty() {
            None
        } else {
            Some(metadata.description().to_string())
        };
        Self {
            rid,
            title: metadata.title().to_string(),
            description,
            commit_id: vc.commit().id().to_string(),
            properties: metadata
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: metadata.labels().iter().map(|l| l.to_string()).collect(),
            created_at: metadata.created_at().to_utc(),
            is_archived: metadata.is_archived(),
            is_published: metadata.is_published(),
            app_base_url: app_base_url.to_string(),
        }
    }
}

/// A query for searching checklists, composable with [`and`](ChecklistQuery::and)
/// and [`or`](ChecklistQuery::or).
#[derive(Debug, Clone)]
pub enum ChecklistQuery {
    /// Fuzzy full-text search against title and description.
    SearchText(String),
    /// Filter by label.
    Label(String),
    /// Filter by property key and value.
    Property(String, String),
    /// Filter to published or unpublished checklists.
    IsPublished(bool),
    /// Filter to archived or non-archived checklists.
    IsArchived(bool),
    /// Invert a sub-query.
    Not(Box<ChecklistQuery>),
    /// All sub-queries must match.
    And(Vec<ChecklistQuery>),
    /// At least one sub-query must match.
    Or(Vec<ChecklistQuery>),
}

impl ChecklistQuery {
    pub fn search_text(text: impl Into<String>) -> Self {
        Self::SearchText(text.into())
    }

    pub fn label(label: impl Into<String>) -> Self {
        Self::Label(label.into())
    }

    pub fn property(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Property(key.into(), value.into())
    }

    pub fn is_published(value: bool) -> Self {
        Self::IsPublished(value)
    }

    pub fn is_archived(value: bool) -> Self {
        Self::IsArchived(value)
    }

    pub fn negate(query: ChecklistQuery) -> Self {
        Self::Not(Box::new(query))
    }

    pub fn and(queries: impl IntoIterator<Item = ChecklistQuery>) -> Self {
        Self::And(queries.into_iter().collect())
    }

    pub fn or(queries: impl IntoIterator<Item = ChecklistQuery>) -> Self {
        Self::Or(queries.into_iter().collect())
    }

    fn into_conjure(self) -> Result<ChecklistSearchQuery> {
        Ok(match self {
            Self::SearchText(s) => ChecklistSearchQuery::SearchText(s),
            Self::Label(l) => ChecklistSearchQuery::Labels(
                LabelsFilter::builder()
                    .operator(SetOperator::Or)
                    .extend_labels([Label(l)])
                    .build(),
            ),
            Self::Property(k, v) => ChecklistSearchQuery::Properties(
                PropertiesFilter::builder()
                    .name(PropertyName(k))
                    .extend_values([PropertyValue(v)])
                    .build(),
            ),
            Self::IsPublished(b) => ChecklistSearchQuery::IsPublished(b),
            Self::IsArchived(b) => ChecklistSearchQuery::IsArchived(b),
            Self::Not(q) => ChecklistSearchQuery::Not(Box::new(q.into_conjure()?)),
            Self::And(qs) => ChecklistSearchQuery::And(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<Result<Vec<_>>>()?,
            ),
            Self::Or(qs) => ChecklistSearchQuery::Or(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<Result<Vec<_>>>()?,
            ),
        })
    }
}

/// Options for [`ChecklistsClient::execute_streaming`].
///
/// Defaults: `evaluation_delay = 0`, `recovery_delay = 15s`, `auto_create_events = false`,
/// no notification integrations.
#[derive(Debug, Clone, Default)]
pub struct ExecuteStreamingChecklist {
    evaluation_delay: Option<Duration>,
    recovery_delay: Option<Duration>,
    auto_create_events: Option<bool>,
    integration_rids: Vec<String>,
}

impl ExecuteStreamingChecklist {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Delay applied before evaluating the checklist. Useful when data lags behind live.
    #[must_use]
    pub fn evaluation_delay(mut self, value: Duration) -> Self {
        self.evaluation_delay = Some(value);
        self
    }

    /// Minimum time that must pass before a check can recover from a failure. Server minimum is 15s.
    #[must_use]
    pub fn recovery_delay(mut self, value: Duration) -> Self {
        self.recovery_delay = Some(value);
        self
    }

    /// If true, events are created when checks fail and recover.
    #[must_use]
    pub fn auto_create_events(mut self, value: bool) -> Self {
        self.auto_create_events = Some(value);
        self
    }

    /// Integration RIDs to notify on check violations.
    #[must_use]
    pub fn integration_rids<I, S>(mut self, value: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.integration_rids = value.into_iter().map(Into::into).collect();
        self
    }
}

/// Live status of a streaming checklist executing against an asset.
///
/// Returned by [`ChecklistsClient::live_status`]. Fields carrying running-only data
/// ([`commit_id`](Self::commit_id) and [`check_results`](Self::check_results)) are only
/// populated when [`state`](Self::state) is [`ChecklistState::Running`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistLiveStatus {
    state: ChecklistState,
    commit_id: Option<String>,
    check_results: Vec<CheckLiveStatus>,
}

impl ChecklistLiveStatus {
    /// Execution state reported by the server.
    pub fn state(&self) -> ChecklistState {
        self.state
    }

    /// The commit ID currently loaded by the running evaluator. `None` unless the
    /// evaluator is running.
    pub fn commit_id(&self) -> Option<&str> {
        self.commit_id.as_deref()
    }

    /// Latest per-check statuses reported by the evaluator. Empty unless the evaluator
    /// is running.
    pub fn check_results(&self) -> &[CheckLiveStatus] {
        &self.check_results
    }

    fn from_conjure(status: &ApiChecklistLiveStatus) -> Self {
        match status {
            ApiChecklistLiveStatus::Running(r) => Self {
                state: ChecklistState::Running,
                commit_id: Some(r.commit_id().to_string()),
                check_results: r
                    .check_results()
                    .iter()
                    .map(CheckLiveStatus::from_conjure)
                    .collect(),
            },
            ApiChecklistLiveStatus::Initializing(_) => Self {
                state: ChecklistState::Initializing,
                commit_id: None,
                check_results: Vec::new(),
            },
            ApiChecklistLiveStatus::Failed(_) => Self {
                state: ChecklistState::Failed,
                commit_id: None,
                check_results: Vec::new(),
            },
            ApiChecklistLiveStatus::Unknown(_) => Self {
                state: ChecklistState::Unknown,
                commit_id: None,
                check_results: Vec::new(),
            },
        }
    }
}

/// Execution state of a streaming checklist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecklistState {
    /// The evaluator is running.
    Running,
    /// The evaluator is starting up.
    Initializing,
    /// The evaluator failed unexpectedly.
    Failed,
    /// The server returned a variant this client does not recognize.
    Unknown,
}

/// Per-check status reported by a running streaming checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckLiveStatus {
    check_rid: String,
    status: CheckStatus,
    check_parameter_index: Option<i32>,
}

impl CheckLiveStatus {
    pub fn check_rid(&self) -> &str {
        &self.check_rid
    }

    pub fn status(&self) -> CheckStatus {
        self.status
    }

    /// Parameter index for a check whose condition has multiple implementations.
    /// `None` for single-implementation checks.
    pub fn check_parameter_index(&self) -> Option<i32> {
        self.check_parameter_index
    }

    fn from_conjure(response: &ApiCheckLiveStatusResponse) -> Self {
        Self {
            check_rid: rid_to_string(response.check_rid()),
            status: CheckStatus::from_conjure(response.status()),
            check_parameter_index: response.check_parameter_index(),
        }
    }
}

/// Result of a single check evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Invalid,
    Skipped,
    /// The server returned a variant this client does not recognize.
    Unknown,
}

impl CheckStatus {
    fn from_conjure(status: &ApiCheckStatus) -> Self {
        match status {
            ApiCheckStatus::Pass(_) => Self::Pass,
            ApiCheckStatus::Fail(_) => Self::Fail,
            ApiCheckStatus::Invalid(_) => Self::Invalid,
            ApiCheckStatus::Skipped(_) => Self::Skipped,
            ApiCheckStatus::Unknown(_) => Self::Unknown,
        }
    }
}

fn duration_to_api(d: Duration) -> Result<ApiDuration> {
    let out_of_range = || Error::DurationOutOfRange {
        seconds: d.as_secs(),
        nanos: d.subsec_nanos(),
    };
    let secs = i64::try_from(d.as_secs()).map_err(|_| out_of_range())?;
    let seconds = SafeLong::new(secs).map_err(|_| out_of_range())?;
    let nanos = SafeLong::new(d.subsec_nanos() as i64).map_err(|_| out_of_range())?;
    Ok(ApiDuration::builder().seconds(seconds).nanos(nanos).build())
}

/// Client for checklist collection operations (get, search) and streaming attachment.
pub struct ChecklistsClient {
    checks_service: AsyncChecklistServiceClient<Client>,
    execution_service: AsyncChecklistExecutionServiceClient<Client>,
    token: BearerToken,
    workspace_rid: Option<WorkspaceRid>,
    app_base_url: String,
}

impl ChecklistsClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<WorkspaceRid>,
        app_base_url: String,
    ) -> Self {
        Self {
            checks_service: AsyncChecklistServiceClient::new(client.clone(), runtime),
            execution_service: AsyncChecklistExecutionServiceClient::new(client, runtime),
            token,
            workspace_rid,
            app_base_url,
        }
    }

    /// Get a checklist by RID. Returns the latest commit on the main branch.
    pub async fn get(&self, rid: &str) -> Result<Checklist> {
        let checklist_rid = parse_rid::<ChecklistRid>(rid)?;
        let response = self
            .checks_service
            .get(&self.token, &checklist_rid, None, None)
            .await
            .map_err(Error::from)?;
        Ok(Checklist::from_versioned(response, &self.app_base_url))
    }

    fn scoped_conjure_query(&self, query: ChecklistSearchQuery) -> ChecklistSearchQuery {
        let Some(ws) = self.workspace_rid.as_ref() else {
            return query;
        };
        ChecklistSearchQuery::And(vec![query, ChecklistSearchQuery::Workspace(ws.clone())])
    }

    fn search_stream(&self, query: ChecklistSearchQuery) -> impl Stream<Item = Result<Checklist>> {
        let service = self.checks_service.clone();
        let token = self.token.clone();
        let app_base_url = self.app_base_url.clone();
        paginate_stream(
            move |page_token| {
                SearchChecklistsRequest::builder()
                    .query(query.clone())
                    .next_page_token(page_token)
                    .build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move { service.search(&token, &req).await.map_err(Error::from) }
            },
            |resp: &VersionedChecklistPage| resp.next_page_token().cloned(),
            move |resp| {
                resp.values()
                    .iter()
                    .cloned()
                    .map(|vc| Checklist::from_versioned(vc, &app_base_url))
                    .collect()
            },
        )
    }

    /// Search checklists with a query, collecting all pages eagerly.
    ///
    /// The query is workspace-scoped when the client is configured with a workspace RID.
    pub async fn search(&self, query: ChecklistQuery) -> Result<Vec<Checklist>> {
        let conjure_query = self.scoped_conjure_query(query.into_conjure()?);
        self.search_stream(conjure_query).try_collect().await
    }

    fn list_for_asset_stream(&self, asset_rid: AssetRid) -> impl Stream<Item = Result<String>> {
        let service = self.execution_service.clone();
        let token = self.token.clone();
        paginate_stream(
            move |page_token: Option<conjure_object::Uuid>| {
                let mut b =
                    ListStreamingChecklistForAssetRequest::builder().asset_rid(asset_rid.clone());
                if let Some(t) = page_token {
                    b = b.page_token(t);
                }
                b.build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move {
                    service
                        .list_streaming_checklist_for_asset(&token, &req)
                        .await
                        .map_err(Error::from)
                }
            },
            |resp: &ListStreamingChecklistForAssetResponse| resp.next_page_token(),
            |resp| resp.checklists().iter().map(rid_to_string).collect(),
        )
    }

    /// List streaming checklists currently attached to the given asset.
    ///
    /// Each attachment is hydrated via [`get`](Self::get), which returns the latest
    /// commit on the main branch.
    pub async fn list_for_asset(&self, asset_rid: &str) -> Result<Vec<Checklist>> {
        let asset_rid = parse_rid::<AssetRid>(asset_rid)?;
        let rids: Vec<String> = self.list_for_asset_stream(asset_rid).try_collect().await?;
        futures::future::try_join_all(rids.iter().map(|rid| self.get(rid))).await
    }

    /// Get the live status of a streaming checklist against a specific asset.
    ///
    /// When the streaming checklist is running, the returned status carries the commit ID
    /// the evaluator currently has loaded, which may differ from the latest commit on the
    /// main branch. Errors with `ChecklistExecution:StreamingChecklistNotFoundForAsset`
    /// if no streaming checklist is attached to the asset.
    pub async fn live_status(
        &self,
        checklist_rid: &str,
        asset_rid: &str,
    ) -> Result<ChecklistLiveStatus> {
        let checklist_rid = parse_rid::<ChecklistRid>(checklist_rid)?;
        let asset_rid = parse_rid::<AssetRid>(asset_rid)?;
        let request = BatchChecklistLiveStatusRequest::builder()
            .requests([ChecklistLiveStatusRequest::new(checklist_rid, asset_rid)])
            .build();
        let response = self
            .execution_service
            .checklist_live_status(&self.token, &request)
            .await
            .map_err(Error::from)?;
        response
            .checklist_live_status_responses()
            .first()
            .map(|r| ChecklistLiveStatus::from_conjure(r.status()))
            .ok_or_else(|| Error::UnexpectedResponse {
                field: "checklistLiveStatusResponses",
            })
    }

    /// Attach a streaming checklist to a set of assets.
    ///
    /// If the checklist is already running on any of the given assets, the existing
    /// configuration is replaced with the one specified here.
    pub async fn execute_streaming<I, S>(
        &self,
        checklist_rid: &str,
        asset_rids: I,
        options: ExecuteStreamingChecklist,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let checklist_rid = parse_rid::<ChecklistRid>(checklist_rid)?;
        let asset_rids = asset_rids
            .into_iter()
            .map(|s| parse_rid::<AssetRid>(s.as_ref()).map_err(Error::from))
            .collect::<Result<BTreeSet<_>>>()?;

        let evaluation_delay =
            duration_to_api(options.evaluation_delay.unwrap_or(Duration::from_secs(0)))?;
        let recovery_delay =
            duration_to_api(options.recovery_delay.unwrap_or(Duration::from_secs(15)))?;

        let notification_configurations = options
            .integration_rids
            .into_iter()
            .map(|r| {
                parse_rid::<IntegrationRid>(&r)
                    .map(NotificationConfiguration::new)
                    .map_err(Error::from)
            })
            .collect::<Result<Vec<_>>>()?;

        let mut b = ExecuteChecklistForAssetsRequest::builder()
            .checklist(checklist_rid)
            .evaluation_delay(evaluation_delay)
            .recovery_delay(recovery_delay)
            .assets(asset_rids)
            .notification_configurations(notification_configurations);

        if let Some(a) = options.auto_create_events {
            b = b.auto_create_events(a);
        }

        self.execution_service
            .execute_streaming_checklist(&self.token, &b.build())
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Stop a streaming checklist on the given assets.
    pub async fn stop_streaming_for_assets<I, S>(
        &self,
        checklist_rid: &str,
        asset_rids: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let checklist_rid = parse_rid::<ChecklistRid>(checklist_rid)?;
        let asset_rids = asset_rids
            .into_iter()
            .map(|s| parse_rid::<AssetRid>(s.as_ref()).map_err(Error::from))
            .collect::<Result<BTreeSet<_>>>()?;
        let request = StopStreamingChecklistForAssetsRequest::builder()
            .checklist(checklist_rid)
            .assets(asset_rids)
            .build();
        self.execution_service
            .stop_streaming_checklist_for_assets(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Stop a streaming checklist on all assets it is currently attached to.
    pub async fn stop_streaming(&self, checklist_rid: &str) -> Result<()> {
        let checklist_rid = parse_rid::<ChecklistRid>(checklist_rid)?;
        self.execution_service
            .stop_streaming_checklist(&self.token, &checklist_rid)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Archive a checklist. Archived checklists are hidden from the UI but not deleted.
    pub async fn archive(&self, checklist_rid: &str) -> Result<()> {
        let rid = parse_rid::<ChecklistRid>(checklist_rid)?;
        let mut rids = BTreeSet::new();
        rids.insert(rid);
        let request = ArchiveChecklistsRequest::builder().rids(rids).build();
        self.checks_service
            .archive(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive a checklist, restoring its visibility in the UI.
    pub async fn unarchive(&self, checklist_rid: &str) -> Result<()> {
        let rid = parse_rid::<ChecklistRid>(checklist_rid)?;
        let mut rids = BTreeSet::new();
        rids.insert(rid);
        let request = UnarchiveChecklistsRequest::builder().rids(rids).build();
        self.checks_service
            .unarchive(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_search_text() {
        let q = ChecklistQuery::search_text("hello");
        assert_eq!(
            q.into_conjure().unwrap(),
            ChecklistSearchQuery::SearchText("hello".into())
        );
    }

    #[test]
    fn query_label() {
        let q = ChecklistQuery::label("my-label");
        let ChecklistSearchQuery::Labels(f) = q.into_conjure().unwrap() else {
            panic!("expected Labels variant");
        };
        assert_eq!(f.labels(), [Label("my-label".into())]);
    }

    #[test]
    fn query_property() {
        let q = ChecklistQuery::property("key", "val");
        let ChecklistSearchQuery::Properties(f) = q.into_conjure().unwrap() else {
            panic!("expected Properties variant");
        };
        assert_eq!(f.name(), &PropertyName("key".into()));
        assert_eq!(f.values(), [PropertyValue("val".into())]);
    }

    #[test]
    fn query_is_published() {
        let q = ChecklistQuery::is_published(true);
        assert_eq!(
            q.into_conjure().unwrap(),
            ChecklistSearchQuery::IsPublished(true)
        );
    }

    #[test]
    fn query_and_or_nesting() {
        let q = ChecklistQuery::and([
            ChecklistQuery::label("x"),
            ChecklistQuery::or([
                ChecklistQuery::property("k", "v1"),
                ChecklistQuery::property("k", "v2"),
            ]),
        ]);
        let ChecklistSearchQuery::And(children) = q.into_conjure().unwrap() else {
            panic!("expected And variant");
        };
        assert!(matches!(children[0], ChecklistSearchQuery::Labels(_)));
        assert!(matches!(children[1], ChecklistSearchQuery::Or(_)));
    }

    #[test]
    fn query_not() {
        let q = ChecklistQuery::negate(ChecklistQuery::is_archived(true));
        let ChecklistSearchQuery::Not(inner) = q.into_conjure().unwrap() else {
            panic!("expected Not variant");
        };
        assert_eq!(*inner, ChecklistSearchQuery::IsArchived(true));
    }

    #[test]
    fn duration_conversion() {
        let d = Duration::new(42, 123);
        let api = duration_to_api(d).unwrap();
        assert_eq!(api.seconds(), SafeLong::new(42).unwrap());
        assert_eq!(api.nanos(), SafeLong::new(123).unwrap());
    }

    #[test]
    fn duration_out_of_range_seconds_errors() {
        // SafeLong upper bound is 2^53 - 1; any value above that must fail.
        let d = Duration::from_secs(u64::MAX);
        let err = duration_to_api(d).unwrap_err();
        assert!(matches!(err, Error::DurationOutOfRange { .. }));
    }
}
