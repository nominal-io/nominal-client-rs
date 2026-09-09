use crate::{args::WaitArgs, output::emit, timestamp::TimestampView};
use anyhow::Context;
use nominal::core::*;
use serde::Serialize;
fn status(v: &IngestJobStatus) -> String {
    match v {
        IngestJobStatus::Submitted => "submitted".into(),
        IngestJobStatus::Queued => "queued".into(),
        IngestJobStatus::InProgress => "in_progress".into(),
        IngestJobStatus::Completed => "completed".into(),
        IngestJobStatus::Failed => "failed".into(),
        IngestJobStatus::Cancelled => "cancelled".into(),
        IngestJobStatus::Unknown(s) => format!("unknown({s})"),
    }
}
#[derive(Serialize)]
pub struct JobView<'a> {
    rid: &'a str,
    dataset_rid: Option<&'a str>,
    status: String,
    origin_files: &'a [String],
    ingest_type: String,
    produced_file_count: Option<i32>,
    created_by_rid: Option<&'a str>,
    created_at: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    url: String,
}
impl<'a> From<&'a IngestJob> for JobView<'a> {
    fn from(v: &'a IngestJob) -> Self {
        Self {
            rid: v.rid(),
            dataset_rid: v.dataset_rid(),
            status: status(v.status()),
            origin_files: v.origin_files(),
            ingest_type: match v.ingest_type() {
                IngestType::Tabular => "tabular".into(),
                IngestType::Mcap => "mcap".into(),
                IngestType::Dataflash => "dataflash".into(),
                IngestType::JournalJson => "journal_json".into(),
                IngestType::Containerized => "containerized".into(),
                IngestType::Video => "video".into(),
                IngestType::AvroStream => "avro_stream".into(),
                IngestType::PointCloud => "point_cloud".into(),
                IngestType::Multi => "multi".into(),
                IngestType::Unknown(s) => format!("unknown({s})"),
            },
            produced_file_count: v.produced_file_count(),
            created_by_rid: v.created_by_rid(),
            created_at: v.created_at().map(|v| v.to_rfc3339()),
            start_time: v.start_time().map(|v| v.to_rfc3339()),
            end_time: v.end_time().map(|v| v.to_rfc3339()),
            url: v.nominal_url(),
        }
    }
}
#[derive(Serialize)]
pub struct FileView<'a> {
    rid: &'a str,
    dataset_rid: &'a str,
    name: &'a str,
    bounds: Option<(i128, i128)>,
    bounds_timestamp_type: Option<String>,
    file_size_bytes: Option<i64>,
    timestamp: Option<TimestampView>,
    ingest_status: String,
    uploaded_at: String,
    ingested_at: Option<String>,
    deleted_at: Option<String>,
    ingest_error: Option<&'a str>,
    timestamp_channel: Option<&'a str>,
    file_tags: Option<std::collections::BTreeMap<String, String>>,
    tag_columns: Option<std::collections::BTreeMap<String, String>>,
}
impl<'a> TryFrom<&'a DatasetFile> for FileView<'a> {
    type Error = anyhow::Error;
    fn try_from(v: &'a DatasetFile) -> anyhow::Result<Self> {
        Ok(Self {
            rid: v.rid(),
            dataset_rid: v.dataset_rid(),
            name: v.name(),
            bounds: v.bounds(),
            bounds_timestamp_type: v.bounds_timestamp_type(),
            file_size_bytes: v.file_size_bytes(),
            timestamp: v
                .timestamp()
                .with_context(|| format!("timestamp metadata for dataset file {}", v.rid()))?
                .as_ref()
                .map(Into::into),
            ingest_status: match v.ingest_status() {
                DatasetFileStatus::Success => "success".into(),
                DatasetFileStatus::InProgress => "in_progress".into(),
                DatasetFileStatus::Failed => "failed".into(),
                DatasetFileStatus::DeletionInProgress => "deletion_in_progress".into(),
                DatasetFileStatus::Deleted => "deleted".into(),
                DatasetFileStatus::Queued => "queued".into(),
                DatasetFileStatus::Parsing => "parsing".into(),
                DatasetFileStatus::Ingesting => "ingesting".into(),
                DatasetFileStatus::Unknown(s) => format!("unknown({s})"),
            },
            uploaded_at: v.uploaded_at().to_rfc3339(),
            ingested_at: v.ingested_at().map(|v| v.to_rfc3339()),
            deleted_at: v.deleted_at().map(|v| v.to_rfc3339()),
            ingest_error: v.ingest_error(),
            timestamp_channel: v.timestamp_channel(),
            file_tags: v.file_tags(),
            tag_columns: v.tag_columns(),
        })
    }
}
#[derive(Serialize)]
struct SubmissionView<'a> {
    job_rid: &'a str,
    dataset_rid: &'a str,
    status: Option<String>,
    omitted: [(); 0],
}
pub async fn wait_job(
    ingest: &IngestClient,
    rid: &str,
    timeout: Option<u64>,
) -> anyhow::Result<IngestJob> {
    let future = ingest.wait_for_ingest_job(rid);
    let job = match timeout {
        Some(seconds) => tokio::time::timeout(std::time::Duration::from_secs(seconds), future)
            .await
            .with_context(|| format!("timed out waiting for job {rid}"))?,
        None => future.await,
    };
    job.with_context(|| format!("waiting for acknowledged job {rid}"))
}
pub async fn submission(
    ingest: &IngestClient,
    rid: &str,
    dataset: &str,
    wait: WaitArgs,
    json: bool,
) -> anyhow::Result<()> {
    let status = if wait.no_wait {
        None
    } else {
        Some(status(wait_job(ingest, rid, wait.timeout).await?.status()))
    };
    emit(
        &SubmissionView {
            job_rid: rid,
            dataset_rid: dataset,
            status,
            omitted: [],
        },
        json,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn no_wait_returns_the_job_id_without_fetching_metadata() {
        let client = NominalClient::builder("token")
            .base_url("http://127.0.0.1:1/api")
            .build()
            .unwrap();
        submission(
            &client.ingest(),
            "acknowledged-job",
            "dataset",
            WaitArgs {
                timeout: None,
                no_wait: true,
            },
            true,
        )
        .await
        .unwrap();
    }
    #[test]
    fn extractor_unknown_status_is_explicit() {
        assert_eq!(
            status(&IngestJobStatus::Unknown("future".into())),
            "unknown(future)"
        );
    }
}
