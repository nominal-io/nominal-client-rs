## `nominal-client-rs` library

The canonical Nominal Rust SDK.

## `nom` cli

A CLI to take any action on Nominal's APIs.

```sh
nom --profile prod user get-profile
```

Note that profile-based configuration parsing works, and a skeleton exists for the commands to interact, but writing back to the config hasn't been implemented yet.

```sh
nom config profile add profile-name \
    -u https://api.gov.nominal.io/api \
    -t $TOKEN \
    -w ri.security.cerulean-staging.workspace.8649e5e7-bf9b-45e8-897f-adfabbdd66b9
```

```sh
nom config profile remove profile-name
```
