# Datasets

{.lead}
A [dataset](https://dev.nominal.io/platform/data-model/#data-model-dataset) holds your test data as [channels](channels.md), such as `altitude` or `temperature`.

To decide whether each unit you test gets its own dataset or units share one, review [Organize and view your data](https://dev.nominal.io/platform/data-model/paradigms/).

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Create a dataset

```rust
use nominal::core::DatasetCreate;

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

{rust:method}`create_dataset <CatalogClient::create_dataset>` always makes a new dataset. [Fetch the dataset](#fetch-datasets) if you know the [RID](https://dev.nominal.io/platform/reference/glossary/#rid) or [search](#find-datasets) first if you want to re-use an existing dataset but don't have the RID saved.

To fill the dataset, [upload files](upload.md) into it or [stream points](stream.md) to it. To organize the dataset under an asset or a run, add it with {rust:method}`AssetsClient::add_dataset` or {rust:method}`RunsClient::add_dataset`.

## Fetch datasets

If the RID of a dataset is known, you can fetch that dataset directly without needing to search.

```rust
let dataset = client.catalog().get_dataset("<ri.catalog.instance.dataset.identifier>").await?;
```

## Find datasets

{rust:method}`search_datasets <CatalogClient::search_datasets>` finds datasets by label, property or name. Combine filters with {rust:method}`and <DatasetQuery::and>` and {rust:method}`or <DatasetQuery::or>`:

```rust
use nominal::core::DatasetQuery;

// Every dataset, most recently ingested first
let datasets = client.catalog().list_datasets().await?;

// Names that contain "telemetry", in any case
let datasets = client.catalog().search_datasets(DatasetQuery::substring_match("telemetry")).await?;

// The "flight-test" label, and "telemetry" in the name
let datasets = client
    .catalog()
    .search_datasets(DatasetQuery::and([
        DatasetQuery::label("flight-test"),
        DatasetQuery::substring_match("telemetry"),
    ]))
    .await?;
```

To list a dataset's channels, see [Channels](channels.md). To read its data, see [Query data with SQL](sql.md). To read channels straight into memory through the REST API instead, see the [Read channel data](../examples/read-channels.md) example.

## Update a dataset

{rust:method}`update_dataset <CatalogClient::update_dataset>` changes the fields you include in the dataset update request by overwriting those fields. That is, `update_dataset(labels)` will _not_ touch properties, but it _will_ overwrite the current set of labels with the ones you provide.

```rust
use nominal::core::DatasetUpdate;

// The labels you set replace the dataset's labels, so start from its current ones
let mut labels = dataset.labels().to_vec();
labels.push("reviewed".to_string());
let dataset = client.catalog().update_dataset(dataset.rid(), DatasetUpdate::new().labels(labels)).await?;
```

## Archive a dataset

{rust:method}`archive_dataset <CatalogClient::archive_dataset>` hides a dataset from the UI without deleting it. {rust:method}`unarchive_dataset <CatalogClient::unarchive_dataset>` brings it back.

```rust
client.catalog().archive_dataset(dataset.rid()).await?;
```
