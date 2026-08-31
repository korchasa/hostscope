---
date: "2026-08-31"
status: done
implements: [FR-3]
tags: [enrichment, card, docker]
related_tasks: []
---
# The card says what the daemon actually answered

## Goal

An engineer who opens the card of a process running in a container wants to
know which image it runs, what state the container is in, when it was created
and how many times it has restarted. That is what FR-3 promises. Today the card
answers with a sentence about the Docker socket being unreadable, on a host
where the socket is read successfully and the container's own name is on the
row.

## Overview

### Context

Found on 2026-08-31 while reading the Docker rig. The card of the `loki`
process printed two lines that contradict each other:

```
container       loki   id 95145fa2864d
image           unavailable - the docker socket is not readable
```

The name in the first line can only come from the daemon, so the socket was
read. `--dump-model json` on the same host says `"docker_available": true`,
both as an ordinary user and under `sudo`, and `/var/run/docker.sock` is
`srw-rw---- root docker` with the account in the `docker` group.

### Current State

`Detail.container` ([src/model.rs](../../../../src/model.rs)) is declared, read
in three places — `dump.rs`, `render.rs`, `app.rs` — and assigned in none. The
map the enricher fills is used exactly once, in `Collector::owner_of`
([src/collect/mod.rs](../../../../src/collect/mod.rs)), and only `info.name` is
taken from it. Everything else `docker.rs` already parses — image, state,
status, created, restarts, labels, ports — stays in the background thread.

`Collector::process_node` computes the container's full identifier on its way
to `short_id` and then throws it away. It receives no `Enrichment`, so it has
nothing to look the identifier up in.

The second half of the defect is the sentence itself. `owner_hint`
([src/render.rs](../../../../src/render.rs)) distinguishes the two cases — data
has not arrived yet against the socket is unreadable — by `docker_available`.
`owner_lines`, which draws the card, does not: its `None` arm blames the socket
whatever the state of the socket is.

Nothing catches it. `a_container_without_the_socket_degrades_to_its_short_identifier`
checks the name and the short identifier only, and the unit tests of the card
build a node by hand and put a `ContainerInfo` into it, so they exercise a path
the live run never takes.

This is not a regression: the field has never been assigned since the first
commit of the repository, which means the image, the state, the creation time
and the restart count of FR-3 have never reached a screen.

### Constraints

- No new dependency: the daemon is already spoken to by hand over the socket.
- The tests may not mock the enricher. A test drives the real binary, and where
  it needs a daemon it opens a unix socket of its own and answers the two
  requests `docker.rs` makes.
- FR-10 stands: nothing outside `/proc`, `/sys/fs/cgroup`, `/etc/passwd` and
  `/sys/class/net` may be opened, and the socket named on the command line is
  what the application already connects to.
- The wording of the card follows `owner_hint`, which already separates the two
  states. This fixes a disagreement between two places on one screen; it does
  not open a new rule of section 11.

## Definition of Done

- [x] FR-3: a process whose cgroup names a container carries the image and the
      state the daemon reported, and they reach the model dump.
  - Test: `tests/model_over_snapshot.rs::the_daemon_answer_reaches_the_row`
  - Evidence: `cargo test --test model_over_snapshot`
- [x] FR-3: the card of that row prints the image, the state and the restart
      count instead of a sentence about the socket.
  - Test: `tests/frame_invariants.rs::the_card_of_a_container_names_its_image`
  - Evidence: `cargo test --test frame_invariants`
- [x] FR-3: with the socket switched off (`--docker-socket none`) the card
      still says the socket is unreadable, and with a socket that answers
      without this container it says the data has not arrived, matching the
      hint line.
  - Test: `tests/frame_invariants.rs::the_card_separates_a_dead_socket_from_a_missing_answer`
  - Evidence: `cargo test --test frame_invariants`
- [x] The whole suite and the live check stay green.
  - Evidence: `make fast` (166 tests) and `make live HOST=<the Docker rig>`,
    which passed 50 checks with none failed and none skipped on 2026-08-31.
    The card of `loki` on that host now reads `grafana/loki:3.4.2   Up 3
    weeks   restarts 0`, with the creation time, the published port and the
    labels under it.

## Solution

1. **RED.** Write `the_daemon_answer_reaches_the_row` first: a fixture with a
   `/system.slice/docker-<64 hex>.scope` cgroup holding one process, a unix
   socket opened by the test that answers `/v1.41/containers/json?all=1` with
   one container and `/v1.41/containers/<id>/json` with a restart count, and
   `--dump-model json` over `--proc-root`/`--cgroup-root`/`--docker-socket`.
   Assert the dump carries `"image"` and `"state"`. Run it and watch it fail —
   today the dump has neither.
2. **GREEN.** Pass the `Enrichment` into `Collector::process_node` and fill
   `Detail.container` from it beside `short_id`, using the full identifier that
   is already computed there.
3. Add the two card tests and make `owner_lines` read `docker_available` the
   way `owner_hint` does, so the two lines of one screen agree.
4. **REFACTOR.** Keep the socket-answering helper in `tests/support`, next to
   the fixture, since two test binaries need it.
5. **CHECK.** `make fast`, then `make live HOST=<the Docker rig>` — the Docker
   rig is the only one with a daemon, and the card is what changed.
6. Record the check in section 13 of `docs/testing.md`: FR-3 gains the socket
   that answers, which is what the mapping does not have today.
