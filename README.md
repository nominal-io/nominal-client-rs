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
cargo install nominal-cli
```

### First-time setup

Recommended first run:

```sh
nom config init
```

This interactive wizard creates `~/.config/nominal/config.yml` with a named profile. You can also add profiles manually:

```sh
nom config profile add test-profile \
    --base-url https://api.gov.nominal.io/api \
    --token $NOMINAL_TOKEN \
    --workspace-rid ri.security.cerulean-staging.workspace.8649e5e7-bf9b-45e8-897f-adfabbdd66b9
```

Profiles are validated by default (`--validate` / `--no-validate`). See the [authentication docs](https://docs.nominal.io/core/sdk/python-client/authentication) for how to create an API token.

If you still have the legacy config at `~/.nominal.yml`, migrate with:

```sh
nom config migrate
```

### Config file

Profiles are stored in `~/.config/nominal/config.yml` as `version: 2`. Rust and Python SDKs share this format:

```yaml
version: 2
default_profile: default
profiles:
  default:
    base_url: https://api.gov.nominal.io/api
    token: nominal_api_key_...
    workspace_rid: ri.security.gov-staging.workspace.82db1f3a-568e-418e-a2d0-0575396f29a2
```

Use a profile with `--profile`, `NOMINAL_PROFILE`, or `default_profile` in the config file.

### Example commands

```
$ nomctl config profile add test-profile \
    -u https://api.gov.nominal.io/api \
    -t $NOMINAL_TOKEN \
    -w ri.security.cerulean-staging.workspace.8649e5e7-bf9b-45e8-897f-adfabbdd66b9
$ nomctl --profile test-profile user who-am-i
RID: ri.authn.cerulean-staging.user.3de9b720-b35d-4ebe-b724-752b66732d20
Org RID: ri.authn.cerulean-staging.organization.c531d9b0-490d-4d5f-abe1-3b83817c20bb
Email: name@nominal.io
Display Name: Firstname Lastname
$ nom config profile list
$ nom config profile show test-profile
$ nom config profile remove test-profile
```

### Coming soon: browser login

Browser-based profile setup (similar to Nominal Connect) is planned as a follow-up once CLI client-integrity registration is in place. Until then, use `nom config init` or `nom config profile add` with a manually created API token.
