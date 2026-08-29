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

- The tree is the process forest. Every row is a process and stands
  under the process that started it.
- Every row carries CPU in busy cores, memory, tasks, disk read and
  write, and network down and up - for the process and everything under
  it.
- `OWNER` names what runs the process: a container, a service, a login
  session. It reads Docker with either cgroup driver, containerd under
  Kubernetes, and podman.
- A row that leads into a container names it in parentheses, and one
  that leads into a pod of several names the pod.
- A chain of processes where each started only the next is drawn as one
  row named for the whole chain. A change of owner ends the chain.
- `>` marks a row with something under it. `Enter` goes deeper,
  `Backspace` comes back, `Esc` undoes the nearest narrowing.
- `i` opens the card of a row: the command line, the pid, the chain, the
  container image, and both measurement modes side by side.
- `c m d n` sort. The bar stands beside the sorted column and moves with
  it.
- `/` filters by name, command line and owner. The match is marked
  wherever it is drawn, and the path line says what was typed and how
  many rows are left.
- `v` lays the level out as a flat list of its ends, which finds the
  process eating the host without knowing which branch it sits on.
- `a` switches between the average since start and the last interval;
  `-` and `+` move the interval between a pause and 60 seconds, and
  space reaches the pause in one key.
- What cannot be read is marked unavailable, never replaced with a zero.
  Names are shown in their own script, and columns are measured in
  cells.

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
