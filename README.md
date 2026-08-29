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

![A level of a Kubernetes node in hostscope](docs/screenshot.png)

```
hostscope - an interactive viewer of the current host state

usage: hostscope [options]

  --tick MS               the interval to start at, in milliseconds
                          (default 3000; - and + move it while running)
  --cgroup-root DIR       read a captured snapshot instead of /sys/fs/cgroup
  --proc-root DIR         read a captured snapshot instead of /proc
  --docker-socket PATH    the docker socket, or 'none' to disable enrichment
  --dump-model json       print the tree model as numbers to stdout and exit
  --dump-frame N          render N frames as text to stdout and exit
  --keys "Right a Esc"    run a key program and stop
  --size WxH              frame size for --dump-frame (default 100x30)
  --log FILE              write the log to FILE; never to the terminal
  -h, --help              this text
  -V, --version           version

Dumps go to standard output: FR-10 forbids writing outside the settings file,
and a verification hook is no reason to make an exception.

The tree is the process forest of the host: every row is a process and stands
under the process that started it. What runs a process - a container, a
service, a login session - is read from its cgroup and shown on the row itself,
in the OWNER column, where the filter reaches it.

A chain of processes where each one started only the next, and all of them
belong to the same owner, is drawn as one row named for the whole chain: it
said nothing a level at a time and cost a keystroke a level.

A row whose work is inside a container says so in parentheses after its own
name: a runtime shim belongs to the runtime and its whole work is one level
down. Where the row leads into several containers of one pod, it names the pod
instead, by the first group of its identifier.

keys: up and down move, PageUp and PageDown move by a screenful, Enter goes
down and opens the card where there is nothing below, Backspace comes back up,
i opens the card of any row, / filters by name, by command line and by owner,
c m d n sort, v lays the level out as a flat list of its ends, a switches the
measurement window between the average since start and the last interval, space
freezes the screen, - and + move the refresh interval between a pause, 1, 2, 3,
5, 10, 30 and 60 seconds, q quits. The right arrow also descends, and in the
list view it puts a row among its neighbours. Escape undoes the narrowing
nearest at hand: the card, then the filter, then the level.

The bar beside a column belongs to the sorting: it is drawn next to the value
the rows are ordered by, so the longest bar is always the top row.

CPU is written in busy cores and in nothing else.
```
## Building

Rust 1.96, no dependencies beyond `ratatui`, `crossterm` and
`unicode-width`, which `ratatui` already draws with. For the target
host, a static build that needs nothing installed there:

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The result is a 925 KB static binary.

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
