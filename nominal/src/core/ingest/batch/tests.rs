use super::*;
#[tokio::test]
async fn batch_uploads_keep_repeated_path_identities() {
    let items: Vec<_> = (0..3)
        .map(|id| PendingItem::Dataflash {
            file: PendingUpload {
                id,
                name: "file".into(),
                path: "same.bin".into(),
                mime: "application/octet-stream",
            },
            options: BatchDataflash::default(),
        })
        .collect();
    let report = upload::upload_all(&items, 2, FailurePolicy::AllowPartial, |_, _| async {
        Ok("s3://location".into())
    })
    .await;
    assert_eq!(report.locations.len(), 3);
    assert!(report.failures.is_empty());
}
#[tokio::test]
async fn batch_failed_sibling_omits_whole_item() {
    let items = vec![PendingItem::Containerized {
        sources: vec![
            PendingUpload {
                id: 0,
                name: "good".into(),
                path: "good".into(),
                mime: "application/octet-stream",
            },
            PendingUpload {
                id: 1,
                name: "bad".into(),
                path: "bad".into(),
                mime: "application/octet-stream",
            },
        ],
        options: ContainerizedIngest::new("extractor"),
    }];
    let report = upload::upload_all(
        &items,
        2,
        FailurePolicy::AllowPartial,
        |path, _| async move {
            if path == std::path::Path::new("bad") {
                Err(invalid("failed"))
            } else {
                Ok("s3://good".into())
            }
        },
    )
    .await;
    assert_eq!(report.failures.len(), 1);
    assert_eq!(
        report.failures[0].uploaded_sources,
        vec![PathBuf::from("good")]
    );
    assert_eq!(report.failures[0].failed_sources[0].name, "bad");
}
#[tokio::test]
async fn batch_upload_concurrency_is_bounded_and_fail_fast_stops_scheduling() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let items: Vec<_> = (0..8)
        .map(|id| PendingItem::Dataflash {
            file: PendingUpload {
                id,
                name: "file".into(),
                path: format!("{id}.bin").into(),
                mime: "application/octet-stream",
            },
            options: BatchDataflash::default(),
        })
        .collect();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let report = upload::upload_all(&items, 2, FailurePolicy::FailFast, |_, _| {
        let (active, peak, calls) = (active.clone(), peak.clone(), calls.clone());
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let n = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            tokio::task::yield_now().await;
            active.fetch_sub(1, Ordering::SeqCst);
            Err(invalid("failure"))
        }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(peak.load(Ordering::SeqCst) <= 2);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(report.failures.len(), 8);
}
#[tokio::test]
async fn batch_builder_validation_and_sidecar_cleanup() {
    let client = crate::core::NominalClient::builder("token")
        .base_url("http://localhost:9999/api")
        .build()
        .unwrap();
    let ingest = client.ingest();
    assert!(
        ingest
            .batch("dataset")
            .add_containerized(ContainerizedIngest::new("extractor"))
            .is_err()
    );
    assert!(
        ingest
            .batch("dataset")
            .add_journal_json(
                "a.jsonl",
                BatchJournalJson::default().timestamp(super::super::Timestamp::iso8601("t"))
            )
            .is_err()
    );
    assert!(
        ingest
            .batch("dataset")
            .add_video("a.mp4", "video", BatchVideoTiming::FrameTimestamps(vec![]))
            .is_err()
    );
    let mut batch = ingest.batch("dataset");
    batch
        .add_video(
            "a.mp4",
            "video",
            BatchVideoTiming::FrameTimestamps(vec![1, 2]),
        )
        .unwrap();
    let path = batch.sidecars[0].path().to_owned();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1,2]");
    let locations = batch.items[0]
        .uploads()
        .into_iter()
        .map(|u| (u.id, format!("s3://{}", u.name)))
        .collect();
    let encoded = encode::encode(&batch.items[0], &locations);
    use nominal_api::tonic::nominal::ingest::v2::{
        ingest_item::Item, video_timestamp_manifest::Manifest,
    };
    let Some(Item::Video(video)) = encoded.item else {
        panic!("expected video")
    };
    let Some(Manifest::TimestampManifestFiles(manifest)) =
        video.ingest.unwrap().timestamp_manifest.unwrap().manifest
    else {
        panic!("expected frame manifest")
    };
    assert_eq!(manifest.sources.len(), 1);
    drop(batch);
    assert!(!path.exists());
}
#[tokio::test]
async fn batch_all_formats_encode_complete_options() {
    use super::super::{TimeUnit, Timestamp};
    use nominal_api::tonic::nominal::ingest::v2::{file_ingest_options::Ingest, ingest_item::Item};
    let client = crate::core::NominalClient::builder("token")
        .base_url("http://localhost:9999/api")
        .build()
        .unwrap();
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
    let locations = batch
        .items
        .iter()
        .flat_map(|i| i.uploads())
        .map(|u| (u.id, format!("s3://{}", u.id)))
        .collect();
    let encoded: Vec<_> = batch
        .items
        .iter()
        .map(|i| encode::encode(i, &locations))
        .collect();
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
}
#[tokio::test]
async fn batch_python_suffix_and_fixed_format_parity() {
    let client = crate::core::NominalClient::builder("token")
        .base_url("http://localhost:9999/api")
        .build()
        .unwrap();
    let ingest = client.ingest();
    assert!(
        ingest
            .batch("dataset")
            .add_avro_stream("data.avro.gz", BatchAvroStream::default())
            .is_ok()
    );
    assert!(
        ingest
            .batch("dataset")
            .add_video(
                "video.m2ts",
                "video",
                BatchVideoTiming::Start(chrono::Utc::now())
            )
            .is_ok()
    );
    assert!(
        ingest
            .batch("dataset")
            .add_mcap("extensionless", BatchMcap::default())
            .is_ok()
    );
    assert!(
        ingest
            .batch("dataset")
            .add_ardupilot_dataflash("extensionless", BatchDataflash::default())
            .is_ok()
    );
}

#[tokio::test]
async fn batch_builds_in_a_loop_and_retains_items_after_rejected_additions() {
    let client = crate::core::NominalClient::builder("token")
        .base_url("http://localhost:1/api")
        .build()
        .unwrap();
    let mut batch = client.ingest().batch("ri.catalog.main.dataset.test");
    for path in ["first.bin", "second.bin"] {
        batch
            .add_ardupilot_dataflash(path, BatchDataflash::default())
            .unwrap();
    }
    assert!(
        batch
            .add_containerized(ContainerizedIngest::new("extractor"))
            .is_err()
    );
    assert!(
        batch
            .add_tabular(
                "bad.txt",
                BatchTabular::new(super::super::Timestamp::iso8601("time"))
            )
            .is_err()
    );
    assert!(
        batch
            .add_video("a.mp4", "camera", BatchVideoTiming::FrameTimestamps(vec![]))
            .is_err()
    );
    batch.add_tags(BTreeMap::from([("batch".into(), "loop".into())]));
    batch
        .add_ardupilot_dataflash("third.bin", BatchDataflash::default())
        .unwrap();
    let report = upload::upload_all(
        &batch.items,
        2,
        FailurePolicy::FailFast,
        |path, _| async move { Ok(format!("s3://{}", path.display())) },
    )
    .await;
    let items = upload::completed_items(&batch.items, &report, FailurePolicy::FailFast).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(
        report.locations.into_values().collect::<Vec<_>>(),
        ["s3://first.bin", "s3://second.bin", "s3://third.bin"]
    );
}
