# Assets

{.lead}
An [asset](https://dev.nominal.io/platform/data-model/#asset) is a system you test, such as a vehicle or a test stand. It helps organize and label the data and videos you add to Nominal.

The examples on this page use a quadcopter as an example, but the asset can represent any physical or conceptual device. To decide what an asset represents in your tests, see [Organize and view your data](https://dev.nominal.io/platform/data-model/paradigms/).

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Create an asset

```rust
use nominal::core::AssetCreate;

let asset = client
    .assets()
    .create(
        AssetCreate::new("Quadcopter 456")
            .description("Airborne observation platform")
            .properties([("serial", "QC-456")]) // identify the physical system
            .labels(["camera", "airborne"]), // group assets for searching
    )
    .await?;
println!("{}", asset.nominal_url());
```

{rust:method}`create <AssetsClient::create>` always makes a new asset with a different RID, even if another asset has the same name. [Fetch the asset](#fetch-assets) if you know the RID or [search](#find-assets) first if you want to re-use an existing asset but don't have the RID saved.

## Fetch assets

If the RID of an asset is known, you can fetch that asset directly without needing to search.

```rust
let asset = client.assets().get("<ri.scout.instance.asset.identifier>").await?;
```

## Find assets

{rust:method}`search <AssetsClient::search>` finds assets by label, property or name. Combine filters with {rust:method}`and <AssetQuery::and>` and {rust:method}`or <AssetQuery::or>`:

```rust
use nominal::core::AssetQuery;

// Every asset, newest first
let assets = client.assets().list().await?;

// Names that contain "quadcopter", in any case
let assets = client.assets().search(AssetQuery::substring_match("quadcopter")).await?;

// The "airborne" label and the serial QC-456, both
let assets = client
    .assets()
    .search(AssetQuery::and([AssetQuery::label("airborne"), AssetQuery::property("serial", "QC-456")]))
    .await?;
```

## Attach data to the asset

A [data scope](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names) adds a dataset to an asset under a ref name, such as `default`.

```rust
client.assets().add_dataset(asset.rid(), "default", "<ri.catalog.instance.dataset.identifier>").await?;
```

When using the [tagged approach](https://dev.nominal.io/platform/data-model/organization/tagged-data/), filter a single shared dataset using the tags unique to your asset with {rust:method}`add_dataset_with_tags <AssetsClient::add_dataset_with_tags>`

```rust
// the example asset is uniquely identified by the tag vehicle=QC-456
client
    .assets()
    .add_dataset_with_tags(asset.rid(), "default", "<ri.catalog.instance.dataset.identifier>", [("vehicle", "QC-456")])
    .await?;
```

{rust:method}`add_data_sources <AssetsClient::add_data_sources>` adds several datasets and videos at once. 

```rust
use nominal::core::DataSource;

client
    .assets()
    .add_data_sources(
        asset.rid(),
        [
            ("default", DataSource::dataset("<ri.catalog.instance.dataset.identifier>")),
            ("nose-camera", DataSource::video("<ri.video.instance.video.identifier>")),
        ],
    )
    .await?;
```

## Update an asset

{rust:method}`update <AssetsClient::update>` changes the fields you include in the asset update request by overwriting those fields. That is, `update(labels)` will _not_ touch properties, but it _will_ overwrite the current set of labels with the ones you provide.

```rust
use nominal::core::AssetUpdate;

// The labels you set replace the asset's labels, so start from its current ones
let mut labels = asset.labels().to_vec();
labels.push("new-motor-vendor".to_string());
let asset = client.assets().update(asset.rid(), AssetUpdate::new().labels(labels)).await?;
```

## Archive an asset

{rust:method}`archive <AssetsClient::archive>` hides an asset from the UI and from searches, without deleting it. {rust:method}`unarchive <AssetsClient::unarchive>` brings it back.

```rust
client.assets().archive(asset.rid()).await?;
```
