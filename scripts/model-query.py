#!/usr/bin/env python3
"""Questions the host check asks of `--dump-model json`, which arrives on stdin.

  zeros            counts the values derived from cumulative counters that are
                   not zero - the FR-15 check at the first tick
  count            the number of nodes in the tree
  node PATH MODE FIELD   one value, by cgroup path or by node name
  order MODE       the node names of the top level, ordered by CPU in MODE
"""

import json
import sys


def walk(node, visit):
    visit(node)
    for child in node.get("children", []):
        walk(child, visit)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    model = json.load(sys.stdin)
    tree = model["tree"]
    what = sys.argv[1]

    if what == "zeros":
        bad = []
        def check(n):
            for field in ("cpu", "rd", "wr"):
                if (n["instant"][field] or 0) != 0 or (n["avg"][field] or 0) != 0:
                    bad.append(f"{n['name']}.{field}")
        walk(tree, check)
        print(len(bad), " ".join(bad[:5]))
        return 0

    if what == "count":
        n = [0]
        walk(tree, lambda _: n.__setitem__(0, n[0] + 1))
        print(n[0])
        return 0

    if what == "node":
        path, mode, field = sys.argv[2], sys.argv[3], sys.argv[4]
        found = []
        def pick(n):
            if n.get("cgroup") == path or n.get("name") == path:
                found.append(n)
        walk(tree, pick)
        if not found:
            return 1
        value = found[0][mode][field]
        print("" if value is None else value)
        return 0

    if what == "order":
        mode = sys.argv[2]
        rows = sorted(tree["children"], key=lambda n: -(n[mode]["cpu"] or 0))
        print(" ".join(r["name"] for r in rows))
        return 0

    print(f"unknown question: {what}")
    return 2


if __name__ == "__main__":
    sys.exit(main())
