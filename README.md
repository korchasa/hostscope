# hostscope

An interactive viewer of the current host state: a tree of resource
usage with drill-down and container data filled in. One static binary,
no daemon, no port, read only.

```
HOST=your.host
cargo build --release --target x86_64-unknown-linux-musl && ssh "$HOST" 'mkdir -p /tmp/hostscope' && scp target/x86_64-unknown-linux-musl/release/hostscope "$HOST:/tmp/hostscope/" && ssh -t "$HOST" 'sudo /tmp/hostscope/hostscope'
```

Build, copy, run. The binary goes into `/tmp/hostscope/`, the same
directory the checks below use, so the two never collide - copying it to
`/tmp/hostscope` instead fails on any host where those checks have
already run and left a directory of that name.

The build cross-compiles from any machine with the
`x86_64-unknown-linux-musl` target - no musl toolchain and no Docker,
`.cargo/config.toml` links it with `rust-lld`. `ssh -t` matters: without
a terminal on the other end the application has nothing to draw on.
Without `sudo` it still runs, in the reduced form of D-13.

## What it shows

What the host runs, one level at a time, with CPU in busy cores, memory,
disk and network in every row. The tree is the process forest: every row
is a process and stands under the process that started it, which is the
one hierarchy that answers "what started this" without the reader having
to know how the host arranges its cgroups.

What runs a process is a property of the row rather than a level of the
tree. The `OWNER` column names it - the container, the service, the
login session - read from the cgroup the process sits in, and it
recognises a container whether the runtime is Docker with either cgroup
driver, containerd under Kubernetes, or podman. The column is always
there and always the name; where nothing runs a row, it is blank. The
filter reaches it, so `grafana` finds every process of that container
wherever its runtime hung them in the forest.

A row marked `>` has children. `Enter` goes deeper, `Backspace` comes
back, and on a row with nothing under it `Enter` opens the card instead,
because there is nowhere to go. `PageUp` and `PageDown` move by a
screenful. `i` opens the card of any row - a process, or the `(self)`
remainder of one. `c m d n` sort, `/` filters by name, by command line
and by owner, `a` switches between the average since start, which it
opens in, and the last interval. `-` and `+` move the refresh interval
between a pause, 1, 2, 3, 5, 10, 30 and 60 seconds - it opens at three -
and the interval stands on the key line between the two keys; space
reaches the pause in one key. CPU is written in busy cores and in nothing
else.

The bar beside a column belongs to the sorting: it stands next to the
value the rows are ordered by and moves with it, so the longest bar is
always the top row.

A filter that is on says so on the path line, with what was typed and
how many rows it left, and the match is marked wherever it is drawn -
in the name, in the path in front of it, in the owner, and in the command
line under the table. `Esc` drops it. `Esc` undoes the narrowing nearest
at hand, in that order: the card, then the filter, then the level.

A chain of processes where each one started only the next, all with the
same owner, is one row named for the whole chain. On the test host a
quarter of the forest was such pass-through nodes, and
`supervisor/app/python3/npm exec chrome/sh/chrome-devtools/node`
was seven levels of one row each. The figures are the first link's,
which already cover the chain, and the card names every link with its
pid. A change of owner ends a chain - a shim stepping into a container
is a boundary worth seeing.

`v` lays the subtree of the level out as a flat list of its ends, which
is how the process eating the host is found without knowing which branch
it sits on. A row is a process with nothing under it, or the `(self)`
remainder of one that has children - together they are the level split
up, so the list adds up to the same total the tree does, and the work a
process does itself does not vanish from it. Each row carries the chain
of names it came from, the filter reaches all of them at once, and `→`
puts the row among its neighbours: the level it lives on, however deep
that is.

A row owned by a container carries the name, image and state from the
Docker socket, filled in by a background thread, so a slow socket never
holds up a frame. What cannot be read is marked unavailable - never
replaced with a zero. Names are shown in their own script, whatever it
is; only what would drive the terminal is stripped out, and columns are
measured in cells.

## Building

Rust 1.96, no dependencies beyond `ratatui`, `crossterm` and
`unicode-width`, which `ratatui` already draws with. For the
target host, a static build that needs nothing installed there:

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The result is a 925 KB static binary. `cargo test` runs 87 tests: the
model over captured `/proc` and `/sys/fs/cgroup` snapshots, the frame
invariants over rendered text, the three environment shapes - Docker
with the cgroupfs driver, a Kubernetes node, a plain systemd server -
and the unit tests of the formatting, sampling and parsing code.

## The steps of a check

There are two, and they are named in the `Makefile` so the set is not
assembled by hand every time:

```
make fast    fmt, clippy and the tests, on the Mac    seconds
make live    build, ship and check on the host        minutes
```

`make fast` is the step after every edit. `make live` is the step once
per batch of work, because it needs a host and two minutes of it.
`make live-quick` runs the sections that raise no load - the walk, the
oracle, the linter, the sizes - in under a minute, and `make live-bg`
runs the whole thing detached, so the wait is spent working rather than
watching.

## Verifying on a live host

Everything the verification procedure describes is in `scripts/`, and
`scripts/live-check.sh` puts it into one command: it cross builds,
ships the binary and the checks in a single trip, runs them, brings the
log back and prints the failures and what each section cost.

```
make live                          the whole run on the default host
make live HOST=your.host           somewhere else
make live SECTIONS='oracle security'   one section by name
make live-bg && make live-log      detached, collected later
```

The whole run takes under two minutes, and every section prints what it
cost, so a section that grows is visible in the log.

Most of a run used to be `sleep`. A scenario was walked through `tmux`,
and a capture taken before the application redrew returns the previous
frame, so every keystroke was followed by 1.3 seconds - a tick plus a
margin. The walks now run as key programs instead: `--keys` feeds one
key per frame and `--dump-frame` prints what was drawn, so a walk costs
its length in ticks and waits for nothing. `tmux` stays where a terminal
is the thing being checked - the sizes, the resize on the fly, and a
short pass that confirms a real terminal draws the frame the dump drew.
The pauses after an induced load went the same way: a scope is in the
tree and its counter is moving in under 20 ms, so the check waits for
that fact rather than for four seconds.

It walks the interface, compares the model against an
independent oracle, lints every captured frame, raises induced states -
a known CPU quota, a spike against a steady load, a disk load, 200 extra
scopes, vanishing processes, long and non-ASCII names, no Docker socket,
no root - checks that no process environment is ever opened and no
external command is ever run, measures the non-functional figures, and
cleans up after itself. Everything it creates is named with the `hs-`
prefix.

Last full run on the Kubernetes rig (2026-08-15): 37 checks passed, none
failed, one skipped. It took 113 seconds, and the frame linter went over 133
frames rather than the 29 the `tmux` walks used to produce - spelling a
filter one key per frame yields a frame per letter, and each is checked
against every invariant for free. A 50 percent quota showed as 0.489
cores, the search found its node among 616, the worst collection on that
full tree was 76 ms, the first frame arrived after 16.0 ms, and the
application spent 2.7 percent of one core and 1.0 MB on itself. `strace`
showed one `execve`, no file opened for writing and no
`/proc/<pid>/environ` opened at all.

The skipped one is the container with a 120 character name: that rig
runs containerd under microk8s and has no Docker to raise that state
with. What it gives instead is the degraded case of FR-3 for real - the
containers are there, the socket is not, and the name falls back to the
short identifier. The Docker side of FR-3 and the long container name
need the Docker rig, where the previous full run passed 30 checks with
none failed; `make live HOST=<that host>` runs them there.

## Verification hooks

The application can be driven without a terminal, which is what the
tests and the checks above use:

- `--cgroup-root DIR` and `--proc-root DIR` read a captured snapshot
  instead of the live host.
- `--docker-socket PATH|none` points at another socket or turns the
  enrichment off.
- `--dump-model json` prints the tree as numbers; two runs over one
  snapshot print the same bytes.
- `--dump-frame N` prints N frames as text, `--keys "Right a Escape"`
  feeds one key per frame, `--size WxH` sets the terminal size.
- `--tick MS` sets the interval the run opens at, which `-` and `+` then
  move; `--log FILE` writes what each tick and each frame cost.

## Documents

- Requirements: [docs/requirements.md](docs/requirements.md). Decisions
  are in section 9, what was measured on the host is in sections 8, 8a,
  8b and 6a, the state of the documents is in section 11.
- How to verify: [docs/testing.md](docs/testing.md). The two rigs, the
  feedback loop through `tmux`, the frame invariants and the induced
  states.
- The rules the screen must keep - one column set on every level, the
  detail of a row on a line under the table, the mark on a node with
  children, a tick of the bar for any non-zero value - are in section 11
  of the requirements. They used to live in a screen mockup, which drew
  the interface of before D-24 and was removed on 2026-08-21.

Every decision of section 9 is settled. D-26 is the last: the `OWNER`
column is always the name, with no switch. D-25 before it left one way
down, one CPU unit, and no levels that hold a single row. D-24 before it made
the tree the process forest and nothing else, with what runs a process a
word on its row instead of a level above it. D-16 before it accepted the
measured cost: 3.0 - 3.5 percent of one core on a host of about 370
processes, which is the host the budget of section 6 now names.
