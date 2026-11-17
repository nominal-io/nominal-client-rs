# Runs

{.lead}
A [run](https://dev.nominal.io/platform/data-model/#runs) marks the time span of one test, such as a flight or an engine firing. To create a run, you need an [asset](assets.md) on which the test was executed.

To decide how runs fit the way you test, see [Organize and view your data](https://dev.nominal.io/platform/data-model/paradigms/).

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Create a run on an asset

A run on an asset marks a time range on the asset's data, not on the data itself.

```rust
use chrono::{DateTime, Utc}; // cargo add chrono
use nominal::core::RunCreate;

let start: DateTime<Utc> = "2026-01-01T12:00:00Z".parse()?;
let end: DateTime<Utc> = "2026-01-01T12:10:00Z".parse()?;
let run = client
    .runs()
    .create(
        RunCreate::new("Flight acceptance test", start)
            .end(end) // optional - remove if the test is ongoing
            .assets(["<ri.scout.instance.asset.identifier>"]) // one or more assets
            .labels(["acceptance"]), // optional - allows for easy filtering of runs by label
    )
    .await?;
println!("{}", run.nominal_url());
```

## Linking a run directly to data

A run's data always lives on an asset. On a run with one asset, adding data to the run is the same as adding it to the asset.

:::{note}
If a run is already associated with more than one asset, adding data to the run returns an error, because Nominal can't tell which asset the data belongs to. Add the data to each asset instead.
:::

A run created without an asset gets an auto-created asset, so you can add data to the run before you've chosen an [approach for your data](https://dev.nominal.io/platform/data-model/paradigms/). Once you have, promote the auto-created asset to a full asset in the Nominal UI.

```rust
let run = client.runs().create(RunCreate::new("High precipitation flight", start).end(end)).await?;
client.runs().add_dataset(run.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>").await?;
```

{rust:method}`add_video <RunsClient::add_video>` adds a video, and {rust:method}`add_data_sources <RunsClient::add_data_sources>` adds several datasets and videos at once.

## Fetch runs

If the RID of a run is known, you can fetch that run directly without needing to search.

```rust
let run = client.runs().get("<ri.scout.instance.run.identifier>").await?;
```

## Find runs

{rust:method}`search <RunsClient::search>` finds runs by label, property, name, run number, or start or end time. Combine filters with {rust:method}`and <RunQuery::and>`, {rust:method}`or <RunQuery::or>` and {rust:method}`not <RunQuery::not>`:

```rust
use nominal::core::RunQuery;

// Every run, newest first
let runs = client.runs().list().await?;

// Names that contain "acceptance", in any case
let runs = client.runs().search(RunQuery::substring_match("acceptance")).await?;

// Acceptance runs from 2026-01-01T12:00:00Z on, except aborted ones
let runs = client
    .runs()
    .search(RunQuery::and([
        RunQuery::label("acceptance"),
        RunQuery::start_time_inclusive("2026-01-01T12:00:00Z".parse()?),
        RunQuery::not(RunQuery::label("aborted")),
    ]))
    .await?;
```

## Update a run

{rust:method}`update <RunsClient::update>` changes the fields you include in the run update request by overwriting those fields. That is, `update(labels)` will _not_ touch properties, but it _will_ overwrite the current set of labels with the ones you provide.

```rust
use nominal::core::RunUpdate;

// The labels you set replace the run's labels, so start from its current ones
let mut labels = run.labels().to_vec();
labels.push("aborted".to_string());
let run = client.runs().update(run.rid(), RunUpdate::new().labels(labels)).await?;

// End a run that's still going
let run = client.runs().update(run.rid(), RunUpdate::new().end(Utc::now())).await?;
```

## Archive a run

{rust:method}`archive <RunsClient::archive>` hides a run from the UI and from searches, without deleting it. {rust:method}`unarchive <RunsClient::unarchive>` brings it back.

```rust
client.runs().archive(run.rid()).await?;
```
