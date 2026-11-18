# Connect to Nominal

{.lead}
A {rust:struct}`NominalClient` holds your API key, base URL and workspace. Create one from a saved profile, or from values you pass in.

## From a profile

{rust:method}`from_profile <NominalClient::from_profile>` reads a profile from the Nominal config file. Follow [API Keys Setup](../api-keys-setup.md) to set one up.

```rust
let client = nominal::NominalClient::from_profile("default")?;

// Or use the profile named by the NOMINAL_PROFILE environment variable
let client = nominal::NominalClient::from_profile_env()?;
```

:::{note}
This scopes requests to the workspace associated with the profile. To work in another workspace, create a second `NominalClient` from a profile for that workspace.
:::

## From an API key

If you prefer to store API keys in a different way, pass your API key to the client builder as shown below. [API Keys Setup](../api-keys-setup.md) explains how to generate and retrieve your API key and workspace.

```rust
let client = nominal::NominalClient::builder("<api key>") // DO NOT COMMIT THIS
    .base_url("<base url>") // defaults to https://api.gov.nominal.io/api
    .workspace_rid(Some("<ri.security.instance.workspace.identifier>".to_string()))
    .build()?;
```

:::{note}
When building the `NominalClient` as shown, the workspace is optional but recommended for most functionality. Specific features, like [Drives](drives.md), require it.

With no workspace specified, entities such as datasets, assets, and runs will be created in the default workspace for your organization, which may not be the workspace you intend.

Leaving workspace unspecified can be useful for searching across multiple workspaces as a `NominalClient` with no workspace specified will search all workspaces the user has access to.
:::

## Check the connection

Creating a client doesn't contact Nominal. To check that your API key works, get the signed-in user:

```rust
let user = client.users().who_am_i().await?;
println!("Signed in as {}", user.email());
```
