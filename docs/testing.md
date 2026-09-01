# hostscope: how to verify the application

Date: 2026-08-14. Status: working procedure.
Every command in this document was executed on the Docker rig on
2026-08-14 and returned the result described. Anything unverified is
marked "not verified".

## 1. Why this document

The application draws a full screen in a terminal, runs as root and
takes almost all of its numbers from the state of a live host. None of
these three properties can be checked by reading code. What is needed is
a way to run the application for real, press keys, read the drawn screen
as text and compare the numbers against an independent source. This
document describes that way and the order in which it is applied.

## 2. The rigs

Two rigs are used, and the host they run on is given to the commands
below rather than written into them: `make live HOST=<name>`.

The Kubernetes rig - Ubuntu 24.04.4, kernel 6.8, 6 cores, 15 GB of
memory, cgroup v2 with the controllers
`cpuset cpu io memory hugetlb pids rdma misc`, about 210 processes and
89 cgroup nodes. The container runtime is containerd under microk8s:
the node `kubepods` is in the tree and six pods run in it. The test
account has passwordless `sudo`. Available: `tmux 3.4`, `strace`,
`systemd-run`, `python3`, `jq`. Rust is not installed, and neither is
Docker.

The absence of Docker is what tells this rig apart from the Docker rig,
where every check up to 2026-08-15 was run: there about 20 Docker
containers run under the systemd driver and the socket is readable, here
there is no socket at all. So the two rigs answer different questions
and neither replaces the other:

- The Kubernetes rig gives the Kubernetes and containerd shape, which
  until now was only checked offline over the fixture of D-23, and it
  gives the degraded case of FR-3 for real: containers are on the host,
  the socket is not there, and the name has to fall back to the short
  identifier.
- The Docker rig gives the Docker shape and the socket, so the container
  name arrives from the enrichment rather than from the cgroup path. The
  induced state "a container with a 120 character name" needs it: there
  is nothing to raise that state with on the Kubernetes rig.

Differences from the reference host, the machine the requirements were
measured on: there it is 128 cores and Ubuntu 22.04. The figures of
section 6 of the requirements (300 ms to first screen, 50 ms per frame)
are measured on the reference host; on the rigs above they are only
checked for order of magnitude and for absence of regression.

Both are live machines with services running on them. The rules for
handling them are in section 10.

## 3. The feedback loop

The application runs in a detached `tmux` session with a fixed window
size. Keys are sent with `send-keys`, the screen is read with
`capture-pane`. Verified on `htop`: a sorting change by the `M` key is
visible in the captured text.

```sh
# start; the window size is set explicitly
tmux new-session -d -s hs-run -x 100 -y 30 'sudo -n /tmp/hostscope/hostscope --log /tmp/hostscope/app.log'

# keystrokes
tmux send-keys -t hs-run Right      # deeper
tmux send-keys -t hs-run Escape     # back
tmux send-keys -t hs-run a          # switch measurement mode

# the screen as plain text, with no control sequences
tmux capture-pane -p  -t hs-run

# the same, keeping trailing spaces - for the grid check
tmux capture-pane -pN -t hs-run

# the same with colours - for selection and the dimmed (self) row
tmux capture-pane -pe -t hs-run

# the whole output stream into a file - for counting bytes per frame
tmux pipe-pane -o -t hs-run 'cat >> /tmp/hostscope/stream.raw'
tmux pipe-pane -t hs-run          # switch off
```

What matters about these commands:

- `capture-pane -p` returns ready text. There is no need to parse
  control sequences, and that is the main reason to choose `tmux` over
  `ssh -tt` with output stripping.
- Without `-N` trailing spaces are cut, so the check "exactly 100
  characters in a line" produces false positives without it.
- `-e` returns colours. That is the only way to check that the `(self)`
  row is drawn dimmed and the selected row is drawn selected.
- `pipe-pane` writes exactly what the application sent to the terminal.
  That is where bytes per frame come from, and the answer to whether the
  whole screen is repainted every time. For `htop` two seconds produced
  1256 bytes and not a single screen-clearing sequence.
- A pause between the keystroke and the capture is always needed:
  `python3 -c "import time; time.sleep(1.5)"`. A capture taken right
  after `send-keys` catches the old frame.

That last property is what made `tmux` the expensive way to walk a
scenario, and it is why the walks no longer go through it. The pause has
to cover a tick, the tick is a second, so every key cost 1.3 seconds and
a walk of twenty one keys cost half a minute. A key program costs its
length in ticks and waits for nothing at all:

```sh
# the same walk, with no terminal and no pause anywhere in it
hostscope --keys "m Down Enter Down i Escape v Space" --dump-frame 8 \
          --tick 150 --size 100x30
```

Measured on the Kubernetes rig on 2026-08-15, the walk of twenty one
keys: 29.6
seconds through `tmux`, 8.3 seconds as a key program including a short
`tmux` pass kept alongside it, and 2.9 seconds for the key program on
its own at a tick of 100 ms. The frames are written under the numbering
`tmux` used, so a check that reads frame `-04` reads the same frame it
did before.

What a key program gives up is the terminal: these frames are what the
application printed, not what a terminal displayed. So `tmux` stays in
three places, and only there - the terminal sizes and the resize on the
fly, where a terminal is the thing under test; the spike of FR-13, where
an event has to arrive from outside while the application keeps running;
and a two key pass next to the main walk, which is what says a real
terminal draws the frame the dump drew. Without that last one the whole
layer would have quietly stopped checking terminals at all.

The same applies to waiting for an induced load. `pause 4` after a
`systemd-run` was a reserve with nothing behind it: measured on the same
day, the scope is in `/sys/fs/cgroup` and its `cpu.stat` counter is
above zero in under 20 ms. What the check waits for now is that fact,
plus one collection tick so the application has looked at the host once
since the load started.

The build happens on the Mac and cross-compiles to the host: the target
`x86_64-unknown-linux-musl` is installed there and `.cargo/config.toml`
links it with `rust-lld` and `link-self-contained`, so no musl toolchain
and no Docker are needed. Measured on 2026-08-15: a release build of the
target takes 49 seconds from scratch and 6.5 seconds after an edit. The
binary it produces needs nothing installed on the host; it was 918 KB
then, 968 KB after the dependency update of 2026-08-29 and 1008 KB on
2026-08-31, when the readings of FR-21 were in it.

Building, shipping and running are one command, `make live`, and not
three. The three were `ssh mkdir`, `scp` and `ssh bash host-check.sh`,
each a round trip with a wait and a decision in between. Counted over
the session transcripts of this project on 2026-08-15: 130 trips to the
host against 11 full runs, and a median verification round of 5.7
minutes of which the commands were running 40 percent of the time - the
rest went on assembling the next command. `make live-bg` detaches the
run on the host and `make live-log` collects it, so the four minutes are
spent working rather than watching.

An earlier version of this paragraph said the target was not installed
on the Mac and advised building natively on the rig. That is no
longer true, and the round trip through `rsync` is not needed.

## 4. What the code must provide for verification

Without these hooks verification comes down to parsing ASCII off the
screen, and that cannot tell an arithmetic error from a layout error.
They are now in the requirements as FR-17 (operator decision
2026-08-14); here is how they are used.

This list is the only place they are written down. `--help` names the
options someone running the application needs and nothing else (operator
decision 2026-09-02), because the README is generated from that help and
a reader looking for the process eating their host has no use for
`--dump-style`. The hooks still parse, and `tests/documents.rs` fails if
one of them turns up in the help again.

- `--cgroup-root DIR` and `--proc-root DIR` - read a captured snapshot
  instead of the live `/sys/fs/cgroup` and `/proc`. Gives repeatable
  tests with no live host involved.
- `--docker-socket PATH|none` - substituting and disabling the socket,
  the acceptance of FR-3 and D-13.
- `--no-etc-passwd` - leave the account database unopened, so the `OWNER`
  column shows the uid of a login session instead of the login name
  (D-41). Every test passes it: a snapshot has to decide the whole model,
  and `/etc/passwd` belongs to the machine the test runs on.
- `--dump-model json` - print the tree model as numbers to standard
  output and exit. Comparison against the oracle runs over this output,
  not over screen text.
- `--dump-frame N` - render N frames as text to standard output and
  exit. Gives a layout check with no terminal and no `tmux`.
- `--dump-style N` - the same N frames, each followed by a map of the
  same shape naming the role of every cell: `.` plain, `c` calm, `u`
  unusual, `a` alarm, `b` a cell of a bar or a sparkline, `s` the ground
  of the selected row, `m` a cell the filter matched. Text cannot say what colour it was drawn in, so without
  this hook the whole linting pipeline is blind to the reading FR-21
  mandates. Every walk of the live check runs through it, and the map
  follows the last line of its frame, so a check that reads a frame by
  line number or by grep never sees the map.
- `--keys "Right Right a Escape"` - run a key program and stop. A
  scenario becomes a single command.
- `--tick MS` - a fixed collection tick, so that the averaging window is
  known exactly (FR-13, FR-15). Since D-27 it also sets the interval the
  interactive run opens at, which `-` and `+` then move; a dumped frame
  says the tick it was run with rather than the default.
- `--log FILE` - the log goes to a file only. A log in the terminal
  ruins the frame. The same log carries the collection time and the
  render time of every frame: there is no way to measure them from
  outside (section 9).

Dumps go to standard output rather than to files: FR-10 forbids writing
outside the log named on the command line, and a hook for tests is no
reason to make an
exception.

## 5. Layers of verification

A check may not take its truth from the thing it checks. Where a check
needs a table, a formula or a constant that the application also needs,
the two must read different sources - the application its own, the check
an independent one. Two copies of one source are one source: when it is
wrong, both are wrong together and the check reports success. This is
why widths are read from the `unicode-width` crate on the Rust side and
from Python's `unicodedata` in `scripts/frame-lint.py` (D-18), and why
the oracle in V2 parses the kernel files itself instead of calling into
the application.

V1. **Offline over a snapshot.** A snapshot of the `/proc` files the
application reads and of `cgroup.procs` across the hierarchy is written
out as a fixture, and the application is started with `--proc-root` and
`--cgroup-root`. Deterministic, runs on the Mac and in CI. Covers FR-1,
FR-5, FR-6, FR-13, FR-14, FR-15, FR-20.

Every run also passes `--no-etc-passwd`. Without it the model is not the
snapshot's alone: the name of a login session comes from the account
database of whatever machine the suite happens to be on. The first CI run
found this on 2026-08-30 - a test that had read `1000` on the developing
Mac, where that uid belongs to nobody, read `packer` on the Linux runner,
where it belongs to somebody (D-41).

The fixtures are built in code (`tests/support/mod.rs`) rather than
stored as files: what has to be checked is a shape - which process has
which parent, which cgroup holds which process - and a shape written in
code can be read next to the assertion about it. Three of them are the
layouts of Docker with the cgroupfs driver, of a Kubernetes node and of
a plain server with the systemd driver, copied from captures of the
three real environments (D-23) and used by
`tests/environment_shapes.rs`; the captures themselves weighed 7.5 MB.

Capturing what a cgroup snapshot needs, now that only one file per
cgroup is read:

```sh
sudo find /sys/fs/cgroup -maxdepth 4 -type f -name 'cgroup.procs' \
  -exec sh -c 'd=/tmp/hostscope/snap$(dirname {}); mkdir -p "$d"; cat {} > "$d/$(basename {})"' \;
```

A fixture is written twice with a known pause between them: a pair is
needed to check the deltas (FR-15) and both measurement modes (FR-13).

V2. **An oracle on the live host, and somebody else's reader beside
it.** `scripts/oracle.py` reads the same kernel files and computes the
totals with its own code: it builds the process forest out of its own
reading of `/proc/<pid>/stat` and sums each subtree itself. It must share
no code with the application - otherwise it repeats the application's
error. It is compared against `--dump-model json`. The tolerances come
from the requirements: 1 percent across the tree (FR-1), widened to what
a live host actually swings by, and 5 percent for the core sum against
`1 - idle` from `/proc/stat` (FR-1a).

The oracle proves the arithmetic and cannot prove the convention: it
reads the same files by the same understanding, so a wrong idea of what
a kernel file means is an idea it shares. `scripts/pidstat-check.py`
closes that. It reads nothing itself - `pidstat` of sysstat gives the CPU
and the disk of every process, `ps` of procps gives the memory, and both
have been reading these files for twenty years. What it compares is the
value and not the shape: `pidstat` knows nothing about parents, so the
subtree of a row is the one the model itself draws, and the tree stays
the oracle's business. The wrong field of `/proc/<pid>/io`, kilobytes
taken for bytes, a percentage of one core taken for a percentage of the
machine - each shows up here and nowhere else. On a host without sysstat
the section says so and skips; measured on the Kubernetes rig on
2026-08-29, it compares 207 rows in ten seconds and skips four as churn.
Removing the division by a hundred from its own reading of `%CPU` makes
seventeen rows disagree by exactly that factor, which is how it is known
to be able to fail.

A figure the model does not give is a problem here rather than a row to
pass over, and the summary counts the figures compared and not the rows
walked. The section runs as root, so a value the model leaves out is one
it failed to produce; passing over it let a model whose every figure was
missing report two hundred rows compared and nothing wrong. A run that
compared nothing at all - every row skipped as churn - is a problem for
the same reason the frame linter treats a run with no frames as one.

The two network figures of the header had nothing behind them at all: the
oracle does not read them, and `pidstat` has nothing to say about a
network namespace. `scripts/netlink-check.py` asks iproute2 for the same
counters over netlink - a different door into the kernel, walked through
by somebody else's code - and compares the delta over the window against
the host row. The two agree closely: 2089 against 2087 bytes a second in
and 11485 against 11478 out on the Kubernetes rig on 2026-08-29, which is
why the tolerance is 10 percent with a floor of 512 bytes a second. The
floor was 16 KB a second at first and made the check inert - this host
talks at a few kilobytes a second, so a probe that doubled the rate
passed with the whole difference sitting under the floor. With the low
floor that probe fails, which is how the check is known to be able to.

V3. **Scenarios through tmux.** A walk down the forest and back, the
card on every level, sorting, search, filter, pause. What is checked is not only
the final screen but also that `Esc` returns to the same row (FR-2) and
that the filter survives a level change (FR-6).

V4. **The frame linter.** The set of invariants from section 7 is run
over every frame captured by any test, rather than as a test of its own.
This is the main source of findings: one scenario yields 10-15 frames,
and each is checked against a dozen rules for free.

A frame check proves nothing until the frame it captured is the frame it
meant to capture. A key program that stops one card short, or a fixture
that leaves `fd/`, `limits`, `smaps_rollup` or `net/tcp` unwritten,
produces an empty card - and an empty card passes every invariant a
broken one would fail. Assert one line of the expected content by name
before asserting anything about the whole frame.

V5. **Induced states.** Host states are created deliberately (section
8): a spike, a steady load, a known quota, many nodes, disappearing
processes, long names, an unavailable Docker socket, absence of root.

V6. **Non-functional measurements** (section 9).

V7. **Security and read-only.** FR-9: start a process with the variable
`HS_CANARY=<known string>`, walk the scenario down to the card of that
process, and search for the string in every frame and in the log -
neither the value nor the name `HS_CANARY` may appear, because the
environment is not read at all. The same `strace` run proves it from the
other side: no open of a `/proc/<pid>/environ`. FR-10: `strace -f -e
trace=%file -o /tmp/hostscope/strace.log` around the whole run,
then check that there was exactly one `execve` - our own - and that no
open for writing went outside the log named on the command line.

`%file` and not a list of syscall names. The trace named `openat` alone
until 2026-08-30, and the binary is linked against musl, which on x86_64
calls `open`: the trace of a run that read several hundred files held two
lines, the `execve` and the exit, and every count taken from it was zero
out of zero. Three checks had been passing on an empty trace. The section
now counts the opens under `/proc` first and fails when there are too few
of them, so a trace that cannot show what was opened says so instead of
reading like a pass.

FR-10a: every path the trace shows opened is under `/proc` or
`/sys/fs/cgroup` or is `/etc/passwd`. D-41: a second run with
`--no-etc-passwd` opens no `/etc/passwd`, and the section fails when the
first run did not open it either - a comparison where neither side has
the file proves nothing.

## 6. The procedure for a single run

1. **Preparation.** Create `/tmp/hostscope`, capture the host state
   before the run: `docker ps --format '{{.Names}}'`, `systemctl
   list-units --failed`, `free -m`, `uptime`. Save it to a file.
2. **Delivery.** Build, put the binary at `/tmp/hostscope/hostscope`,
   write the version and the hash next to it.
3. **Baseline.** Start the application in `tmux` with no load at all,
   capture the first frame. Check FR-15: every value derived from
   cumulative counters is zero right after start.
4. **Oracle.** Capture `--dump-model json` and a `/proc` reading at the
   same moment. Compare the numbers. A mismatch beyond the tolerance is
   an arithmetic defect: stop here and go to step 8.
5. **Scenario.** Run the key program step by step. After every
   keystroke: pause, `capture-pane -p`, `capture-pane -pe`, and save the
   frame with the step number and the key pressed in the file name.
6. **Linter.** Run the invariants of section 7 over every saved frame.
7. **Induced state.** Raise the state needed from section 8, repeat
   steps 4-6, remove the load.
8. **Analysis.** Turn every mismatch into a dossier (section 11).
9. **Cleanup.** Kill the session, remove the load, delete the slice,
   compare the host state against the one recorded in step 1.

Steps 3-6 are worth running after every code change: they take under a
minute and catch layout regressions that the eye does not see.

## 7. The frame linter: invariants

Every invariant is checked on any frame and refers to a requirement.

1. All lines inside the frame take the same number of terminal cells,
   equal to the terminal width (section 11 of the requirements). Cells,
   not characters: a name in a wide script takes two cells per letter.
   The linter counts them with Python's `unicodedata` while the
   application counts them with the `unicode-width` crate (D-18), so a
   frame passes only when two independent tables agree. Capture with
   `-N`.
2. The frame contains no control character (FR-12), the data area
   included. Letters are not restricted: since D-17 a container name or
   a command line is shown in its own script, and the induced state
   "non-ASCII name" in section 8 checks exactly that.
3. Neither the frame nor the log contains the canary string, the name of
   a canary variable, or anything resembling `NAME=value` from the
   environment (FR-9).
4. Columns are separated by at least one space on every line, and no
   name leaves its column (section 11). In cells, as in invariant 1: the
   column offsets are read off the header, which is ASCII, and a row is
   not, so both linters turn a column back into an index through the same
   width tables they measure the line with. Found on 2026-08-15 - the
   offsets were read as character indexes, and a name in a wide script
   made the check report an overrun on a frame that was exactly the width
   of the terminal, and would have hidden a real one just as readily.
5. The `(self)` row, when present, is the first row of the level under
   each of the four sortings (FR-14).
6. The sum of the rows shown equals the parent value for every quantity
   (FR-1, FR-14). Taken from `--dump-model`, not from the text.
7. A non-zero value draws at least one tick of the bar (section 11), and
   the bar stands beside the column the rows are ordered by and nowhere
   else (D-27). Which column that is comes off the path line, not from a
   fixed offset: reading it always beside `CORES` was right only while
   `CORES` was the only column that carried one, and on a frame sorted
   by memory it reported every busy row as drawing no tick.
8. The column set and order are the same on every level (section 11).
9. The path line is present and labelled with the level it is on, `L0`
   and down (FR-12).
10. The measurement mode is labelled with its window: `INSTANT 1.0s` or
    `AVG over ...` (FR-13).
11. Neither the frame nor the log contains a panic, a backtrace or the
    word `panicked`.
12. A node with children is marked in its name with `>`, a node without
    children is not (section 11, D-20).
13. The path line names the view the rows come from - `view: tree` or
    `view: list` (FR-18). Invariant 6 - the rows add up to the parent -
    holds in both: the list shows the ends of the subtree, which are the
    parent split up rather than a second reading of it.
14. Every CPU figure is a core count: the column is headed `CORES` and
    no percentage appears anywhere on the frame (FR-1a, D-25).
15. The swap column, where a frame has one, stands between `MEM` and
    `DISK`. It is the one optional column - a host that has swapped
    nothing draws no `SWAP` at all (D-35).
16. The header holds its places: the `MEM`, `SWAP` and `WAIT` labels of
    the two summary lines sit at the same cell in every frame of one run.
    This is the one invariant read across frames rather than inside one,
    because a header that moves can only be seen by comparing two of
    them (D-39). `MEM` and `WAIT` must be there at all: a label the
    header stopped drawing would otherwise be compared against nothing
    and pass. `SWAP` is the one that may be absent, because a host that
    has swapped nothing draws no swap column (D-35).
17. The style map covers the frame cell for cell: as many lines, and each
    line as many characters as its frame line takes cells. A map that
    does not line up says nothing about the colours it claims to describe
    (FR-21, D-42). Checked only on a frame captured with `--dump-style`;
    a plain frame carries no map and skips this.
18. How loud the screen was: the linter counts the cells that read alarm
    and prints the total. This is a report and not a failure on its own -
    a ceiling fitted to two rigs cannot tell a palette that is too loud
    from a host that is genuinely in trouble, and the second is what this
    tool exists for. Where the right answer is known in advance - the
    induced states the live check raises itself - the section gives
    `--alarm-min N` and the linter fails below it. `--alarm-min` with no
    map anywhere is itself a failure, so a section cannot claim the floor
    while capturing plain frames.

## 8. Induced states

Load is created through `systemd-run --scope` with its own slice: it
gets a separate cgroup node with known limits, which immediately gives
an expected number to compare against. Verified: `CPUQuota=50%` yields
`cpu.max = 50000 100000`, the node appears in
`/sys/fs/cgroup/hs.slice/`, `cpu.stat` grows, and after the run the node
disappears on its own.

```sh
# exactly half a core for 30 seconds; the screen must show 0.500 cores
sudo systemd-run --scope --slice=hs -p CPUQuota=50% --unit=hs-steady -q \
  timeout 30 python3 -c 'while True: pass'
```

- **A known CPU quota** - the check for FR-1a: the application must show
  0.500 cores, not a percentage of an unclear base.
- **A spike against a steady load** - the acceptance of FR-13:
  `hs-steady` with a 20 percent quota runs all the time, `hs-spike` with
  100 percent lives for 3 seconds. In instant mode the spike comes
  first, in average mode the steady load does.
- **A figure the reading must call alarm** - the check for FR-21: a
  scope with `CPUQuota=250%` and three busy processes burns more than
  one core, and the frames captured with `--dump-style` are linted with
  `--alarm-min 1`. Every other state this check raises reads unusual at
  most, so without this one invariant 18 counts zero out of zero on a
  healthy host, which reads exactly like a pass. Measured on the
  reference host on 2026-08-30: 11 alarm cells under the load, 0 over
  the 42 mapped frames of an ordinary run.
- **Memory** - `-p MemoryMax=200M` and a script holding 150 MB: the
  check that the row shows the resident memory of the process and that
  the difference from the children total went into `(self)` (FR-14).
- **Disk** - `dd if=/dev/zero of=/tmp/hostscope/io bs=1M count=200
  oflag=direct` inside its own scope, compared against
  `/proc/<pid>/io`. The same load run against a hostscope started
  without root is the FR-8 case: the file belongs to its owner, so the
  figure has to come back unavailable rather than as a zero.
- **Container network** - a container named `hs-net` pulling data: the
  check for FR-11 through `/proc/<pid>/net/dev` in its namespace.
- **Many nodes** - 200 short-lived processes at once, each a copy of
  `sleep` under its own name so the search has something to find: the
  check for sorting and search (FR-6) and for frame time on a full
  tree.
- **Disappearing processes** - a loop of short `sleep` calls: the check
  that the application does not crash on a vanished `/proc/<pid>` and
  that the work of dead children settles into `(self)`, as FR-14 states.
- **Long names** - a container with a 120-character name and a process
  with a very long command line: the check for ellipsis truncation and
  for columns not merging.
- **A non-ASCII name** - a container named `hs-имя-по-русски` and a
  process with a Cyrillic argument. The letters are expected to reach
  the table, the card and the path line as they are (FR-12, D-17), a
  search for that substring finds the row, and the frame linter passes:
  the columns are measured in cells, so this is where a width error
  would show first. Docker refuses to create a container under a
  non-ASCII name, so on a live host only the process side of this state
  can be raised; the container side is raised offline, over a fixture.
- **Without root** - the same run as an ordinary user: fields are marked
  `n/a` and the application does not crash (FR-8, D-13). There is a
  subtlety here: on the Docker rig the user is a member of the `docker`
  group, so without `sudo` the socket is still reachable. To reproduce
  the conditions of section 8a of the requirements, a separate user without
  that group is needed, or `--docker-socket none`.
- **Docker unavailable** - `--docker-socket none`: the row shows the
  short identifier and an unavailability marker (FR-3).
- **Terminal size** - 60x20, 100x30, 200x60 and resizing on the fly with
  `tmux resize-window`.

## 9. Non-functional measurements

- **To the first screen.** `pipe-pane` is switched on before the start,
  and the time between the start and the first complete frame in the
  stream is measured. The 300 ms threshold is measured on the reference
  host; on the rigs the number is recorded for reference.
- **Frame time.** Not measurable from outside: `capture-pane` shows the
  result, not the duration. That is why the application writes the
  collection time and the render time of every frame into the log
  (FR-17), and the check takes the 95th percentile over 60 seconds of
  that log.
- **Own usage.** Start the application inside `systemd-run --scope
  --slice=hs --unit=hs-app` and read `cpu.stat` and `memory.current` of
  its node from outside. The application then sees itself as a `(self)`
  row on the top level - which doubles as a check that it does not hide
  its own usage.
- **Output volume per frame.** `pipe-pane` into a file for 10 seconds,
  bytes divided by the number of frames. Separately check that the
  screen clear does not arrive on every frame: for `htop` it did not
  arrive once in two seconds.

**The last full run, on both rigs, on the code that carries the card
fix.** The Docker rig passed 50 checks with none failed and nothing
skipped on 2026-08-31, in 181 seconds, the netlink comparison among them
with the thirty second window explained below. The card of a container
names its image, its state and its restart count there, which is what no
run before it could show.

The Kubernetes rig passed 49 with none failed and one skipped on
2026-09-01, in 171 seconds; the skip is the container with a 120
character name, which needs a Docker to raise it. 210 processes on six
cores: 22.0 ms to the first frame, collection 52.6 ms at the 95th
percentile over 19 ticks, render 0.8 ms over 20 frames, 1378 bytes a
frame with five full screen clears in 20 seconds, and the application
itself 3.38 percent of one core and 1.3 MB against the 5 percent a host
with the swap column drawn is held to (D-40). The linter read 45 frames
of the ordinary walk with no alarm cell on any of them, and the two
frames of the induced state with 11. The oracle compared 210 processes
over ten seconds and found no disagreement.

**The run before that, on both rigs, 2026-08-31.** The Kubernetes rig
passed 49 checks with none failed and one skipped - the same container
with a 120 character name. 211 processes on
six cores: 22.5 ms to the first frame, collection 52.1 ms at the 95th
percentile over 19 ticks, render 0.8 ms over 20 frames, five full screen
clears in 20 seconds, and the application itself 3.21 percent of one core
and 1.3 MB. The linter read 45 frames of the ordinary walk with no alarm
cell on any of them, and the two frames of the induced state with 11.

The Docker rig passed 49 and failed one the same day. 289 processes on
four cores: 42.5 ms to the first frame, collection 54.7 ms at the 95th
percentile, render 0.7 ms, 1355 bytes a frame, and 4.00 percent of one
core and 1.6 MB against the 5 percent a host with the swap column drawn
is held to (D-40) - the closest to that ceiling any run has come. The
oracle agreed on 289 processes and sysstat and procps on 1156 figures
with nothing skipped as churn. The 120 character container name was
raised here and kept the columns.

The failure was the netlink comparison, and it was the window rather
than the figures: the model read 48774 bytes a second in against 42313
over netlink, 15 percent apart, and four repeats within the minute came
back 0.2 to 0.3 percent apart. That rig carries a steady 40 to 70 KB/s
of its own, so a burst landing inside the fraction of a second between
the two starts moved a ten second rate by a sixth. The window is thirty
seconds since that day, which divides the same offset by three and takes
nothing away from what the check is for: a counter summed the wrong way
does not shrink with the window.

**On the Docker rig, 2026-08-29, the same day.** 40 checks passed, none
failed, nothing skipped, in 140 seconds - the container sections that
have nothing to work with on the Kubernetes rig all ran here. 366
processes, four cores, and two gigabytes of the swap device in use, so
this is the first full run with the swap column drawn: collection took
48.7 ms at the 95th percentile against 41.3 on the other rig, and the
application spent 3.55 percent of one core against 1.97. An earlier run
the same hour measured 4.12 percent and failed the budget of section 6.
The cause is `status` being read for every process wherever the host has
swapped (D-35), which is measured there as 2.7 ms against 7.2 for both
files. Settled by D-40: the budget carries two numbers now, 4 percent
where nothing is in swap and 5 where the column is drawn, and the check
reads `/proc/meminfo` to know which of them it is holding the host to.

**The run before those, on the Kubernetes rig, 2026-08-30.** 48 checks
passed, one failed, one skipped. The failure was the oracle, and it was
not one: a `containerd-shim` read 226709504 bytes to the oracle and
239030272 to the model, 5 percent apart, and the section re-run alone a
minute later compared 212 processes with no disagreement at all. That is
a process whose memory moved between two reads taken over the same ten
seconds. The figures of that run: 22.3 ms to the first frame, collection
53.1 ms at the 95th percentile over 20 ticks, render 0.8 ms over 21
frames, 1313 bytes a frame with 5 full screen clears in 20 seconds, and
the application itself 3.08 percent of one core and 1.3 MB against the
budget of 5 that a host with the swap column drawn is held to (D-40).

The two readings that cannot be waited for were raised on purpose. All
four of the cgroup facts reached a row (D-45). The machine read itself
as `cpu some avg10` 0.03, memory 0.00 and io 0.00 while idle, and 36.71,
0.00 and 0.00 under four busy scopes per core - the state that says the
three shares of time reach the screen at all (D-46).

An earlier run the same day, before the pressure and the facts were
written: 43 checks passed, none failed, one skipped. It is the first run that can see the
colour of the screen: 42 of the 45 captured frames carry a style map, the
linter checks that every map covers its frame cell for cell, and it
counts how loud each screen was (FR-21, invariants 17 and 18). Over an
ordinary run it counted 0 alarm cells; under the induced state raised for
this - three busy processes in a scope with `CPUQuota=250%` - it counted
11, which is what says the reading reaches the screen at all rather than
being switched off. The run was repeated the same day with the way-down
glyph of D-44 in place and gave the same counts: the rows the induced
state marks are leaves, which carry their reading themselves.

The security section holds the widened surface: nothing is opened outside
`/proc`, `/sys/fs/cgroup`, `/etc/passwd` and `/sys/class/net` (FR-10a),
and the account database is opened once by the default run and by no run
that passes `--no-etc-passwd` (D-41). The first run of the day failed
exactly there, naming `/sys/class/net/eno1/speed` - a read the code had
made before the requirement named it.

The figures of that run: collection took 42.1 ms at the 95th percentile
and a redraw 1.1 ms, the first frame arrived after 16.5 ms, the
application sent 1341 bytes per frame with five full screen clears in 20
seconds - the repaint of D-38 - and it spent 2.41 percent of one core and
1.1 MB on itself, against the 4 percent D-40 allows a host that has
swapped nothing. That rig draws no swap column and pays nothing for it
(D-35). A 50 percent quota showed as 0.496 cores, the oracle agreed on
205 processes, sysstat and procps on 820 figures over 205 rows with 4
skipped as churn, and netlink came within a tenth of a percent on both
directions.

The skipped check is the container with a 120 character name: that rig
runs containerd under microk8s and has no Docker to raise the state
with. What it gives instead is the degraded case of FR-3 for real - the
containers are there, the socket is not, and the name falls back to the
short identifier. The Docker side of FR-3 and the long container name
need the Docker rig, where the previous full run passed 30 checks with
none failed; `make live HOST=<that host>` runs them there.

## 10. Ground rules on a live host

Both rigs are live machines with services running on them, and the
services have users. The rules are mandatory on either.

- Everything of ours is named with the `hs-` prefix: the `tmux` session
  `hs-run`, the slice `hs.slice`, the units `hs-*`, the containers
  `hs-*`, the directory `/tmp/hostscope`.
- Other people's containers and units are never touched: no stopping, no
  restarting, no changing of limits. Our own containers are started with
  `--rm` only.
- Load is always bounded: `CPUQuota` no higher than 100 percent - one
  core out of the six on the Kubernetes rig, out of the four on the
  Docker rig -
  `MemoryMax` no higher than 1 GB, and always under `timeout`. On a
  machine of this size an unbounded load is noticeable to the users of
  the services.
- The `tmux` session is killed by name before the start, not wholesale:
  the user may have more sessions. On 2026-08-14 there were none.
- A run ends with cleanup: `tmux kill-session -t hs-run`,
  `sudo rmdir /sys/fs/cgroup/hs.slice`, removal of `/tmp/hostscope`. An
  empty slice does not disappear on its own - verified, it had to be
  removed by hand.
- After the cleanup the host state is compared against the one captured
  in step 1: the same container list, no new failed units.
- An induced load is stopped before the check waits for it. `wait` waits
  for the background `systemd-run`, and that does not return until the
  `timeout` inside the scope runs out, so a section that had finished its
  work in eight seconds still sat there until the load expired. Measured
  on 2026-08-15: five sections did this, and stopping the scope first
  took the whole run from about five minutes to three. What made it
  invisible is that nothing failed - the run was simply slow, and a slow
  run reads as a thorough one. Later the same day the walks moved off
  `tmux` and the waits for a load became waits for its counter, which
  took the run from three minutes to under two, and the sections added
  since fit inside what was freed: 37 checks now run in 113 seconds
  against the 29 that took 172. Of what is left, the
  averaging window of FR-13, the oracle's ten seconds and the twenty of
  the measurements are not overhead: they are the measurements.
- Two checks that need the same induced state share it. The quota of
  FR-1a and the spike of FR-13 both need a process burning a known
  fraction of a core, and the same is true of the timings and the own
  usage of section 9. Raising the state twice costs a second window and,
  worse, makes two figures describe two different runs that only look
  comparable.
- Every section prints what it cost. The run takes minutes, and the only
  way to know which minute is worth paying for is to see where it goes.

## 11. The defect dossier

A mismatch is useless without the material needed to reproduce it.
Collect it immediately, in one directory:

- the frame in two forms: `capture-pane -pN` and `capture-pane -pe`;
- `--dump-model json` of the same moment;
- a `/sys/fs/cgroup` snapshot of the same moment;
- the application log, and the `strace` output if it was enabled;
- the binary version, the terminal size, the key program up to the
  failure.

Then minimisation: reproduce it offline over the snapshot with
`--cgroup-root`. If it reproduces, the snapshot becomes a fixture and
the check becomes a test in the repository; this is a regression, and it
will not come back. If it does not reproduce over the snapshot, the
defect is tied to timing or to a race, and it has to be looked for in
the collection order rather than in the arithmetic.

## 12. What is not checked automatically

Screen readability, the appropriateness of colour accents, the clarity
of the hints, and whether the screen answers "who is eating the
resources" within a minute. That is checked by eye over `-e` captures
and against the mandatory rules of section 11 of the requirements. The
linter catches layout, not meaning.

## 13. Mapping to the requirements

| Requirement | How it is checked |
| --- | --- |
| FR-1, FR-5 | V1 over the snapshot, V2 against the oracle and against `pidstat` and `ps`, invariant 6 |
| FR-1a | Induced state with a known quota, oracle against `/proc/stat`, invariant 14 |
| FR-2 | V3: a walk down the forest and back on `Enter` and `BSpace`, the card on every level with `i`, return to the same row |
| FR-3 | V1 over a socket the test opens itself and answers on: the image, the state and the restart count the daemon gave reach the row and its card, while a container the answer does not mention stays on its short identifier. Induced state `--docker-socket none`, a run without root; V4: the card says the socket is unreadable only where it is, and says the answer has not arrived where the socket answered about other containers |
| FR-6 | V1 over a snapshot with 200 nodes, induced state "many nodes"; V4: a filter typed, kept and dropped - the path line names it and counts what it left, `Esc` gives the level back; V4: the bar stands beside the sorted column (D-27) |
| FR-7 | V3: pause, two consecutive frames match, after release they do not; V4: `-` and `+` walk the interval to both ends of the list and the key line says which one is on |
| FR-8 | Induced state "without root" with a separate user |
| FR-9 | V7: a canary in the environment, searched across frames and log; no `environ` in the file-syscall trace |
| FR-10 | V7: `strace -e trace=%file`, plus the build-time check |
| FR-10a | V7: every path opened in the file-syscall trace of a live run is under `/proc` or `/sys/fs/cgroup` or is `/etc/passwd` or under `/sys/class/net`, and a second run with `--no-etc-passwd` does not open the account database |
| FR-11 | Induced state "container network"; for host processes the unavailability marker; the host rates against netlink (V2) |
| FR-12 | Invariant 2 on every frame, induced state "non-ASCII name" (FR-4 was withdrawn by D-17) |
| FR-13 | Induced state "spike against steady load", two snapshots with a known pause |
| FR-14 | Invariants 5 and 6, induced states "memory" and "disappearing processes" |
| FR-15 | Step 3 of the procedure, a snapshot with a non-zero base |
| FR-16 | Withdrawn by D-24 |
| FR-17 | Two runs over one snapshot give an identical dump; the file-syscall trace |
| FR-18 | V4: the list scenario `v / r e d i s Enter` over the snapshot, invariants 1-13; the interface walk of the live check presses `v` |
| FR-19 | Withdrawn by D-24 |
| FR-21 | Unit tests over `src/model.rs` fix every threshold and hold the host row to the thresholds of a machine rather than of a process; unit tests over `src/render.rs` hold the figure, the summary line, the card and the row glyph to the same reading, and hold the bar out of it; V4: invariant 17 over every frame the live check captures with `--dump-style`, and invariant 18 reporting how loud each screen was; V2: `--dump-model json` carries the reading of every row and the denominators it was read against, and the facts of its control group beside it; V3: the `induced_ceilings` and `induced_pressure` sections raise the readings that cannot be waited for and demand the known answer |
| FR-20 | V1 over the three environment shapes: every container named on every process it runs, the filter finding those processes by the container name and by the kind of owner; every owner the model names is on a drawn row |
| D-25 | V1: a chain of single children is one row named for the whole chain, and its card names every link with its pid - over a chain of seven under a six-digit pid, at two widths, because a short chain and a three-digit pid are the two sizes at which a lost link stays invisible; V4: a card that cannot hold it says how many lines it hid, checked at the height that holds the card exactly and one line below it, because a guard on a comparison is wrong by one line at a time; V4: a pid too wide for its room is marked as cut, at four widths from 24 cells up, because a silent cut names another process; V3: the walk uses `Enter`, `BSpace` and `i` |
| D-26 | Invariant 8: the `OWNER` column is demanded on every frame, no longer only where it happens to be drawn |
| D-30 | V1 over a snapshot holding a shim with one child in a container: the row names the container in parentheses and keeps its own owner, a row with two containers under it names neither, and the filter typed with the container name finds the row that leads into it |
| D-31 | V1 over a snapshot laid out as a Kubernetes node: a shim with the pod sandbox and the workload container under it names the pod, a shim whose two children sit in different pods names nothing, and the parser reads the pod out of both cgroup driver layouts |
| Section 11 | `tests/documents.rs`: every block of what `--help` prints stands in `README.md` - a table as it is written, a paragraph word for word - so the description of the screen has one source and `make readme` is what moves it. A test of its own holds the hooks of section 4 out of both `--help` and `README.md`, and a third opens every picture the README shows |
| D-32 | V1 over a snapshot with the files, limits and PSS of a process: the card heads its two figure columns once, starts every `now` and every `avg` at the same cell, and gives `own virtual`, `own PSS`, `sockets`, `nofile` and `nproc` a label each |
| D-33 | V1 over a snapshot with a command line longer than the terminal: `pid`, `parent`, `user`, `threads` and `started` each have a label, the command wraps under its own label with nothing lost, and invariant 1 holds on every wrapped line. At 70 and 60 cells the explanations beside the figures and the cgroup path are whole once the wrapped lines are joined, and a pid the room cannot hold is marked as cut rather than broken |
| D-34 | V1 over a snapshot: the card of a process whose `VmSwap` says 2048 kB shows `own swap  2.0M`, and the card of a process with no `status` file shows the row with `n/a` rather than leaving it out |
| D-35 | V1 over two snapshots: a host whose `SwapFree` is below its `SwapTotal` draws a `SWAP` column between `MEM` and `DISK` carrying the `VmSwap` of the row, and a host whose swap device is untouched draws no such column. Invariant 15 of `scripts/frame-lint.py` holds the position of the column on every captured frame |
| D-36 | V1 over a snapshot holding a root process whose `status` carries no `VmSwap` line: its row shows a zero rather than `n/a`, and the `(self)` row of the level is a number rather than an unknown. A process whose `status` cannot be read at all still shows `n/a` on the card |
| D-37 | Unit tests over the palette: every theme is reachable by the name it ships under, no theme puts a band colour or its own text colour on the ground of its selected row, and the first theme is still the sixteen terminal names. A unit test over the renderer walks all eight and checks that each reaches both the frame line and the ground of the selected row. `--theme` takes a name and refuses one it does not know; `HOSTSCOPE_THEME` takes a name and ignores one it does not know |
| D-38 | A unit test over the timer: the first call is not due, a call before the span is not due, and a call after it is due once. On the host, `tmux pipe-pane` over 20 seconds counts the bytes and the full clears at the three second and the one second interval, and section 6a carries what it counted |
| D-39 | A unit test over the renderer draws the header twice, with the same host at two magnitudes of every figure, and demands that `MEM`, `SWAP` and `WAIT` land on the same cell both times, and fails outright where one of the three is missing. Invariant 16 of `scripts/frame-lint.py` holds the same across the frames of every captured run |
| D-40 | The `measurements` section reads `/proc/meminfo` and holds the host to 4 percent of one core where nothing is in swap and to 5 where the swap column is drawn, printing which of the two it applied. Confirmed on both rigs on 2026-08-29: 2.40 percent against the four on the rig that had swapped nothing, 3.66 against the five on the one that had |
| D-41 | A unit test over the command line holds the default on and the flag off. The `security` section traces the file syscalls twice: `/etc/passwd` is opened by the default run and by no run that passes `--no-etc-passwd`, and the section fails when the default run did not open it either, because then the comparison proves nothing |
| D-42 | The heuristics stand as unit tests, one per denominator, so a threshold that moves shows up as a named failure rather than as a screen that looks different. The map of `--dump-style` is checked twice: `tests/frame_invariants.rs` demands that a frame over a snapshot carries a map of the right shape at all, and invariant 17 of `scripts/frame-lint.py` reads its second opinion over every frame of a live run. The `/sys/class/net` read is inside the surface FR-10a names, and the `security` section holds the trace to it |
| D-43 | Unit tests over `src/model.rs`: the heuristics that fired come back named after the card row their figure stands on, with a reason carrying the figure of BOTH columns, the whole and the threshold; a figure that fires in one column only says which; a figure that did not move is written once; the swap reason says it is the subtree, not the process; a calm row returns none; and the worst of the reasons is the flag the row carries, which is what holds the reading and its sentence to one source. Unit tests over `src/render.rs`: the card of a marked row prints the block above its figures, and the card of a calm row prints no block at all |
| D-44 | Unit tests over `src/model.rs`: a parent whose figures are entirely its children's is marked as the way down while the child that carries them is marked as the source; a parent that keeps a reading of its own after its children are subtracted stays the source; a state the kernel reports is always the row's own; and a calm row carries no mark. A unit test over `src/render.rs` reads the glyph off the drawn frame: the arrow on the parent of the top level, and the solid mark on the child one level down. The mark is computed where the row is built, in `src/app.rs`, because the row keeps a shallow copy of the node without the children the remainder is derived from |
| D-45 | Unit tests over `src/collect/cgroup.rs`: the four counters are read beside `cgroup.procs`, a file that is not there leaves the counter unavailable rather than zero, and `memory.events.local` is read and not `memory.events` - the test carries the two figures the reference host gave when the difference was measured. A unit test over `src/collect/mod.rs`: a fact raised on an ancestor reaches a row of a control group below it, and a counter that did not grow between two ticks raises nothing. Unit tests over `src/model.rs`: each fact has its own name, sentence and severity, the sentence says the fact belongs to the control group, and a process row carries no reading of memory, of swap or of tasks. On the host, the `induced_ceilings` section raises all four on purpose and demands each reach a row; the `security` section holds the trace and no longer names `sys/kernel/pid_max` |
| D-46 | Unit tests over `src/model.rs` fix both steps on `some` and hold `full` out of the reading. A unit test over `src/render.rs` reads the colour off the drawn header: an idle machine carrying `io full` 0.16 is unmarked, and 41 percent of time waiting for the processor is an alarm. A second reads the window off the mode, and a third draws a snapshot with no `pressure` files and demands three unavailable figures with no load average in their place. On the host, the `induced_pressure` section raises real contention - four busy scopes per core - prints the three figures and demands the header carry them; invariant 16 of `scripts/frame-lint.py` holds the `WAIT` label in place and fails where it is gone. A unit test draws the header at 100 cells, which is the width the live check draws at, and demands that the three shares of time are there and that the network rates are the segment that gave way; the same test at 110 demands all three |
| Section 6 | Section 9 of this document |

## 14. Link to the requirements

Everything this document demanded of the requirements has been written
into them:

- FR-17 - the verification hooks of section 4, together with writing the
  collection time and the render time of a frame into the log: without
  that the 50 ms threshold cannot be checked.
- D-15 and then D-17 - what a frame may carry from data. D-15 escaped
  every non-ASCII character; D-17 replaced that with removing control
  characters and counting widths in cells. That is where the induced
  state "non-ASCII name" and the wording of invariant 2 come from.

No open questions about verification remain.
