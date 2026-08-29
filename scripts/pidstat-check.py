#!/usr/bin/env python3
"""Layer V2 of the testing document, read by somebody else's code.

`oracle.py` beside this file reads the kernel with its own code, which catches
arithmetic, aggregation and units - but not a wrong idea of what a kernel file
means, because it would hold the same wrong idea. This one does not read the
kernel at all. It takes the figure of every process from `pidstat` of sysstat
and `ps` of procps: two readers written by other people over twenty years,
against the same files.

What it compares is the value, not the shape. `pidstat` knows nothing about
parents, so the subtree of a node is the one the model itself draws; the tree
stays the oracle's business. A convention we got wrong - the wrong field of
`/proc/<pid>/io`, kilobytes taken for bytes, a percentage of one core taken for
a percentage of the machine - shows up here and nowhere else.

usage: pidstat-check.py /path/to/hostscope [--window 10]
"""

import json
import os
import subprocess
import sys
import time

# The same floors the oracle uses for CPU and memory: a live host swings by a
# fraction of a core while nothing in particular happens, and two windows that
# differ by a fraction of a second disagree by far more than FR-1 allows.
CPU_REL, CPU_ABS = 0.25, 0.05  # cores
MEM_REL, MEM_ABS = 0.02, 8 << 20  # bytes
# Disk is lumpier than either: the kernel accounts a write when it reaches the
# device, so a second of writeback lands in one window and not in the next.
# This check is here to catch a convention - a factor of 1024, the wrong field -
# and those are off by orders of magnitude, not by a third.
IO_REL, IO_ABS = 0.30, 1 << 20  # bytes per second


def close_enough(a, b, rel, absolute):
    return abs(a - b) <= max(absolute, rel * max(abs(a), abs(b)))


def read_pidstat(window):
    """Per-process CPU and disk over the window, as sysstat reads them.

    `pidstat` prints only the tasks that did something, so a pid that is not
    here did nothing and counts as zero. The columns are found by their header
    rather than by position: sysstat has added columns between releases, and a
    fixed index would silently read the wrong one.
    """
    env = dict(os.environ, S_TIME_FORMAT="ISO", LC_ALL="C")
    out = subprocess.run(
        ["pidstat", "-h", "-u", "-d", str(int(window)), "1"],
        capture_output=True, text=True, env=env,
    ).stdout
    cols, rows = None, {}
    for line in out.splitlines():
        if line.startswith("#"):
            cols = line.lstrip("#").split()
            continue
        if cols is None or not line.strip():
            continue
        f = line.split()
        if len(f) < len(cols):
            continue
        try:
            pid = int(f[cols.index("PID")])
        except (ValueError, IndexError):
            continue
        rows[pid] = {
            "cpu": float(f[cols.index("%CPU")]) / 100.0,
            "rd": float(f[cols.index("kB_rd/s")]) * 1024.0,
            "wr": float(f[cols.index("kB_wr/s")]) * 1024.0,
        }
    return rows


def read_ps():
    """Resident memory of every process, as procps reads it.

    `ps` lists them all, which `pidstat` does not: memory is a level rather than
    a rate, and a process that did nothing still holds its pages.
    """
    out = subprocess.run(
        ["ps", "-eo", "pid=,rss="], capture_output=True, text=True,
    ).stdout
    rss = {}
    for line in out.splitlines():
        f = line.split()
        if len(f) == 2:
            rss[int(f[0])] = float(f[1]) * 1024.0
    return rss


def pids_of(node):
    """Every pid the node stands for: its own, and every link of a glued chain.

    A chain of single children is drawn as one row (D-25), so a row can carry
    several processes and the figure on it is their sum.
    """
    out = []
    if node.get("pid") is not None:
        out.append(node["pid"])
    for link in node.get("chain", []):
        try:
            out.append(int(link.split()[0]))
        except (ValueError, IndexError):
            pass
    return out


def subtree_pids(node, out):
    out.extend(pids_of(node))
    for c in node.get("children", []):
        subtree_pids(c, out)
    return out


def walk(node, out):
    if node.get("pid") is not None:
        out.append(node)
    for c in node.get("children", []):
        walk(c, out)
    return out


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    binary = sys.argv[1]
    window = 10.0
    if "--window" in sys.argv:
        window = float(sys.argv[sys.argv.index("--window") + 1])

    app = subprocess.Popen(
        [binary, "--dump-model", "json", "--dump-frame", "2",
         "--tick", str(int(window * 1000))],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    # `pidstat` blocks for the window, which is the same window the application
    # is measuring; `ps` comes after it, beside the second reading.
    started = time.time()
    rates = read_pidstat(window)
    rss = read_ps()
    out, err = app.communicate(timeout=60)
    if app.returncode != 0:
        print("hostscope failed:", err.strip())
        return 1
    model = json.loads(out)

    problems, checked, skipped = [], 0, 0
    for node in walk(model["tree"], []):
        pids = set(subtree_pids(node, []))
        # A subtree that lost a process took its counter with it, and the
        # difference would be the churn rather than the work.
        if not pids or any(p not in rss for p in pids):
            skipped += 1
            continue
        checked += 1
        name = f"{node.get('pid')} {node.get('name')}"

        want = sum(rates.get(p, {}).get("cpu", 0.0) for p in pids)
        got = node["instant"]["cpu"]
        if got is not None and not close_enough(want, got, CPU_REL, CPU_ABS):
            problems.append(f"cpu  {name}: pidstat {want:.3f} cores, model {got:.3f}")

        want = sum(rss[p] for p in pids)
        got = node["instant"]["mem"]
        if got is not None and not close_enough(want, got, MEM_REL, MEM_ABS):
            problems.append(f"mem  {name}: ps {want:.0f} bytes, model {got:.0f}")

        for field in ("rd", "wr"):
            want = sum(rates.get(p, {}).get(field, 0.0) for p in pids)
            got = node["instant"][field]
            if got is not None and not close_enough(want, got, IO_REL, IO_ABS):
                problems.append(
                    f"{field}   {name}: pidstat {want:.0f} B/s, model {got:.0f}"
                )

    elapsed = time.time() - started
    print(
        f"pidstat: {checked} rows compared against sysstat and procps over "
        f"{elapsed:.1f} s, {skipped} skipped as churn, {len(problems)} problems"
    )
    for p in problems:
        print("  " + p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
