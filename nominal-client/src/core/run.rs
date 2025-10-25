use crate::core::utils::api_base_url_to_app_base_url;

use super::NominalClient;
use std::collections::HashMap;

/// Represents a run in Nominal.
///
/// Runs are executions of tests, simulations, or analyses within an asset.
/// They contain datasets, events, and other time-series data.
#[derive(Clone)]
pub struct Run {
    /// The resource identifier (RID) for this run
    pub rid: String,

    /// The display name of the run
    pub name: String,

    /// Description of the run
    pub description: String,

    /// Key-value properties for custom metadata
    pub properties: HashMap<String, String>,

    /// Labels for categorizing and filtering runs
    pub labels: Vec<String>,

    /// Start timestamp in nanoseconds since Unix epoch
    pub start: i64,

    /// End timestamp in nanoseconds since Unix epoch (optional)
    pub end: Option<i64>,

    /// Run number (display identifier)
    pub run_number: i64,

    /// Asset RIDs associated with this run
    pub assets: Vec<String>,

    /// Creation timestamp in nanoseconds since Unix epoch
    pub created_at: i64,

    /// Reference to the client for API calls
    client: NominalClient,
}

impl Run {
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
    /// # async fn example(mut run: nominal_client::Run) -> Result<(), Box<dyn std::error::Error>> {
    /// run.update(
    ///     Some("New Name".to_string()),
    ///     Some("New description".to_string()),
    ///     None,  // properties unchanged
    ///     Some(vec!["label1".to_string(), "label2".to_string()]),
    ///     None,  // start unchanged
    ///     None,  // end unchanged
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update(
        &mut self,
        name: Option<String>,
        description: Option<String>,
        properties: Option<HashMap<String, String>>,
        labels: Option<Vec<String>>,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;
        use nominal_api::scout::run::api::{UpdateRunRequest, UtcTimestamp};
        use std::collections::BTreeMap;

        let mut request_builder = UpdateRunRequest::builder();

        if let Some(n) = name {
            request_builder = request_builder.title(n);
        }
        if let Some(d) = description {
            request_builder = request_builder.description(d);
        }
        if let Some(p) = properties {
            // Convert HashMap to the API's expected types
            let props: BTreeMap<_, _> = p.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
            request_builder = request_builder.properties(props);
        }
        if let Some(l) = labels {
            // Convert Vec<String> to BTreeSet<Label>
            let labels_set: std::collections::BTreeSet<_> =
                l.into_iter().map(|s| s.into()).collect();
            request_builder = request_builder.labels(labels_set);
        }
        if let Some(s) = start {
            // Convert nanoseconds to UtcTimestamp (seconds + offset nanos)
            use conjure_object::SafeLong;
            let seconds = s / 1_000_000_000;
            let nanos = s % 1_000_000_000;
            request_builder = request_builder.start_time(
                UtcTimestamp::builder()
                    .seconds_since_epoch(SafeLong::new(seconds).unwrap())
                    .offset_nanoseconds(SafeLong::new(nanos).unwrap())
                    .build(),
            );
        }
        if let Some(e) = end {
            // Convert nanoseconds to UtcTimestamp (seconds + offset nanos)
            use conjure_object::SafeLong;
            let seconds = e / 1_000_000_000;
            let nanos = e % 1_000_000_000;
            request_builder = request_builder.end_time(
                UtcTimestamp::builder()
                    .seconds_since_epoch(SafeLong::new(seconds).unwrap())
                    .offset_nanoseconds(SafeLong::new(nanos).unwrap())
                    .build(),
            );
        }

        let request = request_builder.assets(vec![]).build();
        let service = RunServiceAsyncClient::new(self.client.client.clone());

        // Convert RID string to RunRid
        let resource_id =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid: nominal_api::scout::run::api::RunRid = resource_id.into();

        let response = service
            .update_run(&self.client.token, &run_rid, &request)
            .await
            .map_err(|e| format!("Failed to update run: {:?}", e))?;

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
    pub async fn add_dataset(
        &self,
        ref_name: &str,
        dataset_rid: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut datasets = HashMap::new();
        datasets.insert(ref_name.to_string(), dataset_rid.to_string());
        self.add_datasets(datasets).await
    }

    /// Add multiple datasets to this run.
    ///
    /// # Arguments
    /// * `datasets` - Mapping of logical names to dataset RIDs to add to the run
    pub async fn add_datasets(
        &self,
        datasets: HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;
        use nominal_api::scout::run::api::{CreateRunDataSource, DataSource};
        use std::collections::BTreeMap;

        let service = RunServiceAsyncClient::new(self.client.client.clone());

        // Convert datasets to data sources
        let mut data_sources = BTreeMap::new();
        for (ref_name, dataset_rid) in datasets {
            let ds_rid = ResourceIdentifier::new(&dataset_rid)
                .map_err(|e| format!("Invalid dataset RID: {:?}", e))?;

            let dataset_rid_typed: nominal_api::api::rids::DatasetRid = ds_rid.into();
            data_sources.insert(
                ref_name.into(),
                CreateRunDataSource::builder()
                    .data_source(DataSource::Dataset(dataset_rid_typed))
                    .build(),
            );
        }

        let run_rid =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid_typed: nominal_api::scout::run::api::RunRid = run_rid.into();

        service
            .add_data_sources_to_run(&self.client.token, &run_rid_typed, &data_sources)
            .await
            .map_err(|e| format!("Failed to add datasets to run: {:?}", e))?;

        Ok(())
    }

    /// Add a video to this run.
    ///
    /// # Arguments
    /// * `ref_name` - Logical name for the video within the run
    /// * `video_rid` - Video RID to add to the run
    pub async fn add_video(
        &self,
        ref_name: &str,
        video_rid: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;
        use nominal_api::scout::run::api::{CreateRunDataSource, DataSource};
        use std::collections::BTreeMap;

        let service = RunServiceAsyncClient::new(self.client.client.clone());

        let vid_rid = ResourceIdentifier::new(video_rid)
            .map_err(|e| format!("Invalid video RID: {:?}", e))?;

        let video_rid_typed: nominal_api::api::rids::VideoRid = vid_rid.into();
        let mut data_sources = BTreeMap::new();
        data_sources.insert(
            ref_name.to_string().into(),
            CreateRunDataSource::builder()
                .data_source(DataSource::Video(video_rid_typed))
                .build(),
        );

        let run_rid =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid_typed: nominal_api::scout::run::api::RunRid = run_rid.into();

        service
            .add_data_sources_to_run(&self.client.token, &run_rid_typed, &data_sources)
            .await
            .map_err(|e| format!("Failed to add video to run: {:?}", e))?;

        Ok(())
    }

    /// Add a connection to this run.
    ///
    /// Ref_name maps "ref name" (the name within the run) to a Connection.
    /// The same type of connection should use the same ref name across runs.
    ///
    /// # Arguments
    /// * `ref_name` - Logical name for the connection within the run
    /// * `connection_rid` - Connection RID to add to the run
    pub async fn add_connection(
        &self,
        ref_name: &str,
        connection_rid: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;
        use nominal_api::scout::run::api::{CreateRunDataSource, DataSource};
        use std::collections::BTreeMap;

        let service = RunServiceAsyncClient::new(self.client.client.clone());

        let conn_rid = ResourceIdentifier::new(connection_rid)
            .map_err(|e| format!("Invalid connection RID: {:?}", e))?;

        let connection_rid_typed: nominal_api::scout::run::api::ConnectionRid = conn_rid.into();
        let mut data_sources = BTreeMap::new();
        data_sources.insert(
            ref_name.to_string().into(),
            CreateRunDataSource::builder()
                .data_source(DataSource::Connection(connection_rid_typed))
                .build(),
        );

        let run_rid =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid_typed: nominal_api::scout::run::api::RunRid = run_rid.into();

        service
            .add_data_sources_to_run(&self.client.token, &run_rid_typed, &data_sources)
            .await
            .map_err(|e| format!("Failed to add connection to run: {:?}", e))?;

        Ok(())
    }

    /// Add attachments that have already been uploaded to this run.
    ///
    /// # Arguments
    /// * `attachment_rids` - List of attachment RIDs to add
    pub async fn add_attachments(
        &self,
        attachment_rids: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;
        use nominal_api::scout::run::api::UpdateAttachmentsRequest;

        let service = RunServiceAsyncClient::new(self.client.client.clone());

        // Convert string RIDs to AttachmentRid
        let mut attachments_to_add = Vec::new();
        for rid_str in attachment_rids {
            let rid = ResourceIdentifier::new(&rid_str)
                .map_err(|e| format!("Invalid attachment RID: {:?}", e))?;
            let attachment_rid: nominal_api::api::rids::AttachmentRid = rid.into();
            attachments_to_add.push(attachment_rid);
        }

        let request = UpdateAttachmentsRequest::builder()
            .attachments_to_add(attachments_to_add)
            .attachments_to_remove(vec![])
            .build();

        let run_rid =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid_typed: nominal_api::scout::run::api::RunRid = run_rid.into();

        service
            .update_run_attachment(&self.client.token, &run_rid_typed, &request)
            .await
            .map_err(|e| format!("Failed to add attachments to run: {:?}", e))?;

        Ok(())
    }

    /// Remove attachments from this run.
    /// Does not remove the attachments from Nominal.
    ///
    /// # Arguments
    /// * `attachment_rids` - List of attachment RIDs to remove
    pub async fn remove_attachments(
        &self,
        attachment_rids: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;
        use nominal_api::scout::run::api::UpdateAttachmentsRequest;

        let service = RunServiceAsyncClient::new(self.client.client.clone());

        // Convert string RIDs to AttachmentRid
        let mut attachments_to_remove = Vec::new();
        for rid_str in attachment_rids {
            let rid = ResourceIdentifier::new(&rid_str)
                .map_err(|e| format!("Invalid attachment RID: {:?}", e))?;
            let attachment_rid: nominal_api::api::rids::AttachmentRid = rid.into();
            attachments_to_remove.push(attachment_rid);
        }

        let request = UpdateAttachmentsRequest::builder()
            .attachments_to_add(vec![])
            .attachments_to_remove(attachments_to_remove)
            .build();

        let run_rid =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid_typed: nominal_api::scout::run::api::RunRid = run_rid.into();

        service
            .update_run_attachment(&self.client.token, &run_rid_typed, &request)
            .await
            .map_err(|e| format!("Failed to remove attachments from run: {:?}", e))?;

        Ok(())
    }

    /// Archive this run.
    ///
    /// Archived runs are not deleted, but are hidden from the UI.
    /// NOTE: currently, it is not possible (yet) to unarchive a run once archived.
    pub async fn archive(&self) -> Result<(), Box<dyn std::error::Error>> {
        use conjure_http::client::AsyncService;
        use conjure_object::ResourceIdentifier;
        use nominal_api::scout::RunServiceAsyncClient;

        let service = RunServiceAsyncClient::new(self.client.client.clone());

        // Convert RID string to RunRid
        let resource_id =
            ResourceIdentifier::new(&self.rid).map_err(|e| format!("Invalid RID: {:?}", e))?;
        let run_rid: nominal_api::scout::run::api::RunRid = resource_id.into();

        service
            .archive_run(&self.client.token, &run_rid, None)
            .await
            .map_err(|e| format!("Failed to archive run: {:?}", e))?;

        Ok(())
    }

    /// Internal method to construct a Run from the Conjure API type.
    pub(crate) fn from_conjure(
        client: &NominalClient,
        run: nominal_api::scout::run::api::Run,
    ) -> Self {
        // Convert created_at from DateTime to nanoseconds
        let created_at_nanos = run.created_at().timestamp_nanos_opt().unwrap_or(0);

        // Convert start time (seconds + offset nanos to total nanos)
        let start_seconds = *run.start_time().seconds_since_epoch() * 1_000_000_000;
        let start_nanos = run
            .start_time()
            .offset_nanoseconds()
            .map(|n| *n)
            .unwrap_or(0);
        let start = start_seconds + start_nanos;

        // Convert end time if present
        let end = run.end_time().map(|et| {
            let end_seconds = *et.seconds_since_epoch() * 1_000_000_000;
            let end_nanos = et.offset_nanoseconds().map(|n| *n).unwrap_or(0);
            end_seconds + end_nanos
        });

        // Convert properties from BTreeMap to HashMap
        let properties: HashMap<String, String> = run
            .properties()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Convert labels from Vec to Vec
        let labels: Vec<String> = run.labels().iter().map(|l| l.to_string()).collect();

        // Convert assets from Vec to Vec
        let assets: Vec<String> = run.assets().iter().map(|a| a.to_string()).collect();

        Self {
            rid: run.rid().to_string(),
            name: run.title().to_string(),
            description: run.description().to_string(),
            properties,
            labels,
            start,
            end,
            run_number: *run.run_number(),
            assets,
            created_at: created_at_nanos,
            client: client.clone(),
        }
    }
}
