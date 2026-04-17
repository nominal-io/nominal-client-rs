use std::time::Duration;

use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::ingest::api::{
    IngestJobRid, IngestJobServiceAsyncClient, IngestJobStatus as ApiIngestJobStatus,
};

use crate::core::rid::parse_rid;
use crate::{Error, Result};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The lifecycle status of an ingest job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestJobStatus {
    Submitted,
    Queued,
    InProgress,
    Completed,
    Failed,
    Cancelled,
    /// A status not known to this client. Treated as non-terminal by
    /// [`IngestJobHandle::wait`] so that callers are not accidentally hung.
    Other(String),
}

impl IngestJobStatus {
    /// Whether the job has reached a terminal state (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled
        )
    }
}

impl From<&ApiIngestJobStatus> for IngestJobStatus {
    fn from(s: &ApiIngestJobStatus) -> Self {
        match s {
            ApiIngestJobStatus::Submitted => Self::Submitted,
            ApiIngestJobStatus::Queued => Self::Queued,
            ApiIngestJobStatus::InProgress => Self::InProgress,
            ApiIngestJobStatus::Completed => Self::Completed,
            ApiIngestJobStatus::Failed => Self::Failed,
            ApiIngestJobStatus::Cancelled => Self::Cancelled,
            ApiIngestJobStatus::Unknown(u) => Self::Other(u.to_string()),
        }
    }
}

/// A handle to an in-flight ingest job. Polling methods hit the server
/// each time they are called.
#[derive(Clone)]
pub struct IngestJobHandle {
    rid: String,
    service: IngestJobServiceAsyncClient<Client>,
    token: BearerToken,
}

impl IngestJobHandle {
    pub(crate) fn new(
        rid: String,
        service: IngestJobServiceAsyncClient<Client>,
        token: BearerToken,
    ) -> Self {
        Self { rid, service, token }
    }

    /// The RID of the ingest job.
    pub fn rid(&self) -> &str {
        &self.rid
    }

    /// Fetch the current status of the job.
    pub async fn status(&self) -> Result<IngestJobStatus> {
        let job_rid: IngestJobRid = parse_rid(&self.rid)?;
        let job = self
            .service
            .get_ingest_job(&self.token, &job_rid)
            .await
            .map_err(Error::from)?;
        Ok(IngestJobStatus::from(job.status()))
    }

    /// Poll until the job reaches a terminal state, returning the terminal
    /// status. Polls every 2 seconds; use [`Self::wait_with_interval`] for a
    /// custom cadence.
    ///
    /// Returns `Err(Error::Ingest { .. })` on `Failed` or `Cancelled`.
    pub async fn wait(&self) -> Result<IngestJobStatus> {
        self.wait_with_interval(DEFAULT_POLL_INTERVAL).await
    }

    /// Like [`Self::wait`] but polls on the given interval.
    pub async fn wait_with_interval(&self, interval: Duration) -> Result<IngestJobStatus> {
        loop {
            let status = self.status().await?;
            match status {
                IngestJobStatus::Completed => return Ok(status),
                IngestJobStatus::Failed => {
                    return Err(Error::Ingest {
                        details: format!("ingest job {} failed", self.rid),
                    });
                }
                IngestJobStatus::Cancelled => {
                    return Err(Error::Ingest {
                        details: format!("ingest job {} was cancelled", self.rid),
                    });
                }
                _ => tokio::time::sleep(interval).await,
            }
        }
    }
}
