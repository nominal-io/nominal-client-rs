# One asset per unit

{.lead}
Create an [asset](https://dev.nominal.io/platform/data-model/#asset) and a dataset for each unit you test, and stream each unit's data to its own dataset, as in [One dataset per long-lived unit under test](https://dev.nominal.io/platform/data-model/organization/per-asset/).

Replace the serial numbers and the voltage with your own. Every asset holds its dataset under the same [ref name](https://dev.nominal.io/platform/data-model/#data-scopes-and-ref-names), `default`, so one [workbook template](../guides/workbooks.md) works for every unit.

```shell
cargo add nominal
cargo add tokio --features full
```

```rust
use std::time::UNIX_EPOCH;

use nominal::core::{AssetCreate, DatasetCreate};
use nominal::streaming::prelude::*;
use nominal::streaming::stream::NominalDatasetStreamBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the API key and base URL that API Keys Setup saved
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").ok_or("no profile named default")?;
    let client = nominal::NominalClient::from_profile_config(profile)?;

    for serial in ["SN-001", "SN-002"] {
        // Each unit gets an asset, with its own dataset under the ref name "default"
        let asset = client.assets().create(AssetCreate::new(serial)).await?;
        let dataset = client.catalog().create_dataset(DatasetCreate::new(serial)).await?;
        client.assets().add_dataset(asset.rid(), "default", dataset.rid()).await?;

        // Stream the unit's readings to its own dataset
        let stream = NominalDatasetStreamBuilder::new()
            .stream_to_core(
                BearerToken::new(profile.token())?,
                ResourceIdentifier::new(dataset.rid())?,
                tokio::runtime::Handle::current(),
            )
            .with_options(NominalStreamOpts::default().with_base_api_url(profile.base_url()))
            .build();
        let mut voltage_writer = stream.double_writer(ChannelDescriptor::new("voltage"));
        voltage_writer.push(UNIX_EPOCH.elapsed()?, 12.1);

        // Pass the writer's last points to the stream, then wait until everything is sent
        drop(voltage_writer);
        drop(stream);

        println!("{serial}: {}", asset.nominal_url());
    }
    Ok(())
}
```
