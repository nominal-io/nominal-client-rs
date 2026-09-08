use chrono::{DateTime, Utc};
use nominal_extractor::*;
use std::collections::BTreeMap;
fn env(p: &std::path::Path) -> BTreeMap<String, String> {
    BTreeMap::from([("OUTPUT_DIR".into(), p.display().to_string())])
}
#[test]
fn python_golden_complete_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = run_manifest_with_env(env(dir.path()), |c| {
        for name in ["data.csv", "data.avro.gz", "log.jsonl", "cam.mp4"] {
            std::fs::write(c.output_dir().join(name), "fixture")?;
        }
        let p = c.output_dir().join("data.csv");
        c.add_tabular(
            TabularOutput::new(&p)
                .tag_column("vehicle", "veh_id")
                .channel_prefix("a/")
                .timestamp(
                    "ts",
                    NumericTimestamp::Relative {
                        unit: NumericTimeUnit::Milliseconds,
                        start: DateTime::<Utc>::from_timestamp_nanos(-1),
                    },
                ),
        )?;
        c.add_tabular(TabularOutput::new(p).channel_prefix("b/"))?;
        c.add_avro_stream(
            AvroStreamOutput::new(c.output_dir().join("data.avro.gz"))
                .timestamp(NumericTimestamp::Epoch(NumericTimeUnit::Nanoseconds)),
        )?;
        c.add_journal_json(
            JournalJsonOutput::new(c.output_dir().join("log.jsonl"))
                .timestamp("t", NumericTimestamp::Epoch(NumericTimeUnit::Seconds)),
        )?;
        let p = c.output_dir().join("cam.mp4");
        for (channel, at, scale) in [
            ("start", -1, None),
            (
                "end",
                0,
                Some(VideoScale::EndingTimestamp(
                    DateTime::<Utc>::from_timestamp_nanos(1234567891),
                )),
            ),
            ("rate", 0, Some(VideoScale::TrueFrameRate(59.94))),
            ("factor", 0, Some(VideoScale::Factor(-2.0))),
        ] {
            c.add_video(VideoOutput::new(
                &p,
                channel,
                VideoTiming::Start {
                    at: DateTime::from_timestamp_nanos(at),
                    scale,
                },
            ))?;
        }
        c.add_video(VideoOutput::new(
            p,
            "frames",
            VideoTiming::FrameTimestamps(vec![-1, 1700000000123456789]),
        ))?;
        Ok(())
    })
    .unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/python_manifest.json")).unwrap();
    assert_eq!(ctx.build_manifest().unwrap(), expected);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(dir.path().join("manifest.json")).unwrap()
        )
        .unwrap(),
        expected
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("cam.mp4.timestamps.4.json")).unwrap(),
        "[-1,1700000000123456789]"
    );
}
#[test]
fn rejected_declaration_preserves_state_and_existing_manifest_replaced() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("manifest.json"), "old").unwrap();
    run_manifest_with_env(env(d.path()), |c| {
        let p = c.output_dir().join("bad.bin");
        std::fs::write(&p, "bad")?;
        assert!(c.add_tabular(TabularOutput::new(p)).is_err());
        assert!(
            c.add_journal_json(JournalJsonOutput::new(c.output_dir().join("manifest.json")))
                .is_err()
        );
        assert_eq!(c.build_manifest()?["outputs"].as_array().unwrap().len(), 0);
        let p = c.output_dir().join("DATA.CSV.GZ");
        std::fs::write(&p, "x")?;
        c.add_tabular(TabularOutput::new(p))?;
        Ok(())
    })
    .unwrap();
}
#[test]
fn four_units_and_all_supported_extensions() {
    let d = tempfile::tempdir().unwrap();
    run_manifest_with_env(env(d.path()), |c| {
        for (i, unit) in [
            NumericTimeUnit::Seconds,
            NumericTimeUnit::Milliseconds,
            NumericTimeUnit::Microseconds,
            NumericTimeUnit::Nanoseconds,
        ]
        .into_iter()
        .enumerate()
        {
            let p = c.output_dir().join(format!("{i}.csv"));
            std::fs::write(&p, "x")?;
            c.add_tabular(TabularOutput::new(p).timestamp("t", NumericTimestamp::Epoch(unit)))?;
        }
        for extension in ["csv.gz", "parquet", "parquet.gz"] {
            let p = c.output_dir().join(format!("data.{extension}"));
            std::fs::write(&p, "x")?;
            c.add_tabular(TabularOutput::new(p))?;
        }
        for extension in ["avro", "avro.gz"] {
            let p = c.output_dir().join(format!("data.{extension}"));
            std::fs::write(&p, "x")?;
            c.add_avro_stream(AvroStreamOutput::new(p))?;
        }
        for extension in ["jsonl", "jsonl.gz"] {
            let p = c.output_dir().join(format!("data.{extension}"));
            std::fs::write(&p, "x")?;
            c.add_journal_json(JournalJsonOutput::new(p))?;
        }
        let m = c.build_manifest()?;
        let units = m["outputs"].as_array().unwrap()[..4]
            .iter()
            .map(|v| v["timestampMetadata"]["epochTimeUnit"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            units,
            ["SECONDS", "MILLISECONDS", "MICROSECONDS", "NANOSECONDS"]
        );
        Ok(())
    })
    .unwrap();
}
#[test]
fn empty_manifest_fails_and_write_failure_propagates() {
    let d = tempfile::tempdir().unwrap();
    assert!(matches!(
        run_manifest_with_env(env(d.path()), |_| Ok(())),
        Err(Error::EmptyOutputs)
    ));
    std::fs::create_dir(d.path().join("manifest.json")).unwrap();
    assert!(
        run_manifest_with_env(env(d.path()), |c| {
            let p = c.output_dir().join("x.csv");
            std::fs::write(&p, "x")?;
            c.add_tabular(TabularOutput::new(p))?;
            Ok(())
        })
        .is_err()
    );
    assert!(d.path().join("manifest.json").is_dir());
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
}
