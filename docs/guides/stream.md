# Stream data

{.lead}
Send points to a dataset as your program produces them. `nominal::streaming` is the [nominal-streaming](https://github.com/nominal-io/nominal-streaming) crate, which buffers points and sends them in batches. It needs a dataset to stream to and a tokio runtime.

## Open a stream to a dataset

The stream takes your API key and base URL from a profile:

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

To create the dataset first, see [Datasets](datasets.md#create-a-dataset), and pass its `rid()`.

## Write points

Each channel gets a writer for its value type. A point is a timestamp and a value. Use the time each value was measured, not the time you write it:

```rust
let mut voltage = stream.double_writer(ChannelDescriptor::new("voltage"));
let mut state = stream.string_writer(ChannelDescriptor::new("state"));

for i in 0..100 {
    let now = chrono::Utc::now();
    voltage.push(now, 12.0 + (i % 10) as f64 / 10.0);
    state.push(now, if i < 10 { "idle" } else { "running" });
}
```

Timestamps can be `chrono::DateTime` values, a `std::time::Duration` since the Unix epoch, or `i64` nanoseconds since the Unix epoch. The stream also has `integer_writer`, `uint64_writer`, `struct_writer`, `double_array_writer` and `string_array_writer`.

A writer sends its points to the stream in batches, and when you drop it. Dropping the stream waits until every point has been sent, so drop writers first:

```rust
drop(voltage);
drop(state);
drop(stream);
```

To set a channel's unit and description before its first values arrive, see [Channels](channels.md#set-units-and-descriptions).

## Tag points

Tags tell apart points in the same channel, such as two motors on one test stand:

```rust
let mut left = stream.double_writer(ChannelDescriptor::with_tags("motor.temperature", [("motor_id", "left")]));
let mut right = stream.double_writer(ChannelDescriptor::with_tags("motor.temperature", [("motor_id", "right")]));
```

An asset or a run can then select one tag value, with `add_dataset_with_tags`, as in [Assets](assets.md#add-data-with-data-scopes). Tag with a small set of values, such as component, vehicle or test configuration, not with values that change every point.

## Stream options

`NominalStreamOpts` sets where points go, how they're batched, and when writes wait for the network:

```rust
use std::time::Duration;

let opts = NominalStreamOpts {
    base_api_url: profile.base_url().to_string(),
    max_points_per_record: 100_000,                 // default 250,000
    max_request_delay: Duration::from_millis(250), // default 100 ms
    max_buffered_requests: 4,                       // default 4
    request_dispatcher_tasks: 8,                    // requests sent at once; default 8
};
```

A request goes out when it has `max_points_per_record` points, or when `max_request_delay` has passed. When `max_buffered_requests` requests are waiting to be sent, writes wait for one to go.

## Keep a copy on disk

To save points to a file when they can't be sent, add `.with_file_fallback("fallback.avro")` to the builder above, before `.build()`. To upload the file later:

```rust
use nominal::core::AvroStreamIngest;

let (job, _) = client
    .ingest()
    .upload_avro_stream("fallback.avro", "<ri.catalog.instance.dataset.identifier>", AvroStreamIngest::new())
    .await?;
client.ingest().wait_for_ingest_job(job.rid()).await?;
```
