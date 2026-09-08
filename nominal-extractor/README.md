# nominal-extractor

`nominal-extractor` runs a Rust extractor function and prepares its output for
Nominal. It works with local files and does not require credentials or a network
connection. The crate is experimental.

Write files inside `ctx.output_dir()` and declare them after writing. A
single-file runner requires exactly one declaration; a manifest runner accepts
multiple declarations in order and writes `manifest.json` after the extractor
function succeeds. The same file can be declared more than once. The runner
returns errors to the caller; a `main` function that returns `Result` exits with a
nonzero status on error. Configure a `tracing` subscriber in your binary to see
runtime logs.

```rust
use nominal_extractor::{ExtractResult, ManifestContext, TabularOutput, run_manifest};

fn extract(ctx: &mut ManifestContext) -> ExtractResult {
    // Register PARTS as an optional parameter and DATA as a required file input.
    // Without PARTS, the example produces one output.
    let parts = ctx.optional_param::<usize>("PARTS")?.unwrap_or(1);
    let input = ctx.input("DATA")?;
    for part in 0..parts {
        let output = ctx.output_dir().join(format!("part-{part}.csv"));
        // Copying demonstrates output handling; it does not split the CSV.
        // Replace this step with the extraction or partitioning your format needs.
        std::fs::copy(&input, &output)?;
        // Declare each completed file. Prefixes keep its channels distinct from
        // those in other outputs; timestamps use the job or image defaults.
        ctx.add_tabular(TabularOutput::new(output).channel_prefix(format!("part-{part}/")))?;
    }
    Ok(())
}

fn main() -> nominal_extractor::Result<()> {
    // The runner writes the manifest after extract succeeds. Returning Result
    // makes an extraction error produce a nonzero process exit status.
    run_manifest(extract).map(|_| ())
}
```

`param::<T>` requires a value and parses it with `FromStr`.
`optional_param::<T>` returns `None` when the value is absent; use `unwrap_or` for a default. An empty
string is a present parameter. When registration metadata exists, registered
names and environment-variable names resolve to the same value; unknown names
fail, including optional access. Without metadata, names refer directly to
environment variables.

The output directory must exist. When Nominal supplies input metadata, `inputs()`
returns those paths in the supplied order. Otherwise, it lists files directly
inside `/input` in sorted order; set `NOMINAL_EXTRACTOR_INPUT_DIR` to use another
directory. `input(name)` returns a path, and `sole_input()` requires exactly one
input. The runner logs a warning for missing registered files or required
parameters. Looking up a registered path does not check whether the file exists.

Contexts expose `ingest_job_rid()`, `dataset_rid()`, `additional_tags()`, and
`job_timestamp_metadata()`. Job timestamps include epoch units, relative offsets,
ISO8601 and custom formats. Relative offsets retain nanosecond precision, and
unrecognized timestamp types retain their original data. Use `NumericTimestamp`
to override timestamps for an output; it supports seconds, milliseconds,
microseconds and nanoseconds. Without an override, Nominal uses the job or image
timestamp settings.

For tests, use `run_manifest_with_env(BTreeMap<String, String>, extract)` or
`run_single_file_with_env`. These functions read the supplied map without changing
the process environment. `build_manifest()` returns the JSON that the runner
writes to disk.

The output builders expose format-specific metadata:

- `TabularOutput`: CSV/Parquet (including `.gz`), tag columns, prefix and paired timestamp column/type.
- `AvroStreamOutput`: `.avro`/`.avro.gz`, prefix and timestamp type; series name is always `timestamps`.
- `JournalJsonOutput`: `.jsonl`/`.jsonl.gz` and paired timestamp column/type.
- `VideoOutput`: `.avi`, `.m2ts`, `.mkv`, `.mp4`, `.ts` and a channel plus `VideoTiming`.

Video timing is either `Start { at, scale }`, with optional ending timestamp,
true frame rate or factor, or `FrameTimestamps(Vec<i64>)`. Frame timestamps are
absolute integer nanoseconds. The runner writes them to separate JSON files
without overwriting existing files.
When a video is declared more than once, the timestamp filename includes the
number of earlier declarations for that path. This count includes declarations
that use a start time. Scale factors and rates accept zero and negative values,
as in the Python runtime. Non-finite values are rejected because JSON cannot
represent them.

Output files must stay inside the output directory after resolving symlinks.
`manifest.json` is reserved in manifest mode. After the extractor succeeds, the
runner writes the manifest and replaces any previous manifest in one filesystem
operation. Undeclared files produce warnings. Validation checks paths and metadata;
it does not decode videos or count frames. Video manifests require a sufficiently recent Nominal ingest pipeline;
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
the variables above. `PREFIX` optionally prefixes telemetry channels. With injected
registration, register `DATA` and optional `VIDEO` as file inputs, and `PREFIX`
and `VIDEO_START` as optional scalar parameters. No playable video fixture is bundled.

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

This Dockerfile uses Rust 1.85.1 and a Debian image with glibc. Add any system
libraries required by your file-format or media dependencies. Make the extractor
crate available through your source checkout or registry before building.
The Docker build and live Nominal ingestion are not part of the local test suite.

## Validation

Run `cargo test -p nominal-extractor --all-targets` to check input handling, output
files, errors and video timing. The suite also runs both examples and checks their
exit status. `cargo test -p nominal-extractor --doc` checks that unsupported output
options fail to compile. Expected manifests come from the Python runtime; the
fixture, generator and source version are in `tests/fixtures/`.

For registration, activation, SDK ingestion and nomctl commands, see the
[client guide](../docs/containerized-extractors.md).
