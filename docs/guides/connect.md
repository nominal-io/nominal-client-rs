# Connect to Nominal

{.lead}
A {rust:struct}`NominalClient` holds your API key, base URL and workspace. Create one from a saved profile, or from values you pass in.

## From a profile

{rust:method}`from_profile <NominalClient::from_profile>` reads a profile that [API Keys Setup](../api-keys-setup.md) saved:

```rust
let client = nominal::NominalClient::from_profile("default")?;

// Or use the profile named by the NOMINAL_PROFILE environment variable
let client = nominal::NominalClient::from_profile_env()?;
```

New assets, runs and datasets go in the profile's workspace, and searches look only there. To work in another workspace, add a profile for it.

## From an API key

Without a profile, pass the values that [API Keys Setup](../api-keys-setup.md) has you copy:

```rust
let client = nominal::NominalClient::builder("<api key>") // DO NOT COMMIT THIS
    .base_url("<base url>") // defaults to https://api.gov.nominal.io/api
    .workspace_rid(Some("<ri.security.instance.workspace.identifier>".to_string()))
    .build()?;
```

## Check the connection

Creating a client doesn't contact Nominal. To check that your API key works, get the signed-in user:

```rust
let user = client.users().who_am_i().await?;
println!("Signed in as {}", user.email());
```
