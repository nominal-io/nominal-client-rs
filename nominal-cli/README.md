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

Create a Nominal API token, then run the interactive setup:

```sh
nomctl config init
```

The wizard stores a named profile in `~/.config/nominal/config.yml`, including your API URL, token, and workspace. See the [Nominal authentication docs](https://docs.nominal.io/core/sdk/python-client/authentication) for token creation.

You can also create or update a profile non-interactively:

```sh
nomctl config profile add production \
  --url https://api.nominal.io/api \
  --token "$NOMINAL_TOKEN" \
  --workspace-rid ri.security.example.workspace.00000000-0000-0000-0000-000000000001
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
