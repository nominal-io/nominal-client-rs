# Assets

{.lead}
An asset represents a system you test, such as a vehicle, satellite or test stand. It collects that system's datasets, videos and runs across every test.

## Create an asset

Properties identify the physical system, such as a serial number. Labels group assets for searching.

```rust
use nominal::core::AssetCreate;

let asset = client
    .assets()
    .create(
        AssetCreate::new("Quadcopter 456")
            .description("Airborne observation platform")
            .properties([("serial", "QC-456")])
            .labels(["camera", "airborne"]),
    )
    .await?;
println!("{}", asset.nominal_url());
```

`create` always creates a new asset. To reuse an asset a script created before, [find it](#find-assets) by its properties first.

## Add data with data scopes

An asset holds each dataset under a data scope name, such as `"telemetry"`. Use the same data scope name for the same kind of data on every asset, so workbook templates and checklists work across them.

```rust
client.assets().add_dataset(asset.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>").await?;
```

To include only part of a dataset, such as one vehicle's points in a shared dataset, select them by [tag](stream.md#tag-points):

```rust
client
    .assets()
    .add_dataset_with_tags(asset.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>", [("vehicle", "QC-456")])
    .await?;
```

To add several data sources at once, including videos, use `add_data_sources`:

```rust
use nominal::core::DataSource;

client
    .assets()
    .add_data_sources(
        asset.rid(),
        [
            ("telemetry", DataSource::dataset("<ri.catalog.instance.dataset.identifier>")),
            ("nose-camera", DataSource::video("<ri.catalog.instance.video.identifier>")),
        ],
    )
    .await?;
```

`asset.data_sources()` returns an asset's data sources, keyed by data scope name.

## Find assets

`search` takes an `AssetQuery`. Combine queries with `AssetQuery::and` and `AssetQuery::or`:

```rust
use nominal::core::AssetQuery;

let assets = client
    .assets()
    .search(AssetQuery::and([AssetQuery::label("airborne"), AssetQuery::property("serial", "QC-456")]))
    .await?;
```

`AssetQuery::search_text` is a fuzzy match on names and descriptions, so results may not contain the text verbatim. `AssetQuery::substring_match` matches part of the name, ignoring case.

To get one asset, use the RID from its page in Nominal:

```rust
let asset = client.assets().get("<ri.scout.instance.asset.identifier>").await?;
```

`get_batch` gets several assets by RID, and `list` returns every asset, newest first.

## Update an asset

`update` changes only the fields you set, and replaces each of them. To add a label, include the existing ones:

```rust
use nominal::core::AssetUpdate;

let mut labels = asset.labels().to_vec();
labels.push("new-motor-vendor".to_string());
let asset = client.assets().update(asset.rid(), AssetUpdate::new().labels(labels)).await?;
```

## Archive an asset

`archive` hides the asset from views and searches without deleting its data. `unarchive` restores it.

```rust
client.assets().archive(asset.rid()).await?;
```
