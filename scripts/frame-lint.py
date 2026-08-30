#!/usr/bin/env python3
"""Layer V4 of the testing document: the frame linter, for frames captured off a
live terminal.

The same invariants the Rust tests run over `--dump-frame` output, applied to
what `tmux capture-pane -pN` returns. Every frame any scenario produces goes
through this, rather than it being a test of its own.

usage: frame-lint.py FILE...        one frame per file, or several separated by
                                    blank lines inside one file
"""

import re
import sys
import unicodedata

CONTROL = re.compile(r"[\x00-\x08\x0b-\x1f\x7f]")

# The glyphs `bar` in src/util.rs draws with: a full block and the seven partial
# ones, so that a value too small for a whole block still shows something.
TICKS = "█▉▊▋▌▍▎▏"


# The cells a line takes, from Python's own Unicode tables. This is the second
# opinion on the width of a character: the application reads the `unicode-width`
# crate, this reads `unicodedata`, and a frame is only accepted when both agree.
# Ambiguous-width characters (the box drawing of the frame, Cyrillic) count as
# one, which is what a terminal does unless it is told otherwise.
def width_of(line):
    total = 0
    for ch in line:
        if unicodedata.combining(ch):
            continue
        total += 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
    return total


# The character index at which a cell column begins.
#
# Every column offset below is read off the header, which is ASCII, and there a
# cell is a character. A row is not: a name in a wide script takes two cells per
# letter, so slicing a row by the header's character index lands one character
# short per wide letter. That is how this linter reported a name leaving its
# column on a frame that was exactly the width of the terminal - and it can hide
# a real overrun just as easily, by landing inside the name instead of after it.
def cell_index(line, column):
    total = 0
    for i, ch in enumerate(line):
        if total >= column:
            return i
        if unicodedata.combining(ch):
            continue
        total += 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
    return len(line)


# The roles `--dump-style` names, one character per cell. A map line is made of
# nothing else, which is how a unit of two halves is told from a plain frame.
ROLES = set(".cuasmb")


def split_style(frame):
    """A `--dump-style` unit is a frame and a map of the same shape.

    Returns the frame and its map, or the frame and None where there is no map.
    A drawn frame always carries box drawing, so it can never be mistaken for a
    map made only of `.cuasm`.
    """
    n = len(frame)
    if n < 2 or n % 2:
        return frame, None
    half = frame[len(frame) // 2 :]
    if all(line and set(line) <= ROLES for line in half):
        return frame[: len(frame) // 2], half
    return frame, None


def lint_style(text, roles, name):
    """17. The map covers the frame cell for cell (D-42).

    One character per cell, so a name in a wide script makes the map longer in
    characters than the text and exactly as wide. A map that does not line up
    says nothing about the colours it claims to describe.
    """
    bad = []
    if len(roles) != len(text):
        bad.append(f"{name}: the map has {len(roles)} lines against {len(text)}")
        return bad
    for i, (t, m) in enumerate(zip(text, roles)):
        if len(m) != width_of(t):
            bad.append(
                f"{name}: line {i} is {width_of(t)} cells "
                f"and its map is {len(m)} characters"
            )
    return bad


def role_cells(roles, role):
    """18. How loud the screen is (D-42).

    Reported for every frame and never a failure on its own: a ceiling fitted
    to two rigs cannot tell a palette that is too loud from a host that is
    genuinely in trouble, and the second is what this tool is for. The induced
    states of the live check know the right answer in advance and say so with
    `--alarm-min`.

    Only the alarm count is a floor anybody may stand on. The signal colour is
    drawn by the reading and by the bar, and the bar has a role of its own - so
    an `a` cell is a reading and nothing else. The accent is not so clean: it
    also paints the header of the sorted column and the label of the
    measurement window, so the `u` count is reported and never demanded.
    """
    return sum(line.count(role) for line in roles)


def lint(frame, name):
    bad = []
    if not frame:
        return [f"{name}: the frame is empty"]

    # 1. Every line takes the same number of cells - the width of the terminal.
    widths = {width_of(line) for line in frame}
    if len(widths) != 1:
        counts = sorted(widths)
        bad.append(f"{name}: lines of different width: {counts}")
    width = max(widths)

    for i, line in enumerate(frame):
        # 2. Nothing that drives the terminal reaches the frame (FR-12). A
        # letter of any script is fine; a control character is not.
        if CONTROL.search(line):
            bad.append(f"{name}: line {i} carries a control character: {line!r}")
        # 11. No panic reaches the screen.
        if "panicked" in line or "RUST_BACKTRACE" in line:
            bad.append(f"{name}: a panic reached the frame: {line!r}")

    # 9. The path line is present and labelled with its level.
    if len(frame) > 4 and not re.search(r"L[0-3]\b", frame[4]):
        bad.append(f"{name}: the path line carries no level: {frame[4]!r}")
    # 10. The measurement mode is labelled with the window it was taken over.
    if len(frame) > 2 and not re.search(r"INSTANT |AVG over |PAUSED", frame[2]):
        bad.append(f"{name}: the window is not labelled: {frame[2]!r}")

    if len(frame) < 8 or "NAME" not in frame[6]:
        return bad  # a card is open: the table invariants do not apply

    header = frame[6]
    # Every level draws every column, in this order.
    order = ["NAME", "OWNER", "TASKS", "CORES", "MEM"]
    at = 0
    for column in order:
        pos = header.find(column, at)
        if pos < 0:
            bad.append(f"{name}: column {column} is missing or out of order")
            return bad
        at = pos + len(column)
    # The swap column is optional - it is drawn only where the host has moved
    # something out of RAM (D-35) - but where it is drawn it stands with the
    # other memory figure, between MEM and DISK.
    swap_at = header.find("SWAP")
    if swap_at >= 0:
        if swap_at < header.find("MEM"):
            bad.append(f"{name}: the swap column stands in front of MEM")
        disk_at = header.find("DISK")
        if 0 <= disk_at < swap_at:
            bad.append(f"{name}: the swap column stands past DISK")
    owner_col = header.find("OWNER")
    mem_col = header.find("MEM") - 4
    cpu_end = header.find("CORES") + 5

    # Which column the bar is drawn beside, read off the path line (D-27). The
    # names on that line are the sortings, not the column headings, so they are
    # mapped rather than looked up.
    sort_name = re.search(r"sort:\s*(\w+)", frame[4] if len(frame) > 4 else "")
    sort_head = {
        "cores": "CORES",
        "memory": "MEM",
        "tasks": "TASKS",
        "disk": "DISK",
        "net": "NET",
    }.get(sort_name.group(1) if sort_name else "", "CORES")
    sort_end = header.find(sort_head)
    sort_end = sort_end + len(sort_head) if sort_end >= 0 else 0
    wide_enough_for_a_bar = "DISK" in header and "NET" in header

    rows = frame[7:-4]
    for i, row in enumerate(rows):
        # Every offset is a cell column, so every slice of a row goes through
        # cell_index. A row carries names; the header does not.
        owner_at = cell_index(row, owner_col)
        name_cell = row[cell_index(row, 2):owner_at]
        if not name_cell.strip():
            continue
        # 4. A name never leaves its column.
        if row[owner_at - 1] != " ":
            bad.append(f"{name}: row {i} runs into the owner column: {row!r}")
        # 12. The marker of a node with children is a column of its own.
        if not (name_cell.startswith("> ") or name_cell.startswith("  ")):
            bad.append(f"{name}: row {i} has no marker column: {row!r}")
        # 5. The (self) row, when present, is the first row of the level.
        if name_cell.strip() == "(self)" and i != 0:
            bad.append(f"{name}: the (self) row is at {i}, not first")
        # 7. A non-zero value draws at least one tick of the bar.
        #
        # The bar is not a column of its own: since D-27 it is a strip drawn
        # beside the column the table is ordered by, and the path line says
        # which that is. Reading it always beside CORES was right only while
        # CORES was the only column that carried it - on a frame sorted by
        # memory the strip sits beside MEM, and the rule then reported every
        # busy row as drawing no tick.
        # A narrow terminal drops the bar on purpose - the cells go to the name,
        # which is worth more there - so the rule only applies where there was
        # room for one. What says there was room is the frame itself: the bar is
        # the last thing to go, after the disk and the network columns, so a
        # frame still drawing both was wide enough to draw a bar. Reading that
        # off the frame rather than recomputing the application's own width
        # arithmetic keeps the check independent of the thing it checks.
        if wide_enough_for_a_bar:
            end = sort_end if sort_end else cpu_end
            value_at = cell_index(row, end)
            cell = row[cell_index(row, max(0, end - 8)):value_at].strip().rstrip("%")
            try:
                value = float(cell.rstrip("GMK"))
            except ValueError:
                value = 0.0
            if value > 0 and not any(c in TICKS for c in row[value_at:]):
                bad.append(f"{name}: row {i} shows {value} and draws no tick: {row!r}")

    if width < 20:
        bad.append(f"{name}: the frame is {width} wide")
    return bad


def lint_across(frames, name):
    """16. The header holds its places (D-39).

    Every figure of the two summary lines changes on every tick, and each one
    is drawn in a place wide enough for it. If a place is too narrow the label
    beside it moves, and the reader has to find it again on every frame - so
    the labels are compared across the frames of one run rather than inside
    one of them.
    """
    bad = []
    places = {}
    for n, frame in enumerate(frames):
        head = frame[1:3]
        for label in ("MEM", "SWAP", "LOAD"):
            at = None
            for line in head:
                i = line.find(label)
                if i >= 0:
                    # In cells, not in characters and not in bytes: the line
                    # carries box drawing, arrows and bar blocks, and the bars
                    # trade a block for a space as the numbers move.
                    at = width_of(line[:i])
                    break
            first = places.setdefault(label, (at, n))
            if first[0] != at:
                bad.append(
                    f"{name}: {label} sits at {at} in frame {n} "
                    f"and at {first[0]} in frame {first[1]}"
                )
    return bad


def frames_of(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        text = f.read().rstrip("\n")
    blocks = [b for b in text.split("\n\n") if b.strip()]
    return [b.split("\n") for b in blocks]


def main():
    argv = sys.argv[1:]
    # The floor is only given where the right answer is known in advance - the
    # induced states the live check raises itself.
    alarm_min = None
    if "--alarm-min" in argv:
        i = argv.index("--alarm-min")
        alarm_min = int(argv[i + 1])
        argv = argv[:i] + argv[i + 2 :]
    files = argv
    if not files:
        print(__doc__)
        return 2
    problems = []
    total = 0
    maps = 0
    counts = {"a": 0, "u": 0}
    for path in files:
        # A path that cannot be read is a problem to report, not a traceback to
        # decipher. An unmatched shell glob arrives here as its own pattern.
        try:
            frames = frames_of(path)
        except OSError as e:
            problems.append(f"{path}: cannot be read: {e.strerror}")
            continue
        plain = []
        for n, frame in enumerate(frames):
            total += 1
            text, roles = split_style(frame)
            if roles is not None:
                maps += 1
                for role in counts:
                    counts[role] += role_cells(roles, role)
                problems += lint_style(text, roles, f"{path}#{n}")
            plain.append(text)
            problems += lint(text, f"{path}#{n}")
        problems += lint_across(plain, path)
    # A linter that linted nothing must not report success: silence here means
    # the sections that capture frames did not run, and that is the state this
    # check exists to notice.
    if total == 0:
        problems.append("no frames to lint at all")
    if alarm_min is not None:
        if maps == 0:
            problems.append("--alarm-min was given but no frame carried a map")
        elif counts["a"] < alarm_min:
            problems.append(
                f"the screen stayed quiet: {counts['a']} cells read alarm, "
                f"at least {alarm_min} were expected here"
            )
    print(
        f"linter: {total} frames, {maps} with a style map, "
        f"{counts['a']} alarm and {counts['u']} unusual cells, "
        f"{len(problems)} problems"
    )
    for p in problems:
        print("  " + p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
