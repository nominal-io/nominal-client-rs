# Datasets

{.lead}
A [dataset](https://dev.nominal.io/platform/data-model/#data-model-dataset) holds your test data as [channels](channels.md), such as `altitude` or `temperature`. Files you upload and points you stream add to it over time.

## Create a dataset

```rust
use nominal::core::DatasetCreate;

let client = nominal::NominalClient::from_profile("default")?;

let dataset = client
    .catalog()
    .create_dataset(
        DatasetCreate::new("Flight telemetry")
            .description("Quadcopter 456 flight logs")
            .labels(["flight-test"])
            .channel_delimiter("."), // show motor.temperature and motor.current in a tree, under motor
    )
    .await?;
println!("{}", dataset.nominal_url());
```

To create the dataset as you upload a file, see [Upload files](upload.md). To show the dataset on an asset or a run, add it as in [Assets](assets.md#add-data-with-data-scopes) or [Runs](runs.md#create-a-run-from-datasets).

## Find datasets

{rust:method}`search_datasets <CatalogClient::search_datasets>` finds datasets by label, property or name:

```rust
use nominal::core::DatasetQuery;

let datasets = client
    .catalog()
    .search_datasets(DatasetQuery::and([
        DatasetQuery::label("flight-test"),
        DatasetQuery::substring_match("telemetry"), // in the name, in any case
    ]))
    .await?;

// One dataset, by RID
let dataset = client.catalog().get_dataset("<ri.catalog.instance.dataset.identifier>").await?;
```

To list a dataset's channels, see [Channels](channels.md). To read its data, see [Get data out with SQL](sql.md).

## Update a dataset

{rust:method}`update_dataset <CatalogClient::update_dataset>` changes only the fields you set:

```rust
use nominal::core::DatasetUpdate;

// The properties you set replace the dataset's properties
let dataset = client
    .catalog()
    .update_dataset(dataset.rid(), DatasetUpdate::new().properties([("campaign", "2026-09")]))
    .await?;
```

Labels and properties describe the whole dataset. To tell apart points within it, use [tags](https://dev.nominal.io/platform/data-model/organization/tagged-data/).

## Archive a dataset

{rust:method}`archive_dataset <CatalogClient::archive_dataset>` hides a dataset from the UI without deleting it. {rust:method}`unarchive_dataset <CatalogClient::unarchive_dataset>` brings it back.

```rust
client.catalog().archive_dataset(dataset.rid()).await?;
```
