# Connect to Nominal

{.lead}
A `NominalClient` holds your API key, your Nominal URL and your workspace. Create it from a profile, or from values you pass in.

## From a profile

```rust
let client = nominal::NominalClient::from_profile("default")?;
```

`from_profile` reads the named profile from `~/.config/nominal/config.yml`, and returns an error if the profile isn't there. To create a profile, see step 2 of the [quickstart](../index.md#2-save-your-api-key-in-a-profile).

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

The base URL is your Nominal REST API URL, and defaults to `https://api.gov.nominal.io/api`. To find yours, click the **Settings** icon (a gear) at the bottom of Nominal's sidebar, then **API keys**, and click **COPY URL** beside **API URL**.

## Check the connection

```rust
let user = client.users().who_am_i().await?;
println!("Signed in as {}", user.email());
```
