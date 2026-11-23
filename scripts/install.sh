#!/bin/sh
# Install the latest nomctl release. Keep this script compatible with POSIX sh.
set -eu

main() {
    for command in curl tar uname mktemp install; do
        command -v "$command" >/dev/null 2>&1 || {
            echo "Required command not found: $command" >&2
            exit 1
        }
    done

    case "$(uname -s)" in
        Darwin) platform=apple-darwin ;;
        Linux) platform=unknown-linux-gnu ;;
        *) echo 'Unsupported OS. Use install.ps1 on Windows.' >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        arm64|aarch64) arch=aarch64 ;;
        *) echo 'Unsupported CPU architecture; expected x86_64 or ARM64.' >&2; exit 1 ;;
    esac

    base=https://github.com/nominal-io/nominal-client-rs/releases
    # Resolve once so the tag and filename always refer to the same release.
    release=$(curl --proto '=https' --tlsv1.2 -fsSL -o /dev/null -w '%{url_effective}' "$base/latest")
    tag=${release##*/}
    case "$tag" in
        nominal-v[0-9]*) ;;
        *) echo "Unexpected latest release: $release" >&2; exit 1 ;;
    esac
    name="nomctl-${tag#nominal-}-$arch-$platform"
    destination=${NOMCTL_INSTALL_DIR:-"$HOME/.local/bin"}
    temporary=$(mktemp -d)
    trap 'rm -rf "$temporary"' EXIT
    trap 'exit 1' HUP INT TERM

    echo "Installing $name to $destination"
    curl --proto '=https' --tlsv1.2 -fsSL "$base/download/$tag/$name.tar.gz" -o "$temporary/nomctl.tar.gz" || {
        echo 'Could not download the release artifact. It may still be building; try again shortly.' >&2
        exit 1
    }
    tar -xzf "$temporary/nomctl.tar.gz" -C "$temporary" "$name/nomctl"
    mkdir -p "$destination"
    install -m 755 "$temporary/$name/nomctl" "$destination/nomctl"
    echo "Installed nomctl to $destination/nomctl"
    case ":${PATH:-}:" in
        *:"$destination":*) ;;
        *) printf 'Add this directory to PATH in your shell configuration: %s\n' "$destination" ;;
    esac
}

# Execute only after the entire script has been downloaded by a piped shell.
main "$@"
