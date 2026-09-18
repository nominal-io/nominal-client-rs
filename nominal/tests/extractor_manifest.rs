use nominal::core::{TimeUnit, Timestamp};
use nominal::extractor::*;
use std::{collections::BTreeMap, path::Path};

fn env(output: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([("OUTPUT_DIR".into(), output.display().to_string())])
}
fn manifest(output: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(output.join("manifest.json")).unwrap()).unwrap()
}

#[test]
fn extraction_writes_multiple_outputs_and_video_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    run_manifest_with_env(env(dir.path()), |ctx| -> Result {
        let table = ctx.output_dir().join("data.csv");
        std::fs::write(&table, "ts,value\n0,7\n")?;
        ctx.add_tabular(
            "data.csv",
            TabularOptions::new()
                .channel_prefix("engine/")
                .timestamp(Timestamp::epoch("ts", TimeUnit::Nanoseconds)),
        )?;
        let logs = ctx.output_dir().join("logs.jsonl");
        std::fs::write(&logs, "{\"MESSAGE\":\"ready\"}\n")?;
        // Use the same relative timestamp builder as client ingestion. Preserve
        // subsecond precision even when the reference instant predates the epoch.
        ctx.add_journal_json(
            "logs.jsonl",
            JournalJsonOptions::new().timestamp(
                Timestamp::relative("ts", TimeUnit::Nanoseconds)
                    .with_offset(chrono::DateTime::from_timestamp_nanos(-1)),
            ),
        )?;
        // The runtime declares videos; it does not decode their contents.
        std::fs::write(ctx.output_dir().join("camera.mp4"), "video bytes")?;
        ctx.write_frame_timestamps("camera.frames.json", &[-1, 1_700_000_000_123_456_789])?;
        ctx.add_video(
            "camera.mp4",
            "camera",
            VideoOptions::frame_timestamps("camera.frames.json"),
        )?;
        Ok(())
    })
    .unwrap();
    let written = manifest(dir.path());
    let outputs = written["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0]["relativePath"], "data.csv");
    assert_eq!(outputs[0]["channelPrefix"], "engine/");
    assert_eq!(outputs[0]["timestampMetadata"]["seriesName"], "ts");
    assert_eq!(
        outputs[0]["timestampMetadata"]["epochTimeUnit"],
        "NANOSECONDS"
    );
    assert_eq!(outputs[1]["relativePath"], "logs.jsonl");
    assert_eq!(
        outputs[1]["timestampMetadata"]["relativeOffset"],
        "1969-12-31T23:59:59.999999999Z"
    );
    let video = &written["videoOutputs"][0];
    assert_eq!(video["channel"], "camera");
    let sidecar = video["timestampManifest"]["frameTimestampsRelativePath"]
        .as_str()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join(sidecar)).unwrap(),
        "[-1,1700000000123456789]"
    );
    assert!(dir.path().join("data.csv").is_file());
    assert!(dir.path().join("logs.jsonl").is_file());
    assert!(dir.path().join("camera.mp4").is_file());
}

#[test]
fn callback_failure_does_not_publish_a_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let result = run_manifest_with_env(env(dir.path()), |ctx| -> std::io::Result<()> {
        let path = ctx.output_dir().join("data.csv");
        std::fs::write(&path, "ts,value\n0,7\n")?;
        ctx.add_tabular("data.csv", TabularOptions::new()).unwrap();
        Err(std::io::Error::other("extraction failed"))
    });
    assert!(matches!(result, Err(Error::Author { .. })));
    assert!(!dir.path().join("manifest.json").exists());
}

#[test]
fn output_outside_the_directory_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::with_suffix(".csv").unwrap();
    let result = run_manifest_with_env(env(dir.path()), |ctx| -> Result {
        ctx.add_tabular(outside.path(), TabularOptions::new())?;
        Ok(())
    });
    assert!(result.is_err());
    #[cfg(unix)]
    {
        let alias = dir.path().join("alias.csv");
        std::os::unix::fs::symlink(outside.path(), &alias).unwrap();
        let result = run_manifest_with_env(env(dir.path()), |ctx| -> Result {
            ctx.add_tabular("alias.csv", TabularOptions::new())?;
            Ok(())
        });
        assert!(result.is_err());
    }
    assert!(!dir.path().join("manifest.json").exists());
}

#[test]
fn timestamp_collision_keeps_the_callers_file_and_allows_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let sidecar = dir.path().join("camera.mp4.timestamps.json");
    std::fs::write(&sidecar, "caller data").unwrap();
    run_manifest_with_env(env(dir.path()), |ctx| -> Result {
        let video = ctx.output_dir().join("camera.mp4");
        std::fs::write(&video, "video bytes")?;
        assert!(matches!(
            ctx.write_frame_timestamps("camera.mp4.timestamps.json", &[1]),
            Err(Error::SidecarCollision(_))
        ));
        ctx.add_video(
            "camera.mp4",
            "camera",
            VideoOptions::starting_at(chrono::DateTime::from_timestamp_nanos(0)),
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(sidecar).unwrap(), "caller data");
    let written = manifest(dir.path());
    let videos = written["videoOutputs"].as_array().unwrap();
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0]["channel"], "camera");
}

#[test]
fn repeated_video_declarations_keep_separate_frame_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    run_manifest_with_env(env(dir.path()), |ctx| -> Result {
        let video = ctx.output_dir().join("camera.mp4");
        std::fs::write(&video, "video bytes")?;
        ctx.write_frame_timestamps("first.frames.json", &[1])?;
        ctx.write_frame_timestamps("second.frames.json", &[2])?;
        ctx.add_video(
            "camera.mp4",
            "first",
            VideoOptions::frame_timestamps("first.frames.json"),
        )?;
        ctx.add_video(
            "camera.mp4",
            "second",
            VideoOptions::frame_timestamps("second.frames.json"),
        )?;
        Ok(())
    })
    .unwrap();
    let written = manifest(dir.path());
    let videos = written["videoOutputs"].as_array().unwrap();
    assert_eq!(videos.len(), 2);
    let first = videos[0]["timestampManifest"]["frameTimestampsRelativePath"]
        .as_str()
        .unwrap();
    let second = videos[1]["timestampManifest"]["frameTimestampsRelativePath"]
        .as_str()
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(first)).unwrap(),
        "[1]"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(second)).unwrap(),
        "[2]"
    );
}

#[test]
fn unsupported_timestamp_settings_do_not_publish_a_manifest() {
    // The client accepts more timestamp formats than output manifests support.
    // Reject settings that cannot retain their meaning in a manifest.
    for timestamp in [
        Timestamp::iso8601("ts"),
        Timestamp::custom("ts", "yyyy-MM-dd"),
        Timestamp::epoch("ts", TimeUnit::Minutes),
        Timestamp::epoch("ts", TimeUnit::Hours),
        Timestamp::epoch("ts", TimeUnit::Days),
        Timestamp::relative("ts", TimeUnit::Seconds),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let result = run_manifest_with_env(env(dir.path()), |ctx| -> Result {
            let file = ctx.output_dir().join("data.csv");
            std::fs::write(&file, "ts,value\n0,1\n")?;
            ctx.add_tabular("data.csv", TabularOptions::new().timestamp(timestamp))?;
            Ok(())
        });
        assert!(result.is_err());
        assert!(!dir.path().join("manifest.json").exists());
    }
}

#[test]
fn avro_requires_its_timestamp_field_and_allows_a_corrected_declaration() {
    let dir = tempfile::tempdir().unwrap();
    run_manifest_with_env(env(dir.path()), |ctx| -> Result {
        let file = ctx.output_dir().join("data.avro");
        // The runtime validates declarations, not Avro records.
        std::fs::write(&file, "avro bytes")?;
        let wrong = AvroStreamOptions::new()
            .timestamp(Timestamp::epoch("wrong_field", TimeUnit::Microseconds));
        assert!(ctx.add_avro_stream("data.avro", wrong).is_err());
        ctx.add_avro_stream(
            "data.avro",
            AvroStreamOptions::new()
                .timestamp(Timestamp::epoch("timestamps", TimeUnit::Microseconds)),
        )?;
        Ok(())
    })
    .unwrap();
    let written = manifest(dir.path());
    assert_eq!(written["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(
        written["outputs"][0]["timestampMetadata"]["seriesName"],
        "timestamps"
    );
    assert_eq!(
        written["outputs"][0]["timestampMetadata"]["epochTimeUnit"],
        "MICROSECONDS"
    );
}

#[cfg(unix)]
#[test]
fn video_sidecars_cannot_escape_through_symlinks() {
    let output = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), output.path().join("escape")).unwrap();
    let result = run_manifest_with_env(env(output.path()), |ctx| -> Result {
        assert!(ctx.write_frame_timestamps("escape/new.json", &[1]).is_err());
        assert!(!outside.path().join("new.json").exists());
        std::fs::write(outside.path().join("existing.json"), "[1]")?;
        std::fs::write(ctx.output_dir().join("camera.mp4"), "video bytes")?;
        ctx.add_video(
            "camera.mp4",
            "camera",
            VideoOptions::frame_timestamps("escape/existing.json"),
        )?;
        Ok(())
    });
    assert!(result.is_err());
    assert!(!output.path().join("manifest.json").exists());
}

#[test]
fn invalid_video_timing_does_not_publish_a_manifest() {
    let start = chrono::DateTime::from_timestamp_nanos(0);
    for options in [
        VideoOptions::starting_at(start).frame_rate(f64::NAN),
        VideoOptions::starting_at(start).scale_factor(f64::INFINITY),
        VideoOptions::frame_timestamps("frames.json").ending_at(start),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let result = run_manifest_with_env(env(dir.path()), |ctx| -> Result {
            std::fs::write(ctx.output_dir().join("camera.mp4"), "video bytes")?;
            ctx.add_video("camera.mp4", "camera", options)?;
            Ok(())
        });
        assert!(result.is_err());
        assert!(!dir.path().join("manifest.json").exists());
    }
}
