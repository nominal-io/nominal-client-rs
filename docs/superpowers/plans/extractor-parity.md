# Extractor implementation and parity evidence

Implemented on `codex/containerized-extractors` in an isolated execution worktree.
The execution used Astra workers with low reasoning, as requested after the
original Terra launch plan was approved. Python baseline:
`2a4e588b47346396d4e81cd8211a37e72d52f954`; Rust API dependency remains `0.1349.0`.

This is the execution record for P0–P2, R1–R5, C1–C5 and L1–L3. The original
package checklists describe the planned sequence; this record identifies the
implemented contracts, actual checks and deviations instead of claiming every
suggested fixture or external check was executed literally.

## Client and CLI contract inventory

| Python operation / arguments | Rust surface | nomctl | Named local evidence |
| --- | --- | --- | --- |
| Create extractor: name, description, workspace; get/refresh RID | `ExtractorCreate`, `ExtractorsClient::{create,get,refresh,in_workspace}` | `extractor create/get --workspace` | `create_and_update_preserve_full_request_and_service_errors`, `extractor_refresh_uses_snapshot_workspace_and_missing_response_is_error` |
| Search: include archived, file extension, workspace; paginated results | `ExtractorQuery`, `ExtractorsClient::search` | `extractor search` | `extractor_search_maps_filters`, SDK pagination implementation |
| Update name/description/archive flag; archive/unarchive | `ExtractorUpdate`, lifecycle methods | `extractor update/archive/unarchive` | `extractor_update_preserves_unset_and_false`, `extractor_handlers_preserve_workspace_and_update_fields` |
| Register: extractor, tarball path, immutable tag, format, required timestamp column/type, input name/env/description/suffixes/required, parameter name/env/description/required | `ImageRegistration`, `FileExtractionInput`, `FileExtractionParameter`, `ContainerImagesClient::register` | `extractor image register --contract` | `registration_encodes_completed_upload_and_complete_contract`, `preflight_rejects_invalid_contract_and_timestamp_before_file_or_network`, `extraction_contract_roundtrips` |
| Image get/refresh/delete; search extractor/tag/status/workspace; readiness interval | `ContainerImagesClient`, `ContainerImageQuery`, `WaitOptions` | `extractor image get/search/delete/wait` | `image_search_follows_pages_and_ands_filters`, `extractor_get_image_search_and_wait_map_arguments`, `waiting_unknown_status_rejects_promptly_without_deadline_or_activation` |
| Activate image RID/resource, optionally poll until ready | `ExtractorsClient::activate`, `Activation` | `extractor activate`, `--no-wait`, `--timeout` | `activation_refreshes_pending_then_ready_and_mutates_once_in_snapshot_workspace`, `activation_rejects_pending_failed_unknown_without_mutation`, `mismatch_rejects_before_any_request` |
| Direct ingest: extractor, sources, arguments, tags, optional timestamp column/type, dataset destination | `ContainerizedIngest`, `DatasetTarget::{New,Existing}`, `upload_containerized` | `ingest containerized`, dataset/new-dataset, two-token source/argument/tag, timestamp flags/JSON | `containerized_acknowledgement_does_not_hydrate_job`, `containerized_preflight_empty_sources_depend_on_active_contract`, `malformed_destination_preserves_acknowledged_job_identity` |
| Dataset-scope defaults with explicit tags winning | `with_scope_tags`; existing run/workbook lookup composed with canonical direct ingest | Resolved dataset and tags supplied to direct command | `containerized_scope_defaults_do_not_replace_caller_tags`; compiling workbook→run→dataset doctest |
| Job RID/status/type/origin files/dataset/file count/creator/created/start/end/URL, refresh/get/cancel | `IngestJob`, `get_ingest_job`, `cancel_ingest_job` | `ingest job get/cancel/wait` | Job conversion tests; CLI `JobView` typed serialization; existing wait tests |
| Search dataset RIDs, creator RIDs, statuses, text, inclusive lower/exclusive upper start time, default/specific/all workspace | `IngestJobQuery`, `WorkspaceSelection`, `search_ingest_jobs` | `ingest job search` | `job_query_all_omits_workspace_and_preserves_bounds`, two-page HTTP search fixture in `job_query.rs` |
| Job files snapshot; file wait interval/completion/failure; file identity, status, name, dataset, timestamp, tags, bounds, size | `dataset_files`, `wait_for_job_files`, `DatasetFile`, `CatalogClient::wait_for_dataset_files` | `ingest job files [--wait]` | `job_files_pages_then_waits_independent_fixed_snapshot`, `dataset_file_completion_requires_success`, `job_file_wait_spends_one_deadline_and_keeps_discovered_file_identity` |
| Batch destination, request tags, run expansion, allow partial | owned `IngestBatch`, consuming `submit`, `BatchOptions`, structured failure/omission reports | `ingest batch`, `--allow-partial`, `--max-uploads` | `batch_rpc_partial_omits_failed_item_and_default_submits_nothing`, `batch_upload_concurrency_is_bounded_and_fail_fast_stops_scheduling`, compile-fail reuse doctest |
| Tabular path, timestamp column/type, tag columns, units, prefix, name overrides, tags; CSV/Parquet gzip/archive formats | `BatchTabular`, `add_tabular` | `kind: tabular` | `batch_all_formats_encode_complete_options`, `batch_python_suffix_and_fixed_format_parity` |
| Avro path, numeric timestamp type/default epoch ns, units, prefix, tags | `BatchAvroStream`, `BatchNumericTimestamp`, `add_avro_stream` | `kind: avro_stream` | Same all-format tests; typed numeric contract rejects invalid string timestamp |
| MCAP path, include/exclude topics, ignore invalid topics, tags | `BatchMcap`, `Topics`, `add_mcap` | `kind: mcap` | Same all-format and CLI conversion tests |
| Journal JSON path, optional channel/timestamp column/type, tags | `BatchJournalJson`, `add_journal_json` | `kind: journal_json` | Same all-format tests; `extractor_batch_rejects_empty_sources_and_string_journal_timestamp` |
| DataFlash path, tags | `BatchDataflash`, `add_dataflash` | `kind: dataflash` | Same all-format tests |
| Video path, channel, start time or integer frame ns timestamps, tags | `BatchVideoTiming`, `add_video_with_tags` | `kind: video`, start/frames schema | Same all-format tests; `extractor_batch_all_kinds_convert_without_network` verifies relative frame path and i64 precision |
| Batch extractor sources/arguments/timestamp/tags | `add_containerized` | `kind: containerized` | `batch_uploads_keep_repeated_path_identities`, `batch_failed_sibling_omits_whole_item`, duplicate JSON source regression |

The management snapshots expose optional metadata and preserve unknown enum
values rather than defaulting to READY or inventing timestamps. Registry and
batch timestamp conversion live beside the existing SDK timestamp owner.
Mutation clients explicitly use non-retrying transports. `mutation_unavailable_is_attempted_once`
and `batch_rpc_ack_is_not_hydrated_and_unavailable_is_not_replayed` count real
loopback RPCs. Successful image registration uses the existing multipart uploader;
its complete request encoding and preflight are tested separately from multipart
transport. A full successful multipart→CreateImage platform flow was not run.

## Authoring contract inventory

`nominal-extractor` is synchronous and separate from the platform SDK. Entrypoints
are ordinary callbacks with `SingleFileContext` or `ManifestContext`; no procedural
macro crate, async runtime, network client or generated API dependency is added.

| Python behavior / arguments | Rust surface and evidence |
| --- | --- |
| Registered input metadata authoritative, display/env lookup, sole input, fallback directory discovery | Context `input/inputs/sole_input`; `metadata_order_and_empty_authoritative_contract`, `fallback_parameters_and_discovery` |
| Registered parameters display/env lookup, absent optional, required, malformed values | Generic `param<T>/optional_param<T>`; `registered_parameter_name_resolves_and_parses`, `malformed_metadata_is_contextual` |
| Optional job RID, dataset RID, tags, full timestamp metadata; absent/null/empty handling | Context metadata getters; `complete_timestamp_inspection_and_unknown_preservation`, `null_json_metadata_matches_absence` |
| Single-file set output, once only, no missing output, registered format check, failure exit | `run_single_file`, injected-map runner; `no_output_and_second_output`, `mismatch_precedes_author_and_author_failure_does_not_finalize`, subprocess runner test |
| Tabular path, tag columns, prefix, paired numeric timestamp column/type | `TabularOutput`; `python_golden_complete_manifest`, `four_units_and_all_supported_extensions` |
| Avro path/gzip, prefix, numeric timestamp type, fixed timestamps series | `AvroStreamOutput`; Python semantic golden and compile-fail timestamp type doctest |
| Journal JSON path/gzip, paired numeric timestamp column/type | `JournalJsonOutput`; golden and compile-fail unsupported prefix doctest |
| Video path, channel, start timestamp, ending timestamp/true frame rate/scale factor, frame ns vector | `VideoOutput`, `VideoTiming`, `VideoScale`; Python semantic golden includes every scale and frame sidecars |
| Repeat same path; sidecar index includes prior start-timed declarations | `nested_repeats_have_python_names` and Python fixture |
| Sidecar collision protection; rejected declarations do not mutate; atomic final manifest; scratch warning | `collision_does_not_overwrite_or_record_rejected_entry`, `invalid_video_rejections_do_not_mutate`, `rejected_declaration_preserves_state_and_existing_manifest_replaced` |
| Canonical containment/symlink escape, relative path serialization, reserved manifest filename | `symlink_escape_rejected`, `literal_backslash_filename_is_not_a_directory_separator`, `symlink_alias_extension_is_validated_without_changing_manifest_identity`, `non_utf8_output_identity_is_rejected_instead_of_changed` |
| Real callback process success/failure and authoritative video input metadata | `examples_execute_csv_and_propagate_failure_as_nonzero_exit` builds/runs both examples; registered-input mixed manifest regression |

`tests/fixtures/python_manifest.json` was produced by the Python implementation,
with the exact generator and baseline recorded alongside it. Relevant Python tests
in `tests/experimental/test_extractor.py` include
`test_manifest_entry_carries_what_the_output_declared`,
`test_avro_stream_accepts_gzipped`,
`test_param_resolved_by_registered_display_name`,
`test_manifest_video_from_start_carries_channel_and_starting_timestamp`,
`test_manifest_video_frame_timestamps_writes_sidecar`, and
`test_manifest_relative_path_uses_forward_slashes`. Additional Rust path tests
cover literal backslashes, symlink aliases and unrepresentable UTF-8 identities.

## Verification and reviews

Final command results are recorded here after integration. Runtime fixture videos
are metadata-only bytes, not playable media. The subprocess examples exercise
local file production without Docker or a Nominal account. Three SDK guide
snippets were extracted into a temporary example and compiled successfully; the
temporary source was then removed.

- Baseline: 103 SDK tests, 16 CLI tests and 13 SDK doctests passed.
- Integrated tests before final review fixes: 144 SDK, 38 CLI and 20 runtime tests passed.
- Final CLI worker check: 40 tests plus strict CLI clippy passed.
- Integrated doctests: 15 SDK (including consuming-batch compile-fail) and two runtime compile-fail tests passed.
- Dependency inspection: `cargo tree -p nominal-extractor --edges normal` confirms no nominal/nominal-api/tonic/reqwest/Tokio dependency.
- Final `cargo test --workspace --all-targets`: 144 SDK, 40 CLI and 20 runtime tests passed (204 total).
- Final `cargo test --workspace --doc`: 15 SDK and two runtime doctests passed (17 total).
- Final `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` and `git diff --check`: passed.

Independent reviews covered runtime/shared code, management integration, SDK
spec compliance and maintainability, and CLI spec compliance and maintainability.
Findings corrected include path identity changes, missing resource IDs, unknown
image readiness, acknowledged job retention, file deadline diagnostics, workbook
scope documentation, and duplicate JSON source keys. A registered-input example
regression was additionally reproduced and fixed during final documentation QA.

## Explicit boundaries and unrun checks

- Full Python feature inventory is implemented with Rust-native ownership and typed
  builders. Python decorator function metadata copying has no Rust counterpart.
- Batch v2 accepts existing datasets; direct containerized ingest accepts new or
  existing datasets. Native ingest remains on its existing path.
- Workbook scopes expose assets/runs, not dataset-view filters. The documented
  composition resolves a selected run's attached dataset; scope tags are explicit.
- No point-cloud batch item or unsupported image output registration was added.
- Full CLI HTTP upload/job orchestration fixtures reuse SDK boundary coverage;
  CLI tests focus on conversion, flags, output and management RPC delegation.
- Docker build, playable-video decode, MSRV execution, package publication and live
  registration/ingestion were not run. See `docs/containerized-extractors.md` for
  the opt-in test-profile smoke recipe. Manifest videos require backend support.
- Uploaded objects are not rolled back after partial failure. Generated local
  sidecars are cleaned up. Submission is not replayed after ambiguous failure.
