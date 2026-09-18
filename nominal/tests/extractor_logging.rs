use nominal::extractor::{ManifestContext, TabularOptions, run_manifest_with_env};
use std::collections::BTreeMap;

#[test]
fn lifecycle_logs_explain_success_and_failure() {
    let logs = tempfile::tempfile().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(logs.try_clone().unwrap())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let output = tempfile::tempdir().unwrap();
    let env = BTreeMap::from([("OUTPUT_DIR".into(), output.path().display().to_string())]);
    run_manifest_with_env(
        env.clone(),
        |ctx: &mut ManifestContext| -> nominal::extractor::Result {
            let file = ctx.output_dir().join("data.csv");
            std::fs::write(&file, "ts,value\n0,1\n")?;
            ctx.add_tabular("data.csv", TabularOptions::new())?;
            Ok(())
        },
    )
    .unwrap();
    let result = run_manifest_with_env(env, |_| Err(std::io::Error::other("decoder failed")));
    assert!(result.is_err());

    use std::io::{Read, Seek};
    let mut logs = logs;
    logs.rewind().unwrap();
    let mut text = String::new();
    logs.read_to_string(&mut text).unwrap();
    assert!(text.contains("starting manifest extractor"), "{text}");
    assert!(text.contains("wrote extractor manifest"), "{text}");
    assert!(text.contains("outputs=1"), "{text}");
    assert_eq!(text.matches("extractor completed").count(), 1, "{text}");
    assert!(text.contains("extractor failed"), "{text}");
    assert!(text.contains("decoder failed"), "{text}");
}
