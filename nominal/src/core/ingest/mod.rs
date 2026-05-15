mod filetype;
mod job;
mod multipart;
mod options;
mod progress;
mod timestamp;

pub use filetype::FileType;
pub use job::{IngestJob, IngestJobStatus, IngestType};
pub use options::{
    AvroStreamIngest, CsvIngest, DataflashIngest, DatasetTarget, JournalJsonIngest, McapIngest,
    ParquetIngest, UploadOptions, VideoIngest, VideoTarget,
};
pub use progress::{ProgressCallback, UploadEvent};
pub use timestamp::{TimeUnit, Timestamp};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use conjure_http::client::{AsyncService, ConjureRuntime};
use conjure_object::BearerToken;
use conjure_runtime::Client;
use nominal_api::clients::ingest::api::{
    AsyncIngestJobService, AsyncIngestJobServiceClient, AsyncIngestService,
    AsyncIngestServiceClient,
};
use nominal_api::objects::ingest::api::{
    IngestDetails, IngestJobRid, IngestOptions, IngestRequest,
};
use rustls::client::ResolvesClientCert;

use crate::core::rid::{parse_rid, rid_to_string};
use crate::{Error, Result};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Client for uploading files and managing ingest jobs.
pub struct IngestClient {
    ingest_service: AsyncIngestServiceClient<Client>,
    ingest_job_service: AsyncIngestJobServiceClient<Client>,
    conjure_client: Client,
    runtime: Arc<ConjureRuntime>,
    token: BearerToken,
    workspace_rid: Option<String>,
    tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
}

impl IngestClient {
    pub(crate) fn new(
        client: Client,
        runtime: &Arc<ConjureRuntime>,
        token: BearerToken,
        workspace_rid: Option<String>,
        tls_resolver: Option<Arc<dyn ResolvesClientCert>>,
    ) -> Self {
        Self {
            ingest_service: AsyncIngestServiceClient::new(client.clone(), runtime),
            ingest_job_service: AsyncIngestJobServiceClient::new(client.clone(), runtime),
            conjure_client: client,
            runtime: runtime.clone(),
            token,
            workspace_rid,
            tls_resolver,
        }
    }

    /// Upload a CSV file and ingest it into the given dataset.
    ///
    /// The target accepts anything that converts to [`DatasetTarget`]:
    /// `&str` / `String` for an existing dataset RID, or a [`DatasetCreate`]
    /// for a new dataset (created atomically with the ingest).
    ///
    /// Returns `(job, dataset_rid)`: the newly-created ingest job (use
    /// [`Self::wait_for_ingest_job`] to block until it reaches a terminal
    /// state) and the RID of the dataset the data is landing in (the existing
    /// one, or the freshly-created one).
    ///
    /// [`DatasetCreate`]: crate::core::DatasetCreate
    pub async fn upload_csv(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<DatasetTarget>,
        ingest: CsvIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = FileType::Csv;
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_ingest(IngestOptions::Csv(opts)).await
    }

    /// Upload a Parquet file and ingest it into the given dataset.
    ///
    /// The target accepts anything that converts to [`DatasetTarget`]:
    /// `&str` / `String` for an existing dataset RID, or a [`DatasetCreate`]
    /// for a new dataset (created atomically with the ingest).
    ///
    /// Returns `(job, dataset_rid)`.
    ///
    /// [`DatasetCreate`]: crate::core::DatasetCreate
    pub async fn upload_parquet(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<DatasetTarget>,
        ingest: ParquetIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = FileType::Parquet;
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_ingest(IngestOptions::Parquet(opts)).await
    }

    /// Upload an MCAP file and ingest its protobuf timeseries data into the
    /// given dataset.
    ///
    /// Returns `(job, dataset_rid)`.
    pub async fn upload_mcap(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<DatasetTarget>,
        ingest: McapIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = FileType::Mcap;
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_ingest(IngestOptions::McapProtobufTimeseries(opts))
            .await
    }

    /// Upload a journald JSON file (`.jsonl` or `.jsonl.gz`) and ingest it
    /// into the given dataset.
    ///
    /// Returns `(job, dataset_rid)`.
    pub async fn upload_journal_json(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<DatasetTarget>,
        ingest: JournalJsonIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = FileType::from_path(path)
            .filter(|ft| matches!(ft, FileType::JournalJsonl | FileType::JournalJsonlGz))
            .unwrap_or(FileType::JournalJsonl);
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_ingest(IngestOptions::JournalJson(opts)).await
    }

    /// Upload a Nominal Avro-stream file and ingest it into the given dataset.
    ///
    /// Returns `(job, dataset_rid)`.
    pub async fn upload_avro_stream(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<DatasetTarget>,
        ingest: AvroStreamIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = FileType::AvroStream;
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_ingest(IngestOptions::AvroStream(opts)).await
    }

    /// Upload an ArduPilot DataFlash (`.bin`) file and ingest it into the
    /// given dataset.
    ///
    /// Returns `(job, dataset_rid)`.
    pub async fn upload_ardupilot_dataflash(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<DatasetTarget>,
        ingest: DataflashIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = FileType::Dataflash;
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_ingest(IngestOptions::Dataflash(opts)).await
    }

    /// Upload a video file (`.mp4` / `.mkv` / `.avi` / `.ts`) or an MCAP file
    /// containing a video stream and ingest it into the given video resource.
    ///
    /// Build the `ingest` argument with [`VideoIngest::starting_at`] for a
    /// plain video file, or [`VideoIngest::mcap_topic`] to extract a single
    /// stream out of an MCAP.
    ///
    /// Returns `(job, video_rid)`.
    pub async fn upload_video(
        &self,
        path: impl AsRef<Path>,
        target: impl Into<VideoTarget>,
        ingest: VideoIngest,
    ) -> Result<(IngestJob, String)> {
        let path = path.as_ref();
        let upload_options = ingest.upload_options.clone();
        let file_type = if ingest.is_mcap() {
            FileType::Mcap
        } else {
            FileType::from_path(path)
                .filter(|ft| ft.is_video())
                .unwrap_or(FileType::Mp4)
        };
        let filename = upload_filename(path, file_type);
        let s3_path = multipart::upload_file(
            self.conjure_client.clone(),
            &self.runtime,
            self.token.clone(),
            self.workspace_rid.clone(),
            path,
            filename,
            file_type.mime_type().to_string(),
            upload_options,
            self.tls_resolver.clone(),
        )
        .await?;

        let opts = ingest.into_opts(target.into(), self.workspace_rid.as_deref(), s3_path)?;
        self.trigger_video_ingest(IngestOptions::Video(opts)).await
    }

    async fn trigger_ingest(&self, options: IngestOptions) -> Result<(IngestJob, String)> {
        let (job, details) = self.send_ingest(options).await?;
        let dataset_rid = match details {
            IngestDetails::Dataset(d) => d.dataset_rid().to_string(),
            _ => {
                return Err(Error::Ingest {
                    details: "ingest response did not include a dataset RID".into(),
                });
            }
        };
        Ok((job, dataset_rid))
    }

    async fn trigger_video_ingest(&self, options: IngestOptions) -> Result<(IngestJob, String)> {
        let (job, details) = self.send_ingest(options).await?;
        let video_rid = match details {
            IngestDetails::Video(d) => d.video_rid().to_string(),
            _ => {
                return Err(Error::Ingest {
                    details: "ingest response did not include a video RID".into(),
                });
            }
        };
        Ok((job, video_rid))
    }

    async fn send_ingest(&self, options: IngestOptions) -> Result<(IngestJob, IngestDetails)> {
        let request = IngestRequest::new(options);
        let response = self
            .ingest_service
            .ingest(&self.token, &request)
            .await
            .map_err(Error::from)?;
        let rid = response
            .ingest_job_rid()
            .ok_or_else(|| Error::Ingest {
                details: "ingest response did not include an ingest_job_rid".into(),
            })
            .map(rid_to_string)?;
        let details = response.details().clone();
        let job = self.get_ingest_job(&rid).await?;
        Ok((job, details))
    }

    /// Fetch the current state of an ingest job.
    pub async fn get_ingest_job(&self, rid: &str) -> Result<IngestJob> {
        let job_rid: IngestJobRid = parse_rid(rid)?;
        let job = self
            .ingest_job_service
            .get_ingest_job(&self.token, &job_rid)
            .await
            .map_err(Error::from)?;
        Ok(IngestJob::from_conjure(job))
    }

    /// Poll an ingest job until it reaches a terminal state.
    ///
    /// Returns `Ok` on `Completed`. Returns `Err(Error::Ingest { .. })` on
    /// `Failed` or `Cancelled`. Polls every 2 seconds; use
    /// [`Self::wait_for_ingest_job_with_interval`] to override.
    pub async fn wait_for_ingest_job(&self, rid: &str) -> Result<IngestJob> {
        self.wait_for_ingest_job_with_interval(rid, DEFAULT_POLL_INTERVAL)
            .await
    }

    /// Like [`Self::wait_for_ingest_job`] but polls on the given interval.
    pub async fn wait_for_ingest_job_with_interval(
        &self,
        rid: &str,
        interval: Duration,
    ) -> Result<IngestJob> {
        loop {
            let job = self.get_ingest_job(rid).await?;
            match job.status() {
                IngestJobStatus::Completed => return Ok(job),
                IngestJobStatus::Failed => {
                    return Err(Error::Ingest {
                        details: format!("ingest job {rid} failed"),
                    });
                }
                IngestJobStatus::Cancelled => {
                    return Err(Error::Ingest {
                        details: format!("ingest job {rid} was cancelled"),
                    });
                }
                IngestJobStatus::Unknown(s) => {
                    return Err(Error::Ingest {
                        details: format!("ingest job {rid} reported unknown status: {s}"),
                    });
                }
                IngestJobStatus::Submitted
                | IngestJobStatus::Queued
                | IngestJobStatus::InProgress => tokio::time::sleep(interval).await,
            }
        }
    }
}

/// Build the `filename` passed to `InitiateMultipartUploadRequest`: the file's
/// stem with any unsafe characters replaced and the canonical extension
/// re-appended.
fn upload_filename(path: &Path, file_type: FileType) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload");
    let lowered = name.to_ascii_lowercase();
    let ext = file_type.extension();
    let stem = if lowered.ends_with(ext) {
        &name[..name.len() - ext.len()]
    } else {
        name.split('.').next().unwrap_or("upload")
    };
    format!("{}{}", sanitize(stem), ext)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn upload_filename_strips_and_reappends_extension() {
        let p = PathBuf::from("/tmp/data.csv");
        assert_eq!(upload_filename(&p, FileType::Csv), "data.csv");
    }

    #[test]
    fn upload_filename_sanitizes_unsafe_characters() {
        let p = PathBuf::from("weird name (1).csv");
        assert_eq!(upload_filename(&p, FileType::Csv), "weird_name__1_.csv");
    }

    #[test]
    fn upload_filename_handles_missing_extension() {
        let p = PathBuf::from("data");
        assert_eq!(upload_filename(&p, FileType::Parquet), "data.parquet");
    }

    #[test]
    fn upload_filename_keeps_csv_gz_extension() {
        let p = PathBuf::from("measurements.csv.gz");
        assert_eq!(upload_filename(&p, FileType::CsvGz), "measurements.csv.gz");
    }
}
