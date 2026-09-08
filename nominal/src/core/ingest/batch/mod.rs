mod encode;
mod items;
#[cfg(test)]
mod rpc_tests;
#[cfg(test)]
mod tests;
mod upload;
use super::filetype::IngestFileFormat;
use super::{ContainerizedIngest, IngestClient, IngestJobRef, UploadOptions, multipart};
use crate::{Error, Result};
pub use items::{
    BatchAvroStream, BatchDataflash, BatchJournalJson, BatchMcap, BatchNumericTimestamp,
    BatchTabular, BatchVideoTiming, Topics,
};
use items::{PendingItem, PendingUpload, PendingVideoTiming};
use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailurePolicy {
    #[default]
    FailFast,
    AllowPartial,
}
/// Upload limits and failure handling for a batch.
///
/// Each concurrent file upload can use the multipart concurrency set in `UploadOptions`.
#[derive(Debug, Clone)]
pub struct BatchOptions {
    failure_policy: FailurePolicy,
    max_uploads: NonZeroUsize,
    runs_to_expand: Vec<String>,
    upload_options: UploadOptions,
}
impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            failure_policy: FailurePolicy::FailFast,
            max_uploads: NonZeroUsize::new(4).unwrap(),
            runs_to_expand: vec![],
            upload_options: UploadOptions::default(),
        }
    }
}
impl BatchOptions {
    pub fn failure_policy(mut self, policy: FailurePolicy) -> Self {
        self.failure_policy = policy;
        self
    }
    pub fn max_uploads(mut self, limit: NonZeroUsize) -> Self {
        self.max_uploads = limit;
        self
    }
    pub fn run_to_expand(mut self, rid: impl Into<String>) -> Self {
        self.runs_to_expand.push(rid.into());
        self
    }
    pub fn upload_options(mut self, options: UploadOptions) -> Self {
        self.upload_options = options;
        self
    }
}
#[derive(Debug)]
pub struct BatchSourceFailure {
    pub name: String,
    pub path: PathBuf,
    pub error: Error,
}
#[derive(Debug)]
pub struct BatchItemFailure {
    pub item_index: usize,
    pub failed_sources: Vec<BatchSourceFailure>,
    pub uploaded_sources: Vec<PathBuf>,
}
#[derive(Debug, thiserror::Error)]
#[error("batch uploads left {} incomplete items; no ingest request was submitted", failures.len())]
pub struct BatchUploadError {
    pub failures: Vec<BatchItemFailure>,
}
#[derive(Debug)]
pub struct BatchSubmission {
    pub job: IngestJobRef,
    pub omitted: Vec<BatchItemFailure>,
}

/// A batch of uploads for an existing dataset. Submitting consumes the batch.
///
/// ```no_run
/// # async fn example(client: &nominal::core::NominalClient) -> nominal::Result<()> {
/// use nominal::core::{BatchDataflash, IngestJob};
/// let mut batch = client.ingest().batch("ri.catalog.main.dataset.example");
/// for path in ["first.bin", "second.bin"] {
///     batch.add_ardupilot_dataflash(path, BatchDataflash::default())?;
/// }
/// let job: IngestJob = batch.submit().await?;
/// println!("{}: {:?}", job.rid(), job.status());
/// # Ok(())
/// # }
/// ```
///
/// ```compile_fail
/// # async fn example(batch: nominal::core::IngestBatch) {
/// batch.submit().await;
/// batch.submit().await;
/// # }
/// ```
pub struct IngestBatch {
    client: IngestClient,
    dataset_rid: String,
    tags: BTreeMap<String, String>,
    items: Vec<PendingItem>,
    next_id: usize,
    sidecars: Vec<tempfile::NamedTempFile>,
}
impl IngestClient {
    pub fn batch(&self, dataset_rid: impl Into<String>) -> IngestBatch {
        IngestBatch {
            client: self.clone(),
            dataset_rid: dataset_rid.into(),
            tags: BTreeMap::new(),
            items: vec![],
            next_id: 0,
            sidecars: vec![],
        }
    }
}
impl IngestBatch {
    fn upload(
        &mut self,
        path: PathBuf,
        name: impl Into<String>,
        mime: &'static str,
    ) -> PendingUpload {
        let id = self.next_id;
        self.next_id += 1;
        PendingUpload {
            id,
            name: name.into(),
            path,
            mime,
        }
    }
    pub fn add_tags(&mut self, tags: BTreeMap<String, String>) -> &mut Self {
        self.tags.extend(tags);
        self
    }
    pub fn add_containerized(&mut self, options: ContainerizedIngest) -> Result<&mut Self> {
        if options.sources.is_empty() {
            return Err(invalid("batch containerized sources must not be empty"));
        }
        let sources = options
            .sources
            .iter()
            .map(|(name, path)| self.upload(path.clone(), name.clone(), batch_mime(path)))
            .collect();
        self.items
            .push(PendingItem::Containerized { sources, options });
        Ok(self)
    }
    pub fn add_tabular(
        &mut self,
        path: impl Into<PathBuf>,
        options: BatchTabular,
    ) -> Result<&mut Self> {
        let path = path.into();
        let descriptor = format(&path)?;
        let format = descriptor
            .tabular()
            .ok_or_else(|| invalid("unsupported batch tabular extension"))?;
        let mime = descriptor.batch_mime();
        let file = self.upload(path, "file", mime);
        self.items.push(PendingItem::Tabular {
            file,
            options,
            format,
        });
        Ok(self)
    }
    pub fn add_avro_stream(
        &mut self,
        path: impl Into<PathBuf>,
        options: BatchAvroStream,
    ) -> Result<&mut Self> {
        let path = path.into();
        let descriptor = require_format(&path, IngestFileFormat::is_avro)?;
        let file = self.upload(path, "file", descriptor.batch_mime());
        self.items.push(PendingItem::Avro { file, options });
        Ok(self)
    }
    pub fn add_mcap(&mut self, path: impl Into<PathBuf>, options: BatchMcap) -> Result<&mut Self> {
        let path = path.into();
        let file = self.upload(path, "file", "application/octet-stream");
        self.items.push(PendingItem::Mcap { file, options });
        Ok(self)
    }
    pub fn add_journal_json(
        &mut self,
        path: impl Into<PathBuf>,
        options: BatchJournalJson,
    ) -> Result<&mut Self> {
        if options.timestamp.as_ref().is_some_and(|t| !t.is_numeric()) {
            return Err(invalid("journal JSON timestamps must be numeric"));
        }
        let path = path.into();
        let descriptor = require_format(&path, IngestFileFormat::is_journal)?;
        let file = self.upload(path, "file", descriptor.batch_mime());
        self.items.push(PendingItem::Journal { file, options });
        Ok(self)
    }
    pub fn add_ardupilot_dataflash(
        &mut self,
        path: impl Into<PathBuf>,
        options: BatchDataflash,
    ) -> Result<&mut Self> {
        let path = path.into();
        let file = self.upload(path, "file", "application/octet-stream");
        self.items.push(PendingItem::Dataflash { file, options });
        Ok(self)
    }
    pub fn add_video(
        &mut self,
        path: impl Into<PathBuf>,
        channel: impl Into<String>,
        timing: BatchVideoTiming,
    ) -> Result<&mut Self> {
        self.add_video_with_tags(path, channel, timing, BTreeMap::new())
    }
    pub fn add_video_with_tags(
        &mut self,
        path: impl Into<PathBuf>,
        channel: impl Into<String>,
        timing: BatchVideoTiming,
        tags: BTreeMap<String, String>,
    ) -> Result<&mut Self> {
        let path = path.into();
        let descriptor = require_format(&path, IngestFileFormat::is_video)?;
        let timing = match timing {
            BatchVideoTiming::Start(start) => PendingVideoTiming::Start(start),
            BatchVideoTiming::FrameTimestamps(times) => {
                if times.is_empty() {
                    return Err(invalid("frame timestamps must not be empty"));
                }
                let mut temp = tempfile::Builder::new()
                    .prefix("nominal-frames-")
                    .suffix(".json")
                    .tempfile()?;
                serde_json::to_writer(temp.as_file_mut(), &times)
                    .map_err(|e| invalid(&e.to_string()))?;
                let upload = self.upload(temp.path().to_owned(), "timestamps", "application/json");
                self.sidecars.push(temp);
                PendingVideoTiming::Frames(upload)
            }
        };
        let file = self.upload(path, "video", descriptor.batch_mime());
        self.items.push(PendingItem::Video {
            file,
            timing,
            channel: channel.into(),
            tags,
        });
        Ok(self)
    }
    /// Uploads this batch and fetches the accepted job's current state.
    pub async fn submit(self) -> Result<super::IngestJob> {
        self.submit_with_options(BatchOptions::default()).await
    }

    /// Uploads with the selected failure policy and fetches the accepted job.
    /// Use `submit_with_report` to inspect omissions without fetching job metadata.
    pub async fn submit_with_options(self, options: BatchOptions) -> Result<super::IngestJob> {
        let client = self.client.clone();
        let report = self.submit_with_report(options).await?;
        report.fetch_job(&client).await
    }

    /// Returns the acknowledged job RID and upload report without fetching metadata.
    pub async fn submit_with_report(self, options: BatchOptions) -> Result<BatchSubmission> {
        if self.items.is_empty() {
            return Err(invalid("cannot submit an empty batch"));
        }
        let _: nominal_api::objects::api::rids::DatasetRid =
            crate::core::rid::parse_rid(&self.dataset_rid)?;
        let upload_workspace = Some(self.client.resolved_workspace_rid().await?);
        let client = &self.client;
        let report = upload::upload_all(
            &self.items,
            options.max_uploads.get(),
            options.failure_policy,
            |path, mime| {
                let upload_options = options.upload_options.clone();
                let upload_workspace = upload_workspace.clone();
                async move {
                    let filename = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("input")
                        .to_owned();
                    multipart::upload_file(
                        client.conjure_client.clone(),
                        &client.runtime,
                        client.token.clone(),
                        upload_workspace,
                        &path,
                        filename,
                        mime.into(),
                        upload_options,
                    )
                    .await
                }
            },
        )
        .await;
        self.submit_completed(options, report).await
    }

    async fn submit_completed(
        self,
        options: BatchOptions,
        report: upload::UploadReport,
    ) -> Result<BatchSubmission> {
        let Some(items) = upload::completed_items(&self.items, &report, options.failure_policy)
        else {
            return Err(BatchUploadError {
                failures: report.failures,
            }
            .into());
        };
        use nominal_api::tonic::nominal::ingest::v2::{
            IngestRequest, ingest_service_client::IngestServiceClient,
        };
        let mut service = IngestServiceClient::with_interceptor(
            self.client.grpc.mutation_channel(),
            self.client.grpc.interceptor(),
        );
        let response = service
            .ingest(IngestRequest {
                dataset_rid: self.dataset_rid,
                runs_to_expand: options.runs_to_expand,
                items,
                tags: self.tags.into_iter().collect(),
            })
            .await?
            .into_inner();
        Ok(BatchSubmission {
            job: IngestJobRef::new(response.ingest_job_rid)?,
            omitted: report.failures,
        })
    }
}
fn invalid(details: &str) -> Error {
    Error::Ingest {
        details: details.into(),
    }
}
fn format(path: &std::path::Path) -> Result<IngestFileFormat> {
    IngestFileFormat::from_path(path).ok_or_else(|| {
        invalid(&format!(
            "unsupported batch file extension: {}",
            path.display()
        ))
    })
}
fn require_format(
    path: &std::path::Path,
    accepts: fn(IngestFileFormat) -> bool,
) -> Result<IngestFileFormat> {
    let descriptor = format(path)?;
    if accepts(descriptor) {
        Ok(descriptor)
    } else {
        Err(invalid(&format!(
            "unsupported batch file extension: {}",
            path.display()
        )))
    }
}
fn batch_mime(path: &std::path::Path) -> &'static str {
    IngestFileFormat::from_path(path)
        .map(IngestFileFormat::batch_mime)
        .unwrap_or("application/octet-stream")
}

impl BatchSubmission {
    async fn fetch_job(self, client: &IngestClient) -> Result<super::IngestJob> {
        for omitted in &self.omitted {
            tracing::warn!(job_rid = self.job.rid(), item_index = omitted.item_index, failed_sources = ?omitted.failed_sources, uploaded_sources = ?omitted.uploaded_sources, "batch item omitted after upload failure");
        }
        client
            .get_ingest_job(self.job.rid())
            .await
            .map_err(|source| Error::IngestJobMetadata {
                job_rid: self.job.rid().to_owned(),
                source: Box::new(source),
            })
    }
}
