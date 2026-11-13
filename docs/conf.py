"""Sphinx config for the Nominal Rust client and nomctl docs."""

import sys
from pathlib import Path

from nominal_sphinx_theme import theme_options

sys.path.insert(0, str(Path(__file__).parent / "_ext"))

project = "Rust"
copyright = "Nominal, Inc."

extensions = [
    "myst_parser",
    "sphinx_design",
    "sphinx_copybutton",
    "nominal_sphinx_theme",
    "nomctl_reference",
    "sphinx_rustdoc",
]
myst_enable_extensions = ["attrs_block", "colon_fence"]
exclude_patterns = ["_build"]

# -- Rust API reference ----------------------------------------------------------------------------
# sphinx-rustdoc builds the nominal crate's reference under ref/rust/ from rustdoc's JSON output,
# which `cargo rustdoc` writes. The JSON format changes between Rust releases; CI pins the one
# sphinx-rustdoc reads.
rustdoc_crates = {"nominal": {"package": "nominal"}}
# docs.rs hasn't built nominal-api's recent releases, so its types render as text
rustdoc_unlinked_crates = ["nominal_api", "nominal_api_conjure", "nominal_api_proto"]

html_theme = "shibuya"
# the browser tab and search results; the header shows project
html_title = "Nominal for Rust"
html_copy_source = False
html_context = {
    "source_type": "github",
    "source_user": "nominal-io",
    "source_repo": "nominal-client-rs",
    "source_version": "main",
    "source_docs_path": "/docs/",
}
html_theme_options = theme_options(
    github_url="https://github.com/nominal-io/nominal-client-rs",
    nav_links=[
        {"title": "Quickstart", "url": "index"},
        {"title": "Guides", "url": "guides/index"},
        {"title": "Examples", "url": "examples/index"},
        {"title": "Reference", "url": "ref/index"},
    ],
)
