# Containerized extractors

Use `nominal` to register and manage extractors in Nominal. Use
[`nominal::extractor`](extractor.md) inside your image to
handle inputs, parameters, output declarations, manifests, and video sidecars.
The authoring runtime is experimental.

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
sources if the active image requires none. `with_scope_tags` adds default tags without replacing tags already set on the
request. Its API example resolves a dataset attached to a run in a workbook.
Workbook scopes expose assets and runs, so supply dataset-view tags explicitly.

`dataset_files(job_rid)` returns the files available when called.
`wait_for_job_files` waits for the job to complete, then lists its files and waits
for them to finish ingestion. To wait only for files already listed, pass the
result of `dataset_files` to `catalog().wait_for_dataset_files`.
Job searches accept dataset, creator, status, path and time filters. They can use
the default workspace, a specified workspace or all workspaces. Cancelling a job
returns its updated state.

## nomctl

```sh
nomctl --profile staging extractor create flight-recorder --json
nomctl --profile staging extractor image register "$EXTRACTOR_RID" flight-recorder-v1.tar --contract image.json
nomctl --profile staging extractor activate "$EXTRACTOR_RID" "$IMAGE_RID"
nomctl --profile staging ingest containerized "$EXTRACTOR_RID" --dataset "$DATASET_RID" --source RECORDING flight-42.flight --argument PARTS 4 --no-wait --json
nomctl --profile staging ingest job files "$JOB_RID" --wait --json
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

The tarball and contract paths resolve from the current directory.
Unknown schema fields and unsupported versions are rejected before upload. `--json` emits one document to stdout, with diagnostics on stderr.

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
