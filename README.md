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
