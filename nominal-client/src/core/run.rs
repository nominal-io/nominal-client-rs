use chrono::{DateTime, Utc};
use conjure_http::client::AsyncService;
use nominal_api::scout::RunServiceAsyncClient;
use nominal_api::scout::run::api::{
    CreateRunDataSource, DataSource, UpdateAttachmentsRequest, UpdateRunRequest,
};

use crate::core::{
    datetime::{NominalDateTime, api_timestamp_to_utc_or_panic},
    rid::{parse_rid, rid_to_string},
    utils::api_base_url_to_app_base_url,
};
use crate::{Error, Result};

use super::NominalClient;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default, Clone)]
pub struct RunUpdate {
    name: Option<String>,
    description: Option<String>,
    properties: Option<HashMap<String, String>>,
    labels: Option<Vec<String>>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
}

impl RunUpdate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, value: impl Into<String>) -> Self {
        self.name = Some(value.into());
        self
    }

    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    pub fn properties(mut self, value: HashMap<String, String>) -> Self {
        self.properties = Some(value);
        self
    }

    pub fn labels(mut self, value: Vec<String>) -> Self {
        self.labels = Some(value);
        self
    }

    pub fn start(mut self, value: DateTime<Utc>) -> Self {
        self.start = Some(value);
        self
    }

    pub fn end(mut self, value: DateTime<Utc>) -> Self {
        self.end = Some(value);
        self
    }

    pub(crate) fn into_request(self) -> Result<nominal_api::scout::run::api::UpdateRunRequest> {
        let RunUpdate {
            name,
            description,
            properties,
            labels,
            start,
            end,
        } = self;

        let mut request_builder = UpdateRunRequest::builder();

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
        if let Some(s) = start {
            let s_ts = NominalDateTime::try_from(s)?.into();
            request_builder = request_builder.start_time(Some(s_ts));
        }
        if let Some(e) = end {
            let e_ts = NominalDateTime::try_from(e)?.into();
            request_builder = request_builder.end_time(Some(e_ts));
        }

        Ok(request_builder.assets(vec![]).build())
    }
}

/// Represents a run in Nominal.
///
/// Runs are executions of tests, simulations, or analyses within an asset.
/// They contain datasets, events, and other time-series data.
#[derive(Clone)]
pub struct Run {
    /// The resource identifier (RID) for this run
    rid: String,

    /// The display name of the run
    name: String,

    /// Description of the run
    description: String,

    /// Key-value properties for custom metadata
    properties: HashMap<String, String>,

    /// Labels for categorizing and filtering runs
    labels: Vec<String>,

    /// Start timestamp
    start: DateTime<Utc>,

    /// End timestamp
    end: Option<DateTime<Utc>>,

    /// Run number (display identifier)
    run_number: i64,

    /// Asset RIDs associated with this run
    assets: Vec<String>,

    /// Creation timestamp in nanoseconds since Unix epoch
    created_at: DateTime<Utc>,

    /// Reference to the client for API calls
    client: NominalClient,
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

    pub fn run_number(&self) -> i64 {
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
        let app_base_url = api_base_url_to_app_base_url(self.client.base_url());
        format!("{}/runs/{}", app_base_url, self.run_number)
    }

    /// Update run metadata.
    ///
    /// Only the metadata passed in will be replaced, the rest will remain untouched.
    ///
    /// # Example
    /// ```no_run
    /// # use nominal_client::RunUpdate;
    /// # async fn example(mut run: nominal_client::Run) -> nominal_client::Result<()> {
    /// run.update(
    ///     RunUpdate::default()
    ///         .name("New Name")
    ///         .description("New description")
    ///         .labels(vec!["label1".to_string(), "label2".to_string()]),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update(&mut self, update: RunUpdate) -> Result<()> {
        let request = update.into_request()?;
        let service = RunServiceAsyncClient::new(self.client.service_client());

        let rid = parse_rid(&self.rid)?;

        let response = service
            .update_run(self.client.bearer_token(), &rid, &request)
            .await
            .map_err(Error::from)?;

        // Update self with the response
        *self = Self::from_conjure(&self.client, response);

        Ok(())
    }

    /// Add a dataset to this run.
    ///
    /// Datasets map "ref names" (their name within the run) to a Dataset (or dataset rid).
    /// The same type of datasets should use the same ref name across runs, since checklists
    /// and templates use ref names to reference datasets.
    ///
    /// # Arguments
    /// * `ref_name` - Logical name for the data scope within the run
    /// * `dataset_rid` - Dataset RID to add to the run
    pub async fn add_dataset(&self, ref_name: &str, dataset_rid: &str) -> Result<()> {
        self.add_datasets(HashMap::from([(
            ref_name.to_string(),
            dataset_rid.to_string(),
        )]))
        .await
    }

    /// Add multiple datasets to this run.
    ///
    /// # Arguments
    /// * `datasets` - Mapping of logical names to dataset RIDs to add to the run
    pub async fn add_datasets(&self, datasets: HashMap<String, String>) -> Result<()> {
        let service = RunServiceAsyncClient::new(self.client.service_client());

        let data_sources = datasets
            .into_iter()
            .map(|(ref_name, dataset_rid)| {
                parse_rid(&dataset_rid)
                    .map(|rid| {
                        (
                            ref_name.into(),
                            CreateRunDataSource::builder()
                                .data_source(DataSource::Dataset(rid))
                                .build(),
                        )
                    })
                    .map_err(Error::from)
            })
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;

        let rid = parse_rid(&self.rid)?;

        service
            .add_data_sources_to_run(self.client.bearer_token(), &rid, &data_sources)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Add a video to this run.
    ///
    /// # Arguments
    /// * `ref_name` - Logical name for the video within the run
    /// * `video_rid` - Video RID to add to the run
    pub async fn add_video(&self, ref_name: &str, video_rid: &str) -> Result<()> {
        let service = RunServiceAsyncClient::new(self.client.service_client());

        let rid = parse_rid(video_rid)?;
        let data_sources = BTreeMap::from([(
            ref_name.to_string().into(),
            CreateRunDataSource::builder()
                .data_source(DataSource::Video(rid))
                .build(),
        )]);

        let rid = parse_rid(&self.rid)?;

        service
            .add_data_sources_to_run(self.client.bearer_token(), &rid, &data_sources)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Add attachments that have already been uploaded to this run.
    ///
    /// # Arguments
    /// * `attachment_rids` - List of attachment RIDs to add
    pub async fn add_attachments(&self, attachment_rids: Vec<String>) -> Result<()> {
        let service = RunServiceAsyncClient::new(self.client.service_client());

        let attachments_to_add = attachment_rids
            .into_iter()
            .map(|rid| parse_rid(&rid).map_err(Error::from))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let request = UpdateAttachmentsRequest::builder()
            .attachments_to_add(attachments_to_add)
            .attachments_to_remove(vec![])
            .build();

        let rid = parse_rid(&self.rid)?;

        service
            .update_run_attachment(self.client.bearer_token(), &rid, &request)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Remove attachments from this run.
    /// Does not remove the attachments from Nominal.
    ///
    /// # Arguments
    /// * `attachment_rids` - List of attachment RIDs to remove
    pub async fn remove_attachments(&self, attachment_rids: Vec<String>) -> Result<()> {
        let service = RunServiceAsyncClient::new(self.client.service_client());

        let attachments_to_remove = attachment_rids
            .into_iter()
            .map(|rid| parse_rid(&rid).map_err(Error::from))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let request = UpdateAttachmentsRequest::builder()
            .attachments_to_add(vec![])
            .attachments_to_remove(attachments_to_remove)
            .build();

        let rid = parse_rid(&self.rid)?;

        service
            .update_run_attachment(self.client.bearer_token(), &rid, &request)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Archive this run.
    ///
    /// Archived runs are not deleted, but are hidden from the UI.
    /// NOTE: currently, it is not possible (yet) to unarchive a run once archived.
    pub async fn archive(&self) -> Result<()> {
        let service = RunServiceAsyncClient::new(self.client.service_client());

        let rid = parse_rid(&self.rid)?;

        service
            .archive_run(self.client.bearer_token(), &rid, None)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    /// Internal method to construct a Run from the Conjure API type.
    ///
    /// Panics if any timestamp conversion fails, which indicates corrupted API data.
    pub(crate) fn from_conjure(
        client: &NominalClient,
        run: nominal_api::scout::run::api::Run,
    ) -> Self {
        let start = api_timestamp_to_utc_or_panic(run.start_time());
        let end = run.end_time().map(api_timestamp_to_utc_or_panic);

        let properties = run
            .properties()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let labels = run.labels().iter().map(|l| l.to_string()).collect();

        let assets = run.assets().iter().map(rid_to_string).collect();

        Self {
            rid: rid_to_string(run.rid()),
            name: run.title().to_string(),
            description: run.description().to_string(),
            properties,
            labels,
            start,
            end,
            run_number: *run.run_number(),
            assets,
            created_at: run.created_at().to_utc(),
            client: client.clone(),
        }
    }
}
