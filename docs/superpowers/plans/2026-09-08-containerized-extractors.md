# Containerized Extractors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The user's execution preference is parallel `gpt-5.6-terra` agents with `max` reasoning.

**Goal:** Implement the complete Python containerized-extractor workflow in Rust, including authoring, management, direct/batch ingest, jobs, and nomctl.

**Architecture:** Extend existing client services without migrating native ingest. Build a standalone synchronous runtime. Freeze interfaces first, then run disjoint leaf-module work in parallel, with one integration owner.

**Tech Stack:** Rust 2024/MSRV 1.85; existing nominal-api 0.1349.0, tonic 0.13, Conjure, Tokio, serde, clap; runtime uses std, serde/serde_json, chrono, thiserror, tracing, and tempfile for atomic writes/tests.

---

## Read order and scope

Read `../specs/2026-09-08-containerized-extractors-design.md`, this coordinator plan, then your assigned package:

1. `2026-09-08-containerized-extractors-runtime.md` — R1–R5.
2. `2026-09-08-containerized-extractors-client.md` — C1–C5.
3. `2026-09-08-containerized-extractors-cli.md` — L1–L3.

This is a handoff, not a record of implemented code. All task boxes are initially unchecked. There has been one spec review; do not launch another spec review loop. Implementation testing and normal code review still apply.

Python source root: `/Users/drake/workspace/nominal-client`, baseline `2a4e588b47346396d4e81cd8211a37e72d52f954`. Rust root: `/Users/drake/workspace/nominal-client-rs`; original implementation baseline `d0be9b6`. Preserve existing public methods and native ingestion behavior. If either checkout has moved, compare the named source files against these commits before changing the parity target.

## Swarm protocol

- Coordinator/integrator is the only writer of root/package Cargo manifests, Cargo.lock, `nominal/src/core/mod.rs`, `nominal/src/lib.rs`, `nominal/src/core/client.rs`, `nominal/src/error.rs`, `nominal/src/core/wait.rs`, shared workspace/gRPC/timestamp infrastructure, `nominal-cli/src/main.rs`, and top-level command dispatch.
- Workers use isolated worktrees rooted at the coordinator's frozen interface commit. Use branch names `codex/extractors-runtime`, `codex/extractors-management`, `codex/extractors-ingest`, and `codex/extractors-cli`. Never change the user's source Python checkout.
- No worker commits or stages another worker's files. Workers submit a commit hash, owned-file list, test commands/results, interface deviations, and remaining gaps. Coordinator integrates in dependency order and resolves shared wiring.
- Do not share a mutable Cargo target directory between concurrently testing worktrees. Each worktree uses its own target directory or a unique `CARGO_TARGET_DIR`.
- Use `gpt-5.6-terra`, reasoning `max`, fresh bounded task prompts. Do not give agents the entire project and tell them to divide ownership themselves.
- Runtime and management start after P0. Direct ingest starts after management contracts are frozen; batch implementation may begin while management completes. CLI parsing starts after P0 and its handlers finish only after the relevant SDK APIs land.
- Never broaden into point-cloud support, image cloning, exit-code registration, Docker orchestration, or generic CLI serialization merely because generated schemas expose those concepts. Python-baseline parity is the acceptance boundary.

### Dependency graph

```text
P0 contracts/fixtures/wiring
  ├─ R1 → R2 → R3 → R4 → R5
  ├─ C1 → C2 ───────────────┐
  │    └→ C3 → C4 → C5 ───┤
  └─ L1 (DTO/parser) → L2 → L3
                           ↓
                     P1 integration → P2 verification
```

C3 depends on C1's models and extractor lookup, not on image-upload completion. C4/C5 own one batch subtree and run sequentially under one worker. L2 requires C1/C2; L3 requires C3–C5. With a coordinator plus three slots, start runtime, management, and ingest; launch CLI when management frees a slot. Avoid simultaneous workers in the same subtree.

### Copyable worker prompt

```text
Implement task PACKAGE/TASK on branch BRANCH using gpt-5.6-terra with max reasoning.
Read the approved extractor spec, coordinator plan, and named work package first.
Your writable ownership is exactly the task's Files list. Shared wiring belongs
to the coordinator. Use the frozen interfaces; report a necessary change before
editing another owner's files. Add the named failing behavioral tests, confirm
failure, implement, and run the package checks. Do not edit Python source or
regenerate expectations from the Rust implementation. Do not start another spec
review. Return commit hash, test evidence, public API changes, and parity gaps.
```

Replace PACKAGE/TASK and BRANCH with the assignment; include the exact owned paths in the dispatch message. The coordinator must make interface changes visible to all affected workers before they proceed.

## P0: Freeze interfaces, source fixtures, and shared integration points

**Owner:** Coordinator. **Files:** this plan's referenced contract inventory, root Cargo.toml/Cargo.lock, nominal-extractor/Cargo.toml, nominal/Cargo.toml, nominal-cli/Cargo.toml, shared files listed above, and new leaf `mod.rs` declarations needed by assigned workers.

- [ ] Confirm repository instructions and clean starting state; create isolated worktrees using using-git-worktrees. Record `git rev-parse HEAD`, `git status --short`, `cargo test --workspace`, and `cargo fmt --all -- --check`. Preserve unrelated changes. Report any pre-existing failure separately.
- [ ] Confirm the pinned API dependency exposes `nominal::registry::v2`, `nominal::ingest::v2`, Conjure `ContainerizedOpts`, job search/cancel, and catalog `get_dataset_files_for_job`. The cached 0.1349.0 sources already contain these surfaces; do not bump versions on assumption.
- [ ] Create `docs/superpowers/plans/extractor-parity.md` from the matrix below, adding exact Python test names as fixtures are captured. Include every public argument of the relevant methods, not merely method names.
- [ ] Capture runtime expected JSON from the Python baseline into `nominal-extractor/tests/fixtures/`. Read `tests/experimental/test_extractor.py` and use its declarations with temporary files; store semantic JSON plus sidecar arrays and provenance in `fixtures/README.md`. Never manufacture goldens from Rust output. Capture one mixed manifest, all three video scaling variants, repeated declarations, and relative/Avro timestamps.
- [ ] Freeze the public interfaces in the companion plans. Add compileable type definitions/module wiring first; no `unimplemented!()` production methods, no placeholder success results. Leaf implementation methods can land with their worker's first tests. Announce the interface commit before dispatching consumers.
- [ ] Add `nominal-extractor` to workspace membership. It must not depend on nominal, nominal-api, tonic, reqwest, or Tokio. Use existing version ranges for shared libraries; use tempfile 3 for local finalization and test directories. Add CLI serde derive and SDK test dependencies only when used by a named test.
- [ ] Extend `GrpcConnection` to retain a raw `Channel`; keep existing `channel()` returning the existing retry-enabled transport. Add `mutation_channel()` and `GrpcMutationTransport = InterceptedService<Channel, AuthInterceptor>`. Existing authenticated retry transport remains available for reads. Mutation service constructors select the raw channel explicitly.
- [ ] Add behavioral transport tests using a counting in-process tonic test service: one `Unavailable` mutation response yields exactly one request, while the existing retry test still retries. A create/register RPC gets the same no-replay construction as ingest submission. No path-name checks inside RetryService.
- [ ] Keep timestamp conversions in `nominal/src/core/ingest/timestamp.rs` or a focused child codec module. Add registry and v2 codecs beside Conjure conversion. Preserve ISO/custom/epoch/relative values; round-trip optional image defaults without inventing a value. Avoid duplicating timestamp matches in management and batch modules.
- [ ] Add shared WaitOptions in core/wait.rs as specified by the client package. Add default-workspace lookup returning a Workspace without changing existing resolve_workspace's public Result<()> signature. Keep configured/default resolution in one crate-private helper; resource snapshots retain its result.
- [ ] Thread cloned gRPC connection through IngestClient construction. Add `NominalClient::extractors()` and `NominalClient::container_images()` accessors using the existing client/snapshot pattern. Wire leaf modules as they land; no production behavior implemented here beyond these shared changes.
- [ ] Move the existing `nominal-cli/src/commands/ingest.rs` into `commands/ingest/native.rs`; create `ingest/mod.rs` dispatch and promote only reusable target/timestamp/pair parsing into `ingest/args.rs`. Preserve old argument names and help output. Keep this mechanical split separate from feature commits.
- [ ] Run `cargo fmt --all -- --check`, `cargo test --workspace`, and `git diff --check`; commit shared changes before workers rebase. Expected: existing tests pass, no new CLI behavior yet.

### Public contract ownership

| Shared concept | Canonical owner | Rule |
| --- | --- | --- |
| Authentication/retry channel | Coordinator, core/grpc.rs | Mutation channel is explicit; no blanket transport behavior changes |
| SDK timestamp | Coordinator, ingest/timestamp.rs | One conversion owner for Conjure, registry, ingest v2 |
| Runtime numeric timestamps | R1, runtime/timestamp.rs | No full-client dependency; manifest's narrower type |
| Extractor/image snapshots and queries | C1 | SDK types, private protobuf conversions |
| IngestJob and output DatasetFile snapshots | C3 | No File Store models reused |
| Batch item models | C4 | Existing dataset only; owned inputs and consuming submit |
| JSON input/output DTOs | L1 | CLI versioned schema, no protobuf JSON passthrough |
| Module exports/dependencies | Coordinator | Workers request additions, never race on shared files |

## Parity matrix / acceptance inventory

| Python source surface | Rust owner / target | CLI coverage | Verification |
| --- | --- | --- | --- |
| Client create/get/search extractor; update/archive/unarchive/refresh | C1 ExtractorsClient | L2 extractor verbs | Optional update presence, archived/file extension filters, pagination |
| Register image; get/search/refresh/delete/readiness/activate | C2 ContainerImagesClient + ExtractorsClient | L2 image verbs + activate | Immutable tags, READY/FAILED/unknown, registration does not activate |
| Input/parameter description, required flag, suffixes; image timestamp defaults | C1 models/codecs | L1 contract JSON | All fields, omitted optional values, unsupported register formats |
| Dataset.add_containerized | C3 IngestClient.upload_containerized | L3 ingest containerized | Existing/new target, sources, tags, arguments, timestamp override |
| Dataset scope add_containerized | C3 canonical target/tag helper | Direct CLI accepts resolved dataset/tags | Scope tags merged with caller winning; no second ingest engine |
| Job get/search/cancel/refresh/URL/full metadata | C3 job operations | L3 ingest job verbs | All Python filters, lower-inclusive/upper-exclusive times, workspace modes |
| Job dataset_files/as_files_ingested | C3 job_files + catalog DatasetFile | L3 job files --wait | Paginated snapshot, file failure, completion-before-enumeration workflow |
| Batch add_tags/add_containerized/submit | C4/C5 | L3 ingest batch JSON | Consume once, per-item override, run expansion, failures/partial mode |
| Batch tabular/Avro/MCAP/log/DataFlash/video | C4/C5 | L1 typed item DTOs + L3 | Every Python option, including units/names/selection and video sidecar |
| Runner environment/context/input/params | R1/R2 | Local Rust examples | Registered authoritative metadata, fallback discovery, required/optional |
| Single-file set_output/run | R2 | Example binary | Missing/second output, contract mismatch, nonzero failure |
| Manifest tabular/Avro/logs | R3 | Example binary | Extensions/gzip, format-specific options, relative/epoch units |
| Manifest video + scaling + sidecars | R4 | Example binary | Video-only/mixed, all scaling modes, repeat naming, collisions |
| Manifest build/finalize/warnings | R3/R4 | Local tests | No partial declaration mutation, scratch warnings, atomic final write |
| Optional system metadata | R1 | Local fixture | Job/dataset/tags/full timestamp inspection, empty and malformed values |

Python decorator metadata copying is Python-specific and has no runtime Rust analogue; document this rather than adding macros. Python's unsupported output registration formats and point-cloud batch omission remain unsupported. Generated image cloning and structured exit-code mappings are outside this baseline's public Python operations.

## P1: Integrate work packages

- [ ] Merge R1–R5, C1–C5, and L1–L3 only after each package's targeted checks pass. Resolve shared wiring centrally. Run `cargo check --workspace --all-targets` after each merge; do not wait until every branch is merged to discover incompatible signatures.
- [ ] Update exports in `nominal/src/core/mod.rs` following existing conventions; examples import SDK feature types from `nominal::core`, not an invented crate-root re-export policy.
- [ ] Verify public fallible APIs preserve errors: no missing RID converted to empty string, no unknown image status treated as ready, no failed metadata fetch hiding a successful submission RID.
- [ ] Check that JSON mode prints exactly one result document and errors/progress use stderr. Confirm command help recursively includes new families using the existing full-help command.
- [ ] Check new production file sizes with `wc -l` and inspect any file approaching 1,000 lines. Extract by cohesive responsibility, not one function per file. Keep small pure conversions with the types they convert.
- [ ] Commit integration with a message describing final behavior and validation, not worker history.

## P2: Final evidence and documentation

- [ ] Run `cargo fmt --all -- --check` (exit 0), `cargo test --workspace --all-targets` (all tests pass), `cargo clippy --workspace --all-targets -- -D warnings` (no introduced warnings), and `git diff --check` (exit 0). Record any baseline lint incompatibility without disguising it as a new failure.
- [ ] Run `cargo test -p nominal-extractor --doc` for invalid mode examples and `cargo tree -p nominal-extractor --edges normal` to confirm its dependency boundary.
- [ ] Run both local extractor examples with injected environment directories; inspect generated manifest/sidecars against expected fixtures. No Docker or authentication is necessary for these checks.
- [ ] Add `docs/containerized-extractors.md` with SDK management, registration-contract JSON, direct and batch ingest, jobs, local tests, and complete multi-stage Dockerfile examples. Link from root README and runtime README. Give an explicit image tag/version bump workflow.
- [ ] Add an opt-in platform smoke test recipe using a named test profile. Run only in an explicitly authorized test workspace: create unique extractor/dataset, register/activate single-file image, ingest; register/activate manifest image, ingest mixed/video-only cases; inspect files; clean up created resources where service supports it. Never delete pre-existing resources. If unavailable, report not run and preserve exact commands/configuration needed.
- [ ] Mark parity rows complete only with named tests/evidence. Deliver commit hashes, all test results, schema/backend limitations, and any unrun platform checks. Do not call partial telemetry or direct-only support full parity.

## Launch instruction for the user

```text
Implement docs/superpowers/plans/2026-09-08-containerized-extractors.md.
Use a swarm of gpt-5.6-terra agents at max reasoning. Complete P0 first, then
dispatch the runtime, management, and ingest lanes with disjoint ownership.
Start the CLI lane when a slot is free. Follow the dependency graph, integrate
centrally, and finish all parity rows and validation. The spec has already had
its requested single thermo-nuclear review; do not repeat that review loop.
```
