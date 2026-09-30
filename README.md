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
