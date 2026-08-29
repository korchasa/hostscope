# hostscope

An interactive viewer of the current host state: a tree of resource
usage with drill-down and container data filled in. One static binary,
no daemon, no port, read only.

```
HOST=your.host
cargo build --release --target x86_64-unknown-linux-musl && ssh "$HOST" 'mkdir -p /tmp/hostscope' && scp target/x86_64-unknown-linux-musl/release/hostscope "$HOST:/tmp/hostscope/" && ssh -t "$HOST" 'sudo /tmp/hostscope/hostscope'
```

Build, copy, run. The build cross-compiles from any machine with the
`x86_64-unknown-linux-musl` target - no musl toolchain and no Docker,
`.cargo/config.toml` links it with `rust-lld`. `ssh -t` matters: without
a terminal on the other end the application has nothing to draw on.
Without `sudo` it still runs, in a reduced form: what it cannot read it
marks unavailable rather than showing as a zero.

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
`unicode-width`, which `ratatui` already draws with. For the target
host, a static build that needs nothing installed there:

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The result is a 925 KB static binary.

## Options

`hostscope --help` prints them all. Two of them matter before the first
run: `--tick MS` sets the interval the application opens at, which `-`
and `+` then move, and `--log FILE` writes what each tick and each frame
cost. The rest drive it without a terminal - a captured snapshot instead
of the live host, a key program instead of a person, the model or the
frame printed instead of drawn - and those are what the tests and the
checks use.

## Checking it

```
make fast                   fmt, clippy and 87 tests, on your machine
make live HOST=your.host    the whole check on a Linux host
```

`make fast` takes seconds and is the step after every edit. `make live`
takes two minutes and needs a host: it ships the binary and the checks
in one trip, walks the interface, compares the model against an
independent oracle, lints every captured frame, raises induced states -
a known CPU quota, a spike against a steady load, a disk load, 200 extra
scopes, vanishing processes, long and non-ASCII names, no Docker socket,
no root - checks that no process environment is ever opened and no
external command is ever run, and cleans up after itself. Everything it
creates on the host is named with the `hs-` prefix.

## Documents

- Requirements: [docs/requirements.md](docs/requirements.md). What the
  application must do, what was measured and where, and every settled
  decision with the reason it was taken and what it rejected.
- How to verify: [docs/testing.md](docs/testing.md). The rigs, the frame
  invariants, the induced states, the figures of the last full run, and
  which check covers which requirement.
