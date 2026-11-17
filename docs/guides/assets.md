# Assets

{.lead}
An [asset](https://dev.nominal.io/platform/data-model/#asset) is a system you test, such as a vehicle or a test stand. Datasets and videos you add to an asset are there for every run on that asset.

## Create an asset

```rust
use nominal::core::AssetCreate;

let client = nominal::NominalClient::from_profile("default")?;

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

{rust:method}`create <AssetsClient::create>` always makes a new asset, even if another asset has the same name. To reuse an asset, [find it](#find-assets) first.

## Add data with data scopes

A [data scope](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names) adds a dataset to an asset under a ref name, such as `telemetry`. Workbook templates and checklists find data by ref name, so use the same ref name for the same kind of data on every asset. Each asset can use a ref name once.

```rust
client.assets().add_dataset(asset.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>").await?;
```

When several systems share a dataset, add only one system's points by their [tag](https://dev.nominal.io/platform/data-model/organization/tagged-data/):

```rust
client
    .assets()
    .add_dataset_with_tags(asset.rid(), "telemetry", "<ri.catalog.instance.dataset.identifier>", [("vehicle", "QC-456")])
    .await?;
```

{rust:method}`add_data_sources <AssetsClient::add_data_sources>` adds several datasets and videos at once:

```rust
use nominal::core::DataSource;

client
    .assets()
    .add_data_sources(
        asset.rid(),
        [
            ("telemetry", DataSource::dataset("<ri.catalog.instance.dataset.identifier>")),
            ("nose-camera", DataSource::video("<ri.video.instance.video.identifier>")),
        ],
    )
    .await?;
```

## Find assets

{rust:method}`search <AssetsClient::search>` finds assets by label, property or name. Combine filters with {rust:method}`and <AssetQuery::and>` and {rust:method}`or <AssetQuery::or>`:

```rust
use nominal::core::AssetQuery;

let assets = client
    .assets()
    .search(AssetQuery::and([AssetQuery::label("airborne"), AssetQuery::property("serial", "QC-456")]))
    .await?;

// Names that contain "quadcopter", in any case
let assets = client.assets().search(AssetQuery::substring_match("quadcopter")).await?;

// Every asset, newest first
let assets = client.assets().list().await?;

// One asset, by RID
let asset = client.assets().get("<ri.scout.instance.asset.identifier>").await?;
```

## Update an asset

{rust:method}`update <AssetsClient::update>` changes only the fields you set:

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
