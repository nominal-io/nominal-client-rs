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
}
