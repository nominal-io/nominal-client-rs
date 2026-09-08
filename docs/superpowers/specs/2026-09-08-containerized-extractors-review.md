# Containerized extractor spec: single maintainability review

Reviewed the original specification in commit `bd2e1a3` against both local repositories. One independent review was performed using Terra at max reasoning. Findings were applied directly; there was no second review round. A document consistency check after editing is not a repeat architectural audit.

| Priority | Finding in original spec | Applied resolution | Planned verification |
| --- | --- | --- | --- |
| P0 | Reusing the global retry transport could replay batch submissions | Explicit authenticated mutation channel; acknowledged RID returned before hydration | P0 counting service; C3/C5 one-attempt and no-hydration tests |
| P1 | Direct new/existing targets and existing-only batch conflated | Keep Conjure direct ingestion; v2 batch accepts existing RID only | C3 target tests; C4 consuming existing-dataset builder |
| P1 | Batch upload/item identity and partial results underspecified | Owned variant inputs, stable registration IDs, structured omitted-item report | C4 encoder tests; C5 sibling failure/concurrency/partial tests |
| P1 | Duplicate manifest declaration wording could reject supported behavior | Preserve ordered repeated entries; accounted-file set only tracks warnings | R3 repeated table; R4 repeated video naming |
| P1 | Video timing surface omitted ending timestamp and scale factor | Typed start/per-frame choice and three scale variants | R4 goldens for every scaling variant |
| P1 | Empty sources differ between direct and batch Python APIs | Direct accepts none if none required; batch rejects empty | C3 optional-source regression; C4 empty-item test; L3 parser behavior |
| P1 | Image message lacks workspace needed by later operations | Snapshot retains resolved workspace; lifecycle methods use it | C1 A-vs-B workspace regression and C2 activation pair validation |
| P2 | Successful submit followed by failed metadata read can lose RID | New submission result contains lightweight IngestJobRef; explicit hydration | C3/C5 response acknowledgment tests |

Source evidence: Python `_context.py` declaration/video code; `_video_types.py` scaling union; `_ingest_builder.py` pending-input ownership and submit; `tests/core/test_dataset_add_containerized.py` empty-source case; `container_image.py` workspace preservation. Rust evidence: `core/grpc.rs` RetryService/GrpcConnection and `core/ingest/mod.rs` submit-followed-by-get behavior. The original findings refer to the pre-edit spec; the revised spec changes line numbering.

Additional boundary decisions made while applying the findings: keep dataset-file models in catalog, preserve snapshot file-wait semantics, make sidecar writes non-clobbering and declaration state transactional, scope versioned CLI DTOs to the CLI, and assign shared wiring to one coordinator. These are captured in the spec and work packages.

Review disposition: the identified design issues have concrete resolutions in the revised documents. This is not an approval of unimplemented code or a claim that platform integration tests have passed.
