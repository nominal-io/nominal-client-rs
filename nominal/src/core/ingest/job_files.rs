use super::{IngestClient, IngestJobStatus};
use crate::core::{
    WaitOptions,
    catalog::{CatalogClient, DatasetFile},
    rid::parse_rid,
};
use crate::{Error, Result};
use conjure_http::client::AsyncService;
use nominal_api::clients::scout::catalog::{AsyncCatalogService, AsyncCatalogServiceClient};
impl IngestClient {
    /// Snapshot the files already produced by this job without waiting for completion.
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
    /// Wait for completion before discovering output files, then wait on that fixed snapshot.
    pub async fn wait_for_job_files(
        &self,
        rid: &str,
        options: WaitOptions,
    ) -> Result<Vec<DatasetFile>> {
        options.validate()?;
        let operation = async {
            loop {
                let job = self.get_ingest_job(rid).await?;
                match job.status() {
                    IngestJobStatus::Completed => break,
                    status if status.is_terminal() => {
                        return Err(Error::Ingest {
                            details: format!("ingest job {rid} ended with {status:?}"),
                        });
                    }
                    _ => tokio::time::sleep(options.poll_interval()).await,
                }
            }
            let files = self.dataset_files(rid).await?;
            let catalog = CatalogClient::new(
                self.conjure_client.clone(),
                &self.runtime,
                self.token.clone(),
                self.workspace_rid.clone(),
                self.app_base_url.clone(),
            );
            catalog.wait_for_dataset_files(files, options.clone()).await
        };
        if let Some(timeout) = options.timeout_duration() {
            tokio::time::timeout(timeout, operation)
                .await
                .map_err(|_| Error::Ingest {
                    details: format!("timed out waiting for job {rid} and its files"),
                })?
        } else {
            operation.await
        }
    }
}
