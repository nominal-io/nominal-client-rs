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
        // The same local path can be ingested twice with different tags.
        for tag in ["first", "second"] {
            batch
                .add_ardupilot_dataflash("data.bin", BatchDataflash::default().tag("item", tag))
                .unwrap();
        }
        batch.add_tags(BTreeMap::from([("request".into(), "yes".into())]));
        let result = batch
            .upload_and_submit(BatchOptions::default().run_to_expand("run"), |_, _| async {
                Ok("s3://complete".into())
            })
            .await;
        if status == 0 {
            assert_eq!(result.unwrap().job.rid(), "ri.ingest.main.job.ack");
        } else {
            assert!(result.is_err());
        }
        let requests = mock.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].tags["request"], "yes");
        assert_eq!(requests[0].items.len(), 2);
        assert_eq!(requests[0].items[0].tags["item"], "first");
        assert_eq!(requests[0].items[1].tags["item"], "second");
        assert_eq!(requests[0].runs_to_expand, vec!["run"]);
        task.abort();
    }
}

#[tokio::test]
async fn partial_upload_omits_whole_items_and_reports_successful_siblings() {
    for (policy, all_fail) in [
        (FailurePolicy::FailFast, false),
        (FailurePolicy::AllowPartial, false),
        (FailurePolicy::AllowPartial, true),
    ] {
        let (client, mock, task) = fixture(0).await;
        let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
        batch
            .add_containerized(
                ContainerizedIngest::new("extractor")
                    .source("BAD", "bad.bin")
                    .source("GOOD", "sibling.bin"),
            )
            .unwrap();
        batch
            .add_ardupilot_dataflash("good.bin", BatchDataflash::default())
            .unwrap();
        let result = batch
            .upload_and_submit(
                BatchOptions::default().failure_policy(policy),
                |path, _| async move {
                    if all_fail || path == std::path::Path::new("bad.bin") {
                        Err(invalid("upload failed"))
                    } else {
                        Ok(format!("s3://{}", path.display()))
                    }
                },
            )
            .await;
        if policy == FailurePolicy::FailFast || all_fail {
            assert!(matches!(result, Err(Error::BatchUpload(_))));
            assert!(mock.requests.lock().unwrap().is_empty());
        } else {
            let omitted = result.unwrap().omitted;
            assert_eq!(omitted.len(), 1);
            assert_eq!(omitted[0].item_index, 0);
            assert_eq!(omitted[0].failed_sources[0].name, "BAD");
            assert_eq!(
                omitted[0].uploaded_sources,
                vec![PathBuf::from("sibling.bin")]
            );
            let requests = mock.requests.lock().unwrap();
            assert_eq!(requests[0].items.len(), 1);
            assert!(matches!(
                requests[0].items[0].item,
                Some(p::ingest_item::Item::Dataflash(_))
            ));
        }
        task.abort();
    }
}

#[tokio::test]
async fn upload_concurrency_is_bounded_and_fail_fast_drains_active_uploads() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (client, mock, task) = fixture(0).await;
    let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
    for i in 0..8 {
        batch
            .add_ardupilot_dataflash(format!("{i}.bin"), BatchDataflash::default())
            .unwrap();
    }
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let calls = AtomicUsize::new(0);
    let result = batch
        .upload_and_submit(
            BatchOptions::default().max_uploads(NonZeroUsize::new(2).unwrap()),
            |_, _| async {
                calls.fetch_add(1, Ordering::SeqCst);
                let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                Err(invalid("upload failed"))
            },
        )
        .await;
    assert!(matches!(result, Err(Error::BatchUpload(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(peak.load(Ordering::SeqCst) <= 2);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert!(mock.requests.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn accepted_batch_fetches_full_job_and_preserves_rid_on_metadata_failure() {
    use crate::test_support::{HttpFixture, ingest_job};
    for success in [true, false] {
        let (client, mock, task) = fixture(0).await;
        let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
        batch
            .add_ardupilot_dataflash("data.bin", BatchDataflash::default())
            .unwrap();
        let report = batch
            .upload_and_submit(BatchOptions::default(), |_, _| async {
                Ok("s3://complete".into())
            })
            .await
            .unwrap();
        let server = HttpFixture::new(move |request| async move {
            assert_eq!(
                request.uri().path(),
                "/api/ingest/v1/ingest-job/ri.ingest.main.job.ack"
            );
            if success {
                let mut job = ingest_job("ri.ingest.main.job.ack");
                job["status"] = "QUEUED".into();
                job["producedFileCount"] = 7.into();
                job
            } else {
                serde_json::json!({})
            }
        })
        .await;
        let result = server.run(report.fetch_job(&server.client.ingest())).await;
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
        task.abort();
    }
}

#[tokio::test]
async fn batch_submits_format_options_and_keeps_valid_items_after_rejected_additions() {
    use super::super::{TimeUnit, Timestamp};
    use nominal_api::tonic::nominal::ingest::v2::{file_ingest_options::Ingest, ingest_item::Item};
    let (client, mock, task) = fixture(0).await;
    let ingest = client.ingest();
    let mut batch = ingest.batch("dataset");
    batch
        .add_tabular(
            "a.parquet.tar.gz",
            BatchTabular::new(Timestamp::epoch("t", TimeUnit::Seconds))
                .tag_column("sensor", "column")
                .unit("a", "m")
                .channel_prefix("pre")
                .channel_name_override("a", "b")
                .tag("kind", "tabular"),
        )
        .unwrap()
        .add_avro_stream(
            "a.avro",
            BatchAvroStream::default()
                .unit("a", "s")
                .channel_prefix("avro"),
        )
        .unwrap()
        .add_mcap(
            "a.mcap",
            BatchMcap::default()
                .topics(Topics::Exclude(vec!["bad".into()]))
                .ignore_invalid_topics(true),
        )
        .unwrap()
        .add_journal_json(
            "a.jsonl",
            BatchJournalJson::default()
                .channel("log")
                .timestamp(Timestamp::epoch("time", TimeUnit::Microseconds)),
        )
        .unwrap()
        .add_ardupilot_dataflash("a.bin", BatchDataflash::default())
        .unwrap()
        .add_containerized(
            ContainerizedIngest::new("extractor")
                .source("INPUT", "a.dat")
                .argument("MODE", "fast"),
        )
        .unwrap()
        .add_video(
            "a.mp4",
            "video",
            BatchVideoTiming::Start(chrono::DateTime::from_timestamp(-1, 999_999_999).unwrap()),
        )
        .unwrap();
    assert!(
        batch
            .add_tabular("bad.txt", BatchTabular::new(Timestamp::iso8601("ts")))
            .is_err()
    );
    assert!(
        batch
            .add_containerized(ContainerizedIngest::new("extractor"))
            .is_err()
    );
    assert!(
        batch
            .add_video("a.mp4", "camera", BatchVideoTiming::FrameTimestamps(vec![]))
            .is_err()
    );
    assert!(
        batch
            .add_journal_json(
                "logs.jsonl",
                BatchJournalJson::default().timestamp(Timestamp::iso8601("ts"))
            )
            .is_err()
    );
    batch
        .upload_and_submit(BatchOptions::default(), |path, _| async move {
            Ok(format!("s3://{}", path.display()))
        })
        .await
        .unwrap();
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let encoded = &requests[0].items;
    assert_eq!(encoded.len(), 7);
    let Some(Item::File(file)) = &encoded[0].item else {
        panic!()
    };
    let opts = file.ingest.as_ref().unwrap();
    assert_eq!(opts.units["a"], "m");
    assert_eq!(opts.channel_name_overrides["a"], "b");
    assert!(matches!(opts.ingest,Some(Ingest::Parquet(ref p)) if p.is_archive));
    let Some(Item::File(file)) = &encoded[1].item else {
        panic!()
    };
    assert_eq!(
        file.ingest
            .as_ref()
            .unwrap()
            .timestamp_metadata
            .as_ref()
            .unwrap()
            .column,
        "timestamps"
    );
    assert!(matches!(encoded[2].item,Some(Item::Mcap(ref m)) if m.ignore_invalid_topics));
    assert!(matches!(encoded[3].item,Some(Item::Log(ref l)) if l.channel.as_deref()==Some("log")));
    assert!(matches!(encoded[4].item, Some(Item::Dataflash(_))));
    assert!(
        matches!(encoded[5].item,Some(Item::Containerized(ref c)) if c.arguments["MODE"]=="fast" && c.sources.contains_key("INPUT"))
    );
    assert!(matches!(encoded[6].item, Some(Item::Video(_))));
    task.abort();
}

#[tokio::test]
async fn video_upload_preserves_frame_timestamps_and_removes_its_temporary_file() {
    let (client, mock, task) = fixture(0).await;
    let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
    batch
        .add_video(
            "camera.m2ts",
            "camera",
            BatchVideoTiming::FrameTimestamps(vec![-1, i64::MAX]),
        )
        .unwrap();
    let sidecar = Mutex::new(None);
    batch
        .upload_and_submit(BatchOptions::default(), |path, mime| {
            if mime == "application/json" {
                let frames: Vec<i64> =
                    serde_json::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
                assert_eq!(frames, vec![-1, i64::MAX]);
                *sidecar.lock().unwrap() = Some(path);
            }
            async move { Ok(format!("s3://{mime}")) }
        })
        .await
        .unwrap();
    assert!(!sidecar.lock().unwrap().as_ref().unwrap().exists());
    let requests = mock.requests.lock().unwrap();
    let Some(p::ingest_item::Item::Video(video)) = &requests[0].items[0].item else {
        panic!()
    };
    let Some(p::video_timestamp_manifest::Manifest::TimestampManifestFiles(frames)) = &video
        .ingest
        .as_ref()
        .unwrap()
        .timestamp_manifest
        .as_ref()
        .unwrap()
        .manifest
    else {
        panic!()
    };
    let Some(p::ingest_source::Source::S3(source)) = &frames.sources[0].source else {
        panic!()
    };
    assert_eq!(source.path, "s3://application/json");
    task.abort();
}
