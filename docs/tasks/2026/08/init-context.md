---
date: 2026-08-29
status: done
tags: [init, context]
---
# Discovered context of the repository

## Goal

Give a session that opens this repository for the first time the state of the
work in one file, so it does not have to reconstruct it from the code and the
git history — which is a single commit and carries no record of how the project
got here.

## Overview

### Context

hostscope answers one question on a Linux host over SSH: who is eating the
resources right now. It is finished: every requirement of `docs/requirements.md`
is closed, every decision of its section 9 is settled, and both checks are
green. The work now is maintenance and publication rather than development.

The repository was rebuilt for publication in August 2026. The earlier history
described live machines by name, so it was dropped rather than edited, and the
project starts from one commit. Names of hosts and accounts are gone from the
files: machines are named by the role they played, and the real host of the live
check comes from `local.mk`, which is not in the repository.

### Current State

- Rust, 6115 lines in `src/`, 1820 in `tests/`. One binary, no daemon, no port,
  read only.
- `src/` holds `cli`, `collect/` (procs, cgroup, host), `enrich/` (Docker socket
  on a background thread), `sample`, `model`, `app`, `render`, `dump`, `util`,
  `logging`. The architecture section of `AGENTS.md` describes what each does.
- `docs/requirements.md` — FR-1 … FR-20, decisions D-1 … D-29, the measurements,
  and the mandatory screen rules in section 11.
- `docs/testing.md` — how each requirement is checked: the two rigs, the frame
  invariants, the induced states.
- `scripts/` — `live-check.sh` (one command for the whole live run),
  `host-check.sh` (the checks on the host), `oracle.py` (an independent reading
  to compare against), `frame-lint.py` (frame invariants), `model-query.py`.
- `Makefile` — `fast` after every edit, `live` once per batch of work.
- 87 tests pass. The last live run reported 37 checks passed, none failed, one
  skipped, in 113 seconds.

### Constraints

- The application only reads: no external process is ever run, no process
  environment is ever opened, nothing is written except the log named on the
  command line. `build.rs` fails the build on a violation of the first.
- No host names, accounts or real container identifiers in the repository.
- Dependencies stay at three: `ratatui`, `crossterm`, `unicode-width`.
- The screen rules of section 11 of the requirements are mandatory; changing one
  needs a new numbered decision.

## Definition of Done

- [x] The requirements are closed and the decisions settled
  - Evidence: section 10 of `docs/requirements.md`, section 9 for the decisions
- [x] The fast check passes
  - Test: the whole suite, 87 tests
  - Evidence: `make fast`
- [x] The live check passes on a host
  - Test: `scripts/host-check.sh`
  - Evidence: `make live HOST=<host>` — last run 37 passed, 0 failed, 1 skipped
- [x] Nothing in the repository names a real machine or account
  - Evidence: `git grep -niE '\.lan|4bet|playmecorp|stas[._]ops' | wc -l` prints 0

## Solution

Nothing to build here — this file records the state. What a next session should
know before touching anything:

1. Read `docs/requirements.md`. Every screen rule and every settled decision is
   there, and a change that contradicts one opens a new decision rather than
   editing the old.
2. Run `make fast` before and after your edits. It takes seconds.
3. Run `make live` once, when a batch of work is settled — it needs a host and
   about two minutes. Set the host in `local.mk` or pass `HOST=` on the command
   line.
4. The one check that still cannot run everywhere is the container with a
   120-character name: it needs a host with Docker, not the containerd rig.
5. Publication is not done: the repository has no remote. Before adding one,
   check that nothing new names a machine.
