# Examples

Complete programs, each a `main.rs` you can run with `cargo run` after the [quickstart](../index.md)'s setup. Each one shows a way to organize test data from the [data model guides](https://dev.nominal.io/platform/data-model/paradigms/), and uses the `default` profile.

- [One asset per unit](asset-per-unit.md): an asset and a dataset for each unit, streamed to separately.
- [One dataset for many units](shared-dataset.md): one dataset per tester, with each point tagged by unit.
- [Simulation runs](simulation-runs.md): executions of a model on one timeline, with a run for each.
- [Check a test against its limits](automated-checks.md): upload, create a run, and run a limit check on it.
