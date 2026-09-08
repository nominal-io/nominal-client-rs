# Containerized extractors

Use `nominal` to register and run extractors in Nominal. Use the separate
[`nominal-extractor`](../nominal-extractor/README.md) crate inside your image to
handle inputs, parameters, output declarations, manifests, and video sidecars.
The authoring runtime and v2 batch contract are experimental.

## Register an image

Create an extractor, upload a Docker-save tarball, then activate its image:

```rust,no_run
use nominal::core::{
    Activation, ExtractorCreate, FileExtractionInput, FileExtractionParameter,
    ImageRegistration, NominalClient, RegisterableOutputFormat, TimeUnit,
    Timestamp, WaitOptions,
};
use std::path::Path;

async fn register(client: &NominalClient) -> nominal::Result<()> {
    let extractors = client.extractors();
    let extractor = extractors.create(ExtractorCreate::new("flight-recorder")).await?;
    let contract = ImageRegistration::new(
        "v1", RegisterableOutputFormat::Manifest,
        Timestamp::epoch("ts", TimeUnit::Nanoseconds),
    )
    .input(FileExtractionInput::new("Recording", "RECORDING").suffix("flight").required(true))
    .parameter(FileExtractionParameter::new("Parts", "PARTS"));
    let image = client.container_images()
        .register(&extractor, Path::new("flight-recorder-v1.tar"), contract).await?;
    extractors.activate(&extractor, &image, Activation::Wait(WaitOptions::default())).await?;
    Ok(())
}
```

Tags are immutable within an extractor: use a new tag for a new image.
Registration does not change the active image. Default timestamp metadata is
required even when the manifest supplies per-output overrides. Select `Manifest`
for a manifest runner, or `Csv`, `Parquet`, or `AvroStream` for a single-file runner.

Resource clients support create/get/search/update/archive/unarchive, image
get/search/delete, readiness waiting, and activation. Searches follow pagination.
Use `.in_workspace(rid)` to select a workspace explicitly. Image and extractor
snapshots retain their workspace for subsequent lifecycle operations.

## Submit and inspect

```rust,no_run
use nominal::core::{ContainerizedIngest, DatasetTarget, NominalClient, WaitOptions};

async fn ingest(client: &NominalClient, extractor: &str, dataset: &str) -> nominal::Result<()> {
    let submission = client.ingest().upload_containerized(
        DatasetTarget::Existing(dataset.into()),
        ContainerizedIngest::new(extractor)
            .source("RECORDING", "flight-42.flight")
            .argument("PARTS", "4")
            .tag("vehicle", "n1234"),
    ).await?;
    println!("job: {}", submission.job().rid());
    let files = client.ingest()
        .wait_for_job_files(submission.job().rid(), WaitOptions::default()).await?;
    println!("{} output files", files.len());
    Ok(())
}
```

Direct ingest also accepts `DatasetTarget::New(DatasetCreate::new(name))` to create
the destination as part of the ingest request. Acknowledgement returns the job RID
immediately; fetch its metadata separately with `get_ingest_job`. This preserves
successful submission when a later metadata read fails. Mutating submissions do
not automatically replay ambiguous failures.

Source keys are registered environment-variable names. Direct ingestion allows no
sources if the active image requires none; batch extractor items require at least
one. `with_scope_tags` merges caller-supplied scope defaults below explicit tags.
Its API documentation includes a compiling workbook → run → attached dataset
example. The current workbook API exposes asset/run scopes; dataset-view tag
filters are not exposed, so supply their tags explicitly.

`dataset_files(job_rid)` is a current snapshot. `wait_for_job_files` waits for the
job to complete, then discovers and waits for its files. For snapshot-at-call
behavior, combine `dataset_files` with `catalog().wait_for_dataset_files`.
Job search supports dataset/creator/status filters, path text, time bounds and
default/specific/all workspace selection. Cancellation returns updated job state.

## Batch ingestion

```rust,no_run
use nominal::core::{BatchOptions, ContainerizedIngest, NominalClient};

async fn batch(client: &NominalClient, extractor: &str, dataset: &str) -> nominal::Result<()> {
    let batch = client.ingest().batch(dataset)
        .add_containerized(ContainerizedIngest::new(extractor).source("RECORDING", "one.flight"))?
        .add_containerized(ContainerizedIngest::new(extractor).source("RECORDING", "two.flight"))?;
    let submission = batch.submit(BatchOptions::default()).await?;
    println!("job: {}", submission.job.rid());
    Ok(())
}
```

A batch requires an existing dataset. It can mix extractor, CSV/Parquet, Avro,
MCAP, journal JSON, DataFlash and video items. Format-specific builders expose
timestamps, tags, units, channel names or selection where supported. Batch videos
accept start timing or a per-frame timestamp vector; the authoring runtime also
supports video scaling.

`submit(self)` consumes the batch. Uploads use bounded file concurrency (default
four), each with the multipart uploader's own part concurrency. Default failure
policy stops scheduling, settles in-flight uploads, and submits nothing. It does
not roll back already uploaded objects. `FailurePolicy::AllowPartial` submits
surviving whole items and reports omissions with item indices and source paths.
Zero survivors is an error. Generated temporary video sidecars are cleaned up;
caller-owned files are preserved.

## nomctl

```sh
nomctl --profile staging extractor create flight-recorder --json
nomctl --profile staging extractor image register "$EXTRACTOR_RID" flight-recorder-v1.tar --contract image.json
nomctl --profile staging extractor activate "$EXTRACTOR_RID" "$IMAGE_RID"
nomctl --profile staging ingest containerized "$EXTRACTOR_RID" --dataset "$DATASET_RID" --source RECORDING flight-42.flight --argument PARTS 4 --no-wait --json
nomctl --profile staging ingest job files "$JOB_RID" --wait --json
nomctl --profile staging ingest batch batch.json --allow-partial --no-wait --json
```

`image.json`:

```json
{
  "schema_version": 1,
  "tag": "v1",
  "output_format": "manifest",
  "default_timestamp": {"column": "ts", "kind": "epoch", "unit": "nanoseconds"},
  "inputs": [{"name": "Recording", "environment_variable": "RECORDING", "file_suffixes": ["flight"], "required": true}],
  "parameters": [{"name": "Parts", "environment_variable": "PARTS"}]
}
```

Batch JSON has `schema_version: 1`, an existing `dataset`, optional `tags` and
`runs_to_expand`, and an `items` array. Each item has a `kind` and format-specific
fields. Every item accepts an optional `tags` object. The item-specific fields are:

| `kind` | Required fields | Optional fields |
| --- | --- | --- |
| `containerized` | `extractor`, nonempty `sources` object | `arguments`, `timestamp` |
| `tabular` | `path`, `timestamp` | `tag_columns`, `units`, `channel_name_overrides`, `channel_prefix` |
| `avro_stream` | `path` | numeric `timestamp`, `units`, `channel_prefix` |
| `mcap` | `path` | `topics`, `ignore_invalid_topics` |
| `journal_json` | `path` | `channel`, `timestamp` |
| `dataflash` | `path` | none |
| `video` | `path`, `channel`, `timing` | none |

General timestamps carry `column` and `kind`: `epoch` adds `unit`; `relative`
adds `unit` and RFC3339 `start`; `iso8601` needs no further fields; `custom`
adds `format` and optional `default_year`/`default_day_of_year`. Units are
`nanoseconds`, `microseconds`, `milliseconds`, `seconds`, `minutes`, `hours`, or
`days`. Avro timestamps accept only `epoch`/`relative` without `column` and default
to epoch nanoseconds. MCAP topics are `{"kind":"include","names":["/imu"]}` or
`{"kind":"exclude","names":["/camera"]}`; omission selects all topics.
Video timing is `{"kind":"start","at":"2026-01-01T00:00:00Z"}` or
`{"kind":"frames","timestamps_file":"frames.json"}`. The frame file contains
an array of signed 64-bit integer nanoseconds.

```json
{
  "schema_version": 1,
  "dataset": "ri.catalog.main.dataset.REPLACE_ME",
  "tags": {"vehicle": "n1234"},
  "items": [
    {"kind": "tabular", "path": "telemetry.csv", "timestamp": {"column": "ts", "kind": "epoch", "unit": "nanoseconds"}},
    {"kind": "video", "path": "camera.mp4", "channel": "camera", "timing": {"kind": "frames", "timestamps_file": "frames.json"}}
  ]
}
```

Paths in JSON resolve relative to the JSON file; command-line paths resolve from
the current directory. Unknown schema fields and unsupported versions fail before
mutation. `--json` emits one document to stdout, with diagnostics on stderr.

Ingest waits by default, matching native nomctl commands. `--no-wait` returns the
acknowledged RID. A later wait failure reports that RID; do not resubmit merely to
recover metadata. Use `--timeout` to bound waits and `--timestamp-json` for richer
timestamp types. `nomctl help-all` prints complete command help.

## Authoring, containers and verification

See the [runtime guide](../nominal-extractor/README.md) for ordinary Rust entrypoints,
typed contexts, local tests, complete output options, and a multi-stage Dockerfile.
Build your image with Docker, then save it with `docker save IMAGE -o extractor.tar`.
Format parsing, writing and media libraries remain the author's dependencies.

Local tests verify contracts, generated requests, upload outcomes and error
handling. A live platform smoke test additionally requires an authorized test
profile/workspace and actual container images. Its sequence is create → register →
activate → ingest single-file and mixed/video-only outputs → inspect files → clean
up only newly created resources. No live platform deployment is implied by passing
local tests. Older pipelines may not support manifest video outputs.

For an opt-in smoke test, use a profile explicitly configured for a test workspace
and images you built from the single-file and manifest examples. Save each image
to a tarball. Prepare matching `single-image.json` (`output_format: "csv"`) and
`manifest-image.json` (`output_format: "manifest"`) contracts. Register `DATA` as
the input in both; the manifest contract additionally registers optional `VIDEO`
and optional parameters `PREFIX` and `VIDEO_START`. Use real CSV and playable video files.

```sh
nomctl --profile extractor-smoke extractor create extractor-smoke-UNIQUE --json
# Record the returned extractor RID as EXTRACTOR_RID.
nomctl --profile extractor-smoke dataset create --name extractor-smoke-UNIQUE
# Record the returned dataset RID as DATASET_RID.
nomctl --profile extractor-smoke extractor image register "$EXTRACTOR_RID" single.tar --contract single-image.json --json
# Record the returned image RID as SINGLE_IMAGE_RID.
nomctl --profile extractor-smoke extractor activate "$EXTRACTOR_RID" "$SINGLE_IMAGE_RID" --timeout 300
nomctl --profile extractor-smoke ingest containerized "$EXTRACTOR_RID" --dataset "$DATASET_RID" --source DATA telemetry.csv --timeout 300 --json
# Record the returned job RID as SINGLE_JOB_RID; verify file status and metadata.
nomctl --profile extractor-smoke ingest job files "$SINGLE_JOB_RID" --wait --timeout 300 --json
nomctl --profile extractor-smoke extractor image register "$EXTRACTOR_RID" manifest.tar --contract manifest-image.json --json
# Record the returned image RID as MANIFEST_IMAGE_RID.
nomctl --profile extractor-smoke extractor activate "$EXTRACTOR_RID" "$MANIFEST_IMAGE_RID" --timeout 300
nomctl --profile extractor-smoke ingest containerized "$EXTRACTOR_RID" --dataset "$DATASET_RID" --source DATA telemetry.csv --source VIDEO camera.mp4 --argument VIDEO_START 2026-01-01T00:00:00Z --timeout 300 --json
# Record the returned job RID as MIXED_JOB_RID; verify telemetry and video outputs.
nomctl --profile extractor-smoke ingest job files "$MIXED_JOB_RID" --wait --timeout 300 --json
```

Also exercise a video-only author callback using `ManifestContext::add_video`,
registering only its video input; its successful file results verify backend
support independently of mixed output. Re-run with a new image tag for each
build. If a request times out, inspect its job before retrying submission.
After recording results, archive only the new extractor/dataset and delete only
new image RIDs where the service permits deletion. Do not reuse this cleanup for
pre-existing resources. These live steps and the Docker build were not run during
local implementation validation.
