use super::{IngestClient, IngestJob, IngestJobStatus};
use crate::Result;
use crate::core::rid::parse_rid;
use chrono::{DateTime, Utc};
use nominal_api::clients::ingest::api::AsyncIngestJobService;
use nominal_api::objects::ingest::api::{
    IngestJobSearchFilter as Filter, IngestJobStartTimeRange, SearchIngestJobsRequest,
};

/// Which workspace contributes search results.
#[derive(Debug, Clone, Default)]
pub enum WorkspaceSelection {
    #[default]
    Default,
    Specific(String),
    All,
}

/// Job filters are ANDed; repeated values within each field are ORed.
#[derive(Debug, Clone, Default)]
pub struct IngestJobQuery {
    datasets: Vec<String>,
    creators: Vec<String>,
    statuses: Vec<IngestJobStatus>,
    text: Option<String>,
    after: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
    workspace: WorkspaceSelection,
}
impl IngestJobQuery {
    pub fn dataset(mut self, rid: impl Into<String>) -> Self {
        self.datasets.push(rid.into());
        self
    }
    pub fn created_by(mut self, rid: impl Into<String>) -> Self {
        self.creators.push(rid.into());
        self
    }
    pub fn status(mut self, status: IngestJobStatus) -> Self {
        self.statuses.push(status);
        self
    }
    pub fn search_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
    /// Inclusive lower bound.
    pub fn start_after(mut self, time: DateTime<Utc>) -> Self {
        self.after = Some(time);
        self
    }
    /// Exclusive upper bound.
    pub fn start_before(mut self, time: DateTime<Utc>) -> Self {
        self.before = Some(time);
        self
    }
    pub fn workspace(mut self, workspace: WorkspaceSelection) -> Self {
        self.workspace = workspace;
        self
    }
    fn into_filter(self, default_workspace: Option<&str>) -> Result<Filter> {
        let mut filters = Vec::new();
        if !self.datasets.is_empty() {
            filters.push(Filter::DatasetRids(
                self.datasets
                    .iter()
                    .map(|s| parse_rid(s))
                    .collect::<std::result::Result<_, _>>()?,
            ));
        }
        if !self.creators.is_empty() {
            filters.push(Filter::CreatedByRids(
                self.creators
                    .iter()
                    .map(|s| parse_rid(s))
                    .collect::<std::result::Result<_, _>>()?,
            ));
        }
        if !self.statuses.is_empty() {
            use nominal_api::objects::ingest::api::IngestJobStatus as S;
            filters.push(Filter::Statuses(
                self.statuses
                    .into_iter()
                    .map(|s| match s {
                        IngestJobStatus::Submitted => S::Submitted,
                        IngestJobStatus::Queued => S::Queued,
                        IngestJobStatus::InProgress => S::InProgress,
                        IngestJobStatus::Completed => S::Completed,
                        IngestJobStatus::Failed => S::Failed,
                        IngestJobStatus::Cancelled => S::Cancelled,
                        IngestJobStatus::Unknown(s) => s.parse().unwrap_or_else(|_| unreachable!()),
                    })
                    .collect(),
            ));
        }
        if let Some(text) = self.text {
            filters.push(Filter::SearchText(text));
        }
        if self.after.is_some() || self.before.is_some() {
            filters.push(Filter::StartTimeRange(
                IngestJobStartTimeRange::builder()
                    .start_time_after(self.after)
                    .start_time_before(self.before)
                    .build(),
            ));
        }
        let workspace = match &self.workspace {
            WorkspaceSelection::Default => default_workspace,
            WorkspaceSelection::Specific(rid) => Some(rid.as_str()),
            WorkspaceSelection::All => None,
        };
        if let Some(rid) = workspace {
            filters.push(Filter::Workspace(parse_rid(rid)?));
        }
        Ok(Filter::And(filters))
    }
}
impl IngestClient {
    pub async fn search_ingest_jobs(&self, query: IngestJobQuery) -> Result<Vec<IngestJob>> {
        let default_workspace = if matches!(query.workspace, WorkspaceSelection::Default) {
            Some(self.resolved_workspace_rid().await?)
        } else {
            None
        };
        let filter = query.into_filter(default_workspace.as_ref().map(|rid| rid.0.as_str()))?;
        let mut token = None;
        let mut jobs = Vec::new();
        loop {
            let request = SearchIngestJobsRequest::builder()
                .filter(filter.clone())
                .next_page_token(token)
                .build();
            let page = self
                .ingest_job_service
                .search_ingest_jobs(&self.token, &request)
                .await?;
            jobs.extend(
                page.ingest_jobs()
                    .iter()
                    .cloned()
                    .map(|j| IngestJob::from_conjure(j).with_app_base_url(&self.app_base_url)),
            );
            token = page.next_page_token().cloned();
            if token.is_none() {
                break;
            }
        }
        Ok(jobs)
    }
    pub async fn cancel_ingest_job(&self, rid: &str) -> Result<IngestJob> {
        use conjure_http::client::AsyncService;
        use nominal_api::clients::ingest::api::AsyncIngestJobServiceClient;
        let service = AsyncIngestJobServiceClient::new(self.mutation_client.clone(), &self.runtime);
        let job = service
            .cancel_ingest_job(&self.token, &parse_rid(rid)?)
            .await?;
        Ok(IngestJob::from_conjure(job).with_app_base_url(&self.app_base_url))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_query_all_omits_workspace_and_preserves_bounds() {
        let start = DateTime::from_timestamp(100, 0).unwrap();
        let filter = IngestJobQuery::default()
            .workspace(WorkspaceSelection::All)
            .start_after(start)
            .start_before(start)
            .status(IngestJobStatus::Queued)
            .status(IngestJobStatus::Failed)
            .into_filter(Some("ri.workspace.main.workspace.test"))
            .unwrap();
        let value = serde_json::to_value(filter).unwrap();
        assert_eq!(value["and"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["and"][0]["statuses"],
            serde_json::json!(["QUEUED", "FAILED"])
        );
        assert_eq!(
            value["and"][1]["startTimeRange"]["startTimeAfter"],
            value["and"][1]["startTimeRange"]["startTimeBefore"]
        );
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    use std::io::{Read, Write};
    #[tokio::test]
    async fn job_search_returns_all_pages_without_extra_job_requests() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for page in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let body_start = loop {
                    let n = stream.read(&mut buffer).unwrap();
                    assert_ne!(n, 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break end + 4;
                        }
                    }
                };
                let request: serde_json::Value =
                    serde_json::from_slice(&bytes[body_start..]).unwrap();
                assert_eq!(
                    request["filter"]["and"][0]["statuses"],
                    serde_json::json!(["COMPLETED"])
                );
                assert_eq!(request["filter"]["and"].as_array().unwrap().len(), 1);
                if page == 0 {
                    assert!(request.get("nextPageToken").is_none());
                } else {
                    assert_eq!(request["nextPageToken"], "next");
                }
                let job = serde_json::json!({"ingestJobRid":format!("ri.ingest.main.job.page{page}"),"status":"COMPLETED","ingestType":"MULTI","createdBy":"00000000-0000-0000-0000-000000000000","orgUuid":"00000000-0000-0000-0000-000000000000"});
                let mut response = serde_json::json!({"ingestJobs":[job]});
                if page == 0 {
                    response["nextPageToken"] = "next".into();
                }
                let body = response.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let client = crate::core::NominalClient::builder("token")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        let jobs = client
            .ingest()
            .search_ingest_jobs(
                IngestJobQuery::default()
                    .workspace(WorkspaceSelection::All)
                    .status(IngestJobStatus::Completed),
            )
            .await
            .unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[1].rid(), "ri.ingest.main.job.page1");
        assert!(
            jobs[0]
                .nominal_url()
                .ends_with("/ingestion/ri.ingest.main.job.page0")
        );
        server.join().unwrap();
    }
}
