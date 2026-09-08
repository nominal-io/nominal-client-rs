# Containerized extractors in Rust

Date: 2026-09-08
Status: Architecture and scope approved; written specification awaiting review.

## Objective and compatibility baseline

Provide the Python client's containerized-extractor capabilities through idiomatic Rust APIs and nomctl. Include the complete authoring surface, including video-only manifests and per-frame timestamps. Delivery may be incremental, but telemetry-only support does not satisfy the target.

The baseline is the local nominal-client checkout at commit `2a4e588b47346396d4e81cd8211a37e72d52f954`, particularly:

- `nominal/core/containerized_extractor.py` and `container_image.py`.
- `nominal/core/dataset.py`, `ingestion_job.py`, and client search methods.
- `nominal/experimental/extractor/` and `experimental/ingest/_ingest_builder.py`.
- Corresponding extractor, registry, ingest, and runtime tests.

Parity means equivalent supported workflows and serialized contracts, not identical method names or Python object mutation semantics. The Python decorator is named `single_file_extractor`. Backend limitations documented by that checkout are compatibility constraints, not independently verified claims about every deployed platform.

## Architecture

Extend `nominal` with extractor and image resource clients following the existing `NominalClient` service-accessor and snapshot patterns. Reuse authentication, workspace resolution, multipart uploads, timestamps, transport errors, and ingest job infrastructure. Add required generated service bindings through the existing API dependency; verify its schema coverage before changing dependency versions.

Add a workspace crate named `nominal-extractor` for code running inside containers. It must not depend on the full network client or require credentials, a network connection, Docker, or an async runtime for ordinary local execution. Keep transport models private. Use small runtime-owned contract types and explicit conversions where needed rather than introducing a shared crate solely to eliminate a few duplicate types.

Extend `nominal-cli` with resource management and ingestion commands backed by the public client APIs. Business logic belongs in the libraries.

Alternatives considered were a feature-gated module in `nominal` and procedural attribute macros. A separate crate provides a smaller dependency surface and independent stability boundary. Ordinary Rust functions and typed contexts provide the initial authoring interface; macros can be considered later if usage demonstrates meaningful remaining boilerplate.

## Platform operations

### Extractors and images

- Create, fetch, and search extractors, including pagination, archived-state selection, and file-extension filtering.
- Update name, description, archived state, and active image; expose archive and unarchive conveniences.
- Preserve omitted update fields rather than accidentally clearing them. Do not invent field-clearing behavior absent from the service contract.
- Upload a Docker-save tarball and register an image with immutable tag, declared inputs, string parameters, output format, and required default timestamp metadata.
- Fetch and search images globally or by extractor, tag, and status; expose image metadata and lifecycle state.
- Refresh status, wait for readiness, and delete images, allowing service errors to explain constraints such as an active-image deletion restriction.
- Keep registration separate from activation. Activation can wait for readiness or reject a non-ready image immediately. Surface terminal processing failures clearly.
- Accept precisely the registerable output formats supported by the Python baseline; represent other returned values without allowing unsupported registration silently.

### Ingest and jobs

- Submit containerized extraction using an extractor RID, named local sources keyed by registered environment variable, string arguments, tags, and optional timestamp overrides.
- Support an existing dataset and a create-and-ingest convenience consistent with Rust's dataset target pattern.
- Validate empty source sets, required inputs, missing active images, and timestamp option combinations before avoidable uploads where practical. The service remains authoritative about concurrent image changes.
- Return a job handle/snapshot rather than assuming extraction produces one file.
- Support job get, search and pagination, refresh, wait, cancellation, output dataset-file discovery, and waiting for individual outputs to finish ingestion. Preserve applicable Python search filters and run-expansion behavior in the method-level parity inventory.
- Include batch-builder participation: multiple extractor invocations and composition with Python-supported native ingest item kinds, per-item tags, common tags, timestamp overrides, run expansion, and a single submission. Reuse existing native format option types where possible.
- Batch submission consumes the builder. By default an upload failure prevents submission; explicit partial mode drops entire items with failed inputs and submits eligible items. Report failures and omitted items rather than losing their identity.
- Audit retry behavior for side-effecting RPCs. Do not automatically replay an ambiguous ingest submission unless the service provides an idempotency guarantee. Preserve successful upload information in diagnostics where available.

## Authoring API

Provide separate single-file and manifest contexts and runner functions. Authors pass a normal Rust function or closure returning `Result`; runners perform environment loading, contract checks, invocation, and output finalization. Errors compose with `?`, including errors from an author's format libraries, without forcing those libraries into this crate. A binary entrypoint can propagate failure through `main` to a nonzero process exit. Library execution must return errors rather than terminate the host process.

Use standard `Path`/`PathBuf`, builders for optional metadata, and enums for exclusive choices. Keep required and optional parameter access separate; support raw strings and typed parsing through `FromStr`, with errors identifying the parameter. Exact public signatures will be resolved in the implementation plan and checked through compileable examples.

### Inputs and environment

- Named lookup by registered display name or environment variable, sole-input convenience, and enumeration with Python-compatible discovery/order behavior.
- Required and optional parameters, defaults, and optional registration metadata.
- Optional ingest job RID, dataset RID, additional tags, and resolved job timestamp metadata.
- Support the baseline's `OUTPUT_DIR`, input-directory override, registered format, input/parameter JSON metadata, and optional system metadata environment variables.
- Preserve local-run behavior when injected metadata is absent. Malformed metadata must produce contextual errors.
- Match Python's advisory startup warnings for missing registered inputs/parameters; accessing an unavailable required value fails. Reject a registered single-file/manifest contract mismatch before invoking user code.

### Output contracts

Single-file execution declares exactly one output and validates the file before completion. Manifest execution supports all of:

| Output | Metadata |
| --- | --- |
| CSV/Parquet tabular, including supported gzip forms | Tag columns, channel prefix, numeric epoch/relative timestamp metadata |
| Avro stream, including supported gzip form | Channel prefix and numeric timestamp type |
| Journal JSON logs, including supported gzip form | Timestamp column and numeric timestamp type |
| Video | Channel and either absolute start with optional true frame rate, or explicit absolute per-frame timestamps |

Automatically serialize the manifest and video timestamp sidecars in the platform's expected format. Support mixed outputs and video-only manifests. Validate format extensions, file existence, output-directory containment, duplicate declarations, reserved paths, and empty output sets according to the Python contract. Undeclared scratch files produce warnings where Python does, rather than failing the run.

Encode paired timestamp settings and mutually exclusive video timing choices as coherent types. Preserve per-output versus job-level versus image-default timestamp precedence. Manifest numeric metadata must not pretend to support string timestamp formats; those outputs inherit applicable job-level metadata as in Python. Validate sidecar collisions and preserve nanosecond precision.

Offer explicit environment injection and local execution against temporary directories without changing global process environment. Expose manifest inspection for tests. Logging integrates with normal Rust logging conventions and must not install a global subscriber from reusable library code.

The runtime contract remains experimental, matching the Python package. Document backend requirements for video outputs: local validation cannot make an older pipeline understand newer manifest fields.

## nomctl interface

Use these command families, with exact flags finalized alongside existing CLI conventions:

- `nomctl extractor create|get|search|update|archive|unarchive|activate`.
- `nomctl extractor image register|get|search|wait|delete`.
- `nomctl ingest containerized` for direct invocation, including repeated named sources, arguments, tags, dataset destination, timestamp overrides, and optional waiting.
- `nomctl ingest job get|search|wait|cancel|files` for reusable job operations.
- `nomctl ingest batch` with a declarative request file for batch workflows that are unwieldy as repeated flags.

Image registration accepts a structured contract file for inputs, parameters, format, and default timestamps. CLI output follows existing human-readable and machine-readable conventions, preserves resource identifiers for scripting, and reports failures through nonzero exit status. Keep image activation explicit. Provide examples for the complete build/save/register/activate/ingest workflow.

Docker builds, registry publishing outside Nominal, format conversion engines, and a project scaffolding generator are not required by this scope. Authors choose their own parsing and file-writing libraries. Document how to locate captured logs using existing platform capabilities; do not invent a dedicated logs endpoint.

## Verification and completion criteria

Before implementation, enumerate every relevant public Python operation and runtime behavior in a parity matrix mapped to a Rust API, CLI command where useful, and verification case. This includes batch capabilities rather than silently substituting single-invocation support.

Use fixtures derived from the pinned Python baseline to compare semantic manifest JSON, sidecar contents, environment interpretation, timestamp precision, and representative invalid cases. Avoid tests that only restate Rust implementation details.

Client checks cover request serialization, pagination, workspace propagation, image transitions, upload failure handling, terminal job states, output discovery, and ambiguous submission failures. CLI checks cover argument mapping, structured contracts, batch inputs, machine-readable results, and exit status. Compile and locally execute the single-file and manifest examples.

An available test platform should exercise create, register, activate, submit, inspect outputs, and cleanup using single-file and mixed telemetry/video examples. Record platform/version constraints and distinguish unrun integration checks from passing local checks. Full parity is complete when the matrix is satisfied, including batch ingest and videos, rather than merely when the first slice ships.

## Delivery sequence

1. Inventory baseline methods and wire contracts; verify generated API coverage and dependency compatibility.
2. Implement extractor/image management and associated CLI commands.
3. Implement direct containerized ingest, job lifecycle/output operations, and associated CLI commands.
4. Implement batch composition, upload-failure semantics, run expansion, and CLI request-file support.
5. Implement the independent authoring crate with both output modes and the complete manifest surface.
6. Complete examples, compatibility fixtures, documentation, and end-to-end validation.

Each slice gets a focused implementation plan and preserves the overall parity acceptance criteria. This document defines scope and architecture; implementation begins after written-spec review and planning.
