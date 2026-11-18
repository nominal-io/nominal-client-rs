set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# List the recipes
default:
    @just --list

# The theme and sphinx-rustdoc at their newest commits on GitHub, in place of the ones uv.lock pins
newest := '--with "nominal-sphinx-theme @ git+https://github.com/nominal-io/nominal-sphinx-theme" --with "sphinx-rustdoc @ git+https://github.com/nominal-io/sphinx-rustdoc" --refresh-package nominal-sphinx-theme --refresh-package sphinx-rustdoc'
sphinx := 'python -m sphinx -E -W --keep-going -b dirhtml docs docs/_build/dirhtml'
fallback := "Building with the newest theme and sphinx-rustdoc failed; building with the ones uv.lock pins"

# Build the docs (docs/) into docs/_build/dirhtml with the theme and sphinx-rustdoc uv.lock pins;
# warnings fail the build, as in CI. The nomctl command reference is generated from
# `nomctl help-all`, run with `cargo run`, and the Rust API reference from `cargo rustdoc`'s JSON
# output, by sphinx-rustdoc.
[group('docs')]
build-docs:
    uv run --group docs {{sphinx}}

# As build-docs, with the newest theme and sphinx-rustdoc, and the ones uv.lock pins if that fails.
# CI builds and publishes with this (.github/workflows/docs.yml and deploy-docs.yml).
[group('docs')]
[unix]
build-docs-newest:
    uv run --group docs {{newest}} {{sphinx}} || { echo "{{fallback}}"; uv run --group docs {{sphinx}}; }

# As build-docs, with the newest theme and sphinx-rustdoc, and the ones uv.lock pins if that fails.
# CI builds and publishes with this (.github/workflows/docs.yml and deploy-docs.yml).
[group('docs')]
[windows]
build-docs-newest:
    uv run --group docs {{newest}} {{sphinx}}; if (-not $?) { Write-Host "{{fallback}}"; uv run --group docs {{sphinx}} }

# Live-preview the docs on http://127.0.0.1:<port>, rebuilding on edits (`just serve-docs 8001` for another port)
[group('docs')]
serve-docs port="8000":
    uv run --group docs {{newest}} --with sphinx-autobuild sphinx-autobuild -b dirhtml --port {{port}} docs docs/_build/dirhtml --ignore "docs/ref/cli/*" --ignore "docs/ref/rust/*"
