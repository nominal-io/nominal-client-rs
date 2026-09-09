use super::IngestClient;
use crate::core::{
    WaitOptions,
    catalog::{CatalogClient, DatasetFile},
    rid::parse_rid,
};
use crate::{Error, Result};
use conjure_http::client::AsyncService;
use nominal_api::clients::scout::catalog::{AsyncCatalogService, AsyncCatalogServiceClient};
impl IngestClient {
    /// Lists the files already produced by the job without waiting for it to complete.
    pub async fn dataset_files(&self, job_rid: &str) -> Result<Vec<DatasetFile>> {
        let service = AsyncCatalogServiceClient::new(self.conjure_client.clone(), &self.runtime);
        let job_rid = parse_rid(job_rid)?;
        let mut token = None;
        let mut files = Vec::new();
        loop {
            let page = service
                .get_dataset_files_for_job(&self.token, &job_rid, token.as_ref())
                .await?;
            files.extend(page.files().iter().cloned().map(DatasetFile::from_conjure));
            token = page.next_page().cloned();
            if token.is_none() {
                return Ok(files);
            }
        }
    }
    /// Waits for the job to complete, lists its files, then waits for those files.
    pub async fn wait_for_job_files(
        &self,
        rid: &str,
        options: WaitOptions,
    ) -> Result<Vec<DatasetFile>> {
        options.validate()?;
        let started = tokio::time::Instant::now();
        let operation = async {
            self.wait_for_ingest_job_with_interval(rid, options.poll_interval())
                .await?;
            self.dataset_files(rid).await
        };
        let files = if let Some(timeout) = options.timeout_duration() {
            tokio::time::timeout(timeout, operation)
                .await
                .map_err(|_| Error::Ingest {
                    details: format!(
                        "timed out waiting for job {rid} before its file snapshot was available"
                    ),
                })??
        } else {
            operation.await?
        };
        let file_options = if let Some(timeout) = options.timeout_duration() {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() && !files.is_empty() {
                return Err(Error::Ingest {
                    details: format!(
                        "timed out waiting for dataset files {}",
                        files
                            .iter()
                            .map(DatasetFile::rid)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }
            if files.is_empty() {
                return Ok(files);
            }
            options.timeout(remaining)
        } else {
            options
        };
        let catalog = CatalogClient::new(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            self.app_base_url.clone(),
        );
        catalog.wait_for_dataset_files(files, file_options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{HttpFixture, dataset_file, ingest_job};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::sync::Notify;

    #[tokio::test]
    async fn job_file_wait_spends_one_deadline_and_keeps_discovered_file_identity() {
        let id = "00000000-0000-0000-0000-000000000004";
        let job_requested = Arc::new(Notify::new());
        let release_job = Arc::new(Notify::new());
        let file_requested = Arc::new(Notify::new());
        let signals = (
            job_requested.clone(),
            release_job.clone(),
            file_requested.clone(),
        );
        let server = HttpFixture::new(move |request| {
            let (job_requested, release_job, file_requested) = signals.clone();
            async move {
                if request.uri().path().contains("/ingest/v1/") {
                    job_requested.notify_one();
                    release_job.notified().await;
                    ingest_job("ri.ingest.main.job.test")
                } else if request.uri().path().contains("/ingest-job/") {
                    serde_json::json!({"files":[dataset_file(id, "inProgress")]})
                } else {
                    assert!(request.uri().path().contains(id));
                    file_requested.notify_one();
                    std::future::pending().await
                }
            }
        })
        .await;
        let ingest = server.client.ingest();
        let operation = tokio::spawn(async move {
            ingest
                .wait_for_job_files(
                    "ri.ingest.main.job.test",
                    WaitOptions::default().timeout(Duration::from_secs(10)),
                )
                .await
        });
        server.run(job_requested.notified()).await;
        // Spend part of the deadline waiting for the job, without a wall-clock sleep.
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(4)).await;
        tokio::time::resume();
        release_job.notify_one();
        server.run(file_requested.notified()).await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(7)).await;
        let result = tokio::time::timeout(Duration::from_secs(1), operation).await;
        tokio::time::resume();
        let error = result
            .expect("file waiting restarted the deadline")
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains(id), "{error}");
        assert!(error.to_string().contains("timed out"), "{error}");
    }

    #[tokio::test]
    async fn job_files_pages_then_waits_independent_fixed_snapshot() {
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let replies = Arc::new(Mutex::new(vec![
            (
                "/ingest-job/",
                serde_json::json!({"files":[dataset_file(a,"inProgress")],"nextPage":"next"}),
            ),
            (
                "nextPageToken=next",
                serde_json::json!({"files":[dataset_file(b,"inProgress")]}),
            ),
            (a, dataset_file(a, "success")),
            (b, dataset_file(b, "inProgress")),
            (b, dataset_file(b, "success")),
        ]));
        let responses = replies.clone();
        let server = HttpFixture::new(move |request| {
            let mut responses = responses.lock().unwrap();
            let target = request.uri().to_string();
            let index = responses
                .iter()
                .position(|(expected, _)| target.contains(expected))
                .expect("unexpected catalog request");
            let (_, body) = responses.remove(index);
            async { body }
        })
        .await;
        let ingest = server.client.ingest();
        let files = server
            .run(ingest.dataset_files("ri.ingest.main.job.test"))
            .await
            .unwrap();
        let catalog = server.client.catalog();
        let complete = server
            .run(catalog.wait_for_dataset_files(
                files,
                WaitOptions::default().interval(Duration::from_millis(1)),
            ))
            .await
            .unwrap();
        assert!(complete.iter().all(|f| f.ingest_status().is_complete()));
        assert_eq!(
            complete.iter().map(DatasetFile::rid).collect::<Vec<_>>(),
            vec![a, b]
        );
        assert!(replies.lock().unwrap().is_empty());
    }
}
