use std::sync::Arc;

use chrono::{DateTime, Utc};
use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use futures::Stream;
use nominal_api::clients::scout::{AsyncRunService, AsyncRunServiceClient};
use nominal_api::objects::api::{Label, PropertyName, PropertyValue, SetOperator};
use nominal_api::objects::scout::rids::api::{LabelsFilter, PropertiesFilter};
use nominal_api::objects::scout::run::api::{
    CreateRunDataSource, CreateRunRequest, CustomTimeframeFilter, SearchQuery, SearchRunsRequest,
    SearchRunsResponse, SortField, SortKey, SortOptions, TimeframeFilter, UpdateAttachmentsRequest,
    UpdateRunRequest,
};
use nominal_api::objects::scout::rids::api::AssetRid;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::core::{
    datasource::DataSource,
    datetime::{NominalDateTime, api_timestamp_to_utc_or_panic},
    rid::{parse_rid, rid_to_string},
    utils::paginate_stream,
};
use crate::{Error, Result};
use futures::TryStreamExt;

/// Represents a run in Nominal.
///
/// Runs are executions of tests, simulations, or analyses within an asset.
/// They contain datasets, events, and other time-series data.
#[derive(Debug, Clone)]
pub struct Run {
    rid: String,
    name: String,
    description: String,
    properties: HashMap<String, String>,
    labels: Vec<String>,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    run_number: u32,
    assets: Vec<String>,
    data_sources: HashMap<String, DataSource>,
    created_at: DateTime<Utc>,
    app_base_url: String,
}

impl Run {
    pub fn rid(&self) -> &str {
        &self.rid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    pub fn start(&self) -> &DateTime<Utc> {
        &self.start
    }

    pub fn end(&self) -> Option<&DateTime<Utc>> {
        self.end.as_ref()
    }

    pub fn run_number(&self) -> u32 {
        self.run_number
    }

    pub fn assets(&self) -> &[String] {
        &self.assets
    }

    /// Data sources attached to this run, keyed by ref name.
    ///
    /// Note: this map is empty for multi-asset runs; attach
    /// data sources to the underlying asset(s) instead.
    pub fn data_sources(&self) -> &HashMap<String, DataSource> {
        &self.data_sources
    }

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Get the URL to view this run in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/runs/{}", self.app_base_url, self.run_number)
    }

    pub(crate) fn from_conjure(
        run: nominal_api::objects::scout::run::api::Run,
        app_base_url: &str,
    ) -> Self {
        let data_sources = run
            .data_sources()
            .iter()
            .filter_map(|(ref_name, rds)| {
                DataSource::from_conjure(rds.data_source()).map(|ds| (ref_name.to_string(), ds))
            })
            .collect();
        Self {
            rid: rid_to_string(run.rid()),
            name: run.title().to_string(),
            description: run.description().to_string(),
            properties: run
                .properties()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            labels: run.labels().iter().map(|l| l.to_string()).collect(),
            start: api_timestamp_to_utc_or_panic(run.start_time()),
            end: run.end_time().map(api_timestamp_to_utc_or_panic),
            run_number: *run.run_number() as u32,
            assets: run.assets().iter().map(rid_to_string).collect(),
            data_sources,
            created_at: run.created_at().to_utc(),
            app_base_url: app_base_url.to_string(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct RunUpdate {
    name: Option<String>,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
}

impl RunUpdate {
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

    #[must_use]
    pub fn start(mut self, value: DateTime<Utc>) -> Self {
        self.start = Some(value);
        self
    }

    #[must_use]
    pub fn end(mut self, value: DateTime<Utc>) -> Self {
        self.end = Some(value);
        self
    }

    pub(crate) fn into_request(self) -> Result<UpdateRunRequest> {
        let RunUpdate {
            name,
            description,
            properties,
            labels,
            start,
            end,
        } = self;

        let mut b = UpdateRunRequest::builder();
        if let Some(n) = name {
            b = b.title(n);
        }
        if let Some(d) = description {
            b = b.description(d);
        }
        if let Some(p) = properties {
            let props = p
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect::<BTreeMap<_, _>>();
            b = b.properties(props);
        }
        if let Some(l) = labels {
            b = b.labels(l.into_iter().map(|s| s.into()).collect::<BTreeSet<_>>());
        }
        if let Some(s) = start {
            b = b.start_time(Some(NominalDateTime::try_from(s)?.into()));
        }
        if let Some(e) = end {
            b = b.end_time(Some(NominalDateTime::try_from(e)?.into()));
        }

        Ok(b.assets(vec![]).build())
    }
}

/// Parameters for creating a new run.
#[derive(Debug, Clone)]
pub struct RunCreate {
    name: String,
    description: Option<String>,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
    assets: Option<Vec<String>>,
}

impl RunCreate {
    /// A run requires at minimum a name and a start time.
    pub fn new(name: impl Into<String>, start: DateTime<Utc>) -> Self {
        Self {
            name: name.into(),
            description: None,
            start,
            end: None,
            properties: None,
            labels: None,
            assets: None,
        }
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn end(mut self, value: DateTime<Utc>) -> Self {
        self.end = Some(value);
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

    /// Asset RIDs to associate this run with.
    #[must_use]
    pub fn assets<I, S>(mut self, value: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.assets = Some(value.into_iter().map(Into::into).collect());
        self
    }

    pub(crate) fn into_request(
        self,
        workspace_rid: Option<&str>,
    ) -> Result<CreateRunRequest> {
        use crate::core::datetime::NominalDateTime;
        use nominal_api::objects::api::rids::WorkspaceRid;

        let RunCreate {
            name,
            description,
            start,
            end,
            properties,
            labels,
            assets,
        } = self;

        let mut b = CreateRunRequest::builder()
            .title(name)
            .description(description.unwrap_or_default())
            .start_time(NominalDateTime::try_from(start)?.into());

        if let Some(e) = end {
            b = b.end_time(Some(NominalDateTime::try_from(e)?.into()));
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
        if let Some(a) = assets {
            let asset_rids = a
                .into_iter()
                .map(|s| parse_rid::<AssetRid>(&s).map_err(Error::from))
                .collect::<Result<Vec<_>>>()?;
            b = b.assets(asset_rids);
        }
        if let Some(wid) = workspace_rid {
            b = b.workspace(parse_rid::<WorkspaceRid>(wid)?);
        }

        Ok(b.build())
    }
}

/// A query for searching runs, which can be composed into a tree with [`and`](RunQuery::and), [`or`](RunQuery::or), and [`not`](RunQuery::not).
#[derive(Debug, Clone)]
pub enum RunQuery {
    /// Fuzzy full-text search against title and description.
    SearchText(String),
    /// Case-insensitive substring match against the run name.
    SubstringMatch(String),
    /// Filter by label.
    Label(String),
    /// Filter by property key and value.
    Property(String, String),
    /// Filter by run number.
    RunNumber(u32),
    /// Filter runs whose start time is at or after this timestamp.
    StartTimeInclusive(DateTime<Utc>),
    /// Filter runs whose end time is at or before this timestamp.
    EndTimeInclusive(DateTime<Utc>),
    /// All sub-queries must match.
    And(Vec<RunQuery>),
    /// At least one sub-query must match.
    Or(Vec<RunQuery>),
    /// Negates the sub-query.
    Not(Box<RunQuery>),
}

impl RunQuery {
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

    pub fn run_number(n: u32) -> Self {
        Self::RunNumber(n)
    }

    pub fn start_time_inclusive(t: DateTime<Utc>) -> Self {
        Self::StartTimeInclusive(t)
    }

    pub fn end_time_inclusive(t: DateTime<Utc>) -> Self {
        Self::EndTimeInclusive(t)
    }

    pub fn and(queries: impl IntoIterator<Item = RunQuery>) -> Self {
        Self::And(queries.into_iter().collect())
    }

    pub fn or(queries: impl IntoIterator<Item = RunQuery>) -> Self {
        Self::Or(queries.into_iter().collect())
    }

    #[allow(clippy::should_implement_trait)]
    pub fn not(query: RunQuery) -> Self {
        Self::Not(Box::new(query))
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

    fn into_conjure(self) -> crate::Result<SearchQuery> {
        use crate::core::datetime::NominalDateTime;
        Ok(match self {
            Self::SearchText(s) => SearchQuery::SearchText(s),
            Self::SubstringMatch(s) => SearchQuery::ExactMatch(s),
            Self::Label(l) => SearchQuery::Labels(
                LabelsFilter::builder()
                    .operator(SetOperator::Or)
                    .extend_labels([Label(l)])
                    .build(),
            ),
            Self::Property(k, v) => SearchQuery::Properties(
                PropertiesFilter::builder()
                    .name(PropertyName(k))
                    .extend_values([PropertyValue(v)])
                    .build(),
            ),
            Self::RunNumber(n) => SearchQuery::RunNumber(
                conjure_object::SafeLong::try_from(n as i64)
                    .expect("u32 is always within SafeLong range"),
            ),
            Self::StartTimeInclusive(t) => {
                let ts = NominalDateTime::try_from(t)?.into();
                SearchQuery::StartTime(Box::new(TimeframeFilter::Custom(
                    CustomTimeframeFilter::builder()
                        .start_time(Some(ts))
                        .build(),
                )))
            }
            Self::EndTimeInclusive(t) => {
                let ts = NominalDateTime::try_from(t)?.into();
                SearchQuery::EndTime(Box::new(TimeframeFilter::Custom(
                    CustomTimeframeFilter::builder().end_time(Some(ts)).build(),
                )))
            }
            Self::And(qs) => SearchQuery::And(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<crate::Result<_>>()?,
            ),
            Self::Or(qs) => SearchQuery::Or(
                qs.into_iter()
                    .map(Self::into_conjure)
                    .collect::<crate::Result<_>>()?,
            ),
            Self::Not(q) => SearchQuery::Not(Box::new(q.into_conjure()?)),
        })
    }
}

/// Client for run collection operations (list, get).
pub struct RunsClient {
    service: AsyncRunServiceClient<Client>,
    token: BearerToken,
    workspace_rid: Option<String>,
    app_base_url: String,
}

impl RunsClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<String>,
        app_base_url: String,
    ) -> Self {
        Self {
            service: AsyncRunServiceClient::new(client, runtime),
            token,
            workspace_rid,
            app_base_url,
        }
    }

    /// Create a new run.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use chrono::Utc;
    /// use nominal::core::RunCreate;
    /// let run = client.runs()
    ///     .create(
    ///         RunCreate::new("orbit-raise-burn", Utc::now())
    ///             .description("Three-impulse burn to GTO")
    ///             .labels(["orbit", "production"])
    ///             .assets(["ri.scout.cerulean-staging.asset.<uuid>"]),
    ///     )
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn create(&self, create: RunCreate) -> Result<Run> {
        let request = create.into_request(self.workspace_rid.as_deref())?;
        let response = self
            .service
            .create_run(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(Run::from_conjure(response, &self.app_base_url))
    }

    /// Get a run by RID.
    pub async fn get(&self, rid: &str) -> Result<Run> {
        let run_rid = parse_rid(rid)?;
        let response = self
            .service
            .get_run(&self.token, &run_rid)
            .await
            .map_err(Error::from)?;
        Ok(Run::from_conjure(response, &self.app_base_url))
    }

    /// Get multiple runs by RID.
    ///
    /// Returns a map from RID string to Run. RIDs not found in Nominal are omitted.
    pub async fn get_batch<I, S>(&self, rids: I) -> Result<HashMap<String, Run>>
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
            .get_runs(&self.token, &rid_set)
            .await
            .map_err(Error::from)?;
        Ok(response
            .into_iter()
            .map(|(k, v)| (rid_to_string(&k), Run::from_conjure(v, &self.app_base_url)))
            .collect())
    }

    /// List runs, sorted by creation date descending.
    pub async fn list(&self) -> Result<Vec<Run>> {
        self.search(RunQuery::search_text("")).await
    }

    fn search_stream(&self, query: RunQuery) -> Result<impl Stream<Item = Result<Run>>> {
        let conjure_query = query.into_conjure()?;
        let service = self.service.clone();
        let token = self.token.clone();
        let app_base_url = self.app_base_url.clone();
        Ok(paginate_stream(
            move |page_token| {
                SearchRunsRequest::builder()
                    .sort(
                        SortOptions::builder()
                            .is_descending(true)
                            .sort_key(SortKey::Field(SortField::CreatedAt))
                            .build(),
                    )
                    .page_size(100)
                    .query(conjure_query.clone())
                    .next_page_token(page_token)
                    .build()
            },
            move |req| {
                let service = service.clone();
                let token = token.clone();
                async move { service.search_runs(&token, &req).await.map_err(Error::from) }
            },
            |resp: &SearchRunsResponse| resp.next_page_token().cloned(),
            move |resp| {
                resp.results()
                    .iter()
                    .map(|r| Run::from_conjure(r.clone(), &app_base_url))
                    .collect()
            },
        ))
    }

    /// Search runs with a query, collecting all pages eagerly.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::RunQuery;
    /// let runs = client.runs()
    ///     .search(RunQuery::and([
    ///         RunQuery::label("production"),
    ///         RunQuery::property("vehicle", "rocket"),
    ///     ]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn search(&self, query: RunQuery) -> Result<Vec<Run>> {
        let substrings = query.collect_substring_matches();
        let runs: Vec<Run> = self.search_stream(query)?.try_collect().await?;
        Ok(runs
            .into_iter()
            .filter(|r| crate::core::utils::name_matches_all(r.name(), &substrings))
            .collect())
    }

    /// Update run metadata. Returns the updated run.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal::core::RunUpdate;
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// let run = client.runs()
    ///     .update("ri.scout.cerulean-staging.run.<uuid>", RunUpdate::new().name("New Name").labels(["tag1", "tag2"]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update(&self, rid: &str, update: RunUpdate) -> Result<Run> {
        let request = update.into_request()?;
        let run_rid = parse_rid(rid)?;
        let response = self
            .service
            .update_run(&self.token, &run_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(Run::from_conjure(response, &self.app_base_url))
    }

    /// Attach data sources to a run under the given ref names.
    ///
    /// Ref names should be stable across runs of the same type, since
    /// checklists and templates use them to reference data sources.
    /// Returns the updated run.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal::core::NominalClient) -> nominal::Result<()> {
    /// use nominal::core::DataSource;
    /// client.runs().add_data_sources("ri.scout.cerulean-staging.run.<uuid>", [
    ///     ("flight-data", DataSource::dataset("ri.catalog.cerulean-staging.dataset.<uuid>")),
    ///     ("cockpit-cam", DataSource::video("ri.catalog.cerulean-staging.video.<uuid>")),
    /// ]).await?;
    /// # Ok(()) }
    /// ```
    pub async fn add_data_sources<I, N>(&self, rid: &str, sources: I) -> Result<Run>
    where
        I: IntoIterator<Item = (N, DataSource)>,
        N: Into<String>,
    {
        let data_sources = sources
            .into_iter()
            .map(|(ref_name, ds)| {
                ds.into_conjure().map(|conjure_ds| {
                    (
                        ref_name.into().into(),
                        CreateRunDataSource::builder()
                            .data_source(conjure_ds)
                            .build(),
                    )
                })
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        let run_rid = parse_rid(rid)?;
        let response = self
            .service
            .add_data_sources_to_run(&self.token, &run_rid, &data_sources)
            .await
            .map_err(Error::from)?;
        Ok(Run::from_conjure(response, &self.app_base_url))
    }

    /// Attach a dataset to a run under the given ref name. See [`add_data_sources`](Self::add_data_sources).
    pub async fn add_dataset(&self, rid: &str, ref_name: &str, dataset_rid: &str) -> Result<Run> {
        self.add_data_sources(rid, [(ref_name, DataSource::dataset(dataset_rid))])
            .await
    }

    /// Attach a video to a run under the given ref name. See [`add_data_sources`](Self::add_data_sources).
    pub async fn add_video(&self, rid: &str, ref_name: &str, video_rid: &str) -> Result<Run> {
        self.add_data_sources(rid, [(ref_name, DataSource::video(video_rid))])
            .await
    }

    /// Attach a connection to a run under the given ref name. See [`add_data_sources`](Self::add_data_sources).
    pub async fn add_connection(
        &self,
        rid: &str,
        ref_name: &str,
        connection_rid: &str,
    ) -> Result<Run> {
        self.add_data_sources(rid, [(ref_name, DataSource::connection(connection_rid))])
            .await
    }

    /// Add attachments (by RID) that have already been uploaded to a run.
    pub async fn add_attachments<I, S>(&self, rid: &str, attachment_rids: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let attachments_to_add = attachment_rids
            .into_iter()
            .map(|s| parse_rid(s.as_ref()).map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;

        let request = UpdateAttachmentsRequest::builder()
            .attachments_to_add(attachments_to_add)
            .attachments_to_remove(vec![])
            .build();

        let run_rid = parse_rid(rid)?;
        self.service
            .update_run_attachment(&self.token, &run_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Remove attachments from a run. Does not delete them from Nominal.
    pub async fn remove_attachments<I, S>(&self, rid: &str, attachment_rids: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let attachments_to_remove = attachment_rids
            .into_iter()
            .map(|s| parse_rid(s.as_ref()).map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;

        let request = UpdateAttachmentsRequest::builder()
            .attachments_to_add(vec![])
            .attachments_to_remove(attachments_to_remove)
            .build();

        let run_rid = parse_rid(rid)?;
        self.service
            .update_run_attachment(&self.token, &run_rid, &request)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Archive a run. Archived runs are hidden from the UI but not deleted.
    pub async fn archive(&self, rid: &str) -> Result<()> {
        let run_rid = parse_rid(rid)?;
        self.service
            .archive_run(&self.token, &run_rid, None)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Unarchive a run, restoring its visibility in the UI.
    pub async fn unarchive(&self, rid: &str) -> Result<()> {
        let run_rid = parse_rid(rid)?;
        self.service
            .unarchive_run(&self.token, &run_rid, None)
            .await
            .map_err(Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use nominal_api::objects::scout::run::api::SearchQuery;

    // --- RunQuery::into_conjure ---

    #[test]
    fn query_search_text() {
        let q = RunQuery::search_text("hello");
        assert_eq!(
            q.into_conjure().unwrap(),
            SearchQuery::SearchText("hello".into())
        );
    }

    #[test]
    fn query_substring_match() {
        let q = RunQuery::substring_match("exact");
        assert_eq!(
            q.into_conjure().unwrap(),
            SearchQuery::ExactMatch("exact".into())
        );
    }

    #[test]
    fn query_label() {
        let q = RunQuery::label("my-label");
        let SearchQuery::Labels(f) = q.into_conjure().unwrap() else {
            panic!("expected Labels variant");
        };
        assert_eq!(
            f.labels(),
            [nominal_api::objects::api::Label("my-label".into())]
        );
    }

    #[test]
    fn query_property() {
        let q = RunQuery::property("key", "val");
        let SearchQuery::Properties(f) = q.into_conjure().unwrap() else {
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
    fn query_run_number() {
        let q = RunQuery::run_number(42);
        let SearchQuery::RunNumber(n) = q.into_conjure().unwrap() else {
            panic!("expected RunNumber variant");
        };
        assert_eq!(i64::from(n), 42);
    }

    #[test]
    fn query_start_time_inclusive() {
        let dt = Utc.timestamp_opt(1_000_000, 0).single().unwrap();
        let q = RunQuery::start_time_inclusive(dt);
        let SearchQuery::StartTime(tf) = q.into_conjure().unwrap() else {
            panic!("expected StartTime variant");
        };
        use crate::core::datetime::api_timestamp_to_utc;
        use nominal_api::objects::scout::run::api::TimeframeFilter;
        let TimeframeFilter::Custom(inner) = *tf else {
            panic!("expected Custom timeframe");
        };
        let got = api_timestamp_to_utc(inner.start_time().unwrap()).unwrap();
        assert_eq!(got, dt);
    }

    #[test]
    fn query_end_time_inclusive() {
        let dt = Utc.timestamp_opt(2_000_000, 0).single().unwrap();
        let q = RunQuery::end_time_inclusive(dt);
        let SearchQuery::EndTime(tf) = q.into_conjure().unwrap() else {
            panic!("expected EndTime variant");
        };
        use crate::core::datetime::api_timestamp_to_utc;
        use nominal_api::objects::scout::run::api::TimeframeFilter;
        let TimeframeFilter::Custom(inner) = *tf else {
            panic!("expected Custom timeframe");
        };
        let got = api_timestamp_to_utc(inner.end_time().unwrap()).unwrap();
        assert_eq!(got, dt);
    }

    #[test]
    fn query_not() {
        let q = RunQuery::not(RunQuery::search_text("x"));
        let SearchQuery::Not(inner) = q.into_conjure().unwrap() else {
            panic!("expected Not variant");
        };
        assert_eq!(*inner, SearchQuery::SearchText("x".into()));
    }

    #[test]
    fn query_and_children() {
        let q = RunQuery::and([RunQuery::search_text("a"), RunQuery::search_text("b")]);
        let SearchQuery::And(children) = q.into_conjure().unwrap() else {
            panic!("expected And variant");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn query_or_children() {
        let q = RunQuery::or([RunQuery::label("x"), RunQuery::label("y")]);
        let SearchQuery::Or(children) = q.into_conjure().unwrap() else {
            panic!("expected Or variant");
        };
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn query_nested_and_or_not() {
        let q = RunQuery::and([
            RunQuery::label("prod"),
            RunQuery::not(RunQuery::or([
                RunQuery::property("env", "us"),
                RunQuery::property("env", "eu"),
            ])),
        ]);
        let SearchQuery::And(children) = q.into_conjure().unwrap() else {
            panic!("expected And");
        };
        assert!(matches!(children[0], SearchQuery::Labels(_)));
        assert!(matches!(children[1], SearchQuery::Not(_)));
    }

    // --- RunUpdate::into_request ---

    #[test]
    fn update_empty_request_has_no_optional_fields() {
        let req = RunUpdate::new().into_request().unwrap();
        assert!(req.title().is_none());
        assert!(req.description().is_none());
        assert!(req.properties().is_none());
        assert!(req.labels().is_none());
        assert!(req.start_time().is_none());
        assert!(req.end_time().is_none());
    }

    #[test]
    fn update_name_and_description() {
        let req = RunUpdate::new()
            .name("My Run")
            .description("desc")
            .into_request()
            .unwrap();
        assert_eq!(req.title(), Some("My Run"));
        assert_eq!(req.description(), Some("desc"));
    }

    #[test]
    fn update_properties() {
        let req = RunUpdate::new()
            .properties([("k", "v")])
            .into_request()
            .unwrap();
        let props = req.properties().expect("properties should be set");
        assert_eq!(props.len(), 1);
        assert_eq!(
            props.get(&nominal_api::objects::api::PropertyName("k".into())),
            Some(&nominal_api::objects::api::PropertyValue("v".into()))
        );
    }

    #[test]
    fn update_labels_deduplicated() {
        let req = RunUpdate::new()
            .labels(["a", "b", "a"])
            .into_request()
            .unwrap();
        let labels = req.labels().expect("labels should be set");
        assert_eq!(labels.len(), 2);
    }

    #[test]
    fn update_start_and_end_time_round_trip() {
        let start = Utc.timestamp_opt(1_000_000, 500_000_000).single().unwrap();
        let end = Utc.timestamp_opt(2_000_000, 0).single().unwrap();
        let req = RunUpdate::new()
            .start(start)
            .end(end)
            .into_request()
            .unwrap();

        use crate::core::datetime::api_timestamp_to_utc;
        let got_start = api_timestamp_to_utc(req.start_time().unwrap()).unwrap();
        let got_end = api_timestamp_to_utc(req.end_time().unwrap()).unwrap();
        assert_eq!(got_start, start);
        assert_eq!(got_end, end);
    }

    // --- RunCreate::into_request ---

    #[test]
    fn create_minimal_request() {
        let start = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
        let req = RunCreate::new("my-run", start).into_request(None).unwrap();
        assert_eq!(req.title(), "my-run");
        assert_eq!(req.description(), "");
        assert!(req.end_time().is_none());
        assert!(req.properties().is_empty());
        assert!(req.labels().is_empty());
        assert!(req.assets().is_empty());
        assert!(req.workspace().is_none());

        use crate::core::datetime::api_timestamp_to_utc;
        let got = api_timestamp_to_utc(req.start_time()).unwrap();
        assert_eq!(got, start);
    }

    #[test]
    fn create_full_request() {
        let start = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
        let end = Utc.timestamp_opt(1_700_003_600, 0).single().unwrap();
        let req = RunCreate::new("my-run", start)
            .description("desc")
            .end(end)
            .labels(["prod", "qa", "prod"])
            .properties([("k", "v")])
            .into_request(None)
            .unwrap();

        assert_eq!(req.title(), "my-run");
        assert_eq!(req.description(), "desc");
        // labels deduplicated by BTreeSet
        assert_eq!(req.labels().len(), 2);
        assert_eq!(req.properties().len(), 1);

        use crate::core::datetime::api_timestamp_to_utc;
        assert_eq!(api_timestamp_to_utc(req.start_time()).unwrap(), start);
        assert_eq!(
            api_timestamp_to_utc(req.end_time().unwrap()).unwrap(),
            end
        );
    }

    #[test]
    fn create_invalid_asset_rid_fails() {
        let start = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
        let result = RunCreate::new("my-run", start)
            .assets(["not-a-rid"])
            .into_request(None);
        assert!(result.is_err());
    }
}
