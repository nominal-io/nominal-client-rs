# Stream data

{.lead}
A stream sends [points](https://dev.nominal.io/platform/data-model/#time-series-data) to a dataset as your program produces them. The `nominal` crate includes the [nominal-streaming](https://dev.nominal.io/nominal-streaming/) crate as `nominal::streaming`.

To decide whether each unit you test streams to its own dataset or units share one, told apart by tags, see [Organize and view your data](https://dev.nominal.io/platform/data-model/paradigms/).

To use this page, first make sure you can [Connect to Nominal](connect.md) and obtain a `client`.

## Stream to a dataset

```rust
use nominal::core::DatasetCreate;
use nominal::streaming::prelude::*;
use nominal::streaming::stream::NominalDatasetStreamBuilder;

let dataset = client.catalog().create_dataset(DatasetCreate::new("Bench test")).await?;

// The stream takes the API key and base URL from the profile
let config = nominal::Config::load()?;
let profile = config.get_profile("default").ok_or("no profile named default")?;
let token = BearerToken::new(profile.token())?;
let dataset_rid = ResourceIdentifier::new(dataset.rid())?;

let stream = NominalDatasetStreamBuilder::new()
    .stream_to_core(token, dataset_rid, tokio::runtime::Handle::current())
    .with_options(NominalStreamOpts::default().with_base_api_url(profile.base_url()))
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

If tags tell apart two motors in one dataset, add the dataset to each motor's asset with {rust:method}`add_dataset_with_tags <AssetsClient::add_dataset_with_tags>`.

## Fallback file

With a [fallback file](https://dev.nominal.io/nominal-streaming/guides/fallback/), the points a stream can't send go to a file on disk rather than being lost. The stream creates the file only if a point can't be sent, and adds its start time (UTC) to the name, so `fallback.avro` becomes `fallback_20260101_120000.avro`.

```rust
let stream = NominalDatasetStreamBuilder::new()
    .stream_to_core(token, dataset_rid, tokio::runtime::Handle::current())
    .with_options(NominalStreamOpts::default().with_base_api_url(profile.base_url()))
    .with_file_fallback("fallback.avro")
    .build();
```

When the network is back, upload the file to the same dataset:

```rust
use nominal::core::AvroStreamIngest;

let (job, _) = client
    .ingest()
    .upload_avro_stream("fallback_20260101_120000.avro", dataset.rid(), AvroStreamIngest::new())
    .await?;
client.ingest().wait_for_ingest_job(job.rid()).await?;
```
