#!/usr/bin/env python3
"""Writes what `hostscope --help` prints into README.md, under the screenshots.

The README describes the screen with the application's own words, so the help
text is the single place that description lives (section 11 of the
requirements). This script is what moves it; `tests/documents.rs` is what
notices when it has not been run.

It does not paste the help as one block. Two thirds of that text are ordinary
paragraphs - about the tree, the marks, the bar, the palettes - and a fenced
block renders them as grey monospace that no reader gets through. So the
paragraphs come out as prose and only the tables of options and keys stay
fenced, where the columns are the point. The words are still the help's own,
and their order is the help's own: nothing here decides what goes where.

usage: scripts/readme-help.py [path to the binary]
"""

import pathlib
import subprocess
import sys
import textwrap

HEAD = "## What it shows\n"
NEXT = "\n## "
WIDTH = 72

# Two pictures, because the tree and the card answer different questions: the
# first says who is eating the host, the second says what that one process is.
PICTURES = [
    "![The process forest of a Docker host, sorted by network](docs/screenshot-tree.png)",
    "![The card of a process running in a container](docs/screenshot-card.png)",
]


def blocks(text):
    """The help split on blank lines, each block tagged as a table or as prose.

    A block that holds an indented line is a table: the columns carry the
    meaning and only a fence keeps them. Everything else is prose - except a
    run of one-line blocks standing right before a table, which is what
    introduces it. `hostscope - ...` and `usage: hostscope [options]` are those
    lines, and left as paragraphs they read as two orphans under the pictures.
    They belong in the fence, where they look like the help itself.
    """
    raw = [b for b in text.strip("\n").split("\n\n")]
    kinds = ["table" if any(l.startswith("  ") for l in b.split("\n")) else "prose" for b in raw]

    out = []
    lead = []
    for block, kind in zip(raw, kinds):
        if kind == "prose" and len(block.split("\n")) == 1:
            lead.append(block)
            continue
        if kind == "table" and lead:
            out.append(("table", "\n\n".join(lead + [block])))
        else:
            out.extend(("prose", l) for l in lead)
            out.append((kind, block))
        lead = []
    out.extend(("prose", l) for l in lead)
    return out


def render(help_text):
    parts = list(PICTURES)
    for kind, raw in blocks(help_text):
        if kind == "table":
            parts.append("```\n" + raw + "\n```")
        else:
            parts.append(textwrap.fill(" ".join(raw.split()), WIDTH))
    return "\n" + "\n\n".join(parts) + "\n\n"


root = pathlib.Path(__file__).resolve().parent.parent
binary = sys.argv[1] if len(sys.argv) > 1 else str(root / "target/debug/hostscope")

help_text = subprocess.run(
    [binary, "--help"], capture_output=True, text=True, check=True
).stdout

readme = (root / "README.md").read_text()
start = readme.index(HEAD) + len(HEAD)
end = readme.index(NEXT, start) + 1

(root / "README.md").write_text(readme[:start] + render(help_text) + readme[end:])
print("README.md: the help text is the one the binary prints")
