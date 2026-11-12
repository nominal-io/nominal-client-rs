# Rust client

{.lead}
The `nominal` crate connects Rust programs to Nominal: create datasets, assets and runs, upload files, and stream data. It needs Rust 1.85 or later and a Nominal API key.

```{toctree}
:hidden:
:caption: Quickstart

Quickstart <self>
```

```{toctree}
:hidden:
:caption: Guides

guides/index
guides/connect
guides/upload
guides/stream
```

```{toctree}
:hidden:
:caption: Reference

ref/index
ref/cli/index
```

## 1. Install

Add the crate, and tokio to run its async functions:

```shell
cargo add nominal
cargo add tokio --features full
```

## 2. Save your API key in a profile

The client reads your API key, Nominal URL and workspace from a profile in `~/.config/nominal/config.yml`. nomctl and the Python client read the same file.

To create a profile, [install nomctl](https://dev.nominal.io/nomctl/) and run:

```shell
nomctl config init
```

Or follow [API keys setup](https://dev.nominal.io/quickstart/api-keys-setup/). The examples use a profile named `default`.

## 3. Make a first call

Replace `src/main.rs` with:

```rust
#[tokio::main]
async fn main() -> nominal::Result<()> {
    let client = nominal::NominalClient::from_profile("default")?;
    let user = client.users().who_am_i().await?;
    println!("Signed in as {}", user.email());
    Ok(())
}
```

Run it with `cargo run`.

## 4. Upload a CSV file

Save this as `sample.csv` next to `Cargo.toml`:

```text
timestamp,voltage,current
2026-01-01T14:00:00Z,12.1,0.52
2026-01-01T14:00:01Z,12.0,0.55
2026-01-01T14:00:02Z,11.9,0.61
```

Then replace `src/main.rs` with:

```rust
use nominal::core::{CsvIngest, DatasetCreate, Timestamp};

#[tokio::main]
async fn main() -> nominal::Result<()> {
    let client = nominal::NominalClient::from_profile("default")?;

    let dataset = client.catalog().create_dataset(DatasetCreate::new("Quickstart")).await?;
    // The timestamp column holds ISO 8601 times; each other column becomes a channel
    let (job, _) = client
        .ingest()
        .upload_csv("sample.csv", dataset.rid(), CsvIngest::new(Timestamp::iso8601("timestamp")))
        .await?;
    client.ingest().wait_for_ingest_job(job.rid()).await?;

    println!("{}", dataset.nominal_url());
    Ok(())
}
```

Run it, then open the link it prints to see the `voltage` and `current` channels in Nominal.

## Next

::::{grid} 1 1 2 2
:gutter: 3

:::{grid-item-card} Guides
:link: guides/index
:link-type: doc

Connect with a profile or an API key, upload files into runs, and stream data.
:::

:::{grid-item-card} Reference
:link: ref/index
:link-type: doc

The Rust API and the nomctl commands.
:::
::::
