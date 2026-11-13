"""Write the nomctl command reference (ref/cli/) from `nomctl help-all`, when the build starts.

`nomctl help-all` prints the --help of every command under Markdown headings, one heading level
per command depth. Each top-level command gets a page, with each help text in a code block, and
ref/cli/index.md gets `nomctl --help`. nomctl comes from this repo through `cargo run`; set NOMCTL
to a nomctl binary to use that instead. The pages aren't committed, and a page is rewritten only
when its text changes, so live preview doesn't rebuild in a loop.
"""

import os
import re
import subprocess
from pathlib import Path

from sphinx.application import Sphinx
from sphinx.errors import ExtensionError

REPO = Path(__file__).resolve().parents[2]
HEADING = re.compile(r"^(#+) (nomctl(?: [\w-]+)*)$")


def nomctl(*args: str) -> str:
    binary = os.environ.get("NOMCTL")
    command = [binary, *args] if binary else ["cargo", "run", "--quiet", "--locked", "-p", "nominal-cli", "--", *args]
    result = subprocess.run(command, cwd=REPO, capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        raise ExtensionError(f"`{' '.join(command)}` failed:\n{result.stderr}")
    return result.stdout


def sections(help_all: str) -> list[tuple[int, str, str]]:
    """Split help-all output into (depth, command, help text) with each Usage line fixed."""
    found: list[tuple[int, str, list[str]]] = []
    for line in help_all.splitlines():
        if m := HEADING.match(line):
            found.append((len(m.group(1)), m.group(2), []))
        elif found:
            found[-1][2].append(line.rstrip())
    result = []
    for depth, command, lines in found:
        leaf = command.split()[-1]
        # help-all renders each subcommand alone, so its Usage line names only the leaf
        # ("Usage: list") and, on Windows, the root as nomctl.exe; give both the full command
        fixed = [re.sub(rf"^Usage: (?:nomctl\.exe|{re.escape(leaf)})\b", f"Usage: {command}", line) for line in lines]
        result.append((depth, command, "\n".join(fixed).strip("\n")))
    return result


def block(text: str) -> str:
    return f"```text\n{text}\n```\n"


def write(path: Path, text: str) -> None:
    if not path.exists() or path.read_text(encoding="utf-8") != text:
        path.write_text(text, encoding="utf-8", newline="\n")


def generate(app: Sphinx) -> None:
    out = Path(app.srcdir) / "ref" / "cli"
    version = nomctl("--version").split()[-1]
    found = sections(nomctl("help-all"))
    if not found or found[0][1] != "nomctl":
        raise ExtensionError("Unexpected `nomctl help-all` output: it should start with a `# nomctl` heading.")

    pages: dict[str, list[str]] = {}
    for depth, command, text in found[1:]:
        top = command.split()[1]
        # a top-level command is its page's title; deeper commands are headings under it
        pages.setdefault(top, []).append(f"\n{'#' * (depth - 1)} {command}\n\n{block(text)}")

    out.mkdir(parents=True, exist_ok=True)
    index = [
        "# nomctl commands\n\n",
        f"This reference matches nomctl {version}. Run `nomctl --version` to see which version you have.\n\n",
        block(found[0][2]),
        "\n```{toctree}\n:maxdepth: 1\n\n",
        *(f"{top}\n" for top in pages),
        "```\n",
    ]
    write(out / "index.md", "".join(index))
    for top, parts in pages.items():
        write(out / f"{top}.md", "".join(parts).lstrip("\n"))
    # commands nomctl dropped lose their page
    for stale in set(out.glob("*.md")) - {out / "index.md", *(out / f"{top}.md" for top in pages)}:
        stale.unlink()


def setup(app: Sphinx) -> dict:
    app.connect("builder-inited", generate)
    return {"parallel_read_safe": True, "parallel_write_safe": True}
