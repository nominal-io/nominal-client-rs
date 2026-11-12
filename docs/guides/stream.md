# Stream data

{.lead}
Send points to a dataset as your program produces them. `nominal::streaming` is the [nominal-streaming](https://github.com/nominal-io/nominal-streaming) crate, which buffers points and sends them in batches.

## Open a stream to a dataset

The stream takes your API key and Nominal URL from a profile, and needs a tokio runtime:

```rust
use nominal::streaming::prelude::*;
use nominal::streaming::stream::NominalDatasetStreamBuilder;

let config = nominal::Config::load()?;
let profile = config.get_profile("default").expect("no profile named default");

let stream = NominalDatasetStreamBuilder::new()
    .stream_to_core(
        BearerToken::new(profile.token())?,
        ResourceIdentifier::new("<ri.catalog.instance.dataset.identifier>")?,
        tokio::runtime::Handle::current(),
    )
    .with_options(NominalStreamOpts { base_api_url: profile.base_url().to_string(), ..Default::default() })
    .build();
```

To create the dataset first, use `client.catalog().create_dataset(DatasetCreate::new("<name>"))`, and pass its `rid()`.

## Write points

Each channel has a writer for its type. Points take a timestamp and a value:

```rust
let voltage = ChannelDescriptor::new("voltage");
stream.double_writer(voltage).push(chrono::Utc::now(), 12.1);

let state = ChannelDescriptor::new("state");
stream.string_writer(state).push(chrono::Utc::now(), "RUNNING");
```

Dropping the stream waits until every point it buffered has been sent.

## Keep a copy on disk

To save points to a file when they can't be sent, add `.with_file_fallback("fallback.avro")` to the builder above, before `.build()`. To upload the file later:

```rust
use nominal::core::AvroStreamIngest;

let client = nominal::NominalClient::from_profile("default")?;
let (job, _) = client
    .ingest()
    .upload_avro_stream("fallback.avro", "<ri.catalog.instance.dataset.identifier>", AvroStreamIngest::new())
    .await?;
client.ingest().wait_for_ingest_job(job.rid()).await?;
```
