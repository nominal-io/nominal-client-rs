# One dataset for many units

{.lead}
Keep one dataset per tester, and tag each point with the unit it measured, as in [Using a single dataset for many identical assets](https://dev.nominal.io/platform/data-model/organization/tagged-data/). Each unit's [asset](https://dev.nominal.io/platform/data-model/#asset) adds the dataset filtered to the unit's tag, so the asset shows only that unit's points.

Replace the tester's name, the serial numbers and the currents with your own.

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

    // One dataset for the tester, shared by every unit it tests
    let dataset = client.catalog().create_dataset(DatasetCreate::new("Tester 1")).await?;
    let stream = NominalDatasetStreamBuilder::new()
        .stream_to_core(
            BearerToken::new(profile.token())?,
            ResourceIdentifier::new(dataset.rid())?,
            tokio::runtime::Handle::current(),
        )
        .with_options(NominalStreamOpts::default().with_base_api_url(profile.base_url()))
        .build();

    // Test the units one after another
    for (serial, supply_current) in [("SN-001", 1.21), ("SN-002", 1.19), ("SN-003", 1.24)] {
        // The unit's asset shows only the points tagged with its serial number
        let asset = client.assets().create(AssetCreate::new(serial)).await?;
        client.assets().add_dataset_with_tags(asset.rid(), "default", dataset.rid(), [("UUT", serial)]).await?;

        // Tag every point. An untagged point shows on no unit's asset.
        let mut supply_current_writer = stream.double_writer(ChannelDescriptor::with_tags("supply_current", [("UUT", serial)]));
        supply_current_writer.push(UNIX_EPOCH.elapsed()?, supply_current);
        // Dropping the writer here passes its points to the stream
    }

    drop(stream); // waits until every point has been sent
    Ok(())
}
```
