# Datasets

{.lead}
A dataset holds timestamped data as named channels, such as `altitude` or `temperature`. Files you upload and points you stream add to the same dataset over time.

## Create a dataset

```rust
use nominal::core::DatasetCreate;

let dataset = client
    .catalog()
    .create_dataset(
        DatasetCreate::new("Flight telemetry")
            .description("Quadcopter 456 flight logs")
            .labels(["flight-test"])
            .channel_delimiter("."),
    )
    .await?;
println!("{}", dataset.nominal_url());
```

With a channel delimiter, Nominal shows channels such as `motor.temperature` and `motor.current` as a tree, folded under `motor`.

Upload functions can also create the dataset with the upload, as in [Upload files](upload.md). To give the dataset to an asset or a run, add it under a data scope or ref name, as in [Assets](assets.md#add-data-with-data-scopes) and [Runs](runs.md#create-a-run-from-datasets).

## Find datasets

`search_datasets` takes a `DatasetQuery`, combined with `DatasetQuery::and` and `DatasetQuery::or`:

```rust
use nominal::core::DatasetQuery;

let datasets = client
    .catalog()
    .search_datasets(DatasetQuery::and([DatasetQuery::label("flight-test"), DatasetQuery::substring_match("telemetry")]))
    .await?;
```

To get one dataset, use the RID from its page in Nominal:

```rust
let dataset = client.catalog().get_dataset("<ri.catalog.instance.dataset.identifier>").await?;
```

This returns the dataset's metadata, not its data. To list its channels, see [Channels](channels.md).

## Update a dataset

`update_dataset` changes only the fields you set, and replaces each of them:

```rust
use nominal::core::DatasetUpdate;

let dataset = client
    .catalog()
    .update_dataset(dataset.rid(), DatasetUpdate::new().properties([("campaign", "2026-09")]))
    .await?;
```

Labels and properties describe the whole dataset. To select part of its data, use [tags](stream.md#tag-points).

## Archive a dataset

`archive_dataset` hides the dataset without deleting its data. `unarchive_dataset` restores it.

```rust
client.catalog().archive_dataset(dataset.rid()).await?;
```
