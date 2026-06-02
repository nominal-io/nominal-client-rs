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
    println!("{}", user.email);

    Ok(())
}
```

## `nomctl` cli

A CLI to take actions on Nominal's APIs.

### Install
```sh
$ cargo install nominal-cli
```

### Example
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
$ nomctl config profile remove test-profile
```
