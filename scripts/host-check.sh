#!/bin/bash
# The verification procedure of docs/testing.md, run on the host itself.
#
# Sections 6 (one run), 7 (the frame linter), 8 (induced states) and 9
# (non-functional measurements). Everything it creates is named with the `hs-`
# prefix and removed at the end, as section 10 demands.
#
# usage: host-check.sh [section ...]     with no arguments it runs all of them

set -u
DIR=/tmp/hostscope
BIN=$DIR/hostscope
SESSION=hs-run
PASS=0
FAIL=0
SKIP=0

pass() { PASS=$((PASS + 1)); echo "  PASS  $*"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL  $*"; }
skip() { SKIP=$((SKIP + 1)); echo "  SKIP  $*"; }
part() { echo; echo "== $* =="; }

pause() { python3 -c "import time,sys; time.sleep(float(sys.argv[1]))" "$1"; }

# ---- the feedback loop of section 3 ------------------------------------------

# start_app <name> <extra arguments...>
start_app() {
  local name=$1
  shift
  tmux kill-session -t "$SESSION" 2>/dev/null
  tmux new-session -d -s "$SESSION" -x 100 -y 30 "$*"
  pause 1.5
  tmux capture-pane -pN -t "$SESSION" > "$DIR/frames/$name-00.txt"
}

# send <name> <index> <key>
send() {
  tmux send-keys -t "$SESSION" "$3"
  pause 1.3
  tmux capture-pane -pN -t "$SESSION" > "$DIR/frames/$1-$2.txt"
}

stop_app() { tmux kill-session -t "$SESSION" 2>/dev/null; }

# The tick a walk runs at. A key program is fed one key per frame, so a walk
# costs its length in ticks and nothing else - there is no terminal to wait for.
# Through tmux the same walk cost 1.3 s per key, because a capture taken before
# the application redrew returns the previous frame and the tick is a second.
# Measured on the Kubernetes rig on 2026-08-15: the walk of 21 keys took 29.6 s
# through tmux, 2.9 s as a key program at a tick of 100 ms.
WALK_TICK=${WALK_TICK:-150}

# walk <name> <key program> [extra arguments...]
#
# The same scenario without a terminal: `--keys` feeds the program and
# `--dump-frame` prints what it drew. The frames are written under the numbering
# tmux used - `<name>-00.txt` is the frame before the first key, `<name>-NN.txt`
# the one after key NN - so every check that reads a frame by its number reads
# the same frame as before.
#
# What this gives up is the terminal itself: these frames are what the
# application printed, not what a terminal displayed. That is why `sizes` still
# goes through tmux, and why the scenario keeps a short tmux pass of its own.
walk() {
  local name=$1 keys=$2
  shift 2
  local count
  count=$(printf '%s' "$keys" | wc -w | tr -d ' ')
  rm -f "$DIR"/frames/"$name"-*.txt
  # `--dump-style` rather than `--dump-frame`: a unit is the frame and a map of
  # the same shape naming the role of every cell, so the linter can check that
  # the colours line up and count how loud the screen was (D-42). Every check
  # below reads the frame by line number or by grep, and the map sits after the
  # last line of the frame, so none of them sees it.
  sudo -n "$BIN" --keys "$keys" --dump-style "$count" --tick "$WALK_TICK" \
    --size 100x30 "$@" > "$DIR/frames/$name.raw" 2>"$DIR/frames/$name.err"
  python3 - "$DIR/frames" "$name" <<'PY'
import pathlib, sys
d, name = pathlib.Path(sys.argv[1]), sys.argv[2]
text = (d / f"{name}.raw").read_text(encoding="utf-8", errors="replace")
blocks = [b for b in text.split("\n\n") if b.strip()]
for i, block in enumerate(blocks):
    (d / f"{name}-{i:02d}.txt").write_text(block + "\n", encoding="utf-8")
print(f"  {name}: {len(blocks)} frames")
PY
  rm -f "$DIR/frames/$name.raw"
}

# last_frame <name>   the last frame a walk produced.
#
# A key program spells a filter one letter at a time, so the frame a check wants
# is no longer at a number anybody can hold in their head: `/ c o n t a i n e r
# Enter` is eleven keys. What the checks below want is the frame the walk ended
# on, and that is what this names.
last_frame() { ls "$DIR"/frames/"$1"-*.txt | sort | tail -1; }

# wait_scope <unit>   returns as soon as the load is real, not after a guess.
#
# `pause 4` after a `systemd-run` was a reserve with nothing behind it: measured
# on the Kubernetes rig on 2026-08-15, the scope appears in the tree and its
# counter moves in under 20 ms. What the check actually needs is one collection
# tick on top of that, so the application has looked at the host once since the
# load started.
wait_scope() {
  python3 - "/sys/fs/cgroup/hs.slice/$1.scope/cpu.stat" "$WALK_TICK" <<'PY'
import sys, time
path, tick = sys.argv[1], int(sys.argv[2]) / 1000.0
deadline = time.time() + 10
while time.time() < deadline:
    try:
        if int(open(path).readline().split()[1]) > 0:
            time.sleep(tick * 2)   # one tick for the application to see it
            sys.exit(0)
    except (OSError, ValueError, IndexError):
        pass
    time.sleep(0.02)
print("  note: the scope never showed a counter above zero")
sys.exit(1)
PY
}

# wait_burn <unit> <core seconds>   wait until a load has burnt that much cpu.
#
# A load being real is not the same as the application having seen it. The
# instant window is the last collection interval, and a spike that started
# inside the current one is averaged with the idle part of it - measured on
# the Kubernetes rig on 2026-08-15, the screen of a running application settles
# about every three seconds. Waiting a fixed few seconds for that was guesswork:
# it was `pause 4`, and a run where the spike landed a moment late reported the two
# windows as equal, which reads as the modes being broken rather than as the
# check being early.
#
# What is waited on instead is the load's own counter reaching enough core
# seconds that no window this short can average it away. That is a fact about
# the host, readable from outside, and it does not depend on what the
# application has drawn yet.
wait_burn() {
  python3 - "/sys/fs/cgroup/hs.slice/$1.scope/cpu.stat" "$2" <<'PY'
import sys, time
path, want = sys.argv[1], float(sys.argv[2])
deadline = time.time() + 20
while time.time() < deadline:
    try:
        if int(open(path).readline().split()[1]) / 1e6 >= want:
            sys.exit(0)
    except (OSError, ValueError, IndexError):
        pass
    time.sleep(0.05)
print(f"  note: the load never burnt {want} core seconds")
sys.exit(1)
PY
}

model() { sudo -n "$BIN" "$@" --dump-model json; }

# top_row <frame file>   the name of the first sorted row of the table.
# The (self) row is pinned above the sorting and is skipped here; `cut -c` is
# not usable at all, because the frame carries box drawing characters and cut
# counts bytes.
top_row() {
  python3 - "$1" <<'PY'
import re, sys
lines = open(sys.argv[1], encoding="utf-8", errors="replace").read().split("\n")
for line in lines[7:]:
    name = re.split(r"\s{2,}", line[2:].strip())[0] if len(line) > 2 else ""
    if not name or name == "(self)":
        continue
    print(name)
    break
PY
}

# top_cores <frame file>   the CORES cell of the first sorted row.
top_cores() {
  python3 - "$1" <<'PY'
import re, sys
lines = open(sys.argv[1], encoding="utf-8", errors="replace").read().split("\n")
header = next((l for l in lines if "NAME" in l and "TASKS" in l), "")
end = header.find("CORES") + 5
for line in lines[7:]:
    name = re.split(r"\s{2,}", line[2:].strip())[0] if len(line) > 2 else ""
    if not name or name == "(self)":
        continue
    print(line[end - 8:end].strip() or "0")
    break
PY
}

# node_value <json> <cgroup path> <mode> <field>
node_value() {
  printf '%s' "$1" | python3 "$DIR/model-query.py" node "$2" "$3" "$4"
}

# Every file name an `open`, `openat` or `openat2` actually returned a
# descriptor for. Failed calls are left out: the application probes paths under
# `/proc` that come and go, and an attempt that got ENOENT opened nothing.
opened_paths() {
  # The path is the first quoted field of the call, whether the form is
  # `open("/x", ...)` or `openat(AT_FDCWD, "/x", ...)` - the directory argument
  # of the second form carries no quotes. Lines are filtered to the open calls
  # first, so a `statx` of the same name is not counted as an open.
  grep -E '(^|[^a-z_])(open|openat|openat2)\(' "$1" | grep -v '= -1' \
    | sed -nE 's/[^"]*"([^"]*)".*/\1/p'
}

prepare() {
  part "1. preparation"
  # The frames of the run before this one go first. The linter runs early, and
  # the sections that write frames run late, so a leftover file is linted
  # against today's build: on the Docker rig on 2026-08-29 a full run failed on
  # thirty-six invariants belonging to frames an older binary had left in place.
  # The same leftover hides the opposite fault just as well - a section that
  # writes no frames at all leaves yesterday's good ones for the linter to be
  # satisfied by.
  rm -rf "$DIR/frames"
  mkdir -p "$DIR/frames" "$DIR/state"
  docker ps --format '{{.Names}}' | sort > "$DIR/state/containers-before.txt"
  systemctl list-units --failed --no-legend > "$DIR/state/failed-before.txt"
  free -m > "$DIR/state/free-before.txt"
  uptime > "$DIR/state/uptime-before.txt"
  echo "  containers before: $(wc -l < "$DIR/state/containers-before.txt")"
  echo "  failed units before: $(wc -l < "$DIR/state/failed-before.txt")"
  "$BIN" --version
  sha256sum "$BIN"
}

baseline() {
  part "3. baseline: nothing accumulated before the start reaches the screen (FR-15)"
  local nonzero
  nonzero=$(model | python3 "$DIR/model-query.py" zeros)
  if [ "${nonzero%% *}" = "0" ]; then
    pass "every value derived from a cumulative counter is zero at the first tick"
  else
    fail "cumulative values reached the first tick: $nonzero"
  fi
}

oracle() {
  part "4. oracle on the live host (V2)"
  # Ten seconds, not three. The oracle walks every process of the host twice,
  # and that walk is itself load; over a short window it moves the figures of
  # the shells above it by more than the tolerance. Measured on the Docker rig on
  # 2026-08-14: three seconds disagreed on the root of the forest, ten seconds
  # agreed on every process.
  if sudo -n python3 "$DIR/oracle.py" "$BIN" --window 10; then
    pass "the model agrees with an independently computed one"
  else
    fail "the model disagrees with the oracle"
  fi
}

pidstat() {
  part "4. the same figures read by sysstat and procps (V2)"
  # The oracle proves the arithmetic; this proves the convention. It reads
  # nothing itself: `pidstat` gives the CPU and the disk of every process and
  # `ps` gives the memory, and both have been reading these files for twenty
  # years. Where they are not installed the section says so rather than passing
  # quietly - a check that skips itself in silence is a check nobody notices is
  # gone.
  if ! command -v pidstat >/dev/null 2>&1; then
    skip "sysstat is not installed on this host, so pidstat cannot be compared"
    return
  fi
  if sudo -n python3 "$DIR/pidstat-check.py" "$BIN" --window 10; then
    pass "the figures agree with the ones sysstat and procps read"
  else
    fail "the figures disagree with sysstat or procps"
  fi
}

network() {
  part "4. the host network rates against netlink (V2)"
  # The two network figures of the header had nothing checking them: the oracle
  # does not read them and pidstat has nothing to say about a namespace. The
  # application sums `/proc/<pid>/net/dev`; iproute2 asks the kernel for the
  # same counters over netlink, which is a different door into the kernel and
  # somebody else's code walking through it.
  if ! command -v ip >/dev/null 2>&1; then
    skip "iproute2 is not installed on this host, so netlink cannot be compared"
    return
  fi
  if sudo -n python3 "$DIR/netlink-check.py" "$BIN" --window 10; then
    pass "the host network rates agree with the ones netlink reports"
  else
    fail "the host network rates disagree with netlink"
  fi
}

scenario() {
  part "5. a walk down the forest and back (FR-2)"
  # Enter goes down, Backspace comes back, i opens the card of any row (D-25).
  # The keys are the ones tmux sent, under the names the application knows: one
  # key per frame, so frame NN is what stood on screen after key NN.
  walk walk "m Down Enter Down i Escape Down Enter i Escape Backspace Backspace a a v Down i Escape v Space Space" \
    --log "$DIR/app.log"

  # One pass through a real terminal, because everything above is what the
  # application printed rather than what a terminal displayed. Two keys are
  # enough to tell that tmux sees the same frame the dump does: the width, the
  # header and the labels. The walk itself no longer pays 1.3 s per key for it.
  start_app walkterm "sudo -n $BIN --log $DIR/app-term.log"
  send walkterm 01 Down
  send walkterm 02 v
  stop_app
  if python3 "$DIR/frame-lint.py" "$DIR"/frames/walkterm-*.txt >/dev/null \
     && grep -q "view: list" "$DIR/frames/walkterm-02.txt"; then
    pass "a real terminal draws the frame the dump drew"
  else
    fail "the frame off a real terminal differs from the dumped one"
    python3 "$DIR/frame-lint.py" "$DIR"/frames/walkterm-*.txt | head -5
  fi
  # The card must return to the row it was opened from: the path line and the
  # selected row are the same before and after (FR-2).
  local before after
  before=$(sed -n '5p' "$DIR/frames/walk-04.txt")
  after=$(sed -n '5p' "$DIR/frames/walk-06.txt")
  if [ "$before" = "$after" ]; then
    pass "the card changed neither the path nor the level"
  else
    fail "the card moved the view: '$before' against '$after'"
  fi
  # The application opens in the average (D-19), so the first a shows the
  # interval and the second one brings the average back.
  if grep -q "INSTANT" "$DIR/frames/walk-13.txt" && grep -q "AVG over" "$DIR/frames/walk-14.txt"; then
    pass "the a key switches the window in both directions"
  else
    fail "the a key did not switch the window"
  fi
  if grep -q "view: list" "$DIR/frames/walk-15.txt" && grep -q "view: tree" "$DIR/frames/walk-19.txt"; then
    pass "the v key switches the view in both directions (FR-18)"
  else
    fail "the v key did not switch the view"
  fi
  if grep -q "OWNER" "$DIR/frames/walk-19.txt"; then
    pass "the OWNER column is drawn on every level (FR-20)"
  else
    fail "the OWNER column is missing"
  fi
  if grep -q "PAUSED" "$DIR/frames/walk-20.txt"; then
    pass "the pause is labelled (FR-7)"
  else
    fail "the pause is not labelled"
  fi
}

# The four keys D-28 and D-29 added, on a host with a real forest under them.
# The rows
# of a live host are what the offline fixture cannot give: a level long enough
# to page through, and names long enough for a filter to pick one out of.
keys() {
  part "5a. the page keys, the interval and the filter (FR-6, FR-7, D-28, D-29)"
  # The list view of the root is every end of the forest at once - hundreds of
  # rows on this host, where a screen holds nineteen.
  #
  # The walk pauses before it pages. On a live host the rows are re-sorted every
  # tick - most of them are idle, so their cores are equal and the order among
  # them turns on a name comparison - and a check that reads a row by its
  # position then compares two different lists. Measured here on 2026-08-15: two
  # pages down and one up returned a different row, and the paging was right.
  # Paused, the frame is one snapshot for the whole walk (FR-7), and the
  # position is the only thing that moves.
  walk pages "Space v NPage NPage PPage"
  local top_before top_paged top_back
  top_before=$(sed -n '8p' "$DIR/frames/pages-02.txt")
  top_paged=$(sed -n '8p' "$DIR/frames/pages-03.txt")
  top_back=$(sed -n '8p' "$DIR/frames/pages-05.txt")
  if [ "$top_before" != "$top_paged" ]; then
    pass "PageDown moved the window"
  else
    fail "PageDown left the window where it was: '$top_before'"
  fi
  # Two down and one up land a page below where the paging started, and a page
  # is what the table holds, so the row on top is the same one twice.
  if [ "$top_back" = "$top_paged" ]; then
    pass "PageUp undid one PageDown"
  else
    fail "PageUp did not undo a PageDown: '$top_back' against '$top_paged'"
  fi

  # The interval: two steps up the list, one back down, and then down to the
  # pause at the near end of it. The walk runs at a tick of its own, which is
  # not one of the steps, so the first key is what puts it on one.
  walk interval "+ + - - -"
  if grep -q -- "- + 1s" "$DIR/frames/interval-01.txt" \
     && grep -q -- "- + 2s" "$DIR/frames/interval-02.txt" \
     && grep -q -- "- + 1s" "$DIR/frames/interval-03.txt"; then
    pass "the two keys move the interval and the frame says which one is on"
  else
    fail "the interval is not on the frame or did not move"
    sed -n '29p' "$DIR/frames/interval-03.txt"
  fi
  if grep -q -- "- + paused" "$DIR/frames/interval-04.txt" \
     && grep -q "PAUSED" "$DIR/frames/interval-04.txt" \
     && grep -q -- "- + paused" "$DIR/frames/interval-05.txt"; then
    pass "the pause is the near end of the list and stays there"
  else
    fail "the near end of the interval list is not the pause"
    sed -n '29p' "$DIR/frames/interval-04.txt"
  fi

  # A filter over the flat list: what it is, how much it left, and the key that
  # gives the level back.
  walk filter "v / s s h Enter Escape"
  if grep -q "filter: ssh (" "$DIR/frames/filter-06.txt"; then
    pass "a filter that is on says what it is and how many rows it left"
  else
    fail "the filter is not on the frame"
    sed -n '5p' "$DIR/frames/filter-06.txt"
  fi
  if grep -q "ssh" "$DIR/frames/filter-06.txt"; then
    pass "the filter found the rows it was typed for"
  else
    fail "the filter matched nothing on a host that runs sshd"
  fi
  if ! grep -q "filter:" "$DIR/frames/filter-07.txt"; then
    pass "Escape drops the filter (D-29)"
  else
    fail "Escape did not drop the filter"
  fi
}

linter() {
  part "6. the frame linter over every captured frame (V4)"
  if python3 "$DIR/frame-lint.py" "$DIR"/frames/*.txt; then
    pass "every frame keeps every invariant"
  else
    fail "the linter found problems"
  fi
}

induced_load() {
  # One steady load answers both questions. FR-1a asks what a known quota looks
  # like, FR-13 asks whether the window changes what stands on top; both need a
  # process burning a known fraction of a core for as long as the check runs,
  # and raising that twice cost a second load and a second settle for nothing.
  part "8. induced states: a known CPU quota (FR-1a) and a spike against it (FR-13)"
  sudo -n systemd-run --scope --slice=hs -p CPUQuota=50% --unit=hs-steady -q \
    timeout 45 python3 -c 'while True: pass' hs-load-steady &
  wait_scope hs-steady
  local json cores
  json=$(model --dump-frame 3 --tick 1500)
  # The row is the process itself, found by the cgroup it sits in: the tree is
  # the process forest now, and the cgroup path is what says where a process
  # belongs (FR-20).
  cores=$(node_value "$json" /hs.slice/hs-steady.scope instant cpu)
  if [ -z "$cores" ]; then
    fail "the scope of the load is not in the tree"
  else
    python3 - "$cores" <<'EOF'
import sys
v = float(sys.argv[1])
sys.exit(0 if 0.40 <= v <= 0.60 else 1)
EOF
    if [ $? -eq 0 ]; then
      pass "a 50 percent quota shows as $cores cores"
    else
      fail "a 50 percent quota shows as $cores cores, not about 0.500"
    fi
  fi

  rm -f "$DIR"/frames/spike-*.txt
  # A one second interval, set explicitly. The interface opens at three
  # seconds (D-28), and `send` captures 1.3 s after a keystroke - on a three
  # second screen that captures the frame before the key, and the two
  # windows then read as equal because both are showing the same old frame.
  start_app spike "sudo -n $BIN --tick 1000 --log $DIR/app-spike.log"
  # The induced load is started from this script, so in the process forest it
  # hangs under the shell that runs the check rather than near the root. `v`
  # flattens the forest and the filter reaches the command line, which is where
  # the marker is: two keystrokes instead of a walk down a branch that depends
  # on how the check itself was started (FR-18, FR-2).
  send spike 01 v
  send spike 02 "/"
  tmux send-keys -t "$SESSION" "hs-load-"
  pause 0.5
  send spike 03 Enter
  pause 12
  # The spike outlives the reading by design. It used to be `timeout 6`, which
  # meant the check raced it: waiting for the scope, then two keystrokes, came
  # to about six seconds, and a run that lost the race read 0.000 for the spike
  # and reported the two windows as equal. Nothing about that failure said
  # "too late" - it looked like the modes not working. The load is stopped
  # explicitly below the moment the frames are taken, so a longer timeout costs
  # the section nothing.
  sudo -n systemd-run --scope --slice=hs -p CPUQuota=100% --unit=hs-spike -q \
    timeout 25 python3 -c 'while True: pass' hs-load-spike &
  # The spike has to be on screen before the two sortings are read, and what
  # says it is on screen is its counter moving, not four seconds passing. The
  # `pause 12` above stays: that one is the averaging window of FR-13 filling
  # up, which is the thing being measured rather than an overhead.
  # Three core seconds, because the interface opens at a three second
  # interval (D-28) and the instant window is that interval: a spike that
  # has burnt less than the window is averaged with the idle part of it and
  # does not reach the top row.
  wait_burn hs-spike 2.0
  send spike 04 c          # sort by cores, in the average window it opens in
  send spike 05 a          # the same rows over the last interval
  stop_app
  # The load is stopped before the wait, not after it: `wait` waits for the
  # background `systemd-run`, and that does not return until the `timeout`
  # inside the scope runs out. Stopping the scope first ends the load, the
  # background job returns, and the wait costs nothing.
  sudo -n systemctl stop hs-steady.scope hs-spike.scope 2>/dev/null
  wait
  local instant_first average_first
  average_first=$(top_row "$DIR/frames/spike-04.txt")
  instant_first=$(top_row "$DIR/frames/spike-05.txt")
  echo "  instant first row: $instant_first"
  echo "  average first row: $average_first"
  # Both loads run the same interpreter under the same shell, so on screen the
  # two rows read alike; what tells them apart is the figure. Over the last
  # interval the spike is on top with about a whole core, while the average is
  # still carried by the steady half core it has been burning since the quota
  # was measured - the spike has run four seconds out of the sixteen the
  # average covers, which is a third of a core and not a half.
  local instant_top average_top
  instant_top=$(top_cores "$DIR/frames/spike-05.txt")
  average_top=$(top_cores "$DIR/frames/spike-04.txt")
  echo "  instant top: $instant_top cores   average top: $average_top cores"
  if python3 -c "import sys; sys.exit(0 if float('$instant_top') > float('$average_top') + 0.3 else 1)"; then
    pass "the window changes what stands on top, which is the point of the mode"
  else
    fail "the two windows put the same figure on top: $instant_top against $average_top"
  fi
}

induced_disk() {
  part "8. induced state: disk (FR-1) and a file that cannot be read (FR-8)"
  # The disk figures come from `/proc/<pid>/io`, and that file is readable only
  # by the owner of the process or by root. So one load answers both questions:
  # run as root the write has to be visible, and run as an ordinary user over
  # the same root-owned processes it has to come back unavailable rather than
  # as a zero.
  local json wr wr_user
  sudo -n systemd-run --scope --slice=hs --unit=hs-disk -q \
    timeout 20 sh -c 'while :; do dd if=/dev/zero of='"$DIR"'/io bs=1M count=200 oflag=direct status=none; done' &
  wait_scope hs-disk
  json=$(model --dump-frame 3 --tick 1200)
  wr=$(node_value "$json" /hs.slice/hs-disk.scope instant wr)
  json=$("$BIN" --dump-frame 3 --tick 1200 --dump-model json 2>/dev/null)
  wr_user=$(node_value "$json" /hs.slice/hs-disk.scope instant wr)
  sudo -n systemctl stop hs-disk.scope 2>/dev/null
  wait
  rm -f "$DIR/io"
  if [ -n "$wr" ] && python3 -c "import sys; sys.exit(0 if float('$wr') > 1e6 else 1)"; then
    pass "the write of the load is visible: $wr bytes per second"
  else
    fail "the write of the load is not visible: '$wr'"
  fi
  if [ -z "$wr_user" ]; then
    pass "a process whose io file cannot be opened reports the write as unavailable, not as zero"
  else
    fail "the write of an unreadable process came back as a number: '$wr_user'"
  fi
}

# The one induced state whose colour is known in advance. Everything else this
# check raises reads unusual at most: a 50 percent quota is half a core, and the
# alarm step is above a whole one. So a load that burns more than one core is
# raised on purpose, and the linter is told to fail if the screen stayed quiet -
# without it invariant 18 counts zero out of zero on a healthy host, which reads
# exactly like a pass (FR-21, D-42).
induced_alarm() {
  part "8c. induced state: a figure the reading must call alarm (FR-21)"
  rm -f "$DIR"/frames/alarm-*.txt
  sudo -n systemd-run --scope --slice=hs -p CPUQuota=250% --unit=hs-alarm -q \
    timeout 30 python3 -c '
import os
for _ in range(2):
    if os.fork() == 0:
        while True:
            pass
while True:
    pass
' hs-load-alarm &
  wait_scope hs-alarm
  wait_burn hs-alarm 3.0
  sudo -n "$BIN" --dump-style 2 --tick 800 --size 100x30 \
    > "$DIR/frames/alarm-00.txt" 2>/dev/null
  sudo -n systemctl stop hs-alarm.scope 2>/dev/null
  wait
  if python3 "$DIR/frame-lint.py" --alarm-min 1 "$DIR/frames/alarm-00.txt"; then
    pass "a row burning more than a core reads alarm, and the map says so"
  else
    fail "the reading did not reach the screen under a load it must call alarm"
  fi
}

induced_many() {
  part "8. induced state: many nodes (FR-6)"
  # Each load is a copy of `sleep` under its own name, so the search can look
  # for one of them the way an engineer would: by the name on the row. A bare
  # `sleep 24` would put two hundred rows on screen that nothing tells apart.
  for i in $(seq 1 200); do
    cp /bin/sleep "$DIR/hs-many-$i" 2>/dev/null
    sudo -n systemd-run --scope --slice=hs --unit=hs-many-$i -q timeout 25 "$DIR/hs-many-$i" 24 >/dev/null 2>&1 &
  done
  # Two hundred scopes do not arrive at once, and twelve seconds was a guess at
  # how long they take. What the check needs is that they are all up, and the
  # tree says so: wait until the slice stops growing.
  python3 - <<'PY'
import os, time
d = "/sys/fs/cgroup/hs.slice"
seen, still, deadline = -1, 0, time.time() + 30
while time.time() < deadline:
    n = len([x for x in os.listdir(d) if x.endswith(".scope")]) if os.path.isdir(d) else 0
    still = still + 1 if n == seen and n >= 200 else 0
    if still >= 3:
        break
    seen = n
    time.sleep(0.2)
print(f"  {seen} scopes up")
PY
  local nodes ms
  nodes=$(model | python3 "$DIR/model-query.py" count)
  echo "  nodes in the tree: $nodes"
  # The filter works on the level in view, so the search over the whole host
  # starts by flattening the forest with `v` (FR-18).
  # The tick stays at the walk tick rather than the second the tmux version
  # used. What the log is read for below is the worst collection on a full tree,
  # and a collection costs what it costs however often it is asked for - at 150
  # ms the tree is simply walked more times, which is the harder case, not the
  # easier one.
  walk many "v / h s - m a n y - 1 9 Enter" --log "$DIR/app-many.log"
  local matched
  matched=$(grep -c "hs-many-19" "$(last_frame many)")
  if [ "$matched" -ge 1 ]; then
    pass "the search finds a node among $nodes ($matched rows shown)"
  else
    fail "the search found nothing among $nodes nodes"
  fi
  ms=$(grep -o 'collect_ms=[0-9.]*' "$DIR/app-many.log" | cut -d= -f2 | sort -g | tail -1)
  echo "  worst collection time on a full tree: ${ms} ms"
  sudo -n systemctl stop 'hs-many-*.scope' 2>/dev/null
  wait
  rm -f "$DIR"/hs-many-*
}

induced_vanishing() {
  part "8. induced state: disappearing processes"
  ( for i in $(seq 1 60); do sleep 0.2 & done; wait ) >/dev/null 2>&1 &
  local churn=$!
  # The tick stays at 300 ms here rather than dropping to the walk tick: what is
  # being checked is that a `/proc/<pid>` vanishing between the listing and the
  # read changes nothing, and that race needs a collection wide enough to fall
  # into. The four keys cost four ticks all the same.
  WALK_TICK=300 walk churn "Down Down Down Down" --log "$DIR/app-churn.log"
  kill $churn 2>/dev/null
  if grep -qi "panic" "$DIR/app-churn.log" "$DIR"/frames/churn-*.txt; then
    fail "a vanished process brought the application down"
  else
    pass "vanished processes changed nothing on screen"
  fi
}

induced_names() {
  part "8. induced states: long and non-ASCII names (FR-12)"
  local long_name="hs-$(python3 -c 'print("n"*115)')"
  local image
  image=$(docker images --format '{{.Repository}}:{{.Tag}}' | grep -v '<none>' | head -1)
  if [ -z "$image" ]; then
    skip "no local image to start a container from"
  else
    docker run --rm -d --name "$long_name" "$image" sleep 40 >/dev/null 2>&1 \
      || docker run --rm -d --name "$long_name" --entrypoint sleep "$image" 40 >/dev/null 2>&1
    if docker ps --format '{{.Names}}' | grep -q "^hs-nnn"; then
      pause 2
      # The name of the container is on the row of every process it runs, and
      # the filter reaches it (FR-20). A hundred and twenty characters have to
      # be cut down to the column rather than pushing the table sideways.
      walk long "v / h s - n n n Enter" --log "$DIR/app-long.log"
      if python3 "$DIR/frame-lint.py" "$DIR"/frames/long-*.txt >/dev/null; then
        pass "a 120 character container name keeps the columns"
      else
        fail "a long container name broke the layout"
        python3 "$DIR/frame-lint.py" "$DIR"/frames/long-*.txt | head -5
      fi
      docker rm -f "$long_name" >/dev/null 2>&1
    else
      skip "the container with a long name did not start"
    fi
  fi

  # Docker itself refuses a non-ASCII container name, so the induced state is
  # raised on a process argument instead - the other half of the same case.
  if docker run --rm -d --name "hs-имя" "$image" true >/dev/null 2>&1; then
    docker rm -f "hs-имя" >/dev/null 2>&1
    echo "  note: docker accepted a non-ASCII container name"
  else
    echo "  note: docker refuses a non-ASCII container name, so only the process case is raised"
  fi
  sudo -n systemd-run --scope --slice=hs --unit=hs-cyr -q \
    timeout 40 python3 -c 'import time; time.sleep(39)' --бот hs-cyr-load &
  wait_scope hs-cyr
  # The Cyrillic sits in the command line of the process, which is both what
  # the filter reads and what the card shows, so the walk is: flatten the
  # forest, filter to the load, leave the filter, open the card.
  walk cyr "v / h s - c y r - l o a d Enter Enter" --log "$DIR/app-cyr.log"
  sudo -n systemctl stop hs-cyr.scope 2>/dev/null
  wait
  # The letters must reach the screen as they are, and the columns must hold:
  # what breaks a frame is a control character, not a letter (FR-12).
  if grep -qP '[\x{0400}-\x{04FF}]' "$DIR"/frames/cyr-*.txt; then
    pass "the argument is shown in its own script (FR-12)"
  else
    fail "the non-ASCII argument did not reach any frame"
  fi
  if python3 "$DIR/frame-lint.py" "$DIR"/frames/cyr-*.txt >/dev/null; then
    pass "a non-ASCII argument keeps the columns"
  else
    fail "a non-ASCII argument broke the layout"
    python3 "$DIR/frame-lint.py" "$DIR"/frames/cyr-*.txt | head -5
  fi
}

degraded() {
  part "8. induced states: no docker socket and no root (FR-3, FR-8, D-13)"
  # The row this section needs is a container's, and it has to be the selected
  # one: the line under the table describes whatever the cursor stands on, and
  # that is where the unavailability is spelled out. Filtering on the word
  # `container` does not give that - it matches `containerd` by process name
  # just as well, and what ends up on top is then whatever the host happens to
  # be busy with. So the identifier is read out of the model first and the
  # filter is that identifier: an exact row, the same one on every run.
  local cid
  # Under sudo: without root the forest is only this user's processes, and the
  # containers of the host are not in it - the model then honestly reports no
  # container and the section skips itself for the wrong reason.
  cid=$(sudo -n "$BIN" --docker-socket none --dump-model json 2>/dev/null | python3 -c '
import json, sys
out = []
def walk(n):
    if n.get("owner_kind") == "container" and n.get("owner"):
        out.append(n["owner"])
    for c in n.get("children", []):
        walk(c)
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
root = d.get("tree")
if root:
    walk(root)
print(out[0] if out else "")
')
  if [ -z "$cid" ]; then
    skip "no container on this host to degrade the name of"
  else
    echo "  filtering on the container $cid"
    walk nodocker "v / $(echo "$cid" | sed 's/./& /g')Enter" \
      --docker-socket none --log "$DIR/app-nodocker.log"
    local filtered
    filtered=$(last_frame nodocker)
    # The name degrades to the bare short identifier - twelve hexadecimal
    # characters - and not to `docker-<id>`: only one of the runtimes hostscope
    # recognises is Docker, so the prefix would be a claim (D-13, D-23).
    if grep -qE "[0-9a-f]{12}" "$filtered"; then
      pass "without the socket a container shows its short identifier"
    else
      fail "the degraded name is missing"
    fi
    if grep -q "unavailable" "$filtered"; then
      pass "the unavailability is marked, not hidden"
    else
      fail "nothing marks the unavailable data"
      sed -n '4,6p' "$filtered"
    fi
  fi

  # Without root, and without sudo in front of it: this is the FR-8 case.
  rm -f "$DIR"/frames/noroot-*.txt
  "$BIN" --keys "m Down Right v" --dump-frame 4 --tick "$WALK_TICK" --size 100x30 \
    --docker-socket none --log "$DIR/app-noroot.log" > "$DIR/frames/noroot-01.txt" 2>&1
  if grep -qi "panic" "$DIR/app-noroot.log" "$DIR"/frames/noroot-*.txt; then
    fail "the application fell over without root"
  else
    pass "without root the application keeps working"
  fi
}

security() {
  part "7. security and read only (V7, FR-9, FR-10, FR-10a, D-41)"
  local canary="HS-CANARY-8f2a1c9e"
  sudo -n systemd-run --scope --slice=hs --unit=hs-canary -q \
    --setenv=HS_CANARY=$canary timeout 45 python3 -c 'import time; time.sleep(44)' hs-canary-load &
  wait_scope hs-canary
  rm -f "$DIR/app-canary.log"
  # The same walk as the name check: flatten the forest, filter to the process
  # that carries the variable, then open its card - which is the one place a
  # variable would show up if it were ever read.
  walk canary "v / h s - c a n a r y - l o a d Enter Enter" --log "$DIR/app-canary.log"
  sudo -n systemctl stop hs-canary.scope 2>/dev/null
  wait
  if grep -q "HS_CANARY" "$DIR"/frames/canary-*.txt; then
    fail "a variable name reached the screen: the environment must not be read at all (FR-9)"
  else
    pass "no variable name reached any frame (FR-9)"
  fi
  if grep -q "$canary" "$DIR"/frames/canary-*.txt "$DIR/app-canary.log"; then
    fail "the value of a variable reached the screen or the log (FR-9)"
  else
    pass "no value of a variable reached any frame or the log (FR-9)"
  fi

  rm -f "$DIR/strace.log"
  # `%file` and not `openat`. The binary is linked against musl, and musl on
  # x86_64 calls `open` rather than `openat`, so a filter naming `openat` alone
  # matched nothing: on 2026-08-30 this trace held two lines - the `execve` and
  # the exit - for a run that read several hundred files, and every count below
  # was zero out of zero. `%file` is every syscall that takes a file name, so no
  # future libc can empty the trace the same way.
  sudo -n strace -f -e trace=%file -o "$DIR/strace.log" \
    "$BIN" --dump-frame 3 --tick 400 --size 100x30 >/dev/null 2>&1
  local execs writes environ opened
  execs=$(grep -c 'execve(' "$DIR/strace.log")
  opened=$(opened_paths "$DIR/strace.log" | grep -c '^/proc/')
  writes=$(grep -E '\b(open|openat|openat2)\(' "$DIR/strace.log" \
    | grep -c 'O_WRONLY\|O_RDWR\|O_CREAT')
  environ=$(grep -cE '\b(open|openat|openat2)\(.*/environ' "$DIR/strace.log")
  echo "  execve calls: $execs   files under /proc opened: $opened"
  echo "  opened for writing: $writes   environ opened: $environ"
  # The floor under everything else in this section. A trace that recorded no
  # open at all makes each count below zero out of zero, which reads exactly
  # like a pass.
  if [ "$opened" -gt 100 ]; then
    pass "the trace recorded $opened opens under /proc, so the counts below mean something"
  else
    fail "the trace recorded only $opened opens under /proc: it cannot show what was opened"
  fi
  if [ "$execs" -le 1 ]; then
    pass "the application runs no external command (FR-10)"
  else
    fail "the application ran $execs processes"
  fi
  if [ "$writes" -eq 0 ]; then
    pass "the application opened nothing for writing (FR-10)"
  else
    fail "the application opened $writes files for writing"
    grep -E '\b(open|openat|openat2)\(' "$DIR/strace.log" \
      | grep 'O_WRONLY\|O_RDWR\|O_CREAT' | head -3
  fi
  # FR-9 as a syscall fact rather than as a screen fact: the file that holds
  # the tokens of every service on the host is never opened.
  if [ "$environ" -eq 0 ]; then
    pass "the application never opened a /proc/<pid>/environ (FR-9)"
  else
    fail "the application opened environ $environ times"
    grep -E '\b(open|openat|openat2)\(.*/environ' "$DIR/strace.log" | head -3
  fi

  # FR-10a as the same syscall fact: the whole surface of files opened for data,
  # not just the one file FR-9 forbids. A statically linked binary opens what the
  # application asked for and nothing else, so anything here that is not /proc,
  # /sys/fs/cgroup, the account database or the link speeds is a read nobody
  # declared. `/sys/class/net` is the denominator the network reading of the
  # host row is a share of, and there is no other measured one (D-42).
  local outside
  outside=$(opened_paths "$DIR/strace.log" \
    | grep -vE '^/proc($|/)|^/sys/fs/cgroup($|/)|^/etc/passwd$|^/sys/class/net($|/)' \
    | sort -u)
  if [ -z "$outside" ]; then
    pass "nothing was opened outside /proc, /sys/fs/cgroup, /etc/passwd and /sys/class/net (FR-10a)"
  else
    fail "files were opened that the requirement does not name (FR-10a)"
    echo "$outside" | head -5 | sed 's/^/    /'
  fi

  # D-41: the one file outside the two trees can be refused, and the check says
  # so only when it saw the file opened without the flag. A run where it was
  # never opened at all would pass the first half and prove nothing.
  rm -f "$DIR/strace-nopasswd.log"
  sudo -n strace -f -e trace=%file -o "$DIR/strace-nopasswd.log" \
    "$BIN" --no-etc-passwd --dump-frame 2 --tick 400 --size 100x30 >/dev/null 2>&1
  if opened_paths "$DIR/strace-nopasswd.log" | grep -qx '/etc/passwd'; then
    fail "--no-etc-passwd opened the account database anyway (D-41)"
  elif ! opened_paths "$DIR/strace.log" | grep -qx '/etc/passwd'; then
    fail "/etc/passwd was not opened even by default, so this check proves nothing (D-41)"
  else
    pass "/etc/passwd is opened by default and left alone with --no-etc-passwd (D-41)"
  fi
}

sizes() {
  part "8. induced state: terminal size"
  for size in 60x20 100x30 200x60; do
    local w=${size%x*} h=${size#*x}
    tmux kill-session -t "$SESSION" 2>/dev/null
    tmux new-session -d -s "$SESSION" -x "$w" -y "$h" "sudo -n $BIN"
    pause 1.5
    tmux capture-pane -pN -t "$SESSION" > "$DIR/frames/size-$size.txt"
    tmux send-keys -t "$SESSION" Down
    pause 1.2
    tmux capture-pane -pN -t "$SESSION" > "$DIR/frames/size-$size-b.txt"
    stop_app
  done
  # A resize on the fly must not leave a torn frame.
  tmux kill-session -t "$SESSION" 2>/dev/null
  tmux new-session -d -s "$SESSION" -x 100 -y 30 "sudo -n $BIN"
  pause 1.5
  tmux resize-window -t "$SESSION" -x 140 -y 40
  pause 1.5
  tmux capture-pane -pN -t "$SESSION" > "$DIR/frames/size-resized.txt"
  stop_app
  if python3 "$DIR/frame-lint.py" "$DIR"/frames/size-*.txt; then
    pass "every size keeps every invariant"
  else
    fail "a size broke an invariant"
  fi
}

measurements() {
  part "9. non-functional measurements and own usage"
  rm -f "$DIR/app-perf.log" "$DIR/stream.raw"
  tmux kill-session -t "$SESSION" 2>/dev/null
  # One run answers both questions. The timings come from the application's own
  # log and the bytes from what tmux saw, while the CPU and the memory come from
  # the cgroup of the scope it runs in. Running it twice cost a second window
  # for nothing, and it also meant the two halves of section 6a described two
  # different runs that only looked alike.
  tmux new-session -d -s "$SESSION" -x 100 -y 30 \
    "sudo -n systemd-run --scope --slice=hs --unit=hs-app -q $BIN --log $DIR/app-perf.log --tick 1000"
  tmux pipe-pane -o -t "$SESSION" "cat >> $DIR/stream.raw"
  pause 20
  tmux pipe-pane -t "$SESSION"
  # Read while the scope is still alive: stopping it takes the counters with it.
  local usage mem
  usage=$(sudo -n cat /sys/fs/cgroup/hs.slice/hs-app.scope/cpu.stat 2>/dev/null | head -1 | awk '{print $2}')
  mem=$(sudo -n cat /sys/fs/cgroup/hs.slice/hs-app.scope/memory.current 2>/dev/null)
  stop_app
  sudo -n systemctl stop hs-app.scope 2>/dev/null
  python3 - "$DIR/app-perf.log" "$DIR/stream.raw" <<'EOF'
import re, sys, os
log, stream = sys.argv[1], sys.argv[2]
collect, render, first = [], [], None
for line in open(log):
    m = re.search(r"collect_ms=([0-9.]+) render_ms=([0-9.]+)", line)
    if m:
        if float(m.group(1)) > 0: collect.append(float(m.group(1)))
        if float(m.group(2)) > 0: render.append(float(m.group(2)))
    m = re.search(r"first frame after ([0-9.]+) ms", line)
    if m: first = float(m.group(1))
def p95(xs):
    xs = sorted(xs)
    return xs[int(len(xs) * 0.95)] if xs else 0.0
size = os.path.getsize(stream)
print(f"  to the first frame: {first:.1f} ms" if first else "  to the first frame: not recorded")
print(f"  collection p95: {p95(collect):.1f} ms over {len(collect)} ticks")
print(f"  render p95:     {p95(render):.1f} ms over {len(render)} frames")
print(f"  output over 20 s: {size} bytes, {size/max(1,len(render)):.0f} per frame")
clears = open(stream, "rb").read().count(b"\x1b[2J")
print(f"  full screen clears in 20 s: {clears}")
EOF
  pass "the measurements are recorded above"
  if [ -n "$usage" ]; then
    python3 - "$usage" "$mem" <<'EOF'
import sys
cpu = float(sys.argv[1]) / 1e6 / 20 * 100
mem = float(sys.argv[2]) / (1 << 20)
print(f"  own usage over the same 20 s: {cpu:.2f} percent of one core, {mem:.1f} MB")
# Section 6 as restated by D-16 and widened by D-40: under 4 percent of one core
# on a forest of about 370 processes at a one second interval, and under 5 where
# the host has swapped - the swap column then reads `status` for every process
# beside the `stat` that is read anyway. Which of the two applies is read off the
# host rather than assumed, so neither figure is a guess about the other.
meminfo = {}
with open("/proc/meminfo") as f:
    for line in f:
        k, _, v = line.partition(":")
        meminfo[k] = float(v.split()[0])
swapped = meminfo.get("SwapTotal", 0.0) - meminfo.get("SwapFree", 0.0) > 0
limit = 5.0 if swapped else 4.0
print(f"  the budget here is {limit:.0f} percent: the host has "
      f"{'swapped, so the swap column is drawn' if swapped else 'swapped nothing'}")
sys.exit(0 if cpu < limit and mem < 100 else 1)
EOF
    if [ $? -eq 0 ]; then
      pass "own usage is inside the budget of section 6"
    else
      fail "own usage is above the budget of section 6"
    fi
  else
    skip "the scope of the application was not readable"
  fi
}

cleanup() {
  part "9. cleanup and comparison against the state before the run"
  stop_app
  sudo -n systemctl stop 'hs-*.scope' 2>/dev/null
  pause 2
  sudo -n rmdir /sys/fs/cgroup/hs.slice 2>/dev/null
  docker ps --format '{{.Names}}' | sort > "$DIR/state/containers-after.txt"
  systemctl list-units --failed --no-legend > "$DIR/state/failed-after.txt"
  if diff -q "$DIR/state/containers-before.txt" "$DIR/state/containers-after.txt" >/dev/null; then
    pass "the container list is the one from before the run"
  else
    fail "the container list changed"
    diff "$DIR/state/containers-before.txt" "$DIR/state/containers-after.txt"
  fi
  if diff -q "$DIR/state/failed-before.txt" "$DIR/state/failed-after.txt" >/dev/null; then
    pass "no new failed unit"
  else
    fail "the failed unit list changed"
    diff "$DIR/state/failed-before.txt" "$DIR/state/failed-after.txt"
  fi
  if [ -d /sys/fs/cgroup/hs.slice ]; then
    echo "  note: hs.slice still exists, removing it needs the scopes to be gone"
  fi
}

# The order follows the section numbers the parts print, so a log reads in
# the order of the verification document.
ALL="prepare baseline oracle pidstat network scenario keys linter security induced_load induced_alarm induced_disk induced_many induced_vanishing induced_names degraded sizes measurements cleanup"
# Each section is timed: the run costs minutes, and the only way to know which
# minute is worth paying for is to see where it goes.
for section in ${*:-$ALL}; do
  started=$(python3 -c 'import time; print(time.time())')
  $section
  python3 -c "import sys,time; print('  (%s took %.1f s)' % (sys.argv[1], time.time() - float(sys.argv[2])))" \
    "$section" "$started"
done

echo
echo "== summary: $PASS passed, $FAIL failed, $SKIP skipped =="
[ "$FAIL" -eq 0 ]
