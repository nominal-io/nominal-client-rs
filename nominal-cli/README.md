# nomctl

`nomctl` is the command-line interface for Nominal.

## Install from a GitHub release

1. Open the [latest release](https://github.com/nominal-io/nominal-client-rs/releases/latest) and download the archive that matches your operating system and CPU architecture.
2. Extract the archive. It contains `nomctl` (`nomctl.exe` on Windows) and this README.
3. Move the binary to a directory on your `PATH` and make it executable on macOS or Linux.

For example, on macOS or Linux:

```sh
tar -xzf nomctl-<version>-<target>.tar.gz
mkdir -p ~/.local/bin
mv nomctl-<version>-<target>/nomctl ~/.local/bin/
chmod +x ~/.local/bin/nomctl
```

Ensure `~/.local/bin` is on your `PATH`, then verify the installation:

```sh
nomctl --version
```

On Windows, extract the ZIP and add the directory containing `nomctl.exe` to your user `PATH`.

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
nomctl fs tui                 # interactive Drive browser
nomctl fs tui my-drive        # open a specific Drive

# Generate shell completions
nomctl completions zsh
```

The Drive TUI begins with a table of drives and opens folders in Finder-style
columns, with details for the current selection in a right-hand panel. The
active drive and path are shown in the outer frame. Use the
arrow keys (or `h`/`j`/`k`/`l`) to navigate; Right or Enter opens the selected
folder, and Left or Esc returns to the prior folder or drive table. Click rows
to select them, or double-click to open a drive or folder. Upload, download,
and removal
use in-TUI dialogs (`u`, `d`, and `x`). To move a file, press `m`, optionally
browse to a destination folder, then press `m` again to edit or confirm the
full destination path (including a new folder). Press `r` to refresh or `q` to
quit.

Use `nomctl help-all` to print detailed help for every command.
