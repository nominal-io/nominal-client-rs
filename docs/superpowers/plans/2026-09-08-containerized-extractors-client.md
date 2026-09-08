# Extractor Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Use `gpt-5.6-terra` with `max` reasoning.

**Goal:** Implement complete extractor/image management, direct containerized ingest, batch composition, and job inspection in the Rust SDK.

**Architecture:** Management uses scoped v2 gRPC clients; direct ingestion uses existing Conjure targets; batch owns typed pending items and submits v2 once. One coordinator owns shared transport, exports, dependencies, and timestamp codecs.

**Tech Stack:** Existing nominal-api, Conjure, tonic, Tokio/futures, chrono, thiserror; multipart uploader reused.

---

Read the coordinator plan first. Assign C1/C2 to the management worker and C3/C4/C5 to the ingest worker. C3 can start against frozen C1 contracts. No worker edits shared files without coordinator integration.

## File map

| File under nominal/src/core/ | Owner / responsibility |
| --- | --- |
| extractor/mod.rs | C1 accessor wiring and exports inside this owned subtree |
| extractor/models.rs | C1 extractor/image snapshots, registration/input/parameter models |
| extractor/query.rs | C1 extractor/image filter builders and proto conversion |
| extractor/extractors.rs | C1 create/read/search/update; C2 activation methods |
| extractor/images.rs | C2 upload/register/read/search/delete/wait |
| extractor/tests.rs | C1/C2 behavioral service request/response tests |
| ingest/containerized.rs | C3 direct ingest request/preflight, Conjure submission |
| ingest/job.rs | C3 extend current immutable job snapshot |
| ingest/job_query.rs | C3 job filters and workspace search selection |
| ingest/job_files.rs | C3 job-specific catalog enumeration and file waiting |
| catalog/dataset_file.rs | C3 reusable dataset-file snapshot/status conversion |
| ingest/batch/mod.rs | C4 builder and consuming submission boundary |
| ingest/batch/items.rs | C4 owned pending variants and public option types |
| ingest/batch/upload.rs | C5 bounded upload orchestration and reports |
| ingest/batch/encode.rs | C4 pure completed-item to v2 conversion |
| ingest/batch/tests.rs | C4/C5 fake upload and submission test cases |

Shared wiring to request from coordinator: IngestClient gains gRPC access, extractor lookup, catalog service for files, and application base URL; job methods can live in child-module impl blocks. CatalogClient exposes file inspection/waiting through `dataset_file.rs`. Avoid enlarging `ingest/options.rs` (currently 717 lines) with batch-specific types.

## Frozen SDK interface

Public names live under `nominal::core`. Required constructor arguments are mandatory; optional metadata uses consuming builders. Snapshots expose getters and contain no service handles.

```text
NominalClient::extractors() -> ExtractorsClient
NominalClient::container_images() -> ContainerImagesClient
ExtractorsClient::in_workspace(self, rid: impl Into<String>) -> Self
ContainerImagesClient::in_workspace(self, rid: impl Into<String>) -> Self

ExtractorCreate::new(name).description(description)
ExtractorUpdate::default().name(name).description(description).archived(bool)
ExtractorQuery::default().include_archived(bool).file_extension(extension)
ContainerImageQuery::default().extractor(rid).tag(tag).status(status)
FileExtractionInput::new(name, env).description(text).suffix(suffix).required(bool)
FileExtractionParameter::new(name, env).description(text).required(bool)
ImageRegistration::new(tag, RegisterableOutputFormat, Timestamp)
    .input(FileExtractionInput).parameter(FileExtractionParameter)

ExtractorsClient::create(ExtractorCreate) -> Result<ContainerizedExtractor>
ExtractorsClient::get(&str) -> Result<ContainerizedExtractor>
ExtractorsClient::search(ExtractorQuery) -> Result<Vec<ContainerizedExtractor>>
ExtractorsClient::update(&ContainerizedExtractor, ExtractorUpdate)
    -> Result<ContainerizedExtractor>
ExtractorsClient::archive(&ContainerizedExtractor) -> Result<ContainerizedExtractor>
ExtractorsClient::unarchive(&ContainerizedExtractor) -> Result<ContainerizedExtractor>
ExtractorsClient::refresh(&ContainerizedExtractor) -> Result<ContainerizedExtractor>
ExtractorsClient::activate(&ContainerizedExtractor, &ContainerImage, Activation)
    -> Result<ContainerizedExtractor>

ContainerImagesClient::register(&ContainerizedExtractor, &Path, ImageRegistration)
    -> Result<ContainerImage>
ContainerImagesClient::get(&str) -> Result<ContainerImage>
ContainerImagesClient::search(ContainerImageQuery) -> Result<Vec<ContainerImage>>
ContainerImagesClient::refresh(&ContainerImage) -> Result<ContainerImage>
ContainerImagesClient::wait_ready(&ContainerImage, WaitOptions) -> Result<ContainerImage>
ContainerImagesClient::delete(&ContainerImage) -> Result<()>
```

All service methods above are async. `Activation` is `RequireReady | Wait(WaitOptions)`. Coordinator owns `WaitOptions` in `nominal/src/core/wait.rs`: `WaitOptions::default().interval(Duration).timeout(Duration)` with fallible validation at the operation boundary, defaulting to 1 second/no deadline for Python readiness parity; reject zero interval or timeout. Existing job wait defaults remain unchanged. Image status is Pending/Ready/Failed/Unknown(i32); observed output format preserves unknown numeric values. Registration format is a separate closed enum: Parquet/Csv/AvroStream/Manifest, avoiding runtime rejection of otherwise representable response variants.

Both snapshots retain `workspace_rid`. Operations on a snapshot use its workspace, even if invoked through a differently configured accessor; reject an extractor/image pair with different workspace or extractor RID. RID-only get/search resolve explicit accessor workspace, configured workspace, then server default once. Coordinator adds `WorkspacesClient::get_default_workspace() -> Result<Workspace>` and a crate-private resolving helper; preserve the current public `resolve_workspace(...) -> Result<()>` signature rather than breaking profile validation callers.

```text
ContainerizedIngest::new(extractor_rid)
    .source(env, path).argument(env, value).tag(key, value).timestamp(Timestamp)
IngestClient::upload_containerized(DatasetTarget, ContainerizedIngest)
    -> Result<ContainerizedSubmission>
ContainerizedSubmission::job() -> &IngestJobRef
ContainerizedSubmission::dataset_rid() -> &str
IngestJobRef::rid() -> &str
```

`IngestJobRef` contains an acknowledged RID only. Fetching a complete `IngestJob` is explicit via existing `get_ingest_job`; new submissions must not fail merely because a follow-up metadata fetch fails. Keep old native upload return types unchanged. A missing job RID in the acknowledgement is UnexpectedResponse and must not trigger another submit. The direct response must contain a dataset destination; do not guess it from the requested new dataset name.

```text
IngestClient::search_ingest_jobs(IngestJobQuery) -> Result<Vec<IngestJob>>
IngestClient::cancel_ingest_job(&str) -> Result<IngestJob>
IngestClient::dataset_files(&str) -> Result<Vec<DatasetFile>>
IngestClient::wait_for_job_files(&str, WaitOptions) -> Result<Vec<DatasetFile>>
CatalogClient::wait_for_dataset_files(Vec<DatasetFile>, WaitOptions)
    -> Result<Vec<DatasetFile>>
```

`wait_for_job_files` waits for job completion, then takes a file snapshot and waits for those files. `dataset_files` never waits for job completion. For Python's snapshot-at-call behavior compose `dataset_files` and `wait_for_dataset_files` directly. Query builders expose datasets, creators, statuses, search text, start_after/start_before, and `WorkspaceSelection::{Default, Specific(String), All}`. Preserve `IngestJob::status` and existing fields while adding the baseline's optional metadata.

Freeze query construction as `IngestJobQuery::default().dataset(rid).created_by(rid).status(status).search_text(text).start_after(DateTime<Utc>).start_before(DateTime<Utc>).workspace(WorkspaceSelection)`. Repeated dataset/creator/status calls append values; different fields AND together, values within a field OR together. Optional snapshot getters use the Python field names converted to snake_case; URL getter is `nominal_url() -> String`.

## C1: Extractor/image models, workspace scope, extractor operations

**Files:** extractor/{mod,models,query,extractors,tests}.rs. **Reference:** Python containerized_extractor.py, container_image.py, test_containerized_extractor.py, test_container_image.py; existing Rust fs/drives.rs for tonic construction.

- [ ] Add pure conversion tests proving unset update fields remain unset, false archive is present, unknown status survives, image defaults can be absent, and all input/parameter fields round-trip.
- [ ] Run `cargo test -p nominal extractor`; expect failure before the new conversion/clients exist.
- [ ] Implement the models/builders above. Keep timestamp codec use at the shared boundary; no generated types in public signatures. Define typed extractor/image errors through a leaf error type integrated into crate Error by coordinator.
- [ ] Implement create/get/search/update/archive/unarchive/refresh. Search follows every page until absent next token, ANDs filters, and preserves response order. Use the mutation channel for create/update and the retry-enabled channel for reads.
- [ ] Add in-process/mock service tests asserting workspace selection and full requests, two pages, missing required response fields, and server error preservation. A snapshot from workspace A refreshed through an accessor configured for B must still request A.
- [ ] Run `cargo test -p nominal extractor`; commit `feat: add scoped containerized extractor clients` and send frozen model/API commit to ingest and CLI workers.

## C2: Image registration and activation

**Files:** extractor/images.rs, extractor/extractors.rs, extractor/tests.rs. **Dependencies:** C1 + coordinator mutation transport/timestamp codec.

- [ ] Add tests around register: invalid empty input contract or required timestamp data fails before upload; upload completion feeds CreateImage.object_path; returned image retains resolved workspace; no activation RPC occurs. Request fields include immutable tag and complete inputs/parameters.
- [ ] Add readiness table cases: Pending→Ready activates once; Failed returns error; Unknown never counts as Ready; RequireReady rejects Pending; timeout returns without activation; mismatched extractor/workspace rejects before network mutation. Refresh image before readiness decision rather than trusting stale snapshot state.
- [ ] Run `cargo test -p nominal extractor`; observe named failures.
- [ ] Reuse `ingest::multipart::upload_file` with the resolved workspace and tarball filename/MIME; do not implement another upload protocol. Build registry v2 CreateImage using shared Timestamp conversion. Registration errors retain uploaded object location in diagnostics when useful without exposing credentials.
- [ ] Implement image get/search/refresh/delete/wait and extractor activation as wait/check followed by exactly one update. Server remains authoritative for concurrent changes; no transaction claim across image readiness and extractor update.
- [ ] Prove mutation UNAVAILABLE results in one attempt using P0 transport harness. Immutable tag conflicts remain service errors; never silently choose a new tag or overwrite an image.
- [ ] Run management tests and `cargo check -p nominal --all-targets`; commit `feat: register and activate extractor images`.

## C3: Direct ingest, jobs, and dataset files

**Files:** ingest/containerized.rs, job.rs, job_query.rs, job_files.rs, catalog/dataset_file.rs; tests colocated with these modules. **Dependencies:** C1 and coordinator wiring.

- [ ] Test required-source preflight with two cases: required INPUT absent fails before upload; zero sources and no required active inputs submits successfully. Separately reserve empty-map rejection for C4's batch API. Missing active image fails before upload.
- [ ] Test direct new and existing DatasetTarget conversion, arguments, tags, complete timestamp override, image default fallback, and acknowledgement RID retained without a metadata read. Reuse existing `DatasetTarget::into_api` and `ContainerizedOpts`; do not create datasets with a separate call.
- [ ] Run `cargo test -p nominal containerized`; expect named failures before implementation.
- [ ] Implement ContainerizedIngest with private BTreeMaps and consuming builders. Upload each named source through the canonical uploader, then construct Conjure ContainerizedOpts. Prevent automatic retries of ambiguous Conjure ingest submission: inspect actual Conjure client's retry configuration and construct a no-replay client for this submission if needed. Preserve existing native call behavior; do not make an unverified blanket claim that Conjure POSTs are safe.
- [ ] Implement acknowledgement-only ContainerizedSubmission; direct ingestion does not call get_ingest_job automatically. Add an HTTP test that returns an acknowledged RID while the metadata endpoint would fail, and assert only the submit endpoint is called.
- [ ] Extend IngestJob conversion with dataset RID, count, creator, created/start/end timestamps and URL using `utils::api_base_url_to_app_base_url`. Reuse existing wait status/error semantics. Add search/cancel methods and paginated query tests covering inclusive/exclusive bounds and workspace All omitting the filter.
- [ ] Implement DatasetFile as a catalog model exposing RID, ingest status, and baseline metadata needed to inspect/wait for outputs. Use generated catalog dataset-file union conversion explicitly for supported variants; preserve unknown variants as an error or explicit unknown rather than pretending a File Store resource is the same type.
- [ ] Test two-page job file enumeration and file states transitioning independently. `wait_for_job_files` must not enumerate before job Completed; snapshot waiting does not rediscover files. File failures/timeouts carry failing file RIDs.
- [ ] Add `ContainerizedIngest::with_scope_tags(scope_tags)` that merges defaults below already supplied caller tags. Demonstrate resolving an existing workbook dataset scope using current public scope data and invoking the same direct ingest method. Do not build an unrelated dataset-view subsystem solely to copy a Python wrapper.
- [ ] Run `cargo test -p nominal containerized`, `cargo test -p nominal job`, and `cargo test -p nominal dataset_file`; commit `feat: ingest extractor inputs and inspect resulting jobs`.

## C4: Typed batch items and request encoding

**Files:** ingest/batch/{mod,items,encode,tests}.rs. **Dependencies:** C3 job reference and P0 shared timestamp codec. No runtime-crate dependency.

Public contract:

```text
IngestClient::batch(existing_dataset_rid: impl Into<String>) -> IngestBatch
IngestBatch::add_tags(self, BTreeMap<String,String>) -> Self
IngestBatch::add_containerized(self, ContainerizedIngest) -> Result<Self>
IngestBatch::add_tabular(self, path, BatchTabular) -> Result<Self>
IngestBatch::add_avro_stream(self, path, BatchAvroStream) -> Result<Self>
IngestBatch::add_mcap(self, path, BatchMcap) -> Result<Self>
IngestBatch::add_journal_json(self, path, BatchJournalJson) -> Result<Self>
IngestBatch::add_dataflash(self, path, BatchDataflash) -> Result<Self>
IngestBatch::add_video(self, path, channel, BatchVideoTiming) -> Result<Self>
IngestBatch::submit(self, BatchOptions) -> Result<BatchSubmission>
```

Batch option structs live beside batch items because native options have different target/format semantics. Reuse Timestamp, FileType, upload options, and existing topic-selection semantics; do not reuse a native option type by silently discarding unsupported fields. Constructors/builders expose this complete baseline inventory:

| Type | Required / optional values |
| --- | --- |
| BatchTabular | Timestamp required; tag_columns, units, channel_prefix, channel_name_overrides, tags; CSV/Parquet/archive inference from Python-supported suffixes |
| BatchAvroStream | Optional numeric timestamp type defaulting to epoch nanoseconds, units, channel_prefix, tags; fixed timestamps series; no channel-name overrides |
| BatchMcap | Topics::All/Include(Vec)/Exclude(Vec), ignore_invalid_topics, tags |
| BatchJournalJson | Optional numeric Timestamp, optional channel (server default logs), tags |
| BatchDataflash | tags |
| BatchVideoTiming | Start(DateTime<Utc>) or FrameTimestamps(Vec<i64>); no scaling options because Python batch does not expose them |
| ContainerizedIngest | Nonempty sources in batch; extractor RID, arguments, optional Timestamp, tags |
| BatchOptions | failure_policy: FailFast or AllowPartial; max_uploads: NonZeroUsize default 4; runs_to_expand: Vec<String>; UploadOptions |

Batch video tags use `IngestBatch::add_video_with_tags(path, channel, timing, tags)`; `add_video` delegates with empty tags. Keep constructor signatures stable for CLI conversion.

Use `BatchTabular::new(Timestamp)` and `Default` for the other four format-option structs. Builders use `.tag(key, value)`, `.unit(channel, unit)`, `.tag_column(key, column)`, `.channel_prefix(prefix)`, `.channel_name_override(old, new)`, `.topics(Topics)`, `.ignore_invalid_topics(bool)`, `.channel(name)`, and `.timestamp(Timestamp)` only on the types that support those fields. BatchAvroStream instead exposes `.numeric_timestamp(BatchNumericTimestamp)` to avoid an arbitrary column. Define `BatchNumericTimestamp::{Epoch(TimeUnit), Relative { unit: TimeUnit, start: DateTime<Utc> }}` in items.rs and encode its fixed Avro series name centrally. BatchJournalJson's Timestamp must be numeric; reject ISO/custom during add_journal_json rather than silently rewriting them. Coordinator exposes a crate-private numeric check in the canonical Timestamp implementation. BatchOptions has `Default` plus `.failure_policy(FailurePolicy)`, `.max_uploads(NonZeroUsize)`, `.run_to_expand(rid)`, and `.upload_options(UploadOptions)`. `FailurePolicy` variants are `FailFast` and `AllowPartial`.

- [ ] Add table-driven encoder tests for all seven kinds and their complete option sets, using the Python `_ingest_builder.py` requests as expectations. Use fixed synthetic successful locations; do not invoke a network to test pure encoding.
- [ ] Run `cargo test -p nominal batch`; expect missing batch types/encoding failures.
- [ ] Implement a closed PendingItem enum. Each variant owns its named PendingUpload fields (file, video/sidecar, or source-name map). PendingUpload has a stable integer identity assigned at registration; two registrations of the same path are distinct. Completed locations are keyed by this identity, never vector position or path.
- [ ] Construct each final IngestItem in one pure variant match only after its uploads succeeded. Do not prebuild partial protobufs and patch source fields later. Retain per-item tags separately from request tags; send both maps so item overrides follow server semantics.
- [ ] Generate batch video sidecars in temporary owned files; do not write beside the caller's video. RAII ownership cleans them on builder drop and submission success/error. An empty timestamp vector fails before sidecar creation.
- [ ] Add a compile-fail doctest showing a batch cannot be submitted twice, and an ordinary test showing empty source map rejected by add_containerized. No mutable submitted boolean is needed with submit(self).
- [ ] Run `cargo test -p nominal batch` and `cargo test -p nominal --doc`; commit `feat: model composable batch ingest items`.

## C5: Bounded uploads, partial reports, and one-shot submission

**Files:** ingest/batch/upload.rs, mod.rs, tests.rs.

```rust
// Public result shapes; Error remains crate::Error, not a serialized DTO.
pub struct BatchSubmission {
    pub job: IngestJobRef,
    pub omitted: Vec<BatchItemFailure>,
}
pub struct BatchItemFailure {
    pub item_index: usize,
    pub failed_sources: Vec<BatchSourceFailure>,
    pub uploaded_sources: Vec<std::path::PathBuf>,
}
pub struct BatchSourceFailure {
    pub name: String,
    pub path: std::path::PathBuf,
    pub error: crate::Error,
}
```

The private orchestration helper accepts an upload closure/future for tests; do not introduce a public UploadProvider plugin trait. Private `(item_index, upload_id)` identities are sufficient. Return structured upload failures on total/default failure via a leaf error integrated by the coordinator, with the same source identity data.

- [ ] Build a fake uploader with deterministic successes/failures and an atomic active counter. Test maximum in-flight uploads never exceeds BatchOptions.max_uploads, repeated paths are uploaded separately, and one item with a failed sibling is omitted in its entirety.
- [ ] Test default-mode failure yields zero submission requests; partial mode submits surviving items once; zero survivors produces an error and zero submissions; a failed item reports successful sibling uploads. Preserve original item indices in reports.
- [ ] Run `cargo test -p nominal batch`; observe the new orchestration tests fail.
- [ ] Implement bounded futures using existing futures utilities. Bound the number of files and leave per-file multipart concurrency to UploadOptions; document their multiplicative upper bound. On fail-fast, stop scheduling new files and settle in-flight uploads so their cleanup paths run, then return without ingest submission. Do not claim remote rollback.
- [ ] Assemble only eligible completed items, request-level tags, existing dataset RID, and runs_to_expand into v2 IngestRequest. Submit through the mutation channel exactly once and return IngestJobRef directly from the response.
- [ ] Test a service UNAVAILABLE counts as one mutation request. Test acknowledged success does not issue get_ingest_job. Do not add hidden polling to BatchSubmission construction.
- [ ] Run `cargo test -p nominal batch` and all nominal tests; commit `feat: submit batch ingests with explicit partial outcomes`.
- [ ] Hand off SDK commit hashes, public API signatures, test evidence, and any generated-schema incompatibilities to coordinator/CLI worker. Do not mark platform behavior verified from mocks alone.
