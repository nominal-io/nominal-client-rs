# nomctl

`nomctl` is the command-line interface for Nominal.

## Install

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

## First-time setup

Create an API key, as in [API Keys Setup](https://dev.nominal.io/quickstart/api-keys-setup/), then run the interactive setup:

```sh
nomctl config init
```

The wizard stores a named profile in `~/.config/nominal/config.yml`, including your API URL, API key, and workspace.

You can also create or update a profile non-interactively:

```sh
nomctl config profile add production \
  --url https://api.nominal.io/api \
  --workspace-rid '<ri.security.instance.workspace.identifier>' \
  --token '<api key>'  # DO NOT COMMIT THIS
```

Select a profile with `--profile` or `NOMINAL_PROFILE`:

```sh
nomctl --profile production user who-am-i
NOMINAL_PROFILE=production nomctl fs drive list
```

## Common commands

```sh
# Discover commands and their arguments
nomctl --help
nomctl fs --help

# Manage profiles
nomctl config profile list
nomctl config profile show production

# List drives and their contents
nomctl fs drive list
nomctl fs ls my-drive:/

# Generate shell completions
nomctl completions zsh
```

Use `nomctl help-all` to print detailed help for every command.
