# Examples

Complete programs to copy and adapt. Each connects with the `default` profile that [API Keys Setup](../api-keys-setup.md) saves, and creates everything it uses in Nominal.

- [One asset per unit](asset-per-unit.md): an asset for each unit, with its own dataset to stream to.
- [One dataset for many units](shared-dataset.md): one dataset per tester, with each point tagged with its unit.
- [Simulation runs](simulation-runs.md): every execution of a model in one dataset, with a run for each.
- [Check a test against its limits](automated-checks.md): upload a test, create a run and a checklist, and count the violations.
- [Read channel data](read-channels.md): upload a test, then read two of its channels back into memory as CSV.
- [Ingest with an extractor](extractor-ingest.md): upload a file and ingest it with your workspace's extractor, through the REST API.
- [Put a file in a drive](drive-put.md): upload a file and place it at a path in a drive, through the REST API.
