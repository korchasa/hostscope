#!/usr/bin/env python3
"""Layer V2 for the network figures, read through netlink instead of procfs.

The application sums the byte counters of `/proc/<pid>/net/dev`, skipping the
loopback (FR-11). Nothing else checked those two numbers: the oracle does not
read them, and `pidstat` has nothing to say about a network namespace.

`ip -s -j link` of iproute2 asks the kernel for the same counters over netlink -
a different interface of the kernel, read by other people's code. Two readings
around the window the application measures give the rate to compare against the
host row.

usage: netlink-check.py /path/to/hostscope [--window 30]
"""

import json
import subprocess
import sys
import time

# Both windows are the same length and start within a moment of each other, so a
# burst lands in both and the difference left is the offset between their
# starts. Measured on the Kubernetes rig on 2026-08-29: 1755 against 1754 bytes
# a second in, 7837 against 7832 out - six hundredths of a percent. Ten percent
# is loose against that and still tight enough for any convention error this is
# here to catch.
#
# The window is thirty seconds and not ten, which is what it was until
# 2026-08-31. On the Docker rig, which carries a steady 40 to 70 KB/s of its
# own, a ten second window failed once with the model 15 percent above netlink,
# and four repeats within the minute came back 0.2 to 0.3 percent apart. The
# offset between the two starts is a fixed fraction of a second, so a burst
# inside it costs three times less of a rate averaged over thirty seconds than
# of one averaged over ten. What the check is here to catch - a counter summed
# the wrong way - does not shrink with the window at all.
#
# The floor is small on purpose. A floor of 16 KB/s was tried first and made the
# check inert: this host talks at a few kilobytes a second, so a probe that
# doubled the rate - 5384 against 2689 - passed with the whole difference
# sitting under the floor. A floor has to be below what the host actually does,
# not above it.
NET_REL, NET_ABS = 0.10, 512.0  # bytes per second


def counters():
    """Bytes in and out of every interface but the loopback, over netlink."""
    out = subprocess.run(
        ["ip", "-s", "-j", "link"], capture_output=True, text=True,
    ).stdout
    rx = tx = 0.0
    for link in json.loads(out):
        if link.get("ifname") == "lo":
            continue
        s = link.get("stats64") or {}
        rx += float(s.get("rx", {}).get("bytes", 0))
        tx += float(s.get("tx", {}).get("bytes", 0))
    return rx, tx


def close_enough(a, b):
    return abs(a - b) <= max(NET_ABS, NET_REL * max(abs(a), abs(b)))


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    binary = sys.argv[1]
    window = 30.0
    if "--window" in sys.argv:
        window = float(sys.argv[sys.argv.index("--window") + 1])

    app = subprocess.Popen(
        [binary, "--dump-model", "json", "--dump-frame", "2",
         "--tick", str(int(window * 1000))],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    a = time.time()
    rx0, tx0 = counters()
    time.sleep(window)
    rx1, tx1 = counters()
    elapsed = time.time() - a
    out, err = app.communicate(timeout=60)
    if app.returncode != 0:
        print("hostscope failed:", err.strip())
        return 1
    host = json.loads(out)["host"]

    problems, seen = [], []
    for field, before, after in (("rx", rx0, rx1), ("tx", tx0, tx1)):
        want = (after - before) / elapsed
        got = host[f"net_{field}"]
        seen.append(f"{field} netlink {want:.0f} model {got if got is None else round(got)}")
        if got is None:
            problems.append(f"{field}: the host row has no figure at all")
        elif not close_enough(want, got):
            problems.append(
                f"{field}: netlink {want:.0f} B/s, model {got:.0f} B/s"
            )

    # The figures are printed on the way past, passing or not. A check that says
    # only "agreed" hides the case where both sides are zero and the agreement
    # means nothing.
    print(
        f"netlink: {', '.join(seen)} B/s over {elapsed:.1f} s, "
        f"{len(problems)} problems"
    )
    for p in problems:
        print("  " + p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
