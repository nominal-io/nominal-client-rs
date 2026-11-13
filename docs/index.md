# Rust client

{.lead}
The `nominal` crate connects Rust programs to Nominal: upload files, stream data, and organize it into assets and runs. It needs Rust 1.85 or later and an API key saved in a profile.

```{toctree}
:hidden:
:caption: Quickstart

Quickstart <self>
```

```{toctree}
:hidden:
:caption: Get started

guides/index
guides/api-keys-setup
guides/connect
guides/cheatsheet
```

```{toctree}
:hidden:
:caption: Organize data

guides/assets
guides/runs
guides/datasets
guides/channels
```

```{toctree}
:hidden:
:caption: Get data in

guides/upload
guides/ingest-jobs
guides/stream
guides/drives
```

```{toctree}
:hidden:
:caption: Review data

guides/workbooks
guides/checklists
guides/sql
```

```{toctree}
:hidden:
:caption: Beyond the crate

guides/rest-api
```

```{toctree}
:hidden:
:caption: Examples

examples/index
examples/asset-per-unit
examples/shared-dataset
examples/simulation-runs
examples/automated-checks
```

```{toctree}
:hidden:
:caption: Reference

ref/index
ref/rust/nominal/index
ref/cli/index
```

This quickstart uploads a minute of sample engine data, streams more to the same dataset, and marks a run over it.

## 1. Install

Create a project, and add the crate with tokio to run its async functions:

```shell
cargo new nominal-quickstart
cd nominal-quickstart
cargo add nominal anyhow chrono
cargo add tokio --features full
```

## 2. Set up your API key

Follow [API Keys Setup](guides/api-keys-setup.md) to save your API key in a profile. The examples use the profile it creates, `default`.

## 3. Connect

Replace `src/main.rs` with:

```rust
use chrono::{Duration, Utc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = nominal::NominalClient::from_profile("default")?;
    println!("Signed in as {}", client.users().who_am_i().await?.email());

    // Add the blocks below here, in order

    Ok(())
}
```

Run it with `cargo run`.

## 4. Upload a file

An asset represents the system you test, such as a vehicle or a test stand. Its datasets hold the channels you record.

Write one minute of sample engine data, at 10 Hz, to a CSV file:

```rust
let start = Utc::now() - Duration::minutes(1);
let mut csv = String::from("timestamp,rpm,coolant_temp_c\n");
for i in 0..600 {
    let t = i as f64 / 10.0;
    let time = start + Duration::milliseconds(i * 100);
    csv += &format!("{},{:.1},{:.2}\n", time.to_rfc3339(), 3000.0 + 500.0 * (t / 5.0).sin(), 80.0 + t / 6.0);
}
std::fs::write("engine.csv", csv)?;
```

Create the asset, upload the file into a new dataset, and add the dataset to the asset:

```rust
use nominal::core::{AssetCreate, CsvIngest, DatasetCreate, Timestamp};

let asset = client
    .assets()
    .create(AssetCreate::new("Quickstart engine").properties([("serial", "QS-1")]))
    .await?;
let (job, dataset_rid) = client
    .ingest()
    .upload_csv("engine.csv", DatasetCreate::new("Quickstart engine"), CsvIngest::new(Timestamp::iso8601("timestamp")))
    .await?;
client.ingest().wait_for_ingest_job(job.rid()).await?; // errors if the ingest fails
client.assets().add_dataset(asset.rid(), "engine", &dataset_rid).await?;

println!("{}", asset.nominal_url());
```

Open the printed link to see the asset and its `engine` dataset in Nominal. Every column other than `timestamp` becomes a channel: `rpm` and `coolant_temp_c`. For other file types, see [Upload files](guides/upload.md).

## 5. Stream live data

A stream sends values as your code produces them, and batches them in the background. This block streams 10 seconds of `rpm` values to the same dataset:

```rust
use nominal::streaming::prelude::*;
use nominal::streaming::stream::NominalDatasetStreamBuilder;

let config = nominal::Config::load()?;
let profile = config.get_profile("default").expect("no profile named default");
let stream = NominalDatasetStreamBuilder::new()
    .stream_to_core(
        BearerToken::new(profile.token())?,
        ResourceIdentifier::new(&dataset_rid)?,
        tokio::runtime::Handle::current(),
    )
    .with_options(NominalStreamOpts { base_api_url: profile.base_url().to_string(), ..Default::default() })
    .build();

let mut rpm = stream.double_writer(ChannelDescriptor::new("rpm"));
for i in 0..100 {
    rpm.push(Utc::now(), 3000.0 + (i % 20) as f64 * 10.0);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
drop(rpm); // sends the writer's buffered points to the stream
drop(stream); // waits until every point has been sent
```

See [Stream data](guides/stream.md) for tags, other value types and a fallback file.

## 6. Mark a run

A run names a span of time on an asset, such as one test. It sees the asset's datasets, so you don't add them again:

```rust
use nominal::core::RunCreate;

let run = client
    .runs()
    .create(RunCreate::new("Quickstart run", start).end(Utc::now()).assets([asset.rid()]))
    .await?;
println!("{}", run.nominal_url());
```

Read more about [runs](guides/runs.md) and [assets](guides/assets.md).

## 7. Clean up

Archiving hides the run, the dataset and the asset from views and searches. It doesn't delete the data.

```rust
client.runs().archive(run.rid()).await?;
client.catalog().archive_dataset(&dataset_rid).await?;
client.assets().archive(asset.rid()).await?;
```

## Next

::::{grid} 1 1 2 2
:gutter: 3

:::{grid-item-card} Guides
:link: guides/index
:link-type: doc

Organize data in assets and runs, upload each file type, stream, and run checklists.
:::

:::{grid-item-card} Reference
:link: ref/index
:link-type: doc

The Rust API and the nomctl commands.
:::
::::
