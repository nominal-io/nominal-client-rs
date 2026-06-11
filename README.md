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

## `nomctl` cli

A CLI to take actions on Nominal's APIs.

### Install
```sh
$ cargo install nominal-cli
```

### First-time setup

Recommended first run:

```sh
$ nomctl config init
```

This interactive wizard creates `~/.config/nominal/config.yml` with a named profile. You can also add profiles manually:

```sh
$ nomctl config profile add test-profile \
    --base-url https://api.example.com/api \
    --token $NOMINAL_TOKEN \
    --workspace-rid ri.security.example.workspace.00000000-0000-0000-0000-000000000001
```

Profiles are validated by default; pass `--no-validate` to skip. See the [authentication docs](https://docs.nominal.io/core/sdk/python-client/authentication) for how to create an API token.

### Config file

Profiles are stored in `~/.config/nominal/config.yml` as `version: 2`. See [nominal/tests/fixtures/config/config-v2-example.yml](nominal/tests/fixtures/config/config-v2-example.yml) for the full v2 format. Rust and Python SDKs share this format.

Use a profile with `--profile` or the `NOMINAL_PROFILE` environment variable.

### Example commands

```sh
$ nomctl config profile add test-profile \
    -u https://api.example.com/api \
    -t $NOMINAL_TOKEN \
    -w ri.security.example.workspace.00000000-0000-0000-0000-000000000001
```

```sh
$ nomctl --profile test-profile user who-am-i
RID: ri.authn.example.user.00000000-0000-0000-0000-000000000001
Org RID: ri.authn.example.organization.00000000-0000-0000-0000-000000000002
Email: user@example.com
Display Name: Example User
```

```sh
$ nomctl config profile list
$ nomctl config profile show test-profile
$ nomctl config profile remove test-profile
```

### Coming soon: browser login

Browser-based profile setup (similar to Nominal Connect) is planned as a follow-up once CLI client-integrity registration is in place. Until then, use `nomctl config init` or `nomctl config profile add` with a manually created API token.
