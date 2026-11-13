# One asset per unit

{.lead}
Create an asset and a dataset for each unit you test, and stream each unit's data to its own dataset. This is Nominal's default way to organize test data, described in [One dataset per long-lived unit under test](https://dev.nominal.io/platform/data-model/organization/per-asset/).

```shell
cargo add nominal anyhow
cargo add tokio --features full
```

```rust
use std::time::UNIX_EPOCH;

use anyhow::Context;
use nominal::core::{AssetCreate, DatasetCreate};
use nominal::streaming::prelude::*;
use nominal::streaming::stream::NominalDatasetStreamBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = nominal::NominalClient::from_profile("default")?;
    let config = nominal::Config::load()?;
    let profile = config.get_profile("default").context("no profile named default")?;

    for unit in ["UUT1", "UUT2"] {
        // One asset per unit under test, with its own dataset under the data scope "default"
        let asset = client.assets().create(AssetCreate::new(unit)).await?;
        let dataset = client.catalog().create_dataset(DatasetCreate::new(unit)).await?;
        client.assets().add_dataset(asset.rid(), "default", dataset.rid()).await?;

        // Write the unit's data to its own dataset
        let stream = NominalDatasetStreamBuilder::new()
            .stream_to_core(
                BearerToken::new(profile.token())?,
                ResourceIdentifier::new(dataset.rid())?,
                tokio::runtime::Handle::current(),
            )
            .with_options(NominalStreamOpts { base_api_url: profile.base_url().to_string(), ..Default::default() })
            .build();
        let mut signal = stream.double_writer(ChannelDescriptor::new("Vsignal"));
        signal.push(UNIX_EPOCH.elapsed()?, 5.0);
        drop(signal);
        drop(stream); // waits until the points are sent

        println!("{unit}: {}", asset.nominal_url());
    }
    Ok(())
}
```

Each asset's page in Nominal shows its own `Vsignal` channel. Every unit's datasets use the same data scope name, `default`, so one [workbook template](../guides/workbooks.md) works for all of them.
