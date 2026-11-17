# Stream data

{.lead}
A stream sends [points](https://dev.nominal.io/platform/data-model/#time-series-data) to a dataset as your program produces them. The `nominal` crate includes the [nominal-streaming](https://dev.nominal.io/nominal-streaming/) crate, version 0.9, as `nominal::streaming`.

## Stream to a dataset

```rust
use nominal::core::DatasetCreate;
use nominal::streaming::prelude::*;
use nominal::streaming::stream::NominalDatasetStreamBuilder;

let client = nominal::NominalClient::from_profile("default")?;
let dataset = client.catalog().create_dataset(DatasetCreate::new("Bench test")).await?;

// The stream takes the API key and base URL from the profile
let config = nominal::Config::load()?;
let profile = config.get_profile("default").ok_or("no profile named default")?;
let token = BearerToken::new(profile.token())?;
let dataset_rid = ResourceIdentifier::new(dataset.rid())?;

let stream = NominalDatasetStreamBuilder::new()
    .stream_to_core(token, dataset_rid, tokio::runtime::Handle::current())
    .with_options(NominalStreamOpts { base_api_url: profile.base_url().to_string(), ..Default::default() })
    .build();

let mut voltage_writer = stream.double_writer(ChannelDescriptor::new("voltage"));
let mut state_writer = stream.string_writer(ChannelDescriptor::new("state"));

// Push each reading with the time it was measured
let now = std::time::UNIX_EPOCH.elapsed()?;
voltage_writer.push(now, 12.1);
state_writer.push(now, "running");

// Drop the writers, then the stream, which waits until every point is sent
drop(voltage_writer);
drop(state_writer);
drop(stream);
```

For integers, structs, arrays and tags, see [Write points](https://dev.nominal.io/nominal-streaming/guides/write/). For batching options and failed requests, see [Stream configuration](https://dev.nominal.io/nominal-streaming/guides/configure/). To give a channel a unit and description, see [Channels](channels.md#set-units-and-descriptions).

If tags tell apart two motors in one dataset, add the dataset to each motor's asset with {rust:method}`add_dataset_with_tags <AssetsClient::add_dataset_with_tags>`, as in [Assets](assets.md#add-data-with-data-scopes).

:::{note}
The nominal-streaming guides cover version 0.11. Where they call a `with_` method on `NominalStreamOpts`, set the field of the same name, as above, and call `build` in place of `try_build`. [Log streams](https://dev.nominal.io/nominal-streaming/guides/logs/) and metrics need nominal-streaming 0.10.6 or later.
:::

## Fallback file

With a fallback file, the points a stream can't send go to a file on disk rather than being lost.

```rust
let stream = NominalDatasetStreamBuilder::new()
    .stream_to_core(token, dataset_rid, tokio::runtime::Handle::current())
    .with_options(NominalStreamOpts { base_api_url: profile.base_url().to_string(), ..Default::default() })
    .with_file_fallback("fallback.avro")
    .build();
```

:::{warning}
In 0.9, a stream empties its fallback file when it starts. Upload the file before you start another stream, or give each stream its own file.
:::

When the network is back, upload the file to the same dataset:

```rust
use nominal::core::AvroStreamIngest;

// The file is empty when no points went to it
if std::fs::metadata("fallback.avro")?.len() > 0 {
    let (job, _) = client
        .ingest()
        .upload_avro_stream("fallback.avro", dataset.rid(), AvroStreamIngest::new())
        .await?;
    client.ingest().wait_for_ingest_job(job.rid()).await?;
}
```
