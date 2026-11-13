# Runs

{.lead}
A run names a span of time for one test or analysis, such as a flight or a test-stand firing. It references data rather than copying it.

## Create a run on an asset

When the data already belongs to an [asset](assets.md), create the run with the asset and the test's time window. The run sees the asset's datasets through their data scopes, so you don't add them again.

```rust
use chrono::{TimeZone, Utc};
use nominal::core::RunCreate;

let start = Utc.with_ymd_and_hms(2026, 9, 10, 14, 38, 0).unwrap();
let end = Utc.with_ymd_and_hms(2026, 9, 10, 14, 48, 0).unwrap();
let run = client
    .runs()
    .create(
        RunCreate::new("Flight acceptance test", start)
            .end(end)
            .assets(["<ri.scout.instance.asset.identifier>"])
            .labels(["acceptance"]),
    )
    .await?;
println!("{}", run.nominal_url());
```

Times are `chrono::DateTime<Utc>`. Leave out `end` for a test that's still running. For a run across several assets, list each in `assets`.

## Create a run from datasets

Without an asset, add each dataset to the run under a ref name:

```rust
let run = client.runs().create(RunCreate::new("High precipitation flight", start).end(end)).await?;
client.runs().add_dataset(run.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>").await?;
```

The ref name, here `"telemetry"`, is the dataset's name within the run. Use the same ref name for the same kind of data on every run, so workbook templates and checklists work across runs. `add_video` and `add_data_sources` add videos, or several sources at once, the same way.

`run.data_sources()` returns only data sources added to the run itself. A run across several assets has none: add data to its assets instead.

## Find runs

`search` takes a `RunQuery`, which can also filter by time and run number, and negate a query with `RunQuery::not`:

```rust
use nominal::core::RunQuery;

let runs = client
    .runs()
    .search(RunQuery::and([
        RunQuery::label("needs-review"),
        RunQuery::start_time_inclusive(Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap()),
        RunQuery::not(RunQuery::label("reviewed")),
    ]))
    .await?;
```

To get one run, use the RID from its page in Nominal:

```rust
let run = client.runs().get("<ri.scout.instance.run.identifier>").await?;
```

## Update a run

`update` changes only the fields you set, and replaces each of them. To close a run that's still going, set its end:

```rust
use nominal::core::RunUpdate;

let run = client.runs().update(run.rid(), RunUpdate::new().end(Utc::now()).labels(["reviewed"])).await?;
```

Changing a run's start or end changes the time span it shows, not the stored data.

## Archive a run

`archive` hides the run without deleting its data. `unarchive` restores it.

```rust
client.runs().archive(run.rid()).await?;
```
