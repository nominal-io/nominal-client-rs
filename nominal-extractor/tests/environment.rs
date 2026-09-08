use nominal_extractor::*;
use std::collections::BTreeMap;
#[test]
fn registered_parameter_name_resolves_and_parses() {
    let dir = tempfile::tempdir().unwrap();
    let env = BTreeMap::from([
        ("OUTPUT_DIR".into(), dir.path().display().to_string()),
        (
            "_NOMINAL_PARAMETERS".into(),
            r#"[{"name":"Parts","environmentVariable":"PARTS","required":true}]"#.into(),
        ),
        ("PARTS".into(), "2".into()),
    ]);
    run_single_file_with_env(env, |ctx| {
        assert_eq!(ctx.param::<usize>("Parts")?, 2);
        assert!(ctx.optional_param::<String>("UNKNOWN").is_err());
        let p = ctx.output_dir().join("out");
        std::fs::write(&p, "x")?;
        ctx.set_output(p)?;
        Ok(())
    })
    .unwrap();
}
fn inspect(
    extra: &[(&str, &str)],
    f: impl FnOnce(&SingleFileContext),
) -> Result<SingleFileContext> {
    let d = tempfile::tempdir().unwrap();
    let mut e = BTreeMap::from([("OUTPUT_DIR".into(), d.path().display().to_string())]);
    e.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    run_single_file_with_env(e, |c| {
        f(c);
        let p = c.output_dir().join("x");
        std::fs::write(&p, "x")?;
        c.set_output(p)?;
        Ok(())
    })
}
#[test]
fn registered_inputs_keep_their_order_and_exclude_unregistered_values() {
    let inputs = r#"[
        {"environmentVariable":"B","path":"/missing/b"},
        {"name":"Alpha","environmentVariable":"A","path":"/missing/a"}
    ]"#;
    inspect(&[("_NOMINAL_INPUTS", inputs)], |c| {
        assert_eq!(
            c.inputs().unwrap(),
            vec![std::path::PathBuf::from("/missing/b"), "/missing/a".into()]
        );
        assert_eq!(
            c.input("Alpha").unwrap(),
            std::path::PathBuf::from("/missing/a")
        );
        assert!(c.sole_input().is_err());
    })
    .unwrap();
    inspect(
        &[
            ("_NOMINAL_INPUTS", "[]"),
            ("A", "/exists"),
            ("_NOMINAL_PARAMETERS", "[]"),
            ("P", "x"),
        ],
        |c| {
            assert!(c.inputs().unwrap().is_empty());
            assert!(c.input("A").is_err());
            assert!(c.optional_param::<String>("P").is_err());
        },
    )
    .unwrap();
}
#[test]
fn fallback_parameters_and_discovery() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("b"), "x").unwrap();
    std::fs::write(d.path().join("a"), "x").unwrap();
    std::fs::create_dir(d.path().join("dir")).unwrap();
    inspect(
        &[
            ("NOMINAL_EXTRACTOR_INPUT_DIR", d.path().to_str().unwrap()),
            ("EMPTY", ""),
            ("BAD", "secret"),
        ],
        |c| {
            assert!(c.inputs().unwrap()[0].ends_with("a"));
            assert_eq!(c.param::<String>("EMPTY").unwrap(), "");
            assert_eq!(c.optional_param::<usize>("ABSENT").unwrap(), None);
            assert!(
                c.param::<usize>("BAD")
                    .unwrap_err()
                    .to_string()
                    .contains("BAD")
            );
        },
    )
    .unwrap();
    inspect(
        &[("NOMINAL_EXTRACTOR_INPUT_DIR", "/definitely-missing")],
        |c| assert!(c.inputs().unwrap().is_empty()),
    )
    .unwrap();
}
#[test]
fn malformed_metadata_reports_the_environment_variable() {
    for (key, raw) in [
        ("_NOMINAL_INPUTS", "{"),
        (
            "_NOMINAL_INPUTS",
            r#"[{"environmentVariable":5,"path":"x"}]"#,
        ),
        (
            "_NOMINAL_PARAMETERS",
            r#"[{"environmentVariable":"X","required":"yes"}]"#,
        ),
        ("_NOMINAL_ADDITIONAL_TAGS", r#"{"x":5}"#),
        (
            "_NOMINAL_TIMESTAMP_METADATA",
            r#"{"seriesName":"t","timestampType":{"type":"relative","relative":{"timeUnit":"SECONDS"}}}"#,
        ),
    ] {
        match inspect(&[(key, raw)], |_| {
            panic!("extractor runs with invalid metadata")
        }) {
            Err(Error::Metadata { variable, .. }) => assert_eq!(variable, key),
            _ => panic!("expected a metadata error for {key}"),
        }
    }
}
#[test]
fn complete_timestamp_inspection_and_unknown_preservation() {
    let relative = r#"{"seriesName":"t","timestampType":{"type":"relative","relative":{"timeUnit":"NANOSECONDS","offset":"1969-12-31T23:59:59.999999999Z"}}}"#;
    inspect(&[("_NOMINAL_TIMESTAMP_METADATA", relative)], |c| {
        match &c.job_timestamp_metadata().unwrap().timestamp_type {
            JobTimestampType::Relative(r) => assert_eq!(r.offset.timestamp_nanos_opt(), Some(-1)),
            _ => panic!(),
        }
    })
    .unwrap();
    for wire in [
        r#"{"type":"iso8601","iso8601":{}}"#,
        r#"{"type":"customFormat","customFormat":{"format":"yyyy-DDD","defaultYear":2026}}"#,
        r#"{"type":"epochOfTimeUnit","epochOfTimeUnit":{"timeUnit":"MICROSECONDS"}}"#,
    ] {
        let raw = format!(
            r#"{{"seriesName":"t","timestampType":{{"type":"absolute","absolute":{wire}}}}}"#
        );
        inspect(&[("_NOMINAL_TIMESTAMP_METADATA", &raw)], |c| {
            assert!(matches!(
                c.job_timestamp_metadata().unwrap().timestamp_type,
                JobTimestampType::Absolute(_)
            ))
        })
        .unwrap();
    }
    let future = r#"{"seriesName":"t","timestampType":{"type":"future","future":{"x":42}}}"#;
    inspect(&[("_NOMINAL_TIMESTAMP_METADATA", future)], |c| {
        assert!(matches!(
            &c.job_timestamp_metadata().unwrap().timestamp_type,
            JobTimestampType::Unknown { kind, .. } if kind == "future"
        ));
    })
    .unwrap();
    inspect(
        &[
            ("_NOMINAL_TIMESTAMP_METADATA", ""),
            ("_NOMINAL_DATASET_RID", ""),
            ("_NOMINAL_ADDITIONAL_TAGS", ""),
        ],
        |c| {
            assert!(c.dataset_rid().is_none());
            assert!(c.job_timestamp_metadata().is_none());
            assert!(c.additional_tags().is_empty());
        },
    )
    .unwrap();
}
#[test]
fn null_json_metadata_matches_absence() {
    inspect(
        &[
            ("_NOMINAL_INPUTS", "null"),
            ("_NOMINAL_PARAMETERS", "null"),
            ("_NOMINAL_ADDITIONAL_TAGS", "null"),
            ("_NOMINAL_TIMESTAMP_METADATA", "null"),
            ("P", "value"),
        ],
        |c| {
            assert_eq!(c.param::<String>("P").unwrap(), "value");
            assert!(c.additional_tags().is_empty());
            assert!(c.job_timestamp_metadata().is_none());
        },
    )
    .unwrap();
}
