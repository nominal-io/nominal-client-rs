# Channels

{.lead}
A channel is one named signal in a dataset, such as `altitude` or `fault_code`, with a data type, and an optional unit and description.

## List and search channels

`list_channels` returns every channel on a dataset, or on any other data source, such as a video:

```rust
for channel in client.catalog().list_channels("<ri.catalog.instance.dataset.identifier>").await? {
    println!("{} {:?} {}", channel.name(), channel.data_type(), channel.unit().unwrap_or(""));
}
```

`search_channels` takes a `ChannelQuery`. Its filters all have to match:

```rust
use nominal::core::{ChannelDataType, ChannelQuery};

let channels = client
    .catalog()
    .search_channels(
        ChannelQuery::new()
            .data_source("<ri.catalog.instance.dataset.identifier>")
            .substring_match("temp")
            .data_type(ChannelDataType::Double),
    )
    .await?;
```

To get one channel, use `get_channel` with the data source's RID and the channel's name.

## Set units and descriptions

`set_channel_metadata` sets a channel's unit and description. It changes the label, not the stored values:

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

`ChannelUpdate` needs the channel's data type, because `set_channel_metadata` creates the channel's metadata when the channel doesn't exist yet. So you can set a unit and description before you [stream](stream.md) to a channel.

`clear_unit` removes a unit.
