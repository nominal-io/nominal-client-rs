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

This quickstart uploads a CSV file to a new dataset and prints a link to it in Nominal.

## 1. Install

Create a project, and add the crate with tokio to run its async functions:

```shell
cargo new nominal-quickstart
cd nominal-quickstart
cargo add nominal
cargo add tokio --features full
```

## 2. Set up a profile

Follow [API Keys Setup](guides/api-keys-setup.md) to generate a profile for your `default` workspace and then create a second profile, `sandbox`, for your sandbox workspace. The code below is written against `sandbox`, but it can be changed to use any profile. 

## 3. Upload a file

Download {download}`sample.csv <assets/sample.csv>`, a minute of test data, into the `nominal-quickstart` folder.

Replace `src/main.rs` with:

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

#[tokio::main]
async fn main() -> nominal::Result<()> {
    // Connect with the sandbox profile
    let client = nominal::NominalClient::from_profile("sandbox")?;

    // Create a dataset and upload the file into it
    let (job, dataset_rid) = client
        .ingest()
        .upload_csv("sample.csv", DatasetCreate::new("Quickstart"), CsvIngest::new(Timestamp::iso8601("timestamp")))
        .await?;

    // Wait for Nominal to finish ingesting the file
    client.ingest().wait_for_ingest_job(job.rid()).await?;

    // Print the link to the dataset in Nominal
    let dataset = client.catalog().get_dataset(&dataset_rid).await?;
    println!("{}", dataset.nominal_url());
    Ok(())
}
```

Run it with `cargo run`, and open the printed link to view your dataset. You can then press **QUICK PLOT** to view the data.

```{image} assets/quick-plot.png
:alt: Quick plot of the sample dataset, with its current, temperature and voltage channels over one minute
```

For a detailed upload guide, including other file types, see [Upload files](guides/upload.md).

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
