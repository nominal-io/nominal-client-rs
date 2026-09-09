# Containerized extractors

Use `nominal` to register and run extractors in Nominal. Use
[`nominal::extractor`](extractor.md) inside your image to
handle inputs, parameters, output declarations, manifests, and video sidecars.
The authoring runtime and batch API are experimental.

## Register an image

Create an extractor, upload an image saved with `docker save`, then activate it:

```rust,no_run
use nominal::core::{
    Activation, ExtractorCreate, FileExtractionInput, FileExtractionParameter,
    ImageRegistration, NominalClient, RegisterableOutputFormat, TimeUnit,
    Timestamp, WaitOptions,
};
use std::path::Path;

async fn register(client: &NominalClient) -> nominal::Result<()> {
    // The extractor is the stable resource. Images provide versioned implementations.
    let extractors = client.extractors();
    let extractor = extractors.create(ExtractorCreate::new("flight-recorder")).await?;

    // Declare how the container receives inputs and what it produces. Manifest
    // outputs can override this default interpretation of the ts column.
    let contract = ImageRegistration::new(
        "v1", RegisterableOutputFormat::Manifest,
        Timestamp::epoch("ts", TimeUnit::Nanoseconds),
    )
    // RECORDING carries the input file path; PARTS carries a scalar string.
    .input(FileExtractionInput::new("Recording", "RECORDING").suffix("flight").required(true))
    .parameter(FileExtractionParameter::new("Parts", "PARTS"));

    // Upload the archive created by docker save. Registration alone does not
    // change which image the extractor runs.
    let image = client.container_images()
        .register(&extractor, Path::new("flight-recorder-v1.tar"), contract).await?;

    // Wait for Nominal to prepare the image before making it active.
    extractors.activate(&extractor, &image, Activation::Wait(WaitOptions::default())).await?;
    Ok(())
}
```

Tags are immutable within an extractor: use a new tag for a new image.
Registration does not change the active image. Default timestamp metadata is
required even when the manifest supplies per-output overrides. Use `Manifest`
for images built with `nominal::extractor`. The registry also accepts `Csv`,
`Parquet`, and `AvroStream` for compatible existing images.

Use `client.extractors()` to find or update an extractor and `client.container_images()`
to inspect its images. Searches return all pages. Use `.in_workspace(rid)` to select
a workspace. Operations on an existing extractor or image use the workspace
stored with that resource.

## Submit and inspect

```rust,no_run
use nominal::core::{ContainerizedIngest, DatasetTarget, NominalClient, WaitOptions};

async fn ingest(client: &NominalClient, extractor: &str, dataset: &str) -> nominal::Result<()> {
    // These are the environment names from the image registration. The SDK
    // uploads the local source and sends the argument and data tags with the job.
    let submission = client.ingest().upload_containerized(
        DatasetTarget::Existing(dataset.into()),
        ContainerizedIngest::new(extractor)
            .source("RECORDING", "flight-42.flight")
            .argument("PARTS", "4")
            .tag("vehicle", "n1234"),
    ).await?;
    // Keep the job ID so you can inspect this submission even if waiting fails.
    println!("job: {}", submission.job().rid());

    // A job can create several files. Wait for the job before listing its outputs,
    // then wait for those files to finish ingestion.
    let files = client.ingest()
        .wait_for_job_files(submission.job().rid(), WaitOptions::default()).await?;
    println!("{} output files", files.len());
    Ok(())
}
```

Direct ingest also accepts `DatasetTarget::New(DatasetCreate::new(name))` to create
the destination as part of the ingest request. Submission returns the job RID;
use `get_ingest_job` to fetch its metadata. A failed metadata request does not hide
a successful submission. The client does not retry submissions automatically,
because a failed response can still mean that the server accepted the job.

Source keys are registered environment-variable names. Direct ingestion allows no
sources if the active image requires none; batch extractor items require at least
one. `with_scope_tags` adds default tags without replacing tags already set on the
request. Its API example resolves a dataset attached to a run in a workbook.
Workbook scopes expose assets and runs, so supply dataset-view tags explicitly.

`dataset_files(job_rid)` returns the files available when called.
`wait_for_job_files` waits for the job to complete, then lists its files and waits
for them to finish ingestion. To wait only for files already listed, pass the
result of `dataset_files` to `catalog().wait_for_dataset_files`.
Job searches accept dataset, creator, status, path and time filters. They can use
the default workspace, a specified workspace or all workspaces. Cancelling a job
returns its updated state.

## Batch ingestion

```rust,no_run
use nominal::core::{ContainerizedIngest, NominalClient};

async fn batch(client: &NominalClient, extractor: &str, dataset: &str) -> nominal::Result<()> {
    // All recordings target the same existing dataset and belong to one job.
    // Adding items records work; the uploads begin when submit is called.
    let mut batch = client.ingest().batch(dataset);
    for path in ["one.flight", "two.flight"] {
        batch.add_containerized(ContainerizedIngest::new(extractor).source("RECORDING", path))?;
    }

    // Default settings upload at most four files at once and submit no job if
    // an upload fails. Submitting consumes the batch to prevent accidental reuse.
    let job = batch.submit().await?;
    println!("job: {}", job.rid());
    Ok(())
}
```

A batch requires an existing dataset. It can mix extractor, CSV/Parquet, Avro,
MCAP, journal JSON, DataFlash and video items. Format-specific builders expose
timestamps, tags, units, channel names or selection where supported. Batch videos
accept start timing or a per-frame timestamp vector; the authoring runtime also
supports video scaling.

`submit(self)` consumes the batch, so it cannot be submitted twice. By default,
four files upload at a time; each file can upload several parts concurrently.
If an upload fails, the default policy stops starting new uploads, waits for
active uploads to finish, and submits no ingest request. Files already uploaded
remain on the server. `FailurePolicy::AllowPartial` submits items whose uploads
all succeeded and reports omitted item indices and source paths. If no items
succeed, submission returns an error. Temporary video timestamp files are removed;
files supplied by the caller are kept.

The `add_*` methods modify the batch in place. A rejected addition leaves earlier
items in the batch. Use `add_tabular` for CSV or Parquet and
`add_ardupilot_dataflash` for DataFlash, matching the existing Rust ingest names.

`submit()` returns an `IngestJob`, including its current state. To change upload
limits, partial-failure handling or run expansion, pass `BatchOptions` to
`submit_with_options(options)`. If the server accepts the job but fetching its
metadata fails, `Error::IngestJobMetadata` contains the accepted `job_rid`.
Inspect that job before submitting again.

For callers that need the omitted-item report or want to skip the metadata
request, `submit_with_report(options)` returns a `BatchSubmission` with a job
reference and omitted items. It submits the batch once; it is an alternative to
`submit()`, not a follow-up operation. `nomctl` uses this path for `--no-wait` and
its JSON omission report.

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
the current directory. Unknown schema fields and unsupported versions are rejected before
upload or submission. `--json` emits one document to stdout, with diagnostics on stderr.

Ingest waits by default, matching native nomctl commands. `--no-wait` returns the
job RID. A later wait failure reports that RID; inspect that job before submitting
again. Use `--timeout` to limit how long the command waits and `--timestamp-json`
for relative timestamps or custom formats. `nomctl help-all` prints complete command help.

## Authoring, containers and verification

See the [runtime guide](extractor.md) for extractor functions,
output options, local tests and a multi-stage Dockerfile.
Build your image with Docker, then save it with `docker save IMAGE -o extractor.tar`.
Choose the libraries your extractor needs to read and write its file formats.

Local tests check requests, output files and errors. Testing the complete workflow
requires a Nominal test workspace and container images. Older ingest pipelines
may not support manifest video outputs.

To test against Nominal, use a profile configured for a test workspace
and an image built from the runtime README example. Save the image to `manifest.tar`.
Prepare `manifest-image.json` with `output_format: "manifest"`, required input
`DATA`, optional input `VIDEO`, and optional parameters `PREFIX` and `VIDEO_START`.
Use real CSV and playable video files.

```sh
nomctl --profile extractor-smoke extractor create extractor-smoke-UNIQUE --json
# Record the returned extractor RID as EXTRACTOR_RID.
nomctl --profile extractor-smoke dataset create --name extractor-smoke-UNIQUE
# Record the returned dataset RID as DATASET_RID.
nomctl --profile extractor-smoke extractor image register "$EXTRACTOR_RID" manifest.tar --contract manifest-image.json --json
# Record the returned image RID as MANIFEST_IMAGE_RID.
nomctl --profile extractor-smoke extractor activate "$EXTRACTOR_RID" "$MANIFEST_IMAGE_RID" --timeout 300
nomctl --profile extractor-smoke ingest containerized "$EXTRACTOR_RID" --dataset "$DATASET_RID" --source DATA telemetry.csv --timeout 300 --json
# Record the returned job RID as TELEMETRY_JOB_RID; verify file status and metadata.
nomctl --profile extractor-smoke ingest job files "$TELEMETRY_JOB_RID" --wait --timeout 300 --json
nomctl --profile extractor-smoke ingest containerized "$EXTRACTOR_RID" --dataset "$DATASET_RID" --source DATA telemetry.csv --source VIDEO camera.mp4 --argument VIDEO_START 2026-01-01T00:00:00Z --timeout 300 --json
# Record the returned job RID as MIXED_JOB_RID; verify telemetry and video outputs.
nomctl --profile extractor-smoke ingest job files "$MIXED_JOB_RID" --wait --timeout 300 --json
```

Also test an extractor that outputs only video with `ManifestContext::add_video`,
registering only its video input. Check that its output finishes ingestion. Re-run with a new image tag for each
build. If a request times out, inspect its job before retrying submission.
After recording results, archive only the new extractor/dataset and delete only
new image RIDs where the service permits deletion. Do not reuse this cleanup for
pre-existing resources. The Docker build and live ingestion checks have not been
run for this change.
