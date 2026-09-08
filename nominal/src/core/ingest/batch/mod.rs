mod encode;
mod items;
#[cfg(test)]
mod tests;
mod upload;
use super::{ContainerizedIngest, FileType, IngestClient, IngestJobRef, UploadOptions, multipart};
use crate::{Error, Result};
pub use items::{
    BatchAvroStream, BatchDataflash, BatchJournalJson, BatchMcap, BatchNumericTimestamp,
    BatchTabular, BatchVideoTiming, Topics,
};
use items::{PendingItem, PendingUpload, TabularFormat};
use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailurePolicy {
    #[default]
    FailFast,
    AllowPartial,
}
/// File concurrency multiplies multipart concurrency in `UploadOptions`.
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

/// An owned, single-use batch targeting an existing dataset.
///
/// ```compile_fail
/// # async fn example(batch: nominal::core::IngestBatch<'_>) {
/// use nominal::core::BatchOptions;
/// batch.submit(BatchOptions::default()).await;
/// batch.submit(BatchOptions::default()).await;
/// # }
/// ```
pub struct IngestBatch<'a> {
    client: &'a IngestClient,
    dataset_rid: String,
    tags: BTreeMap<String, String>,
    items: Vec<PendingItem>,
    next_id: usize,
    sidecars: Vec<tempfile::NamedTempFile>,
}
impl IngestClient {
    pub fn batch(&self, dataset_rid: impl Into<String>) -> IngestBatch<'_> {
        IngestBatch {
            client: self,
            dataset_rid: dataset_rid.into(),
            tags: BTreeMap::new(),
            items: vec![],
            next_id: 0,
            sidecars: vec![],
        }
    }
}
impl IngestBatch<'_> {
    fn upload(&mut self, path: PathBuf, name: impl Into<String>) -> PendingUpload {
        let id = self.next_id;
        self.next_id += 1;
        PendingUpload {
            id,
            name: name.into(),
            path,
        }
    }
    pub fn add_tags(mut self, tags: BTreeMap<String, String>) -> Self {
        self.tags.extend(tags);
        self
    }
    pub fn add_containerized(mut self, options: ContainerizedIngest) -> Result<Self> {
        if options.sources.is_empty() {
            return Err(invalid("batch containerized sources must not be empty"));
        }
        let sources = options
            .sources
            .iter()
            .map(|(name, path)| self.upload(path.clone(), name.clone()))
            .collect();
        self.items
            .push(PendingItem::Containerized { sources, options });
        Ok(self)
    }
    pub fn add_tabular(mut self, path: impl Into<PathBuf>, options: BatchTabular) -> Result<Self> {
        let path = path.into();
        let name = path.to_string_lossy().to_ascii_lowercase();
        let format = if name.ends_with(".csv") || name.ends_with(".csv.gz") {
            TabularFormat::Csv
        } else if name.ends_with(".parquet") || name.ends_with(".parquet.gz") {
            TabularFormat::Parquet { archive: false }
        } else if [".parquet.tar", ".parquet.tar.gz", ".parquet.zip"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        {
            TabularFormat::Parquet { archive: true }
        } else {
            return Err(invalid("unsupported batch tabular extension"));
        };
        let file = self.upload(path, "file");
        self.items.push(PendingItem::Tabular {
            file,
            options,
            format,
        });
        Ok(self)
    }
    pub fn add_avro_stream(
        mut self,
        path: impl Into<PathBuf>,
        options: BatchAvroStream,
    ) -> Result<Self> {
        let path = path.into();
        require_type(&path, &[FileType::AvroStream])?;
        let file = self.upload(path, "file");
        self.items.push(PendingItem::Avro { file, options });
        Ok(self)
    }
    pub fn add_mcap(mut self, path: impl Into<PathBuf>, options: BatchMcap) -> Result<Self> {
        let path = path.into();
        require_type(&path, &[FileType::Mcap])?;
        let file = self.upload(path, "file");
        self.items.push(PendingItem::Mcap { file, options });
        Ok(self)
    }
    pub fn add_journal_json(
        mut self,
        path: impl Into<PathBuf>,
        options: BatchJournalJson,
    ) -> Result<Self> {
        if options.timestamp.as_ref().is_some_and(|t| !t.is_numeric()) {
            return Err(invalid("journal JSON timestamps must be numeric"));
        }
        let path = path.into();
        require_type(&path, &[FileType::JournalJsonl, FileType::JournalJsonlGz])?;
        let file = self.upload(path, "file");
        self.items.push(PendingItem::Journal { file, options });
        Ok(self)
    }
    pub fn add_dataflash(
        mut self,
        path: impl Into<PathBuf>,
        options: BatchDataflash,
    ) -> Result<Self> {
        let path = path.into();
        require_type(&path, &[FileType::Dataflash])?;
        let file = self.upload(path, "file");
        self.items.push(PendingItem::Dataflash { file, options });
        Ok(self)
    }
    pub fn add_video(
        self,
        path: impl Into<PathBuf>,
        channel: impl Into<String>,
        timing: BatchVideoTiming,
    ) -> Result<Self> {
        self.add_video_with_tags(path, channel, timing, BTreeMap::new())
    }
    pub fn add_video_with_tags(
        mut self,
        path: impl Into<PathBuf>,
        channel: impl Into<String>,
        timing: BatchVideoTiming,
        tags: BTreeMap<String, String>,
    ) -> Result<Self> {
        let path = path.into();
        require_type(
            &path,
            &[FileType::Mp4, FileType::Mkv, FileType::Avi, FileType::Ts],
        )?;
        let (sidecar, start) = match timing {
            BatchVideoTiming::Start(start) => (None, Some(start)),
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
                let upload = self.upload(temp.path().to_owned(), "timestamps");
                self.sidecars.push(temp);
                (Some(upload), None)
            }
        };
        let file = self.upload(path, "video");
        self.items.push(PendingItem::Video {
            file,
            sidecar,
            start,
            channel: channel.into(),
            tags,
        });
        Ok(self)
    }
    pub async fn submit(self, options: BatchOptions) -> Result<BatchSubmission> {
        if self.items.is_empty() {
            return Err(invalid("cannot submit an empty batch"));
        }
        let _: nominal_api::objects::api::rids::DatasetRid =
            crate::core::rid::parse_rid(&self.dataset_rid)?;
        let report = upload::upload_all(
            &self.items,
            options.max_uploads.get(),
            options.failure_policy,
            |path| {
                let upload_options = options.upload_options.clone();
                async move {
                    let filename = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("input")
                        .to_owned();
                    let mime = FileType::from_path(&path)
                        .map(|t| t.mime_type())
                        .unwrap_or("application/octet-stream");
                    multipart::upload_file(
                        self.client.conjure_client.clone(),
                        &self.client.runtime,
                        self.client.token.clone(),
                        self.client.workspace_rid.clone(),
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
        let items: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                !report
                    .failures
                    .iter()
                    .any(|failure| failure.item_index == *index)
            })
            .map(|(_, item)| encode::encode(item, &report.locations))
            .collect();
        if items.is_empty()
            || (options.failure_policy == FailurePolicy::FailFast && !report.failures.is_empty())
        {
            return Err(BatchUploadError {
                failures: report.failures,
            }
            .into());
        }
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
fn require_type(path: &std::path::Path, allowed: &[FileType]) -> Result<()> {
    if FileType::from_path(path).is_some_and(|t| allowed.contains(&t)) {
        Ok(())
    } else {
        Err(invalid(&format!(
            "unsupported batch file extension: {}",
            path.display()
        )))
    }
}
