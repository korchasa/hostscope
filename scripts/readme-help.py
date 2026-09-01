#!/usr/bin/env python3
"""Writes what `hostscope --help` prints into README.md, under the screenshot.

The README describes the application with the application's own words, so the
help text is the single place the description lives (section 11 of the
requirements). This script is what moves it; `tests/documents.rs` is what
notices when it has not been run.

usage: scripts/readme-help.py [path to the binary]
"""

import pathlib
import subprocess
import sys

HEAD = "## What it shows\n"
NEXT = "\n## "

root = pathlib.Path(__file__).resolve().parent.parent
binary = sys.argv[1] if len(sys.argv) > 1 else str(root / "target/debug/hostscope")

help_text = subprocess.run(
    [binary, "--help"], capture_output=True, text=True, check=True
).stdout.rstrip("\n")

readme = (root / "README.md").read_text()
start = readme.index(HEAD) + len(HEAD)
end = readme.index(NEXT, start) + 1

# Two pictures, because the tree and the card answer different questions: the
# first says who is eating the host, the second says what that one process is.
tree = "![The process forest of a Docker host, sorted by network](docs/screenshot-tree.png)"
card = "![The card of a process running in a container](docs/screenshot-card.png)"
section = f"\n{tree}\n\n{card}\n\n```\n{help_text}\n```\n"

(root / "README.md").write_text(readme[:start] + section + readme[end:])
print("README.md: the help text is the one the binary prints")
