# Channels

{.lead}
A [channel](https://dev.nominal.io/platform/data-model/#channels) is one named signal in a [dataset](datasets.md), such as `altitude`. It has a data type, and can have a unit and a description.

## List and search channels

```rust
let client = nominal::NominalClient::from_profile("default")?;

// Every channel in the dataset
for channel in client.catalog().list_channels("<ri.catalog.instance.dataset.identifier>").await? {
    println!("{} {:?} {}", channel.name(), channel.data_type(), channel.unit().unwrap_or(""));
}
```

To find only some of a dataset's channels, give {rust:method}`search_channels <CatalogClient::search_channels>` filters that all have to match:

```rust
use nominal::core::{ChannelDataType, ChannelQuery};

// Floating-point channels with "temp" in their names, in any case
let channels = client
    .catalog()
    .search_channels(
        ChannelQuery::new()
            .data_source("<ri.catalog.instance.dataset.identifier>")
            .substring_match("temp")
            .data_type(ChannelDataType::Double),
    )
    .await?;

// One channel, by name
let altitude = client.catalog().get_channel("<ri.catalog.instance.dataset.identifier>", "altitude").await?;
```

## Set units and descriptions

{rust:method}`set_channel_metadata <CatalogClient::set_channel_metadata>` sets a channel's unit and description, without changing its data:

```rust
use nominal::core::{ChannelDataType, ChannelUpdate};

client
    .catalog()
    .set_channel_metadata(
        "<ri.catalog.instance.dataset.identifier>",
        "altitude",
        ChannelUpdate::new(ChannelDataType::Double).unit("m").description("Barometric altitude"),
    )
    .await?;
```

{rust:struct}`ChannelUpdate` takes the channel's data type, so you can set a unit and description before the channel has data, such as before you [stream](stream.md) to it.
