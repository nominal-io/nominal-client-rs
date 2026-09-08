use nominal_extractor::*;
use std::collections::BTreeMap;
fn env(p: &std::path::Path) -> BTreeMap<String, String> {
    BTreeMap::from([("OUTPUT_DIR".into(), p.display().to_string())])
}
#[test]
fn nested_repeats_have_python_names() {
    let d = tempfile::tempdir().unwrap();
    let c = run_manifest_with_env(env(d.path()), |c| {
        for folder in ["a", "b"] {
            let dir = c.output_dir().join(folder);
            std::fs::create_dir(&dir)?;
            let p = dir.join("cam.mp4");
            std::fs::write(&p, "metadata-only fixture")?;
            for i in 0..2 {
                c.add_video(VideoOutput::new(
                    &p,
                    format!("{folder}{i}"),
                    VideoTiming::FrameTimestamps(vec![i]),
                ))?;
            }
        }
        Ok(())
    })
    .unwrap();
    let m = c.build_manifest().unwrap();
    let paths = m["videoOutputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v["timestampManifest"]["frameTimestampsRelativePath"]
                .as_str()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            "a/cam.mp4.timestamps.json",
            "a/cam.mp4.timestamps.1.json",
            "b/cam.mp4.timestamps.json",
            "b/cam.mp4.timestamps.1.json"
        ]
    );
    assert!(m["outputs"].as_array().unwrap().is_empty());
}
#[test]
fn collision_does_not_overwrite_or_record_rejected_entry() {
    let d = tempfile::tempdir().unwrap();
    let c = run_manifest_with_env(env(d.path()), |c| {
        let p = c.output_dir().join("cam.mp4");
        std::fs::write(&p, "fixture")?;
        let sidecar = c.output_dir().join("cam.mp4.timestamps.json");
        std::fs::write(&sidecar, "caller-owned")?;
        assert!(matches!(
            c.add_video(VideoOutput::new(
                &p,
                "cam",
                VideoTiming::FrameTimestamps(vec![1])
            )),
            Err(Error::SidecarCollision(_))
        ));
        assert_eq!(std::fs::read_to_string(&sidecar)?, "caller-owned");
        assert!(
            c.build_manifest()?["videoOutputs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        c.add_video(VideoOutput::new(
            &p,
            "cam",
            VideoTiming::Start {
                at: chrono::DateTime::from_timestamp_nanos(0),
                scale: None,
            },
        ))?;
        c.add_video(VideoOutput::new(
            &p,
            "frames",
            VideoTiming::FrameTimestamps(vec![1]),
        ))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        c.build_manifest().unwrap()["videoOutputs"][1]["timestampManifest"]["frameTimestampsRelativePath"],
        "cam.mp4.timestamps.1.json"
    );
}
#[test]
fn invalid_video_rejections_do_not_mutate() {
    let d = tempfile::tempdir().unwrap();
    run_manifest_with_env(env(d.path()), |c| {
        let p = c.output_dir().join("cam.mp4");
        std::fs::write(&p, "fixture")?;
        for (channel, timing) in [
            ("", VideoTiming::FrameTimestamps(vec![1])),
            ("cam", VideoTiming::FrameTimestamps(vec![])),
            (
                "cam",
                VideoTiming::Start {
                    at: chrono::DateTime::from_timestamp_nanos(0),
                    scale: Some(VideoScale::Factor(f64::NAN)),
                },
            ),
        ] {
            assert!(c.add_video(VideoOutput::new(&p, channel, timing)).is_err());
        }
        assert!(
            c.build_manifest()?["videoOutputs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        c.add_video(VideoOutput::new(
            p,
            "valid",
            VideoTiming::FrameTimestamps(vec![i64::MIN, i64::MAX]),
        ))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(d.path().join("cam.mp4.timestamps.json")).unwrap(),
        format!("[{},{}]", i64::MIN, i64::MAX)
    );
}
