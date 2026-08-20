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

The repository also contains `nomctl`, a CLI for Nominal. Install it from a
GitHub release or with Cargo:

```sh
cargo install nominal-cli
```

See the [CLI README](nominal-cli/README.md) for installation from release
artifacts, configuration, and usage.
