# Extractor Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Use `gpt-5.6-terra` with `max` reasoning.

**Goal:** Deliver a standalone, locally testable Rust analogue of both Python extractor decorators with complete manifest support.

**Architecture:** Compose both contexts from private input/environment and output-path helpers. Keep ordered manifest declarations separate from accounted-for paths. Encode output-specific metadata and video timing as types, not mode booleans.

**Tech Stack:** std, serde/serde_json, chrono, thiserror, tracing, tempfile; Rust 2024/MSRV 1.85.

---

Read the coordinator plan and approved spec first. Ownership is `nominal-extractor/src/`, `nominal-extractor/tests/`, `nominal-extractor/examples/`, and `nominal-extractor/README.md`. Package manifest and root workspace edits belong to the coordinator. P0 supplies Python-derived fixtures; this worker may add fixtures only by recording Python provenance.

## File map and public API contract

| Path under nominal-extractor/ | Responsibility |
| --- | --- |
| src/lib.rs | Public exports and crate documentation |
| src/error.rs | Typed runtime errors and author-error boundary |
| src/environment.rs | Snapshot environment, metadata decoding, discovery/name resolution |
| src/context.rs | Shared read-only context data; no manifest/single mode switch |
| src/paths.rs | Canonical containment, reserved paths, accounted-for files, atomic writes |
| src/timestamp.rs | Numeric manifest types and full injected timestamp inspection |
| src/single.rs | SingleFileContext declaration/finalization |
| src/manifest/mod.rs | ManifestContext, ordered lists, build/finalize |
| src/manifest/tabular.rs | TabularOutput, AvroStreamOutput, JournalJsonOutput builders |
| src/manifest/video.rs | VideoOutput, VideoTiming, VideoScale and sidecar creation |
| src/manifest/wire.rs | Private serde wire structs matching Python output |
| src/runner.rs | run_single_file/run_manifest and explicit-environment counterparts |
| tests/environment.rs, tests/single.rs | Input/parameter and single-output behavior |
| tests/manifest.rs, tests/video.rs | Golden output and validation behavior |
| tests/runner.rs | Author failures, diagnostics, no finalization after failure |
| examples/single_file.rs, examples/manifest.rs | Real local examples using CSV and a caller-provided video |

Keep each file cohesive and normally below 500 lines; split tests by behavior rather than creating one 1,200-line runtime test file.

Freeze these public shapes before implementation consumers begin:

```rust
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T> = std::result::Result<T, Error>;
pub type ExtractResult = std::result::Result<(), BoxError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericTimeUnit { Seconds, Milliseconds, Microseconds, Nanoseconds }

#[derive(Clone, Debug)]
pub enum NumericTimestamp {
    Epoch(NumericTimeUnit),
    Relative { unit: NumericTimeUnit, start: chrono::DateTime<chrono::Utc> },
}

#[derive(Clone, Debug)]
pub enum VideoScale {
    EndingTimestamp(chrono::DateTime<chrono::Utc>),
    TrueFrameRate(f64),
    Factor(f64),
}

#[derive(Clone, Debug)]
pub enum VideoTiming {
    Start {
        at: chrono::DateTime<chrono::Utc>,
        scale: Option<VideoScale>,
    },
    FrameTimestamps(Vec<i64>),
}
```

`Error` is a thiserror enum with contextual variants for environment metadata, unknown/missing input/parameter, parse failure, invalid output, empty outputs, format mismatch, sidecar collision, I/O/serialization, and `Author { source: BoxError }`. Do not include secrets/raw parameter values in errors unnecessarily. Required access is `param<T: FromStr>(&self, name: &str) -> Result<T>`; optional is `optional_param<T: FromStr>(&self, name: &str) -> Result<Option<T>>`. Error formatting only needs the parse error's Display; do not require all FromStr errors to implement std::error::Error.

Both contexts expose `inputs() -> Result<Vec<PathBuf>>`, `input(name: &str) -> Result<PathBuf>`, `sole_input() -> Result<PathBuf>`, `output_dir() -> &Path`, `param`, `optional_param`, and optional job/dataset/tags/timestamp inspection. Return owned input paths to avoid holding a borrow of a context while declaring outputs. Prefer explicit forwarding of these few methods to a public inheritance-like Deref interface.

`run_manifest` and `run_single_file` take `FnOnce(&mut Context) -> ExtractResult` and return `Result<Context>`. Their `_with_env` forms additionally accept an owned `BTreeMap<String, String>` snapshot. Capture process environment once in the ordinary form. Runtime `Error` converts into BoxError through Rust's standard conversion; arbitrary author errors can use `?`. Do not add a blanket generic From<E> for Error that conflicts with From<T> for T.

Output builder constructors:

```text
TabularOutput::new(path)
  .tag_column(key, column).channel_prefix(prefix)
  .timestamp(column, NumericTimestamp)
AvroStreamOutput::new(path)
  .channel_prefix(prefix).timestamp(NumericTimestamp)
JournalJsonOutput::new(path).timestamp(column, NumericTimestamp)
VideoOutput::new(path, channel, VideoTiming)
```

Context methods are `set_output(path) -> Result<PathBuf>` for single mode and `add_tabular`, `add_avro_stream`, `add_journal_json`, `add_video` returning `Result<PathBuf>` for manifest mode. `build_manifest() -> Result<serde_json::Value>` is an inspection convenience, not storage of arbitrary JSON internally.

## R1: Environment, timestamp types, errors

**Files:** src/error.rs, environment.rs, context.rs, timestamp.rs; tests/environment.rs.

- [ ] Add a failing environment test covering registered-name resolution rather than only direct environment lookup. The public scenario is:

```rust
use std::collections::BTreeMap;
use nominal_extractor::{run_single_file_with_env, ExtractResult};

#[test]
fn registered_parameter_name_resolves_and_parses() {
    let dir = tempfile::tempdir().unwrap();
    let env = BTreeMap::from([
        ("OUTPUT_DIR".into(), dir.path().display().to_string()),
        ("_NOMINAL_PARAMETERS".into(),
         r#"[{"name":"Parts","environmentVariable":"PARTS","required":true}]"#.into()),
        ("PARTS".into(), "2".into()),
    ]);
    run_single_file_with_env(env, |ctx| -> ExtractResult {
        assert_eq!(ctx.param::<usize>("Parts")?, 2);
        assert!(ctx.optional_param::<String>("UNREGISTERED").is_err());
        let out = ctx.output_dir().join("out.csv");
        std::fs::write(&out, "ts,value\n0,1\n")?;
        ctx.set_output(out)?;
        Ok(())
    }).unwrap();
}
```

- [ ] Run `cargo test -p nominal-extractor --test environment`; expect initial missing API failure. Land this public test with R2; meanwhile unit-test Environment directly in its own module so R1 can compile independently.
- [ ] Implement environment decoding with typed input/parameter metadata. Distinguish absent metadata from an explicitly empty list: an empty registered list prohibits unregistered lookup, whereas absence permits direct environment fallback.
- [ ] Add tests for metadata enumeration order, fallback sorted immediate files, missing input directory, sole-input ambiguity, empty parameter string as a present value, absent optional parameter, parse failure, and malformed JSON/field types. Input lookup returns paths; it does not prematurely turn Python's advisory missing-file warning into a failure.
- [ ] Decode optional job/dataset IDs, tags, and full timestamp metadata. Empty system metadata behaves like absence. Preserve numeric epoch/relative, ISO8601, and custom timestamp values in a typed inspection representation; retain unknown wire variants explicitly rather than defaulting them. Use numeric-only types for per-output overrides.
- [ ] Test integer nanosecond preservation, relative offsets crossing the epoch, and the four supported manifest units. Fixtures determine wire formatting, especially UTC offset formatting.
- [ ] Run `cargo test -p nominal-extractor --lib` and commit `feat: add extractor environment and timestamp contracts` with only owned files.

## R2: Runners, paths, and single-file execution

**Files:** src/paths.rs, single.rs, runner.rs, lib.rs; tests/single.rs, runner.rs; complete R1 public environment test.

- [ ] Add tests for no output, second output, nonexistent file, symlink escape, format mismatch before user code, and author failure not finalizing. Use a `Cell<bool>` closure marker to prove user code is not called after startup mismatch.
- [ ] Run `cargo test -p nominal-extractor --test single --test runner`; expect failure before implementation.
- [ ] Compose a private OutputDirectory helper holding the canonical root and accounted-for paths. Resolve existing declared files with canonicalize and compare against the root. Format manifest relative paths with forward slashes. Never rely on lexical `starts_with` against uncanonicalized user paths.
- [ ] Implement single-output state as `Option<PathBuf>`: inspect it before accepting a declaration and mutate only after validation. At completion require Some. Do not scan for a guessed output file or enforce an extension beyond Python's mode contract.
- [ ] Implement runners as direct load/check/invoke/finalize sequences. Closure failure becomes Error::Author; return the context only after finalization. Emit tracing warnings/info without installing a subscriber. Do not catch panics to report success or call process::exit from the library.
- [ ] Run public environment, single, and runner tests. Add a subprocess example-failure test proving a Result-returning main exits nonzero, and a library test proving errors return without terminating the process.
- [ ] Commit `feat: run single-file extractors with validated local contexts`.

## R3: Tabular, Avro, and log manifests

**Files:** src/manifest/mod.rs, tabular.rs, wire.rs; tests/manifest.rs.

- [ ] Add golden tests from P0 for tabular tags/prefix/timestamps, relative times, Avro's fixed `timestamps` column, and logs. Parse expected JSON with serde_json and compare Values so irrelevant object ordering is ignored; array declaration order remains significant.
- [ ] Add the deliberate-repeat regression:

```rust
#[test]
fn repeated_table_declarations_are_distinct_entries() {
    use nominal_extractor::{run_manifest_with_env, TabularOutput};
    let dir = tempfile::tempdir().unwrap();
    let env = std::collections::BTreeMap::from([
        ("OUTPUT_DIR".into(), dir.path().display().to_string()),
    ]);
    let ctx = run_manifest_with_env(env, |ctx| {
        let file = ctx.output_dir().join("data.csv");
        std::fs::write(&file, "ts,value\n0,1\n")?;
        ctx.add_tabular(TabularOutput::new(&file).channel_prefix("a/"))?;
        ctx.add_tabular(TabularOutput::new(&file).channel_prefix("b/"))?;
        Ok(())
    }).unwrap();
    assert_eq!(ctx.build_manifest().unwrap()["outputs"].as_array().unwrap().len(), 2);
}
```

- [ ] Run `cargo test -p nominal-extractor --test manifest`; expect failure until output handling exists.
- [ ] Implement separate builders with only format-relevant options. Private wire structs use exact field/enum spellings from Python goldens. Maintain ordered vectors for telemetry/video entries and a set only for stray-file accounting; never deduplicate declarations by path.
- [ ] Validate supported extensions using the Python FileType rules for each format, including gzip. Reject `manifest.json` as a declared output. A rejected declaration must not alter vectors or the accounted-for set. Compare counts before/after a caught failure in a test.
- [ ] Finalize only if at least one telemetry or video entry exists. Warn about undeclared files, serialize to a temporary sibling, and persist/rename over the runtime manifest only after a successful write. Propagate serialization/write errors. Keep per-declaration sidecar transactions separate from final manifest persistence.
- [ ] Add compile-fail doctests demonstrating that Avro cannot take a timestamp column and logs cannot take tag-column metadata; do not simulate illegal states with runtime booleans.
- [ ] Run `cargo test -p nominal-extractor --test manifest` and `cargo test -p nominal-extractor --doc`; commit `feat: emit typed telemetry extractor manifests`.

## R4: Complete video support

**Files:** src/manifest/video.rs, wire.rs; tests/video.rs. Sequential after R3 under the same owner.

- [ ] Add golden cases for start alone, end timestamp, true frame rate, scale factor, per-frame timestamps, video-only, mixed output, and nested paths. Reference Python `_context.py:add_video`, `_write_frame_timestamps`, `_video_types.py:_scale_parameter`, and all video tests.
- [ ] Add repeated naming tests: two frame declarations produce `cam.mp4.timestamps.json` and `cam.mp4.timestamps.1.json`; a start declaration followed by a frame declaration produces `.timestamps.1.json`. The index counts previous successful video declarations, not files found on disk.
- [ ] Run `cargo test -p nominal-extractor --test video`; expect failure.
- [ ] Implement the VideoTiming/VideoScale enums above. Validate empty channel, empty frame list, supported extension, path containment, and reserved name before state mutation. Serialize each i64 frame timestamp as an integer JSON array item without floating conversion. Do not claim to validate actual media frame count without decoding media.
- [ ] Use exclusive creation (`OpenOptions::create_new(true)`) for sidecars, write fully, and remove only a newly created partial file on failure. Append the entry/accounted paths only after success. Pre-existing sidecar remains byte-for-byte unchanged after rejection.
- [ ] Add a test that catches a rejected sidecar declaration, then successfully declares another output and finalizes: no rejected entry appears. Include same basename in different folders and repeated video declarations with different channels/timings.
- [ ] Match Python's supported scaling semantics rather than silently dropping an option or adding incompatible numerical restrictions based on assumption. Reject values only when necessary for JSON representation or the pinned contract and document that behavior.
- [ ] Run `cargo test -p nominal-extractor --test video`; commit `feat: support extractor videos and timestamp sidecars`.

## R5: Examples, crate boundary, runtime evidence

**Files:** examples/single_file.rs, examples/manifest.rs, README.md, tests/runner.rs.

- [ ] Write a single-file CSV passthrough example that copies a named input to output and declares it. It must run against a real temporary CSV without any format dependency.
- [ ] Write a manifest example that copies telemetry CSV plus an optional supplied video, demonstrating NumericTimestamp and VideoTiming. Do not label fake video bytes a playable sample; unit tests can use fixture bytes because they test metadata only.
- [ ] Document `ExtractResult` in the author function and runtime `Result<()>` in main, using `run_manifest(extract).map(|_| ())`. Show typed parameters, explicit injected environment for tests, and runtime metadata inspection.
- [ ] Add a multi-stage Dockerfile recipe in README with a pinned Rust image compatible with the project's MSRV selected during implementation, release build, and a slim runtime containing only the binary and any required system libraries. Verify chosen images before claiming availability.
- [ ] Run `cargo test -p nominal-extractor --all-targets`, `cargo test -p nominal-extractor --doc`, `cargo tree -p nominal-extractor --edges normal`, and `cargo fmt --all -- --check`.
- [ ] Return runtime commit hashes, passing case counts, fixture provenance, dependency-tree evidence, and any unrun Docker checks. Commit `docs: demonstrate local Rust extractor authoring`.
