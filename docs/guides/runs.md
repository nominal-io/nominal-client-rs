# Runs

{.lead}
A [run](https://dev.nominal.io/platform/data-model/#runs) marks the time span of one test, such as a flight or an engine firing. Its data comes from its assets, or from datasets you add to the run.

## Create a run on an asset

A run on an [asset](assets.md) uses the asset's data, so you don't add the data to the run.

```rust
use chrono::{DateTime, Utc}; // cargo add chrono
use nominal::core::RunCreate;

let client = nominal::NominalClient::from_profile("default")?;

let start: DateTime<Utc> = "2026-01-01T12:00:00Z".parse()?;
let end: DateTime<Utc> = "2026-01-01T12:10:00Z".parse()?;
let run = client
    .runs()
    .create(
        RunCreate::new("Flight acceptance test", start)
            .end(end) // leave out while the test is still going
            .assets(["<ri.scout.instance.asset.identifier>"]) // one or more assets
            .labels(["acceptance"]),
    )
    .await?;
println!("{}", run.nominal_url());
```

## Create a run from datasets

A run without an asset needs data of its own. Add each dataset under a name, as on an [asset](assets.md#add-data-with-data-scopes):

```rust
let run = client.runs().create(RunCreate::new("High precipitation flight", start).end(end)).await?;
client.runs().add_dataset(run.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>").await?;
```

{rust:method}`add_video <RunsClient::add_video>` adds a video, and {rust:method}`add_data_sources <RunsClient::add_data_sources>` adds several datasets and videos at once.

:::{note}
Adding data to a run on several assets returns an error. Add the data to its assets instead.
:::

## Find runs

{rust:method}`search <RunsClient::search>` finds runs by label, property, name, run number, or start or end time. Combine filters with {rust:method}`and <RunQuery::and>`, {rust:method}`or <RunQuery::or>` and {rust:method}`not <RunQuery::not>`:

```rust
use nominal::core::RunQuery;

// Acceptance runs from 2026-01-01T12:00:00Z on, except aborted ones
let runs = client
    .runs()
    .search(RunQuery::and([
        RunQuery::label("acceptance"),
        RunQuery::start_time_inclusive("2026-01-01T12:00:00Z".parse()?),
        RunQuery::not(RunQuery::label("aborted")),
    ]))
    .await?;

// One run, by RID
let run = client.runs().get("<ri.scout.instance.run.identifier>").await?;
```

## Update a run

{rust:method}`update <RunsClient::update>` changes only the fields you set. Labels and properties you set replace the ones the run has, as on an [asset](assets.md#update-an-asset).

```rust
use nominal::core::RunUpdate;

// End a run that's still going
let run = client.runs().update(run.rid(), RunUpdate::new().end(Utc::now())).await?;
```

## Archive a run

{rust:method}`archive <RunsClient::archive>` hides a run from the UI and from searches, without deleting it. {rust:method}`unarchive <RunsClient::unarchive>` brings it back.

```rust
client.runs().archive(run.rid()).await?;
```
