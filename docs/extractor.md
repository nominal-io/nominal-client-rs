# Rust extractors

`nominal::extractor` runs a Rust extractor function and prepares its output for
Nominal. The runtime works with local files and does not require credentials or a
network connection. It is experimental and is included in the `nominal` SDK.
The runtime serializes the manifest in the format Nominal expects.

Write files inside `ctx.output_dir()` and declare them after writing. The
runner accepts one or more declarations in order and writes `manifest.json` after
the extractor function succeeds. The same file can be declared more than once. The runner
returns errors to the caller; a `main` function that returns `Result` exits with a
nonzero status on error. The runtime emits `tracing` events for startup, manifest
output counts, completion and failures. Your application configures logging; the
example below enables timestamped INFO logs on stderr. Enable DEBUG in your
subscriber to see individual output declarations. Register images built with
this module with the `manifest` output format, including extractors that produce
only one file.

```rust
use nominal::extractor::{ManifestContext, TabularOptions, VideoOptions, run_manifest};
use nominal::core::{TimeUnit, Timestamp};

fn extract(ctx: &mut ManifestContext) -> nominal::extractor::Result {
    // Register PREFIX as an optional parameter. An absent value leaves channel
    // names unchanged; a prefix keeps channels from different sources distinct.
    let prefix = ctx.optional_param::<String>("PREFIX")?.unwrap_or_default();

    // This example copies a CSV; replace the copy with your format conversion.
    // Write inside the output directory so Nominal can collect the result.
    let output = ctx.output_dir().join("data.csv");
    std::fs::copy(ctx.input("DATA")?, &output)?;

    // Declaring the file adds it to the manifest. Here the ts column contains
    // nanoseconds since the Unix epoch, overriding the image timestamp default.
    ctx.add_tabular(
        "data.csv",
        TabularOptions::new()
            .channel_prefix(prefix)
            .timestamp(Timestamp::epoch("ts", TimeUnit::Nanoseconds)),
    )?;

    // VIDEO is an optional file input, not a scalar parameter. A missing input
    // leaves this run with CSV output only; other errors still fail the run.
    let video = match ctx.input("VIDEO") {
        Ok(input) => Some(input),
        Err(nominal::extractor::Error::Input(_)) => None,
        Err(error) => return Err(error),
    };
    if let Some(input) = video {
        // The video needs a start time to align it with telemetry. Parse the
        // registered VIDEO_START parameter as an RFC3339 timestamp.
        let start = ctx.param::<chrono::DateTime<chrono::Utc>>("VIDEO_START")?;
        let name = input.file_name().and_then(|name| name.to_str())
            .ok_or_else(|| std::io::Error::other("VIDEO has no UTF-8 file name"))?;
        let output = ctx.output_dir().join(name);
        std::fs::copy(&input, &output)?;

        // Use the frame timing in the media file, starting at the supplied time.
        ctx.add_video(name, "camera", VideoOptions::starting_at(start))?;
    }
    Ok(())
}

fn main() -> nominal::extractor::Result {
    // Configure logging once at the application entrypoint. Send logs to stderr
    // so they appear in container logs, and disable terminal color escapes.
    // If your application already installs a subscriber, use that setup instead.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init(); // The default maximum level is INFO.

    // Write manifest.json only after all extraction steps succeed. Returning
    // errors from main lets Nominal detect a failed container run.
    run_manifest(extract)
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
`job_timestamp_metadata()`. The timestamp accessor returns
`Result<Option<nominal::core::Timestamp>>`: absent metadata returns `Ok(None)`,
while an unsupported encoding returns an error. Use `metadata.series_name()` and
`metadata.encoding()` to inspect the column and `nominal::core::TimestampKind`.
The shared timestamp type supports all known epoch, relative, ISO8601 and custom
formats. Relative offsets may be absent in job metadata; supplied offsets retain
nanosecond precision. Without an output override, Nominal uses the job or image
timestamp settings.

Set an output override with `.timestamp(Timestamp::epoch("ts", TimeUnit::Nanoseconds))`.
For relative timestamps, use
`.timestamp(Timestamp::relative("ts", TimeUnit::Milliseconds).with_offset(start))`,
where `start` is a `chrono::DateTime<chrono::Utc>`. Manifest overrides support only
seconds, milliseconds, microseconds and nanoseconds. The `ctx.add_*` methods
reject ISO8601, custom formats, minutes, hours, days and relative timestamps
without an explicit offset. `ctx.add_avro_stream` also requires the timestamp
series to be `timestamps`.

For tests, use `run_manifest_with_env(BTreeMap<String, String>, extract)`. It reads
the supplied map without changing the process environment or configuring logging.
Both runners return `Ok(())` on success. Callbacks can use `nominal::extractor::Result`
or return their own error type implementing `std::error::Error + Send + Sync + 'static`,
so decoder errors can propagate with `?`. `build_manifest()` returns the JSON that the runner writes to disk.

Pass a path relative to the output directory and options to the declaration method:

- `ctx.add_tabular(path, TabularOptions)`: CSV/Parquet (including `.gz`), tag columns, prefix and timestamp override.
- `ctx.add_avro_stream(path, AvroStreamOptions)`: `.avro`/`.avro.gz`, prefix and timestamp override with series name `timestamps`.
- `ctx.add_journal_json(path, JournalJsonOptions)`: `.jsonl`/`.jsonl.gz` and timestamp override.
- `ctx.add_video(path, channel, VideoOptions)`: `.avi`, `.m2ts`, `.mkv`, `.mp4` or `.ts` files and video timing.

Use `VideoOptions::starting_at(start)` with a `chrono::DateTime<chrono::Utc>` to
use the timing from the media file. To adjust playback timing, add `.ending_at(end)`,
`.frame_rate(rate)`, or `.scale_factor(factor)`. These adjustments require
`starting_at`; they are invalid with explicit frame timestamps. Scale factors
and rates accept zero and negative values, as in the Python runtime. Non-finite
values are rejected because JSON cannot represent them.

For explicit frame timestamps, write a named sidecar and reference it in the
video options:

```rust
use nominal::extractor::{ManifestContext, VideoOptions};

fn declare_camera(ctx: &mut ManifestContext, timestamps: &[i64]) -> nominal::extractor::Result {
    // camera.mp4 is already written inside ctx.output_dir().
    ctx.write_frame_timestamps("camera.frames.json", timestamps)?;
    ctx.add_video(
        "camera.mp4", "camera", VideoOptions::frame_timestamps("camera.frames.json"),
    )?;
    Ok(())
}
```

Frame timestamps are absolute integer nanoseconds. `write_frame_timestamps`
writes the caller-named JSON file inside the output directory and returns `Ok(())`.
It does not overwrite an existing file. Choose a new sidecar name when declaring
a second timestamp sequence; names are not generated automatically.

Output files must stay inside the output directory after resolving symlinks.
`manifest.json` is reserved in manifest mode. After the extractor succeeds, the
runner writes the manifest and replaces any previous manifest in one filesystem
operation. Undeclared files produce warnings. Validation checks paths and metadata;
it does not decode videos or count frames. Video manifests require a sufficiently recent Nominal ingest pipeline;
older pipelines may ignore video fields or reject video-only manifests.

## Run the example locally

Copy the Rust example above into `src/main.rs` of a binary project named
`my-extractor`, with `nominal` and `chrono` dependencies. Add
`tracing-subscriber = { version = "0.3", default-features = false, features = ["fmt"] }`
to enable the logging setup shown above. Make the `nominal` dependency available
through your source checkout or registry. From that project, with
an existing CSV containing `ts,value` columns:

```sh
mkdir -p /tmp/extractor-manifest
DATA=/absolute/input.csv OUTPUT_DIR=/tmp/extractor-manifest \
  cargo run --bin my-extractor
```

The manifest example optionally copies a caller-supplied real video. Set
`VIDEO=/absolute/camera.mp4` and `VIDEO_START=2026-01-01T00:00:00Z` in addition to
the variables above. `PREFIX` optionally prefixes telemetry channels. With injected
registration, register `DATA` and optional `VIDEO` as file inputs, and `PREFIX`
and `VIDEO_START` as optional scalar parameters. No playable video fixture is bundled.

## Container recipe

For a standalone binary project named `my-extractor` using `nominal::extractor`, place this
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
libraries required by your file-format or media dependencies. Make `nominal`
available through your source checkout or registry before building.
The Docker build and live Nominal ingestion are not part of the local test suite.

## Register the example container

Registration runs outside the container using the `nominal` SDK. The runtime inside
it only reads inputs and writes outputs. The registration routes are introduced in
[the companion management PR](https://github.com/nominal-io/nominal-client-rs/pull/162).

Build the `my-extractor` project from the example above using the container
recipe above, then save the image before registering it:

```sh
# Run beside the Dockerfile for your extractor binary.
docker build -t my-extractor:v1 .
# The registration API uploads a Docker image archive, not a registry URL.
docker save my-extractor:v1 -o my-extractor-v1.tar
```

This contract matches the names used by the manifest example:

```rust,no_run
use nominal::core::{
    Activation, ExtractorCreate, FileExtractionInput, FileExtractionParameter,
    ImageRegistration, NominalClient, RegisterableOutputFormat, TimeUnit,
    Timestamp, WaitOptions,
};
use std::path::Path;

async fn register_example(client: &NominalClient) -> nominal::Result<()> {
    let extractors = client.extractors();
    // Create the stable extractor once. For later builds, get it by its saved RID.
    let extractor = extractors.create(ExtractorCreate::new("telemetry-and-video")).await?;
    println!("extractor: {}", extractor.rid());

    // Every Rust extractor writes a manifest, even if it produces only a CSV.
    // Registration requires a timestamp default. The example also declares its
    // CSV timestamps explicitly, so its output settings take precedence.
    let contract = ImageRegistration::new(
        "v1",
        RegisterableOutputFormat::Manifest,
        Timestamp::epoch("ts", TimeUnit::Nanoseconds),
    )
    // DATA and VIDEO are file inputs: Nominal supplies their local paths.
    .input(FileExtractionInput::new("Telemetry", "DATA").required(true))
    .input(FileExtractionInput::new("Camera video", "VIDEO").required(false))
    // Scalar parameters arrive as strings. PREFIX can be omitted; VIDEO_START
    // is required by the example only when a VIDEO input is supplied.
    .parameter(FileExtractionParameter::new("Channel prefix", "PREFIX").required(false))
    .parameter(FileExtractionParameter::new("Video start time", "VIDEO_START").required(false));

    // Use a new tag for each build. Uploading an image does not activate it.
    let image = client.container_images()
        .register(&extractor, Path::new("my-extractor-v1.tar"), contract).await?;
    println!("image: {}", image.rid());

    // Wait until Nominal has prepared the image, then make it the version used
    // by subsequent jobs for this extractor.
    extractors.activate(&extractor, &image, Activation::Wait(WaitOptions::default())).await?;
    Ok(())
}
```

Call `register_example` with an authenticated `NominalClient` configured for your
workspace. Save the extractor RID and reuse it with `client.extractors().get(rid)`
for subsequent image registrations. When submitting data, use `DATA` and optional
`VIDEO` as source keys, and `PREFIX` and `VIDEO_START` as argument keys. A
`VIDEO_START` value such as `2026-01-01T00:00:00Z` supplies the video's UTC start time.

## Validation

Run `cargo test -p nominal --all-targets` to check extraction with registered
and local inputs, manifest publication, and video timestamp
files. Tests inspect the files an extractor writes and check that invalid inputs or
failed callbacks do not publish a manifest. They do not decode video or contact Nominal.
