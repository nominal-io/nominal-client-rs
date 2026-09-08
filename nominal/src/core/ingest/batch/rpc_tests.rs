use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use nominal_api::tonic::nominal::ingest::v2 as p;
use prost::Message;
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
#[derive(Clone)]
struct Mock {
    requests: Arc<Mutex<Vec<p::IngestRequest>>>,
    status: u16,
}
impl tonic::server::NamedService for Mock {
    const NAME: &'static str = "nominal.ingest.v2.IngestService";
}
impl tower::Service<http::Request<tonic::body::Body>> for Mock {
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Infallible>> + Send>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<std::result::Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        let mock = self.clone();
        Box::pin(async move {
            assert_eq!(
                request.uri().path(),
                "/nominal.ingest.v2.IngestService/Ingest"
            );
            assert_eq!(request.headers()["authorization"], "Bearer test");
            let bytes = request.into_body().collect().await.unwrap().to_bytes();
            mock.requests
                .lock()
                .unwrap()
                .push(p::IngestRequest::decode(&bytes[5..]).unwrap());
            let payload = p::IngestResponse {
                ingest_job_rid: "ri.ingest.main.job.ack".into(),
            }
            .encode_to_vec();
            let mut bytes = vec![0];
            bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            bytes.extend(payload);
            Ok(http::Response::builder()
                .header("content-type", "application/grpc")
                .header("grpc-status", mock.status.to_string())
                .body(tonic::body::Body::new(Full::new(Bytes::from(bytes))))
                .unwrap())
        })
    }
}
async fn fixture(
    status: u16,
) -> (
    crate::core::NominalClient,
    Mock,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = futures::stream::unfold(listener, |listener| async {
        Some((listener.accept().await.map(|(stream, _)| stream), listener))
    });
    let mock = Mock {
        requests: Default::default(),
        status,
    };
    let server = mock.clone();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(server)
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    let client = crate::core::NominalClient::builder("test")
        .base_url(format!("http://{address}/api"))
        .build()
        .unwrap();
    (client, mock, task)
}
#[tokio::test]
async fn batch_returns_the_job_id_without_fetching_metadata_or_retrying_submission() {
    for status in [0, 14] {
        let (client, mock, task) = fixture(status).await;
        let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
        batch
            .add_ardupilot_dataflash("data.bin", BatchDataflash::default().tag("item", "yes"))
            .unwrap()
            .add_tags(BTreeMap::from([("request".into(), "yes".into())]));
        let report = upload::UploadReport {
            locations: BTreeMap::from([(0, "s3://complete".into())]),
            failures: vec![],
        };
        let result = batch
            .submit_completed(BatchOptions::default().run_to_expand("run"), report)
            .await;
        if status == 0 {
            assert_eq!(result.unwrap().job.rid(), "ri.ingest.main.job.ack");
        } else {
            assert!(result.is_err());
        }
        let requests = mock.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].tags["request"], "yes");
        assert_eq!(requests[0].items[0].tags["item"], "yes");
        assert_eq!(requests[0].runs_to_expand, vec!["run"]);
        task.abort();
    }
}
#[tokio::test]
async fn batch_rpc_partial_omits_failed_item_and_default_submits_nothing() {
    for (policy, all_fail) in [
        (FailurePolicy::FailFast, false),
        (FailurePolicy::AllowPartial, false),
        (FailurePolicy::AllowPartial, true),
    ] {
        let (client, mock, task) = fixture(0).await;
        let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
        batch
            .add_ardupilot_dataflash("bad.bin", BatchDataflash::default())
            .unwrap()
            .add_ardupilot_dataflash("good.bin", BatchDataflash::default())
            .unwrap();
        let report = upload::upload_all(
            &batch.items,
            2,
            FailurePolicy::AllowPartial,
            |path, _| async move {
                if all_fail || path == std::path::Path::new("bad.bin") {
                    Err(invalid("bad"))
                } else {
                    Ok("s3://good".into())
                }
            },
        )
        .await;
        let result = batch
            .submit_completed(BatchOptions::default().failure_policy(policy), report)
            .await;
        if policy == FailurePolicy::FailFast || all_fail {
            assert!(result.is_err());
            assert!(mock.requests.lock().unwrap().is_empty());
        } else {
            let result = result.unwrap();
            assert_eq!(result.omitted[0].item_index, 0);
            assert_eq!(mock.requests.lock().unwrap()[0].items.len(), 1);
        }
        task.abort();
    }
}

#[tokio::test]
async fn accepted_batch_fetches_full_job_and_preserves_rid_on_metadata_failure() {
    use std::io::{Read, Write};
    for success in [true, false] {
        let (client, mock, task) = fixture(0).await;
        let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
        batch
            .add_ardupilot_dataflash("data.bin", BatchDataflash::default())
            .unwrap();
        let report = batch
            .submit_completed(
                BatchOptions::default(),
                upload::UploadReport {
                    locations: BTreeMap::from([(0, "s3://complete".into())]),
                    failures: vec![],
                },
            )
            .await
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 2048];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            assert!(
                String::from_utf8_lossy(&bytes)
                    .starts_with("GET /api/ingest/v1/ingest-job/ri.ingest.main.job.ack ")
            );
            let body = if success {
                serde_json::json!({
                "ingestJobRid":"ri.ingest.main.job.ack", "status":"QUEUED", "ingestType":"MULTI",
                "createdBy":"00000000-0000-0000-0000-000000000000", "orgUuid":"00000000-0000-0000-0000-000000000000",
                "datasetRid":"ri.catalog.main.dataset.test", "producedFileCount":7,
            }).to_string()
            } else {
                "{}".into()
            };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(),body).unwrap();
        });
        let metadata_client = crate::core::NominalClient::builder("test")
            .base_url(format!("http://{address}/api"))
            .build()
            .unwrap();
        let result = report.fetch_job(&metadata_client.ingest()).await;
        if success {
            let job: crate::core::IngestJob = result.unwrap();
            assert_eq!(job.rid(), "ri.ingest.main.job.ack");
            assert_eq!(job.produced_file_count(), Some(7));
            assert_eq!(job.status(), &crate::core::IngestJobStatus::Queued);
        } else {
            let error = result.unwrap_err();
            assert!(
                matches!(&error, Error::IngestJobMetadata { job_rid, .. } if job_rid == "ri.ingest.main.job.ack")
            );
            assert!(error.to_string().contains("do not resubmit"));
        }
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        server.join().unwrap();
        task.abort();
    }
}
