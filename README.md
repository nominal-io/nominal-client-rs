## `nominal-client-rs` library

The canonical Nominal Rust SDK.

### Install
```sh
cargo add nominal
```

### Example
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = nominal::NominalClient::from_profile("test-profile")?;

    let user = client.users().who_am_i().await?;
    println!("{}", user.email());

    Ok(())
}
```

### Native streaming writes (v2)

`client.channel_writer().write_stream(schemas).await?` opens the native bidirectional
`nominal.channel_writer.v2.NominalChannelWriterService/WriteStream` RPC and waits for
schema acknowledgment. Use `write_chunk` to send timestamp/value columns,
`declare_schemas` to add schemas, and `finish` to half-close and drain the stream.
All v2 value types (including arrays, structs and logs) are available under
`nominal::channel_writer::proto`.

Try it against a server with streaming enabled and a native gRPC ingress:

```sh
export NOMINAL_BASE_URL='https://<your-api-host>/api'
export NOMINAL_DATASET_RID='ri.catalog.main.dataset.<uuid>'
# Set NOMINAL_TOKEN securely, with write access to that dataset.
cargo run -p nominal --example write_stream
```

The client pipelines **up to N outstanding messages on one stream**, with a
background sender/acknowledgment loop. `write_chunk(&self, ...)` is the public
chunk-writing API: each future waits for count/byte capacity and then its own
acknowledgment. Poll writes concurrently to pipeline them:

```rust,ignore
futures::try_join!(
    stream.write_chunk(first_channels),
    stream.write_chunk(second_channels),
)?;
stream.finish().await?;
```

For a larger input stream, use `StreamExt::buffer_unordered(n)` as shown in the
example. Sequentially awaiting `write_chunk` calls is still serial. Enqueueing,
message IDs and completion handles are internal implementation details.

Batched and out-of-order acknowledgments remove only matching retained messages
and release capacity immediately. Cancelling a write future after admission does
not cancel the retained write or its retries. Dropping the stream cancels all
pending work. `finish` stops admission, drains outstanding messages (with retries),
and half-closes; it also reports the first per-message rejection so a cancelled
write future cannot hide failure.

Configure `WriteStreamOptions` through `write_stream_with_options`: defaults are
16 retained messages, 64 MiB of retained encoded payloads, 30-second acknowledgment
timeouts, and 3 retries per message. Each message is limited to 50 MiB; a message
larger than the configured byte window is rejected locally, never left waiting
for impossible capacity. Transport encoding/HTTP2 buffering adds memory overhead.
Initial schemas and all restored schemas must fit the server's 10,000-schema / 8
MiB registry limit. There is no fixed RPC-lifetime timeout.

**Automatic reconnect/replay:** transient RPC failures (UNAVAILABLE,
RESOURCE_EXHAUSTED, DEADLINE_EXCEEDED, CANCELLED), EOF and acknowledgment timeouts
close the old connection, wait with capped exponential full-jitter backoff,
redeclare all acknowledged schemas, and replay only unanswered messages. Each
message has its own retry budget, so unrelated successful messages cannot keep an
unacknowledged write retrying forever. Pending declarations are replayed too;
rejected declarations are never installed. A connection lost while idle is
reopened when another message is submitted. Permanent authentication/authorization
and protocol errors fail the stream without retry.

Delivery is **at least once**: replay can duplicate data if the server persisted a
write but its acknowledgment was lost. Per-message `Rejected` responses preserve
rich status details, complete that receipt with an error, and are not automatically
retried; other outstanding messages continue. Acceptance means durable queue/retry
acceptance, not immediate query visibility. V2 unary `Write` remains unsupported.

### Fixed-size streaming pool

Use `write_stream_pool` to spread concurrent `write_chunk` calls across a fixed set
of streams, without changing the chunk-writing API:

```rust,ignore
use nominal::channel_writer::{WriteStreamOptions, WriteStreamPoolOptions};

let pool = client.channel_writer().write_stream_pool(
    schemas,
    WriteStreamPoolOptions {
        stream_count: 2,
        max_pending_messages: 32,             // GLOBAL, not multiplied by streams
        max_pending_bytes: 64 * 1024 * 1024,  // GLOBAL retained payload budget
        stream_options: WriteStreamOptions {
            max_in_flight_messages: 16,       // Per stream
            ..Default::default()
        },
    },
).await?;

futures::try_join!(pool.write_chunk(first_channels), pool.write_chunk(second_channels))?;
pool.finish().await?;
```

Defaults match the example: two streams, 32 globally pending messages and 64 MiB
of globally retained payloads. All streams open and acknowledge initial schemas
before construction succeeds; startup failure or cancellation closes any streams
already opened. Writes choose the least-loaded worker with sufficient count/byte
capacity, with round-robin tie-breaking. If all workers are full, admission waits
for any worker to free capacity. A cancelled admitted write keeps both its local
and global permits until its terminal response.

Each worker reconnects/redeclares/replays its own unanswered messages; the pool
never concurrently reassigns those messages to another worker. Healthy workers
can keep sending while another reconnects. A permanent worker failure or retry
exhaustion closes admission for the pool; already admitted work on other workers
can still drain through `finish`. A per-message rejection alone does not close
admission. `finish` drains every worker and reports failures. Pool-wide payload
limits exclude transport buffers and each stream's bounded schema registry.

`pool.declare_schemas` installs the new schemas on every worker and blocks new
chunk admission until all declarations are acknowledged. Its declarations also
use the shared payload window. If broadcast fails or is cancelled mid-operation,
admission closes because immutable schemas cannot be rolled back atomically
across streams. Finish/drop that pool before constructing a replacement.

There is **no cross-stream ordering guarantee**, adaptive resizing, or stream
creation in response to rate limiting. More streams do not bypass server-wide or
per-organization admission/caps. Configure the pool within your server's stream
limits. Sequential `.write_chunk(...).await` calls are still serial; poll futures
concurrently, as in the runnable two-stream example:

```sh
cargo run -p nominal --example write_stream_pool
```

### Matched unary/streaming bandwidth benchmark

`bandwidth_compare` is a bounded, paced double-column benchmark. It compares native
v1 `WriteColumnBatches` with the SDK's v2 stream pool over the same TLS endpoint;
compression and retries are disabled in both modes. The v1 codec is an example-only
wire-compatible subset, not another public SDK writing API. Disable the ingress's
**whole-request** body cap for long-lived streaming RPCs (NGINX
`proxy-body-size: "0"`); the server's 50 MiB **per-message** limit remains enforced.
A 50 MiB ingress cap applies to the entire streaming RPC body and can terminate
an otherwise healthy stream after only seconds of sustained traffic.

```sh
# Set NOMINAL_TOKEN and NOMINAL_BASE_URL securely, as above.
# Both native RPC paths must be routed by the test environment's ingress.
export BENCH_TIMESTAMP_BASE="$(date +%s)" # keep identical for both runs
BENCH_MODE=unary BENCH_DATASET_RID='<fresh-unary-dataset>' \
  cargo run --release -p nominal --example bandwidth_compare
BENCH_MODE=stream BENCH_DATASET_RID='<fresh-stream-dataset>' \
  cargo run --release -p nominal --example bandwidth_compare
```

Defaults: 100 channels, 500 points per channel per batch (50,000 points/request),
1,000,000 scheduled points/sec, 20 seconds, concurrency 16; streaming uses four
workers with four outstanding messages each. `BENCH_RATE`, `BENCH_CHANNELS`,
`BENCH_SAMPLES`, and `BENCH_SECONDS` configure the shape (duration is capped at 60
seconds). `BENCH_CHANNELS_PER_REQUEST` defaults to all channels; a smaller divisor
rotates disjoint channel groups round-robin, preserving per-channel timestamp
continuity. Total points/request = channels/request × samples/channel.

For a metadata-heavy profile with **20 points total per request**, keep 100 channels
but send one channel/request, with 32 long tags/channel (the maximum accepted by
both protocols: native v1 permits 64 tags, but v2 schema validation permits 32):

```sh
export BENCH_CHANNELS=100 BENCH_CHANNELS_PER_REQUEST=1 BENCH_SAMPLES=20
export BENCH_TAG_COUNT=32 BENCH_TAG_KEY_BYTES=128 BENCH_TAG_VALUE_BYTES=128
export BENCH_RATE=400 BENCH_SECONDS=60 # 20 requests/sec, not a capacity test
# Run each mode against a fresh dataset using the commands above.
```

Nonzero `BENCH_TAG_COUNT` generates distinct, fixed-length ASCII keys and values;
zero (the default) retains the original two short tags. Both modes use identical
tags; no body compression is enabled. A four-worker pool declares all 100 channel
schemas on each worker, so cold-start declaration bytes can be substantial. They
must be included in total-byte comparisons rather than excluded as warmup.
`BENCH_SCHEMA_BATCH` (default 100, range 1..=100) can split initial registration:
the first batch opens the pool, and subsequent batches use `declare_schemas`.
All schemas are acknowledged on every worker before pacing starts. Oversized
validation-error headers may appear as ingress 502s; smaller declaration batches
make the underlying per-schema validation errors easier to inspect.

Each run prints JSON with acknowledged point count/rate, elapsed time
including final acknowledgment drain, scheduling lag and failures. Initial stream
schema declarations occur before the paced interval. Run the generator inside the
test cluster for high-rate tests; a remote client's bandwidth/RTT can limit throughput.

Measure ingress request/response byte-counter deltas from before process startup
until after all RPCs have closed, split by RPC path, with no other traffic to those
paths. This includes schema declarations and acknowledgments. Validate exact RPC
counts (unary: batch count; streaming: four connections) and divide bytes by the
acknowledged point count. NGINX-observed sizes are not TCP/TLS wire sizes. Pod RX/TX
include dependency traffic and cached kubelet sampling, so are not an exact substitute.
Do not use the converted internal-request histogram as a network bandwidth metric.
Use fresh, equally configured datasets and report actual achieved rates, not merely
the configured target; large batches amortize metadata overhead substantially.

The unreleased v2 contract is temporarily snapshotted in `nominal/proto` from
Scout commit `51f4b6ca4e3b859c8e9b875375c1e9b455a7ffe0`, with validation/codegen
options removed but wire fields unchanged. Replace this snapshot with generated
`nominal-api` types once that contract is published. Builds use a vendored protoc;
no extra system protoc installation is needed for this module.

## `nomctl` CLI

`nomctl` is the command-line interface for Nominal.

Install the latest release for your system on macOS/Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/nominal-io/nominal-client-rs/main/scripts/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/nominal-io/nominal-client-rs/main/scripts/install.ps1 | iex
```

On macOS/Linux, add `~/.local/bin` to your `PATH`. Windows updates your user `PATH` automatically.
Rerun to update, or install from source with `cargo install nominal-cli --locked`.

See the [CLI README](nominal-cli/README.md) for configuration and usage.
