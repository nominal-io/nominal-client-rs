# Extractor CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Use `gpt-5.6-terra` with `max` reasoning.

**Goal:** Expose extractor management, images, containerized and batch ingest, and reusable job operations through nomctl.

**Architecture:** clap arguments and versioned JSON DTOs convert once into SDK models. SDK methods own validation and network orchestration. New command families support human output and one-document JSON output without changing unrelated commands.

**Tech Stack:** Existing clap/anyhow/serde_json, add serde derive through coordinator; public nominal SDK methods from the client package plan.

---

## Ownership and prerequisites

Own `nominal-cli/src/commands/extractor/`, new `commands/ingest/containerized.rs`, `batch.rs`, `jobs.rs`, `contract.rs`, `render.rs`, and new CLI integration tests. Coordinator owns main.rs, command mod wiring, Cargo files, and the mechanical split of current ingest.rs into ingest/native.rs, args.rs, and mod.rs. Do not modify native format behavior while adding new commands.

L1 can run against the frozen SDK contracts from P0; handlers L2/L3 require the client implementation. Request extra public getters from the owner; never use protobuf reflection as a workaround for missing SDK data.

## Command contract

All new leaf commands accept `--json`. Existing global `--profile` remains authoritative. Resource families accept `--workspace RID`; job search additionally accepts mutually exclusive `--all-workspaces`. Default workspace resolution remains in SDK.

```text
nomctl extractor create NAME [--description TEXT] [--workspace RID] [--json]
nomctl extractor get RID [--workspace RID] [--json]
nomctl extractor search [--include-archived] [--file-extension EXT] [--workspace RID] [--json]
nomctl extractor update RID [--name TEXT] [--description TEXT] [--workspace RID] [--json]
nomctl extractor archive RID [--workspace RID] [--json]
nomctl extractor unarchive RID [--workspace RID] [--json]
nomctl extractor activate RID IMAGE_RID [--no-wait] [--timeout SECONDS] [--workspace RID] [--json]
nomctl extractor image register EXTRACTOR_RID TARBALL --contract FILE [--workspace RID] [--json]
nomctl extractor image get RID [--workspace RID] [--json]
nomctl extractor image search [--extractor RID] [--tag TEXT] [--status pending|ready|failed] [--workspace RID] [--json]
nomctl extractor image wait RID [--timeout SECONDS] [--workspace RID] [--json]
nomctl extractor image delete RID [--workspace RID] [--json]
nomctl ingest containerized EXTRACTOR_RID (--dataset RID | --name NAME)
  [--source ENV PATH]... [--argument ENV VALUE]... [--file-tag KEY VALUE]...
  [--timestamp-column COLUMN --timestamp-type SPEC] [--relative-to RFC3339]
  [--timestamp-json FILE] [--description TEXT] [--label TEXT]... [--property KEY VALUE]...
  [--no-wait] [--timeout SECONDS] [--json]
nomctl ingest batch FILE [--allow-partial] [--max-uploads N] [--no-wait] [--timeout SECONDS] [--json]
nomctl ingest job get RID [--json]
nomctl ingest job search [--dataset RID]... [--created-by RID]... [--status STATUS]...
  [--search-text TEXT] [--start-after RFC3339] [--start-before RFC3339]
  [--workspace RID | --all-workspaces] [--json]
nomctl ingest job wait RID [--timeout SECONDS] [--json]
nomctl ingest job cancel RID [--json]
nomctl ingest job files RID [--wait] [--timeout SECONDS] [--json]
```

Use existing two-token repeatable KEY VALUE conventions. Preserve strings with spaces and `=`; do not split them on `=`. Reject duplicate source environment keys; repeated tag/argument keys use last value, documented and tested. `--timeout` must be positive and is incompatible with `--no-wait`. `--relative-to` requires a numeric timestamp flag pair. `--timestamp-json` supplies richer/custom timestamp support and conflicts with all timestamp flags. New-dataset metadata requires `--name`. Batch has no new-dataset flag.

Default ingest behavior waits for the job, matching existing nomctl native ingest. `--no-wait` prints the acknowledged job RID without hydration. `job files --wait` waits for job completion and file completion; without it, return a current file snapshot. Activation default waits for image readiness; `--no-wait` means RequireReady, not activate a Pending image.

## JSON schemas, version 1

The CLI owns these input DTOs. Parse with serde `deny_unknown_fields` on structs, explicit enums for kinds, and reject schema_version other than 1 before any upload. Keep paths relative to the request/contract file's parent, including video timestamp sidecar inputs; command-line paths remain relative to current directory. Empty/malformed schemas fail with file path and field context. No permissive fallback to raw generated protobufs.

Image contract example (`schema_version`, `tag`, `output_format`, `default_timestamp`, `inputs` required; parameters defaults empty):

```json
{
  "schema_version": 1,
  "tag": "v1",
  "output_format": "manifest",
  "default_timestamp": {"column": "ts", "kind": "epoch", "unit": "nanoseconds"},
  "inputs": [{
    "name": "Recording",
    "environment_variable": "RECORDING",
    "description": "Flight recorder file",
    "file_suffixes": ["flight"],
    "required": true
  }],
  "parameters": [{"name": "Parts", "environment_variable": "PARTS", "required": false}]
}
```

Allowed output_format values: parquet/csv/avro_stream/manifest. Input/parameter description optional, required defaults false, input suffixes default empty. Registration never activates implicitly.

Timestamp object tagged by `kind`: epoch has column+unit; relative has column+unit+start RFC3339; iso8601 has column; custom has column+format and optional default_year/default_day_of_year. Unit names follow SDK TimeUnit; numeric-only batch formats reject ISO/custom and enforce the baseline's supported range via SDK validation. `--timestamp-json` uses this object directly without an additional schema wrapper.

Batch example:

```json
{
  "schema_version": 1,
  "dataset": "ri.scout.main.dataset.00000000-0000-0000-0000-000000000001",
  "tags": {"vehicle": "n1234"},
  "runs_to_expand": [],
  "items": [
    {
      "kind": "containerized",
      "extractor": "ri.ingest.main.containerized-extractor.00000000-0000-0000-0000-000000000002",
      "sources": {"RECORDING": "flight-42.flight"},
      "arguments": {"PARTS": "4"},
      "tags": {"phase": "flight"}
    },
    {
      "kind": "tabular",
      "path": "annotations.csv",
      "timestamp": {"column": "ts", "kind": "epoch", "unit": "nanoseconds"},
      "tag_columns": {},
      "units": {},
      "channel_name_overrides": {},
      "channel_prefix": "annotations/",
      "tags": {}
    }
  ]
}
```

Documentation uses visibly illustrative RIDs; tests generate syntactically valid fixed RIDs and must not invoke production. Schema field names are fixed by this plan, not extracted from protobuf field names. Every item has optional tags defaulting empty:

| kind | Additional fields |
| --- | --- |
| containerized | extractor, nonempty sources required; arguments default empty; timestamp optional |
| tabular | path, timestamp required; tag_columns/units/channel_name_overrides default empty; channel_prefix optional |
| avro_stream | path required; units default empty; channel_prefix and numeric timestamp optional (column absent, fixed by SDK) |
| mcap | path required; topics optional tagged `{kind: include|exclude, names: [...]}`; ignore_invalid_topics defaults false |
| journal_json | path required; channel and numeric timestamp optional |
| dataflash | path required |
| video | path, channel, timing required; timing tagged start with `at` RFC3339, or frames with `timestamps_file` path to an integer nanosecond JSON array |

Avro's timestamp object is numeric `{kind: epoch, unit: nanoseconds}` or `{kind: relative, unit: milliseconds, start: RFC3339}`, with no column field. Omission defaults to epoch nanoseconds. Batch video has no scaling fields, matching Python's batch API; runtime video scaling is a separate authoring capability.

JSON outputs are command-specific typed views, not Debug strings. Resource getters/create/update/activate emit a resource object, searches/files emit arrays, delete emits `{ "rid": "...", "deleted": true }`. Submission emits `{ "job_rid": "...", "dataset_rid": "...", "status": null, "omitted": [] }` immediately; after waiting status is the observed terminal value. Batch dataset_rid comes from the request. Unknown observed statuses are represented explicitly. Nonempty omitted entries contain item_index, failed_sources with name/path/message, and uploaded_sources. Partial mode success exits 0 but never hides omissions. Wait failure exits nonzero and preserves job RID in diagnostics so it can be inspected.

## L1: Parser and typed DTO boundaries

**Files:** commands/extractor/{mod,args,contract,render}.rs; commands/ingest/{contract,render}.rs; tests/extractor_cli.rs. Coordinator wires dispatch to args as it becomes available.

- [ ] Add parser tests for the command contract before handlers. Use clap::Parser::try_parse_from against the existing Cli test module or a test wrapper whose only responsibility is exposing the actual command enum.
- [ ] Cover two-token sources with spaces, duplicate source rejection, `--name`/`--dataset` exclusivity, half timestamps, timeout/no-wait conflict, invalid output format, unsupported schema version, and unknown JSON field. Errors occur before client construction/upload where possible.
- [ ] Run `cargo test -p nominal-cli extractor`; expect parser/DTO failures before code exists.
- [ ] Implement owned serde DTOs and TryFrom conversions to SDK builders. Read both contract and referenced timestamp files before invoking mutation. Do not turn the CLI into a second upload scheduler or reproduce image readiness checks.
- [ ] Add a no-network JSON schema test:

```rust
#[test]
fn image_contract_rejects_unknown_fields() {
    let json = r#"{
      "schema_version":1,"tag":"v1","output_format":"manifest",
      "default_timestamp":{"column":"ts","kind":"epoch","unit":"nanoseconds"},
      "inputs":[],"activate":true
    }"#;
    assert!(serde_json::from_str::<ImageContract>(json).is_err());
}
```

`ImageContract` is the actual DTO in extractor/contract.rs; place this unit test there, not in an integration file with inaccessible private types.

- [ ] Round-trip valid examples into SDK builders and inspect named public getters or capture resulting service requests. Add all seven batch item shapes and custom/relative timestamp cases.
- [ ] Run targeted tests and `cargo fmt --all -- --check`; commit `feat: define extractor CLI commands and JSON contracts`.

## L2: Resource and image handlers

**Files:** commands/extractor/{mod,images,render}.rs; tests/extractor_cli.rs. **Dependencies:** C1/C2.

- [ ] Add mock-service CLI tests for create/get/search/update/archive/unarchive/register/activate/delete; assert correct SDK request/workspace and that registration does not activate.
- [ ] Run `cargo test -p nominal-cli extractor`; expect handler failures.
- [ ] Implement direct SDK delegation. CLI fetches snapshots by RID before lifecycle operations because SDK preserves workspace on those snapshots. Keep explicit/required-field resolution in contract conversion and server lifecycle checks in SDK.
- [ ] Human mode prints concise resource fields including RID/tag/status/active image; JSON mode serializes dedicated views using getters. Include full image execution contract and timestamps in image inspection. Add Display mappings for statuses at the view boundary only if SDK lacks Display.
- [ ] Assert JSON stdout parses as a single document; progress and warnings are stderr. Delete and failed activation produce unambiguous results; no silent retries in CLI.
- [ ] Run targeted CLI tests and full `cargo test -p nominal-cli`; commit `feat: manage extractor images with nomctl`.

## L3: Direct ingest, batch, and job handlers

**Files:** commands/ingest/{containerized,batch,jobs,render}.rs; tests/ingest_cli.rs. **Dependencies:** C3–C5 and L1.

- [ ] Add end-to-end command mapping tests using fake services/uploader fixtures: direct input/arguments/tags/timestamps; batch common and item tags; job filters/cancel/files. No live profile required.
- [ ] Verify direct optional-input zero-source command is accepted; batch empty sources is rejected. Reuse SDK's distinct validations instead of a shared CLI nonempty-source rule.
- [ ] Run `cargo test -p nominal-cli ingest`; expect missing handler failures.
- [ ] Implement direct submission through upload_containerized and batch through consuming IngestBatch. `--allow-partial` selects FailurePolicy; `--max-uploads` must parse to a nonzero integer. Per-frame video timestamp files deserialize to Vec<i64>, preserving precision.
- [ ] Use acknowledged IngestJobRef for immediate output. Default waiting fetches/polls separately; any wait failure reports the acknowledged RID and exits nonzero without resubmitting.
- [ ] Implement job get/search/wait/cancel/files with SDK calls. Search All workspace omits scope; time bounds preserve inclusive lower/exclusive upper semantics. `files --wait` uses wait_for_job_files; no flag uses dataset_files only.
- [ ] Add partial-output JSON test proving omitted item index and both failed/successful sibling paths are present; human output must warn on stderr. Test all-omitted submission is nonzero and triggers no ingest call.
- [ ] Run `cargo test -p nominal-cli`, `cargo run -p nominal-cli -- extractor --help`, and `cargo run -p nominal-cli -- ingest --help`; expected new families present and existing native verbs preserved.
- [ ] Commit `feat: run extractor and batch ingests through nomctl`; hand coordinator exact commands and test evidence for final documentation.
