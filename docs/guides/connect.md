# Connect to Nominal

{.lead}
A `NominalClient` holds your API key, your Nominal URL and your workspace. Create it from a profile, or from values you pass in.

## From a profile

```rust
let client = nominal::NominalClient::from_profile("default")?;
```

`from_profile` reads the named profile from the config file, and returns an error if the profile isn't there. To create a profile, follow [API Keys Setup](api-keys-setup.md).

To choose the profile each time the program runs, read its name from the `NOMINAL_PROFILE` environment variable:

```rust
let client = nominal::NominalClient::from_profile_env()?;
```

## From an API key

```rust
let client = nominal::NominalClient::builder("<api key>") // DO NOT COMMIT THIS
    .base_url("https://api.gov.nominal.io/api")
    .workspace_rid(Some("<ri.security.instance.workspace.identifier>".to_string()))
    .build()?;
```

The three values are the ones [API Keys Setup](api-keys-setup.md) has you copy. The base URL defaults to `https://api.gov.nominal.io/api`.

## Check the connection

```rust
let user = client.users().who_am_i().await?;
println!("Signed in as {}", user.email());
```
