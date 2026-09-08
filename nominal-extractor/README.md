# nominal-extractor

Experimental Rust authoring runtime for Nominal containerized extractors. It runs
synchronously against local files with no credentials, network client, Docker, or
async executor. The contract follows Python nominal-client commit
`2a4e588b47346396d4e81cd8211a37e72d52f954`.

Write files inside `ctx.output_dir()` and declare them after writing. A
single-file runner requires exactly one declaration; a manifest runner accepts
ordered, repeated declarations and writes `manifest.json` after the author
function succeeds. Author errors return through the library, and a Result-returning
`main` exits unsuccessfully. The library never installs a logging subscriber or
terminates the process.

```rust
use nominal_extractor::{ExtractResult, ManifestContext, TabularOutput, run_manifest};

fn extract(ctx: &mut ManifestContext) -> ExtractResult {
    let parts = ctx.optional_param::<usize>("PARTS")?.unwrap_or(1);
    let input = ctx.input("DATA")?;
    for part in 0..parts {
        let output = ctx.output_dir().join(format!("part-{part}.csv"));
        std::fs::copy(&input, &output)?;
        ctx.add_tabular(TabularOutput::new(output).channel_prefix(format!("part-{part}/")))?;
    }
    Ok(())
}

fn main() -> nominal_extractor::Result<()> {
    run_manifest(extract).map(|_| ())
}
```

`param::<T>` requires a value and parses it with `FromStr`.
`optional_param::<T>` preserves absence; use `unwrap_or` for a default. An empty
string is a present parameter. When registration metadata exists, registered
names and environment-variable names resolve to the same value; unknown names
fail, including optional access. Without metadata, names refer directly to
environment variables.

The output directory must exist. `inputs()` preserves injected registration order,
or discovers sorted immediate files in `/input` (overridden by
`NOMINAL_EXTRACTOR_INPUT_DIR`). `input(name)` returns an owned path and
`sole_input()` requires exactly one input. Missing registered files/required
parameters produce startup tracing warnings; lookup of a registered path itself
does not require the file to exist.

Contexts expose `ingest_job_rid()`, `dataset_rid()`, `additional_tags()`, and
`job_timestamp_metadata()`. Rich timestamp inspection distinguishes numeric epoch,
relative offsets, ISO8601, custom formats and unknown future variants. Relative
inspection preserves nanoseconds. This is separate from `NumericTimestamp`, which
is the per-output override type and supports seconds, milliseconds, microseconds,
and nanoseconds only. Omitted per-output timestamps inherit the resolved job/image
metadata; the runtime does not synthesize overrides.

For tests, use `run_manifest_with_env(BTreeMap<String, String>, extract)` or
`run_single_file_with_env`. These snapshot helpers never mutate process
environment. `build_manifest()` returns the same semantic JSON written to disk.

The output builders expose format-specific metadata:

- `TabularOutput`: CSV/Parquet (including `.gz`), tag columns, prefix and paired timestamp column/type.
- `AvroStreamOutput`: `.avro`/`.avro.gz`, prefix and timestamp type; series name is always `timestamps`.
- `JournalJsonOutput`: `.jsonl`/`.jsonl.gz` and paired timestamp column/type.
- `VideoOutput`: `.avi`, `.m2ts`, `.mkv`, `.mp4`, `.ts` and a channel plus `VideoTiming`.

Video timing is either `Start { at, scale }`, with optional ending timestamp,
true frame rate or factor, or `FrameTimestamps(Vec<i64>)`. Frame timestamps are
absolute integer nanoseconds and are written to exclusive-created sidecars.
Repeated declarations count previous successful videos for that path, including
start-timed declarations. Existing sidecars are preserved on collision. Scale
factors and rates follow Python's semantics (including zero/negative values),
except non-finite values are rejected because JSON cannot represent them.

Output containment follows canonical paths, including symlink resolution.
`manifest.json` is reserved in manifest mode; successful finalization atomically
replaces an existing runtime manifest. Scratch files only produce tracing warnings.
Local validation checks metadata and paths, not video decoding or actual frame
counts. Video manifests require a sufficiently recent Nominal ingest pipeline;
older pipelines may ignore video fields or reject video-only manifests.

## Run the examples locally

From the workspace root, with an existing CSV containing `ts,value` columns:

```sh
mkdir -p /tmp/extractor-single /tmp/extractor-manifest
DATA=/absolute/input.csv OUTPUT_DIR=/tmp/extractor-single \
  cargo run -p nominal-extractor --example single_file
DATA=/absolute/input.csv OUTPUT_DIR=/tmp/extractor-manifest \
  cargo run -p nominal-extractor --example manifest
```

The manifest example optionally copies a caller-supplied real video. Set
`VIDEO=/absolute/camera.mp4` and `VIDEO_START=2026-01-01T00:00:00Z` in addition to
the variables above. `PREFIX` optionally prefixes telemetry channels. With
injected parameter registration, register these example parameters before using
them. No playable video fixture is bundled.

## Container recipe

For a standalone binary project named `my-extractor` using this crate, place this
Dockerfile beside its Cargo.toml and committed Cargo.lock. Choose a binary name
matching the project:

```dockerfile
FROM rust:1.85.1-bookworm@sha256:e51d0265072d2d9d5d320f6a44dde6b9ef13653b035098febd68cce8fa7c0bc4 AS build
WORKDIR /app
COPY . .
RUN cargo build --locked --release --bin my-extractor

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
COPY --from=build /app/target/release/my-extractor /usr/local/bin/my-extractor
ENTRYPOINT ["/usr/local/bin/my-extractor"]
```

Official Docker Hub tag metadata confirmed both image digests on 2026-09-08.
Rust 1.85.1 supports the crate's Rust 2024/MSRV 1.85 contract. The slim image
provides glibc for this runtime; add libraries required by your chosen file-format
or media libraries. Image tag existence was verified; Docker build and platform
registration/ingestion were not run. This recipe does not claim the crate has
already been published: make the dependency available through your source checkout
or chosen registry before building.

## Validation

`cargo test -p nominal-extractor --all-targets` checks environment interpretation,
canonical containment, failure propagation, declaration transactions, sidecar
collisions, repeated outputs and all video timing modes. The test suite runs both
CSV examples as subprocesses, checking success and nonzero failure exits.
`cargo test -p nominal-extractor --doc` also checks illegal builder combinations.
The semantic golden and its Python generator/provenance are in `tests/fixtures/`.
