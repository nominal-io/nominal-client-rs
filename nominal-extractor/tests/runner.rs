use std::process::Command;
#[test]
fn examples_execute_csv_and_propagate_failure_as_nonzero_exit() {
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let build = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "nominal-extractor",
            "--examples",
            "--offline",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let d = tempfile::tempdir().unwrap();
    let input = d.path().join("input.csv");
    std::fs::write(&input, "ts,value\n0,1\n").unwrap();
    for example in ["single_file", "manifest"] {
        let out = d.path().join(example);
        std::fs::create_dir(&out).unwrap();
        let binary = target.join("debug/examples").join(example);
        let success = Command::new(&binary)
            .env_clear()
            .env("OUTPUT_DIR", &out)
            .env("DATA", &input)
            .output()
            .unwrap();
        assert!(
            success.status.success(),
            "{}",
            String::from_utf8_lossy(&success.stderr)
        );
        assert_eq!(
            std::fs::read(out.join("data.csv")).unwrap(),
            std::fs::read(&input).unwrap()
        );
        let failure = Command::new(binary)
            .env_clear()
            .env("OUTPUT_DIR", &out)
            .output()
            .unwrap();
        assert!(!failure.status.success());
        assert!(String::from_utf8_lossy(&failure.stderr).contains("DATA"));
    }
    // Platform-injected input metadata is authoritative; VIDEO is a file input,
    // not a scalar parameter. These bytes test declaration plumbing, not decoding.
    let video = d.path().join("camera.mp4");
    std::fs::write(&video, b"metadata-only fixture").unwrap();
    let output = d.path().join("registered");
    std::fs::create_dir(&output).unwrap();
    let result = Command::new(target.join("debug/examples/manifest"))
        .env_clear()
        .env("OUTPUT_DIR", &output)
        .env("_NOMINAL_INPUTS", serde_json::json!([
            {"environmentVariable":"DATA", "path":input},
            {"environmentVariable":"VIDEO", "path":video}
        ]).to_string())
        .env("_NOMINAL_PARAMETERS", r#"[{"environmentVariable":"PREFIX","required":false},{"environmentVariable":"VIDEO_START","required":false}]"#)
        .env("VIDEO_START", "2026-01-01T00:00:00Z")
        .output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("camera.mp4").is_file());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["videoOutputs"].as_array().unwrap().len(), 1);
}
