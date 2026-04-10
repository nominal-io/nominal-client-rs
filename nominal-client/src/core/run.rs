use chrono::{DateTime, Utc};
use conjure_http::client::AsyncService;
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::scout::RunServiceAsyncClient;
use nominal_api::scout::run::api::{
    CreateRunDataSource, DataSource, SearchQuery, SearchRunsRequest, SortField, SortKey,
    SortOptions, UpdateAttachmentsRequest, UpdateRunRequest,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::core::{
    datetime::{NominalDateTime, api_timestamp_to_utc_or_panic},
    rid::{parse_rid, rid_to_string},
};
use crate::{Error, Result};

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

    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Get the URL to view this run in the Nominal web app.
    pub fn nominal_url(&self) -> String {
        format!("{}/runs/{}", self.app_base_url, self.run_number)
    }

    pub(crate) fn from_conjure(run: nominal_api::scout::run::api::Run, app_base_url: &str) -> Self {
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

/// Client for run collection operations (list, get).
pub struct RunsClient {
    service: RunServiceAsyncClient<Client>,
    token: BearerToken,
    app_base_url: String,
}

impl RunsClient {
    pub(crate) fn new(client: Client, token: BearerToken, app_base_url: String) -> Self {
        Self {
            service: RunServiceAsyncClient::new(client),
            token,
            app_base_url,
        }
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
        let request = SearchRunsRequest::new(
            SortOptions::builder()
                .is_descending(true)
                .sort_key(SortKey::Field(SortField::CreatedAt))
                .build(),
            100, // page_size
            SearchQuery::SearchText("".to_string()),
        );
        let response = self
            .service
            .search_runs(&self.token, &request)
            .await
            .map_err(Error::from)?;
        Ok(response
            .results()
            .iter()
            .map(|r| Run::from_conjure(r.clone(), &self.app_base_url))
            .collect())
    }

    /// Update run metadata. Returns the updated run.
    ///
    /// Only fields set on the update will be changed; the rest remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal_client::RunUpdate;
    /// # async fn example(client: nominal_client::NominalClient) -> nominal_client::Result<()> {
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

    /// Add datasets to a run.
    ///
    /// Datasets map "ref names" (their logical name within the run) to a dataset RID.
    /// The same type of dataset should use the same ref name across runs, since checklists
    /// and templates use ref names to reference datasets.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(client: nominal_client::NominalClient) -> nominal_client::Result<()> {
    /// client.runs()
    ///     .add_datasets("ri.scout.cerulean-staging.run.<uuid>", [("flight-data", "ri.catalog.cerulean-staging.dataset.<uuid>")])
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn add_datasets<I, K, V>(&self, rid: &str, datasets: I) -> Result<()>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let data_sources = datasets
            .into_iter()
            .map(|(ref_name, dataset_rid)| {
                let dataset_rid = dataset_rid.into();
                parse_rid(&dataset_rid)
                    .map(|parsed| {
                        (
                            ref_name.into().into(),
                            CreateRunDataSource::builder()
                                .data_source(DataSource::Dataset(parsed))
                                .build(),
                        )
                    })
                    .map_err(Error::from)
            })
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;

        let run_rid = parse_rid(rid)?;
        self.service
            .add_data_sources_to_run(&self.token, &run_rid, &data_sources)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    /// Add a video to a run.
    pub async fn add_video(&self, rid: &str, ref_name: &str, video_rid: &str) -> Result<()> {
        let vid_rid = parse_rid(video_rid)?;
        let data_sources = BTreeMap::from([(
            ref_name.to_string().into(),
            CreateRunDataSource::builder()
                .data_source(DataSource::Video(vid_rid))
                .build(),
        )]);
        let run_rid = parse_rid(rid)?;
        self.service
            .add_data_sources_to_run(&self.token, &run_rid, &data_sources)
            .await
            .map_err(Error::from)?;
        Ok(())
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
    ///
    /// Note: runs cannot currently be unarchived once archived.
    pub async fn archive(&self, rid: &str) -> Result<()> {
        let run_rid = parse_rid(rid)?;
        self.service
            .archive_run(&self.token, &run_rid, None)
            .await
            .map_err(Error::from)?;
        Ok(())
    }
}
