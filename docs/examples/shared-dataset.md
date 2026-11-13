# One dataset for many units

{.lead}
Keep one dataset per tester, and tag each point with the unit it measured. Each unit's asset adds the dataset filtered to its tag. This fits testing many identical units, as described in [Using a single dataset for many identical assets](https://dev.nominal.io/platform/data-model/organization/tagged-data/).

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

    // One dataset for the tester, shared by every unit it tests
    let dataset = client.catalog().create_dataset(DatasetCreate::new("Tester X1")).await?;

    // One asset per unit. Each adds the same dataset under the same data scope name,
    // selecting only the points tagged with that unit's ID.
    for uut in ["A", "B", "C"] {
        let asset = client.assets().create(AssetCreate::new(format!("UUT {uut}"))).await?;
        client.assets().add_dataset_with_tags(asset.rid(), "default", dataset.rid(), [("UUT", uut)]).await?;
    }

    // Every point carries the UUT tag, which decides the asset it shows on
    let stream = NominalDatasetStreamBuilder::new()
        .stream_to_core(
            BearerToken::new(profile.token())?,
            ResourceIdentifier::new(dataset.rid())?,
            tokio::runtime::Handle::current(),
        )
        .with_options(NominalStreamOpts { base_api_url: profile.base_url().to_string(), ..Default::default() })
        .build();
    for (uut, value) in [("A", 100.0), ("B", 101.5), ("C", 99.2)] {
        let mut measurement = stream.double_writer(ChannelDescriptor::with_tags("Measurement", [("UUT", uut)]));
        measurement.push(UNIX_EPOCH.elapsed()?, value);
    }
    drop(stream); // waits until the points are sent
    Ok(())
}
```

Each unit's asset shows only its own `Measurement` points, while the dataset holds every unit's. A point without the `UUT` tag shows on no asset.
