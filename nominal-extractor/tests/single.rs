use nominal_extractor::*;
use std::{cell::Cell, collections::BTreeMap};
fn env(p: &std::path::Path) -> BTreeMap<String, String> {
    BTreeMap::from([("OUTPUT_DIR".into(), p.display().to_string())])
}
#[test]
fn no_output_and_second_output() {
    let d = tempfile::tempdir().unwrap();
    assert!(matches!(
        run_single_file_with_env(env(d.path()), |_| Ok(())),
        Err(Error::EmptyOutputs)
    ));
    run_single_file_with_env(env(d.path()), |c| {
        let p = c.output_dir().join("any.extension");
        assert!(c.set_output(&p).is_err());
        std::fs::write(&p, "data")?;
        c.set_output(&p)?;
        assert!(c.set_output(&p).is_err());
        Ok(())
    })
    .unwrap();
}
#[test]
fn mismatch_precedes_author_and_author_failure_does_not_finalize() {
    let d = tempfile::tempdir().unwrap();
    let called = Cell::new(false);
    let mut e = env(d.path());
    e.insert("_NOMINAL_OUTPUT_FORMAT".into(), "MANIFEST".into());
    assert!(matches!(
        run_single_file_with_env(e, |_| {
            called.set(true);
            Ok(())
        }),
        Err(Error::FormatMismatch(_))
    ));
    assert!(!called.get());
    assert!(matches!(
        run_manifest_with_env(env(d.path()), |c| {
            let p = c.output_dir().join("x.csv");
            std::fs::write(&p, "x")?;
            c.add_tabular(TabularOutput::new(p))?;
            Err(std::io::Error::other("author failure").into())
        }),
        Err(Error::Author { .. })
    ));
    assert!(!d.path().join("manifest.json").exists());
}
#[cfg(unix)]
#[test]
fn symlink_escape_rejected() {
    let d = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let p = d.path().join("escape.csv");
    std::os::unix::fs::symlink(outside.path(), &p).unwrap();
    run_single_file_with_env(env(d.path()), |c| {
        assert!(c.set_output(&p).is_err());
        let good = c.output_dir().join("good");
        std::fs::write(&good, "x")?;
        c.set_output(good)?;
        Ok(())
    })
    .unwrap();
}
