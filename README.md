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

The client permits **one outstanding message per stream**, providing bounded
backpressure without a background queue. Await each call before sending more data;
open independent streams for parallelism. Each message is limited to 50 MiB. The
acknowledgment timeout defaults to 30 seconds and can be configured through
`write_stream_with_options`; there is no fixed RPC-lifetime timeout.

**No automatic reconnect/replay:** stream termination, server rotation, operation
cancellation and timeouts invalidate the session. Open a new stream, redeclare
schemas, then explicitly replay unanswered chunks. Delivery is at least once:
replayed chunks can duplicate data, and even a rejected chunk can have partial
effects. `WriteStreamError::Rejected` preserves the rich gRPC status and leaves the
session usable; other stream errors require reopening. Acceptance means durable
queue/retry acceptance, not immediate query visibility. V2 unary `Write` is not
implemented by this API.

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
