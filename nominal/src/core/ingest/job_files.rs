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
        let started = tokio::time::Instant::now();
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
    use std::io::{Read, Write};
    fn file(id: &str, status: &str) -> serde_json::Value {
        serde_json::json!({"id":id,"datasetRid":"ri.catalog.main.dataset.test","name":"output",
            "handle":{"type":"future","future":{}},"uploadedAt":"2026-01-01T00:00:00Z",
            "ingestStatus":{"type":status,status:{}}})
    }
    #[tokio::test]
    async fn job_file_wait_spends_one_deadline_and_keeps_discovered_file_identity() {
        let id = "00000000-0000-0000-0000-000000000004";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 2048];
                let n = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..n]);
                if step == 0 {
                    assert!(request.contains("/ingest/v1/ingest-job/"));
                } else if step == 1 {
                    assert!(request.contains("/catalog/v1/ingest-job/"));
                } else {
                    assert!(request.contains(id));
                }
                if step == 2 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    break;
                }
                let body = if step == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    serde_json::json!({"ingestJobRid":"ri.ingest.main.job.test","status":"COMPLETED","ingestType":"MULTI","createdBy":"00000000-0000-0000-0000-000000000000","orgUuid":"00000000-0000-0000-0000-000000000000"})
                } else { serde_json::json!({"files":[file(id,"inProgress")]}) }.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let client = crate::core::NominalClient::builder("token")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let error = client
            .ingest()
            .wait_for_job_files(
                "ri.ingest.main.job.test",
                WaitOptions::default().timeout(std::time::Duration::from_millis(150)),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains(id));
        assert!(
            started.elapsed() < std::time::Duration::from_millis(225),
            "file waiting restarted the timeout"
        );
        server.join().unwrap();
    }
    #[tokio::test]
    async fn dataset_file_rpc_timeout_reports_the_file_identity() {
        let id = "00000000-0000-0000-0000-000000000003";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
            drop(stream);
        });
        let client = crate::core::NominalClient::builder("token")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        let file =
            DatasetFile::from_conjure(serde_json::from_value(file(id, "inProgress")).unwrap());
        let result = client
            .catalog()
            .wait_for_dataset_files(
                vec![file],
                WaitOptions::default().timeout(std::time::Duration::from_millis(10)),
            )
            .await;
        assert!(result.unwrap_err().to_string().contains(id));
        server.join().unwrap();
    }
    #[tokio::test]
    async fn job_files_pages_then_waits_independent_fixed_snapshot() {
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let responses = vec![
            (
                "/ingest-job/",
                serde_json::json!({"files":[file(a,"inProgress")],"nextPage":"next"}),
            ),
            (
                "nextPageToken=next",
                serde_json::json!({"files":[file(b,"inProgress")]}),
            ),
            (a, file(a, "success")),
            (b, file(b, "inProgress")),
            (b, file(b, "success")),
        ];
        let server = std::thread::spawn(move || {
            for (expected, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 1024];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buffer).unwrap();
                    assert_ne!(n, 0);
                    bytes.extend_from_slice(&buffer[..n]);
                }
                assert!(
                    String::from_utf8_lossy(&bytes)
                        .lines()
                        .next()
                        .unwrap()
                        .contains(expected)
                );
                let body = body.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let client = crate::core::NominalClient::builder("token")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        let files = client
            .ingest()
            .dataset_files("ri.ingest.main.job.test")
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        let complete = client
            .catalog()
            .wait_for_dataset_files(
                files,
                WaitOptions::default().interval(std::time::Duration::from_millis(1)),
            )
            .await
            .unwrap();
        assert!(complete.iter().all(|f| f.ingest_status().is_complete()));
        server.join().unwrap();
    }
}
