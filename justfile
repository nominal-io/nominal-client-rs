set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# List the recipes
default:
    @just --list

# Build the docs (docs/) into docs/_build/dirhtml; warnings fail the build, as in CI.
# The nomctl command reference is generated from `nomctl help-all`, run with `cargo run`.
# Published by .github/workflows/deploy-docs.yml.
[group('docs')]
build-docs:
    uv run --group docs sphinx-build -E -W --keep-going -b dirhtml docs docs/_build/dirhtml

# Live-preview the docs on http://127.0.0.1:<port>, rebuilding on edits (`just serve-docs 8001` for another port)
[group('docs')]
serve-docs port="8000":
    uv run --group docs --with sphinx-autobuild sphinx-autobuild -b dirhtml --port {{port}} docs docs/_build/dirhtml --ignore "docs/ref/cli/*"
