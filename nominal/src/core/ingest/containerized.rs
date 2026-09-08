use super::{DatasetTarget, IngestClient, Timestamp, UploadOptions, multipart};
use crate::core::rid::parse_rid;
use crate::{Error, Result};
use conjure_http::client::AsyncService;
use nominal_api::clients::ingest::api::{AsyncIngestService, AsyncIngestServiceClient};
use nominal_api::objects::ingest::api::{
    ContainerizedOpts, IngestDetails, IngestOptions, IngestRequest, IngestSource, S3IngestSource,
};
use std::{collections::BTreeMap, path::PathBuf};

/// An acknowledged job RID. Use `get_ingest_job` to fetch its current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestJobRef {
    rid: String,
}
impl IngestJobRef {
    pub fn rid(&self) -> &str {
        &self.rid
    }
    pub(crate) fn new(rid: String) -> Result<Self> {
        if rid.is_empty() {
            return Err(Error::UnexpectedResponse {
                field: "ingest_job_rid",
            });
        }
        Ok(Self { rid })
    }
}
#[derive(Debug, Clone)]
pub struct ContainerizedSubmission {
    job: IngestJobRef,
    dataset_rid: String,
}
impl ContainerizedSubmission {
    pub fn job(&self) -> &IngestJobRef {
        &self.job
    }
    pub fn dataset_rid(&self) -> &str {
        &self.dataset_rid
    }
}
/// Named inputs and arguments for a containerized extractor.
#[derive(Debug, Clone)]
pub struct ContainerizedIngest {
    pub(crate) extractor_rid: String,
    pub(crate) sources: BTreeMap<String, PathBuf>,
    pub(crate) arguments: BTreeMap<String, String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) timestamp: Option<Timestamp>,
}
impl ContainerizedIngest {
    pub fn new(extractor_rid: impl Into<String>) -> Self {
        Self {
            extractor_rid: extractor_rid.into(),
            sources: BTreeMap::new(),
            arguments: BTreeMap::new(),
            tags: BTreeMap::new(),
            timestamp: None,
        }
    }
    pub fn extractor_rid(&self) -> &str {
        &self.extractor_rid
    }
    pub fn sources(&self) -> &BTreeMap<String, PathBuf> {
        &self.sources
    }
    pub fn arguments(&self) -> &BTreeMap<String, String> {
        &self.arguments
    }
    pub fn tags(&self) -> &BTreeMap<String, String> {
        &self.tags
    }
    pub fn timestamp_metadata(&self) -> Option<&Timestamp> {
        self.timestamp.as_ref()
    }
    pub fn source(mut self, env: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.sources.insert(env.into(), path.into());
        self
    }
    pub fn argument(mut self, env: impl Into<String>, value: impl Into<String>) -> Self {
        self.arguments.insert(env.into(), value.into());
        self
    }
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }
    /// Add default scope tags without replacing tags already set on the request.
    ///
    /// A workbook run scope identifies datasets through each run's
    /// named data sources. `WorkbookDataScope` exposes assets and runs, not dataset views
    /// or tag filters, so tags must be supplied explicitly. This example requires a
    /// run-scoped workbook and a dataset directly attached to the selected run;
    /// multi-asset runs may instead require looking up the underlying asset.
    ///
    /// ```no_run
    /// use nominal::{Result, Error};
    /// use nominal::core::{NominalClient, WorkbookDataScope, DataSource,
    ///     ContainerizedIngest, ContainerizedSubmission, DatasetTarget};
    /// use std::collections::BTreeMap;
    /// async fn ingest_run_dataset(
    ///     client: &NominalClient, workbook_rid: &str, run_rid: &str,
    ///     ref_name: &str, extractor_rid: &str, scope_tags: BTreeMap<String, String>,
    /// ) -> Result<ContainerizedSubmission> {
    ///     let workbook = client.workbooks().get(workbook_rid).await?;
    ///     if !matches!(workbook.data_scope(), WorkbookDataScope::Runs(runs)
    ///         if runs.iter().any(|rid| rid == run_rid)) {
    ///         return Err(Error::Ingest { details: "select a run in this workbook's run scope".into() });
    ///     }
    ///     let run = client.runs().get(run_rid).await?;
    ///     let Some(DataSource::Dataset(dataset_rid)) = run.data_sources().get(ref_name) else {
    ///         return Err(Error::Ingest { details: "selected run reference is not an attached dataset".into() });
    ///     };
    ///     let ingest = ContainerizedIngest::new(extractor_rid)
    ///         .source("INPUT", "measurements.bin")
    ///         .tag("operator", "caller")
    ///         .with_scope_tags(scope_tags);
    ///     client.ingest().upload_containerized(
    ///         DatasetTarget::Existing(dataset_rid.clone()), ingest).await
    /// }
    /// ```
    pub fn with_scope_tags(mut self, tags: BTreeMap<String, String>) -> Self {
        for (k, v) in tags {
            self.tags.entry(k).or_insert(v);
        }
        self
    }
}
impl IngestClient {
    pub async fn upload_containerized(
        &self,
        target: DatasetTarget,
        ingest: ContainerizedIngest,
    ) -> Result<ContainerizedSubmission> {
        // Check the target and required inputs before uploading files.
        let extractor = self.extractors.get(&ingest.extractor_rid).await?;
        let target = target.into_api(Some(extractor.workspace_rid()))?;
        let image = preflight(&extractor, &ingest)?;
        let upload_workspace = Some(parse_rid(extractor.workspace_rid())?);
        let timestamp = ingest.timestamp.as_ref().or(image.default_timestamp());
        let mut sources = BTreeMap::new();
        for (name, path) in ingest.sources {
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("input")
                .to_owned();
            let location = multipart::upload_file(
                self.conjure_client.clone(),
                &self.runtime,
                self.token.clone(),
                upload_workspace.clone(),
                &path,
                filename,
                "application/octet-stream".into(),
                UploadOptions::default(),
            )
            .await?;
            sources.insert(name.into(), IngestSource::S3(S3IngestSource::new(location)));
        }
        let opts = ContainerizedOpts::builder()
            .extractor_rid(parse_rid(&ingest.extractor_rid)?)
            .target(target)
            .sources(sources)
            .arguments(ingest.arguments.into_iter().map(|(k, v)| (k.into(), v)))
            .additional_file_tags(ingest.tags.into_iter().map(|(k, v)| (k.into(), v.into())))
            .timestamp_metadata(timestamp.map(|t| t.clone().into_conjure()))
            .build();
        self.submit_containerized(opts).await
    }
    async fn submit_containerized(
        &self,
        opts: ContainerizedOpts,
    ) -> Result<ContainerizedSubmission> {
        let service = AsyncIngestServiceClient::new(self.mutation_client.clone(), &self.runtime);
        let response = service
            .ingest(
                &self.token,
                &IngestRequest::new(IngestOptions::Containerized(opts)),
            )
            .await?;
        ContainerizedSubmission::from_response(response)
    }
}
impl ContainerizedSubmission {
    fn from_response(response: nominal_api::objects::ingest::api::IngestResponse) -> Result<Self> {
        let job = IngestJobRef::new(
            response
                .ingest_job_rid()
                .map(ToString::to_string)
                .unwrap_or_default(),
        )?;
        let dataset_rid = match response.details() {
            IngestDetails::Dataset(d) => d.dataset_rid().to_string(),
            _ => {
                return Err(Error::Ingest {
                    details: format!(
                        "ingest job {} was acknowledged but its response omitted the dataset destination; inspect this job rather than resubmitting",
                        job.rid()
                    ),
                });
            }
        };
        Ok(ContainerizedSubmission { job, dataset_rid })
    }
}
fn preflight<'a>(
    extractor: &'a crate::core::extractor::ContainerizedExtractor,
    ingest: &ContainerizedIngest,
) -> Result<&'a crate::core::extractor::ContainerImage> {
    let image = extractor
        .active_container_image()
        .ok_or_else(|| Error::Ingest {
            details: format!("extractor {} has no active image", ingest.extractor_rid),
        })?;
    for input in image.inputs() {
        if input.is_required() && !ingest.sources.contains_key(input.environment_variable()) {
            return Err(Error::Ingest {
                details: format!(
                    "required source {} is missing",
                    input.environment_variable()
                ),
            });
        }
    }
    Ok(image)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn containerized_scope_defaults_do_not_replace_caller_tags() {
        let ingest = ContainerizedIngest::new("extractor")
            .tag("scope", "caller")
            .with_scope_tags(BTreeMap::from([
                ("scope".into(), "default".into()),
                ("other".into(), "value".into()),
            ]));
        assert_eq!(ingest.tags()["scope"], "caller");
        assert_eq!(ingest.tags()["other"], "value");
    }
}
#[cfg(test)]
mod request_tests {
    use super::*;
    use nominal_api::tonic::nominal::{ingest::v2, registry::v2 as registry};
    #[test]
    fn containerized_preflight_empty_sources_depend_on_active_contract() {
        let ingest = ContainerizedIngest::new("extractor");
        let mut proto = v2::ContainerizedExtractor {
            rid: "extractor".into(),
            workspace_rid: "workspace".into(),
            ..Default::default()
        };
        let snapshot =
            crate::core::extractor::ContainerizedExtractor::from_proto(proto.clone()).unwrap();
        assert!(preflight(&snapshot, &ingest).is_err());
        proto.active_container_image = Some(registry::ContainerImage {
            rid: "image".into(),
            extractor_rid: "extractor".into(),
            ..Default::default()
        });
        let snapshot =
            crate::core::extractor::ContainerizedExtractor::from_proto(proto.clone()).unwrap();
        assert!(preflight(&snapshot, &ingest).is_ok());
        proto.active_container_image.as_mut().unwrap().inputs.push(
            crate::core::extractor::FileExtractionInput::new("input", "INPUT")
                .required(true)
                .into_proto(),
        );
        let snapshot = crate::core::extractor::ContainerizedExtractor::from_proto(proto).unwrap();
        assert!(preflight(&snapshot, &ingest).is_err());
        assert!(preflight(&snapshot, &ingest.source("INPUT", "does-not-need-to-exist")).is_ok());
    }
    #[tokio::test]
    async fn containerized_submission_returns_the_job_id_without_fetching_metadata() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = stream.read(&mut buffer).unwrap();
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
                        break;
                    }
                }
            }
            assert!(String::from_utf8_lossy(&bytes).starts_with("POST /api/ingest/v1/ingest "));
            let body = r#"{"ingestJobRid":"ri.ingest.main.job.test","details":{"type":"dataset","dataset":{"datasetRid":"ri.catalog.main.dataset.test"}}}"#;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            // The listener closes here. Any implicit metadata fetch makes the SDK operation fail.
        });
        let client = crate::core::NominalClient::builder("token")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        let target = DatasetTarget::Existing("ri.catalog.main.dataset.test".into())
            .into_api(None)
            .unwrap();
        let result = client
            .ingest()
            .submit_containerized(ContainerizedOpts::new(
                parse_rid("ri.ingest.main.extractor.test").unwrap(),
                target,
            ))
            .await
            .unwrap();
        assert_eq!(result.job().rid(), "ri.ingest.main.job.test");
        server.join().unwrap();
    }
}

#[cfg(test)]
mod acknowledgement_tests {
    use super::*;
    #[test]
    fn acknowledgement_without_job_rid_is_rejected() {
        let response = serde_json::from_value(serde_json::json!({
            "details":{"type":"dataset","dataset":{"datasetRid":"ri.catalog.main.dataset.test"}}
        }))
        .unwrap();
        assert!(matches!(
            ContainerizedSubmission::from_response(response),
            Err(Error::UnexpectedResponse {
                field: "ingest_job_rid"
            })
        ));
    }
    #[test]
    fn malformed_destination_preserves_acknowledged_job_identity() {
        let response = serde_json::from_value(serde_json::json!({
            "ingestJobRid":"ri.ingest.main.job.acknowledged",
            "details":{"type":"future","future":{}}
        }))
        .unwrap();
        let error = ContainerizedSubmission::from_response(response).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ri.ingest.main.job.acknowledged")
        );
        assert!(error.to_string().contains("acknowledged"));
    }
}
