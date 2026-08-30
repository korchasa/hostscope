---
date: "2026-08-30"
status: to do
implements: [FR-21]
tags: [screen, heuristics, cgroup, psi]
related_tasks: [highlight-risky-values]
---
# Readings that mean the same on every machine

## Goal

An engineer reads the screen on a machine they have never seen, and the marks
have to mean the same there as on the machine they learnt them on. Today three
of the row readings are shares of a setting somebody chose - the size of the
swap device, `pid_max`, the amount of RAM the box happens to have - so the same
process reads calm on one host and marked on the next. The reader cannot build
a habit out of that, and a mark nobody trusts is worse than no mark.

## Overview

### Context

FR-21 gave every figure a reading and every row a mark (D-42), and D-44 then
split the mark into the source of a reading and the way down to it. The
operator's next question was narrower: which of these readings survive a move
between machines with one core and machines with hundreds.

The comparison was made against the `node_exporter` alert set of
`samber/awesome-prometheus-alerts` (`dist/rules/host-and-hardware/node-exporter.yml`,
read on 2026-08-30). Two things came out of it. Every server-independent rule
there is normalised either against a property of the hardware or against time,
and never against a setting an administrator picked. And the set says nothing
at all about a process: it is a host-level set, and process-level thresholds
have no upstream to copy.

So the answer for a row had to come from somewhere else, and it comes from two
places the kernel already offers. The first is the cgroup a process sits in: it
records, as counters, that the process was held back by its own quota, that
something in it was killed for memory, that it hit its memory ceiling, that a
fork failed against its pid ceiling. Those are facts, not comparisons - there
is no threshold to be wrong about, and no denominator to depend on the machine.
The second is pressure stall information, which the kernel exports per cgroup
and for the machine: the share of time something was stalled waiting for CPU,
memory or io. A share of time is the same quantity on one core and on two
hundred.

Measured on the reference host on 2026-08-30 before any of this was written:

- The hierarchy is cgroup v2 (`cgroup2fs`). There are 92 cgroups against 558
  tasks and 210 rows, so a per-cgroup read costs less than a per-row read.
- PSI is present for the machine (`/proc/pressure/{cpu,memory,io}`) and in
  every cgroup (`cpu.pressure`, `memory.pressure`, `io.pressure`).
- `cpu.stat` carries `nr_throttled` and `throttled_usec`; `memory.events`
  carries `oom_kill` and `max`; `pids.current` and `pids.max` are there too.
- A memory limit is set on 3 cgroups out of 90, and no cgroup has a CPU quota:
  `cpu.max` reads `max 100000` even on `kubepods`. So on this host the four
  facts will be silent, which is what a fact should be on a healthy machine.
  They are not measurable here by waiting; they have to be induced.

The operator chose, on 2026-08-30: the four cgroup facts first (1A); the three
server-dependent row rules removed in the same step rather than left to overlap
with their replacements (2A); and the load average in the header replaced by
the three pressure figures (3A).

### Current State

- `src/collect/cgroup.rs` walks the hierarchy and opens `cgroup.procs` and
  nothing else. The module says so in its own header, and that is what makes a
  captured snapshot a valid substitute for the live tree under `--cgroup-root`
  (FR-17). Adding reads changes that sentence and the snapshots under `tests/`.
- `src/model.rs` holds `Readings::read`, where the host row takes one set of
  thresholds and every other row takes another (D-42). The row branch reads
  memory as a share of `MemTotal` (25 and 10 percent), swap as a share of
  `swap_total` (10 percent), and tasks as a share of `pid_max` (10 percent).
- `Limits.pid_max` is read by `src/collect/host.rs::pid_max` from
  `sys/kernel/pid_max` and is used by that one rule and by nothing else.
- `src/render.rs` draws the header as segments that give way one at a time on a
  narrow terminal. The `LOAD` segment shows `host.load[0..3]`, with the first
  figure coloured by `HostReadings::of(...).load` and the other two left plain.
- `src/sample.rs` turns counters into rates and holds the state between ticks,
  keyed by a string. The four facts are counters and need it.

### Constraints

- Read only. Nothing outside `/proc` and `/sys/fs/cgroup` is opened, and both
  new families are inside what FR-10a already allows, so the security section
  of the live check must keep passing unchanged (FR-10, D-13).
- What cannot be read is marked unavailable and never replaced with a zero
  (D-13). A kernel built without PSI, and a cgroup file that is not there, both
  take that path.
- The cost of a tick is bounded by D-16: 3.0 - 3.5 percent of one core on a
  host of about 370 processes. Six extra reads per cgroup is roughly 550 extra
  opens per tick, and the figure has to be measured before and after, not
  estimated.
- Section 11 of the requirements fixes the header. Replacing the load average
  there needs a new decision, not an edit to an old one.
- A reading taken from a cgroup belongs to the cgroup, not to the single
  process on the row. The card must say so, the way D-43 already makes the
  memory reason say "with children" and the swap reason say "over the subtree".

## Definition of Done

- [ ] FR-21: A process whose cgroup was held back by its quota carries a mark,
      and its card names the fact and says the quota is the cgroup's.
  - Test: `src/collect/cgroup.rs::the_facts_of_a_cgroup_are_read_beside_its_processes`,
    `src/model.rs::a_throttled_cgroup_marks_every_row_it_owns`
  - Evidence: `cargo test --bin hostscope throttle`
- [ ] FR-21: The three facts about a ceiling - memory killed, memory ceiling
      reached, pid ceiling reached - each raise their own reading, and each
      fires only on the tick where the counter grew.
  - Test: `src/model.rs::a_counter_that_did_not_grow_raises_nothing`,
    `src/model.rs::each_ceiling_has_its_own_name_and_sentence`
  - Evidence: `cargo test --bin hostscope ceiling`
- [ ] FR-21: A process row carries no reading of memory, swap or tasks. The
      host row keeps all three.
  - Test: `src/model.rs::a_process_row_is_read_only_by_what_does_not_depend_on_the_machine`
  - Evidence: `cargo test --bin hostscope row_is_read_only_by`
- [ ] FR-21: `sys/kernel/pid_max` is no longer opened by any run.
  - Test: `tests/model_over_snapshot.rs` over a snapshot without the file
  - Evidence: `make live SECTIONS="prepare security cleanup"` - the traced
    file set no longer names it
- [ ] FR-1a: The header shows the three pressure figures in place of the load
      average, in the window the current mode names. On a kernel that does not
      export pressure all three read unavailable and the load average does not
      return in their place (D-13).
  - Test: `src/render.rs::the_header_reads_the_machine_as_three_shares_of_time`,
    `tests/frame_invariants.rs` over a snapshot with no `pressure` files
  - Evidence: `cargo test --test frame_invariants`
- [ ] FR-21: The pressure thresholds are measured, not chosen. The figure each
      induced state produces is written into the requirements with the date and
      the rig.
  - Test: `scripts/frame-lint.py --alarm-min N` over the induced states
  - Evidence: `make live SECTIONS="induced_load induced_alarm measurements"`
- [ ] Non-functional: the cost of a tick after the change is measured and
      stands inside D-16, or D-16 is reopened with the new figure.
  - Evidence: `make live SECTIONS=measurements`, compared against the figure in
    section 9 of `docs/testing.md`
- [ ] The requirements carry D-45 (the row reads facts, not shares) and D-46
      (the header reads pressure), FR-21 and section 11 are amended, and
      section 13 of `docs/testing.md` names the check for each.
  - Evidence: `make fast` - `tests/documents.rs` holds the help text and the
    documents to the binary

## Solution

Work in the order below; each step ends green under `make fast`, and `make
live` runs once when the batch is settled.

1. **Read the facts.** Extend `src/collect/cgroup.rs::read_one` to open
   `cpu.stat`, `memory.events` and `pids.events` beside `cgroup.procs`, parsing
   only `nr_throttled`, `oom_kill`, `max` and the pid `max`. A file that is not
   there leaves the counter unavailable rather than zero (D-13). Amend the
   module header, which currently promises that nothing but `cgroup.procs` is
   opened. Extend the captured snapshots under `tests/` with the new files so a
   snapshot run can raise the facts.

2. **Turn the counters into facts.** The four are counters, so a fact is a
   growth between ticks, taken through `src/sample.rs` keyed by the cgroup
   path. Carry them on the owner in `src/model.rs`, and let every row owned by
   that cgroup read them. Proposed severities, to be written into D-45 and
   visible for objection: held back by the quota - unusual; memory ceiling
   reached - unusual; killed for memory - alarm; pid ceiling reached - alarm,
   because a fork that failed is a process that did not start.

3. **Give each fact its sentence.** Add them to `Readings::findings` with names
   that match a card row, and with a scope string saying the fact belongs to
   the cgroup and not to the process - the discipline D-43 already fixed. They
   have no figure and no threshold, so they take the `fact` path that `zombie`
   and `stuck in kernel` already use, and they sort ahead of the figures in
   `ORDER`.

4. **Remove the three shares.** Delete the memory, swap and tasks rules from
   the non-host branch of `Readings::read`, delete `Limits.pid_max` and the
   `sys/kernel/pid_max` read in `src/collect/host.rs`, and narrow the FR-10a
   allow-list accordingly. The host branch keeps all three.

5. **Read pressure.** Add `/proc/pressure/{cpu,memory,io}` to
   `src/collect/host.rs`, parsing `some` and `full` at avg10 and avg60. Absent
   files leave the figures unavailable.

6. **Draw pressure in the header.** Replace the `LOAD` segment in
   `src/render.rs` with a `PSI` segment carrying the three resources at one
   window: avg10 in the instant mode, avg60 in the average mode, which is the
   mapping FR-13 already implies. Colour from the reading. `full` above zero is
   the kernel's own definition of thrashing and needs no threshold of ours; the
   step on `some` is measured in step 8. Where the files are absent all three
   figures read unavailable and nothing takes their place.

7. **Raise the facts on purpose.** Add induced states to
   `scripts/host-check.sh`, next to `induced_alarm`, each with a known answer:
   a scope with a CPU quota small enough to be throttled, a scope with a memory
   limit small enough to be killed, and a scope with a low `pids.max`. Without
   these the four facts cannot be checked at all - the reference host sets no
   limits, so the rules would count zero out of zero and read exactly like a
   pass, which is the failure D-42 already recorded once.

8. **Measure and write it down.** Take the pressure figures under the induced
   load, choose the step on `some` from what was measured, and record the
   figure, the date and the rig in the requirements. Measure the cost of a tick
   against D-16. Write D-45 and D-46, amend FR-21 and section 11, and add the
   rows to section 13 of `docs/testing.md`.

## What the header does without PSI

Settled by the operator on 2026-08-30. On a kernel that does not export
pressure the header shows three unavailable figures and the state of the
machine is simply not on the screen. The load average does not come back as a
fallback: it is a different quantity, and putting it in the same place under
the same colour would say the screen is reading pressure when it is not. This
is D-13 applied to the header - what cannot be read is marked unavailable and
never replaced by something else.
