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

## nomctl

```sh
nomctl --profile staging extractor create flight-recorder --json
nomctl --profile staging extractor image register "$EXTRACTOR_RID" flight-recorder-v1.tar --contract image.json
nomctl --profile staging extractor activate "$EXTRACTOR_RID" "$IMAGE_RID"
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

## Authoring, containers and verification

See the [runtime guide](extractor.md) for extractor functions,
output options, local tests and a multi-stage Dockerfile.
Build your image with Docker, then save it with `docker save IMAGE -o extractor.tar`.
Choose the libraries your extractor needs to read and write its file formats.

Local tests check registry requests and errors. Live verification requires a Nominal test workspace and container images.
