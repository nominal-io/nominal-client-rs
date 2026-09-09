use nominal::core::{TimeUnit, TimestampKind};
use nominal::extractor::*;
use std::{cell::Cell, collections::BTreeMap, path::Path};

fn env(output: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([("OUTPUT_DIR".into(), output.display().to_string())])
}

#[test]
fn registered_input_and_parameter_produce_one_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.csv");
    std::fs::write(&input, "ts,value\n0,7\n").unwrap();
    let output = dir.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let mut environment = env(&output);
    environment.extend([
        (
            "_NOMINAL_INPUTS".into(),
            serde_json::json!([
                {"name":"Recording", "environmentVariable":"DATA", "path":input}
            ])
            .to_string(),
        ),
        (
            "_NOMINAL_PARAMETERS".into(),
            r#"[{"name":"Copies","environmentVariable":"COPIES","required":true}]"#.into(),
        ),
        (
            "_NOMINAL_TIMESTAMP_METADATA".into(),
            r#"{"seriesName":"ts","timestampType":{"type":"relative","relative":{"timeUnit":"NANOSECONDS"}}}"#.into(),
        ),
        ("COPIES".into(), "2".into()),
        ("UNREGISTERED".into(), "ignored".into()),
    ]);
    run_manifest_with_env(environment, |ctx| -> Result {
        let data = std::fs::read_to_string(ctx.input("Recording")?)?;
        let copies = ctx.param::<usize>("Copies")?;
        let metadata = ctx.job_timestamp_metadata()?.unwrap();
        assert_eq!(metadata.series_name(), "ts");
        assert!(matches!(
            metadata.encoding(),
            TimestampKind::Relative {
                unit: TimeUnit::Nanoseconds,
                offset: None,
            }
        ));
        assert!(ctx.optional_param::<String>("UNREGISTERED").is_err());
        let path = ctx.output_dir().join("result.csv");
        std::fs::write(&path, data.repeat(copies))?;
        ctx.add_tabular("result.csv", TabularOptions::new())?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(output.join("result.csv")).unwrap(),
        "ts,value\n0,7\nts,value\n0,7\n"
    );
}

#[test]
fn local_environment_extracts_without_registration_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.csv");
    std::fs::write(&input, "ts,value\n0,7\n").unwrap();
    let output = dir.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let mut environment = env(&output);
    environment.insert("DATA".into(), input.display().to_string());
    environment.insert("PREFIX".into(), "copied: ".into());
    run_manifest_with_env(environment, |ctx| -> Result {
        let data = std::fs::read_to_string(ctx.input("DATA")?)?;
        let prefix = ctx.optional_param::<String>("PREFIX")?.unwrap_or_default();
        let path = ctx.output_dir().join("result.csv");
        std::fs::write(&path, format!("{prefix}{data}"))?;
        ctx.add_tabular("result.csv", TabularOptions::new())?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(output.join("result.csv")).unwrap(),
        "copied: ts,value\n0,7\n"
    );
}

#[test]
fn malformed_registration_prevents_extraction() {
    let dir = tempfile::tempdir().unwrap();
    let called = Cell::new(false);
    let mut environment = env(dir.path());
    environment.insert("_NOMINAL_INPUTS".into(), "not JSON".into());
    let result = run_manifest_with_env(environment, |_| -> Result {
        called.set(true);
        Ok(())
    });
    assert!(
        matches!(result, Err(Error::Metadata { variable, .. }) if variable == "_NOMINAL_INPUTS")
    );
    assert!(!called.get());
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}
