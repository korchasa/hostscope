#!/usr/bin/env python3
"""Layer V2 of the testing document: an oracle on the live host.

Reads the same kernel files as hostscope and computes the totals with its own
code, then compares them against `--dump-model json`. It shares no code with the
application on purpose - otherwise it would repeat the application's error.

The tree is the process forest, so this reads `/proc` and nothing else: for
every process the model shows, the oracle builds the same subtree out of its own
reading of `/proc/<pid>/stat` and sums it itself.

usage: oracle.py /path/to/hostscope [--window 2.0]
"""

import json
import os
import subprocess
import sys
import time

HZ = os.sysconf("SC_CLK_TCK")
PAGE = os.sysconf("SC_PAGE_SIZE")

# The tolerances come from the requirements, widened by what a live host
# actually does. Measured on the Docker rig on 2026-08-14: the per-second load of a
# busy subtree swings by a fraction of a core while nothing in particular is
# happening, so two windows that differ by a fraction of a second disagree by
# far more than the 1 percent of FR-1. The relative check stays; CPU gets an
# absolute floor that covers that swing.
CPU_REL, CPU_ABS = 0.25, 0.05      # cores
MEM_REL, MEM_ABS = 0.02, 8 << 20   # bytes
TASK_REL, TASK_ABS = 0.10, 3.0
CORES_REL = 0.05                   # FR-1a, against 1 - idle of /proc/stat


def read_proc():
    """Every process of the host: parent, CPU ticks, resident pages, threads.

    `starttime` comes along because the kernel reuses a pid: two walks a second
    apart can find different processes under one number, and counting the
    difference of their counters would invent CPU time out of nothing.
    """
    out = {}
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            with open(f"/proc/{name}/stat") as f:
                text = f.read()
        except OSError:
            continue  # it ended between the listing and the read
        close = text.rfind(")")
        if close < 0:
            continue
        fields = text[close + 2:].split()
        if len(fields) < 22:
            continue
        try:
            out[int(name)] = {
                "ppid": int(fields[1]),
                "ticks": float(fields[11]) + float(fields[12]),
                "threads": float(fields[17]),
                "starttime": fields[19],
                "rss": float(fields[21]) * PAGE,
            }
        except ValueError:
            continue
    return out


def children_of(procs):
    kids = {}
    for pid, p in procs.items():
        if p["ppid"] in procs and p["ppid"] != pid:
            kids.setdefault(p["ppid"], []).append(pid)
    return kids


def subtree(pid, kids):
    out = [pid]
    stack = list(kids.get(pid, []))
    while stack:
        p = stack.pop()
        out.append(p)
        stack.extend(kids.get(p, []))
    return out


def proc_stat():
    with open("/proc/stat") as f:
        for line in f:
            if line.startswith("cpu "):
                v = [float(x) for x in line.split()[1:]]
                return sum(v), v[3] + v[4]
    return 0.0, 0.0


def flatten(node, out):
    """The model tree keyed by the pid of each row."""
    pid = node.get("pid")
    if pid is not None:
        out[pid] = node
    for child in node.get("children", []):
        flatten(child, out)
    return out


def close_enough(a, b, rel, absolute):
    return abs(a - b) <= max(absolute, rel * max(abs(a), abs(b)))


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    binary = sys.argv[1]
    window = 2.0
    if "--window" in sys.argv:
        window = float(sys.argv[sys.argv.index("--window") + 1])

    tick_ms = int(window * 1000)
    app = subprocess.Popen(
        [binary, "--dump-model", "json", "--dump-frame", "2", "--tick", str(tick_ms)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    # The window is measured between the middles of the two walks: a walk over a
    # few hundred processes is not instantaneous, and counting it into the
    # window is what made the oracle disagree with the application by a tenth of
    # a core.
    a0 = time.time()
    before = read_proc()
    cpu_total_0, cpu_idle_0 = proc_stat()
    a1 = time.time()
    time.sleep(window)
    b0 = time.time()
    after = read_proc()
    cpu_total_1, cpu_idle_1 = proc_stat()
    b1 = time.time()
    elapsed = (b0 + b1) / 2 - (a0 + a1) / 2
    out, err = app.communicate(timeout=30)
    if app.returncode != 0:
        print("hostscope failed:", err.strip())
        return 1
    model = json.loads(out)
    nodes = flatten(model["tree"], {})
    kids = children_of(after)

    problems = []
    checked = 0
    for pid, node in nodes.items():
        if pid not in after:
            continue  # it ended inside the window
        members = subtree(pid, kids)
        # A subtree the walks disagree about is not compared: a process that
        # came or went takes its counter with it, and the difference would be
        # the churn rather than the work.
        settled = [
            p for p in members
            if p in before and before[p]["starttime"] == after[p]["starttime"]
        ]
        checked += 1

        if len(settled) == len(members):
            want = sum(after[p]["ticks"] - before[p]["ticks"] for p in settled) / HZ / elapsed
            got = node["instant"]["cpu"]
            if got is not None and not close_enough(want, got, CPU_REL, CPU_ABS):
                problems.append(f"cpu   {pid} {node['name']}: oracle {want:.3f} cores, model {got:.3f}")

        want = sum(after[p]["rss"] for p in members)
        got = node["instant"]["mem"]
        if got is not None and not close_enough(want, got, MEM_REL, MEM_ABS):
            problems.append(f"mem   {pid} {node['name']}: oracle {want:.0f} bytes, model {got:.0f}")

        want = sum(after[p]["threads"] for p in members)
        got = node["instant"]["tasks"]
        if got is not None and not close_enough(want, got, TASK_REL, TASK_ABS):
            problems.append(f"tasks {pid} {node['name']}: oracle {want:.0f}, model {got:.0f}")

    # FR-1a: the busy cores of the header against 1 - idle of /proc/stat.
    total = cpu_total_1 - cpu_total_0
    idle = cpu_idle_1 - cpu_idle_0
    cores = model["host"]["cores"]
    want_cores = (total - idle) / total * cores if total > 0 else 0.0
    got_cores = model["host"]["busy_cores"]
    if abs(want_cores - got_cores) > max(CORES_REL * cores, 0.05):
        problems.append(f"busy cores: oracle {want_cores:.3f}, model {got_cores:.3f}")

    # FR-5: a row carries its whole subtree, so it is never below the sum of the
    # rows drawn under it. Memory is the one field where the kernel itself makes
    # that false - a page shared by two processes is resident in both, and the
    # sum over a subtree counts it twice - so the check is on what accumulates.
    for pid, node in nodes.items():
        children = [c for c in node.get("children", []) if c.get("pid") is not None]
        for field in ("cpu", "rd", "wr"):
            total_children = sum(c["instant"][field] or 0 for c in children)
            value = node["instant"][field] or 0
            if total_children > value + max(0.05, 0.01 * total_children):
                problems.append(
                    f"sum   {pid} {node['name']}: children {field} {total_children:.3f} "
                    f"above the row {value:.3f}"
                )

    print(f"oracle: {checked} processes compared over {elapsed:.2f} s, {len(problems)} problems")
    for p in problems:
        print("  " + p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
