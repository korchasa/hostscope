---
date: "2026-08-30"
status: done
implements: [FR-21]
tags: [screen, colour, readability]
related_tasks: []
---
# The screen says which figures deserve a second look

## Goal

An engineer opens an ssh session to answer "who is eating the resources right
now" within a minute. Today every figure on the table is drawn in one colour,
so the eye has to read all of them to find the one that matters. Colour that
separates a calm figure from one worth a second look turns reading into
noticing, and does it without adding a column, a key or a mode.

## Overview

### Context

The request is: the application should help by drawing dangerous and unusual
values in a different colour, and mark not only the values but whole rows where
a problem was found.

The tool already colours by value in exactly one place. `bar_style` in
`src/render.rs` gives the bar beside the sorted column three colours: red above
one busy core, amber above a tenth of a core, calm below. For the other columns
it has no fixed threshold, so it reads the share of the largest row of the
level: red above two thirds, amber above a third. D-20 fixed that colour as a
reading rather than decoration, which is why no palette may put a band colour
on the ground of the selected row.

So the request does not introduce colour-as-meaning to a screen that had none.
It asks for the reading that already exists on the bar to reach the figures
themselves and, beyond that, to reach whole rows.

Section 2 of the requirements puts "alerts and thresholds" out of scope, on the
grounds that a monitoring stack does that. Read against `bar_style`, that line
excludes the monitoring function - alert rules, notification, history - and not
the colouring of a reading, which the same document mandates in section 11.
That reading is stated here so it can be corrected before any code is written.

### Current State

Four things already put colour on a row, and a fifth has to compose with all of
them rather than override them:

- The bar beside the sorted column, coloured by `bar_style` (D-20, D-27).
- The ground of the selected row, `sel_bg` (D-37).
- The `(self)` row, drawn pale.
- The filter match, drawn with `mark_bg` and `mark_fg` by `marked()`.

Two facts bound what any solution can look like:

- `--dump-frame` renders the frame as plain text. `to_text` in `src/render.rs`
  drops every span style, so the frame linter and every frame test are blind to
  colour. Today the only check that can see a colour is a `#[cfg(test)]` unit
  test inside `src/render.rs`, which is how D-20 and D-39 are verified.
- D-37 requires all eight palettes to define every colour role. Reusing
  `signal`, `accent` and `calm` costs nothing; a new role costs eight palettes
  and two invariant tests.

### Constraints

- Section 11 of the requirements is mandatory. A rule about colouring figures
  or rows is a new rule there and a new numbered decision in section 9, not an
  edit to an existing one.
- Numbers are measured, never estimated. Every threshold this task fixes has to
  say where it came from, and the tick cost has to be re-measured against the
  budget of section 6, whose figures were taken before this feature existed
  (D-16, D-40).
- The application only reads. Nothing here may add a file to the read surface
  declared by FR-10a.
- A colour that fires on half the rows is not a reading, it is a background.
  The screen has to stay legible on a busy host, which is the case this feature
  is for. Two rules were dropped during the critique for failing exactly this:
  colouring an unreadable value, and colouring any non-zero swap.
- Section 2 of the requirements puts "alerts and thresholds" out of scope. How
  that line is read is the human's ruling and the first item of the Definition
  of Done, not a conclusion this task may reach on its own.

### Affected Surface

The report of the independent surface pass, verbatim:

```
Now I have full picture of the surface. Let me produce the final report.

## Surface

- `docs/requirements.md` section 2 "Scope" (line 48-52) — direct textual contradiction: "Out of scope: … Alerts and thresholds. A monitoring stack does that." The request ("подсвечивать опасные и необычные значения") asks the tool to judge values as dangerous/unusual, which is exactly a threshold/alert judgement this line excludes. — evidence: `docs/requirements.md:48-52`.
- `docs/requirements.md` section 11 "Mandatory rules of the screen" (line 1712-1791) — this is the place any new highlighting rule (value colour, row colour) would have to be added as a new mandatory rule; it currently has no rule about colouring by value or by "problem" rows, only the existing bar-colour and palette rules. — evidence: `docs/requirements.md:1727-1734` (palette/bar rules), no threshold-colour rule present.
- `docs/requirements.md` section 9 "Open decisions" (D-1…D-40, line 725-1791) — a new decision would be needed here (thresholds are a "what counts as above-class" call per project rules) to define what "dangerous" and "unusual" mean per column (CPU, memory, swap, disk, net) and per row ("problem" row). — evidence: existing precedent decisions D-20, D-27, D-37 govern colour semantics already.
- `src/render.rs` `bar_style()` (lines 665-682) — the ONLY existing "danger" colouring in the codebase: thresholds red/amber/teal for the bar beside the sorted column (CPU cores >1.0/>0.1; other metrics frac>0.66/>0.33). Parallel implementation the request would need to extend: the same colouring is not applied to the numeric value cells (`CORES`, `MEM`, `SWAP`, `DISK`, `NET`) themselves, only the bar strip. — evidence: `src/render.rs:660-682`.
- `src/render.rs` `table_lines()` (lines 485-593) — where each value span (`cores_opt`, `mem_str`, `pair_rate_opt`, tasks) is built with a flat `base`/`plain` style; this is where per-value colouring would attach, and where a "row has a problem" style would need to override `base` for the whole row (name, owner, tasks, cpu, mem, swap, disk, net spans use the same `base`). — evidence: `src/render.rs:511-528` (base/pale/bar_base selection), `:548-584` (per-column value spans).
- `src/render.rs` `marked()` (lines 603-649) — the existing mechanism for colouring parts of a row (filter match highlight, using `theme.mark_bg`/`mark_fg`). A "row with a found problem" highlight is a parallel concern to filter-match highlighting on the same spans (`lead`, `name`, `row_owner`, and the hint line) and the two would need to compose without one hiding the other.
- `src/render.rs` `card_lines()` and `fig()` closure (lines 1081-1461) — the process/host/(self) card shows the same metrics (cpu, memory RSS/virtual/PSS, swap, disk, net) in plain style; if a value is "dangerous," the card view of that same value is a second place needing the same colour, or the two would disagree (card shows red CPU but table row not, or vice versa).
- `src/theme.rs` `Theme` struct and `THEMES` array (lines 19-196) — colour roles are named per palette (`signal`/`accent`/`calm` already used for danger/attention/calm). Any new "danger" hue must reuse `signal`/`accent` (already used for red/amber) rather than invent a new field, because D-37 requires every one of the 8 palettes to define every colour role, and a new field means updating all 8 palettes plus the two invariant tests below.
- `src/theme.rs` tests `no_theme_hides_its_reading_in_the_ground_of_the_selected_row` (line 294) and `the_frame_is_drawn_in_the_theme_that_is_current` (render.rs:1537) — these enumerate the roles that must not collide with `sel_bg`; a new "danger" role (if added) must be added to this enumeration in both places or an unreadable combination on the selected row would slip through untested.
- `src/model.rs` `Metrics` (lines 79-183), especially `cpu_or_zero()` (158), `any_nonzero()` (142), `net_total()`/`disk_total()` (150/154) — candidate site for a "how unusual is this metric" judgement (e.g., "is this row a problem") if the decision is to compute it once in the model rather than per-render in `render.rs`; a decision (open per project rules) about where thresholds are computed (model vs. render) affects this file.
- `src/app.rs` `Row`/`App` (lines 107-176) — `rows()` (325) builds the per-tick row list; if "problem rows" need to be found/counted/jumped-to (e.g., a key to jump to the next flagged row, analogous to the filter), `App` is the state owner, parallel to `filter`/`sort`/`cursor` state already there.
- `docs/testing.md` section 13 "Mapping to the requirements" (line 622-661) — every existing colour-carrying decision (D-20, D-27, D-37, D-39) has a stated verification method; a new FR/decision for danger-highlighting needs a new row here per the Traceability rule in `CLAUDE.md` ("every FR names the way it is verified").
- `docs/testing.md` section 7 "The frame linter: invariants" (`scripts/frame-lint.py`, line 365) and `tests/frame_invariants.rs` — both operate on `--dump-frame` **plain text**, which currently carries NO colour/style information (`dump.rs` and `render::to_text()` strip styles to bare characters, `src/render.rs:218-228`). A colour-based feature is invisible to this whole verification pipeline as it exists today; either a new dump hook that also serializes styles is needed, or verification must live only in `render.rs`'s own `#[cfg(test)]` unit tests (as `the_frame_is_drawn_in_the_theme_that_is_current` and `the_selected_row_keeps_the_colour_of_its_bar` already do) — evidence: `src/render.rs:218-228` (`to_text` drops all `Span` styles), `src/dump.rs` (no style serialization).
- `src/cli.rs` (`--dump-frame`, `--keys`, `--size`, verification hooks, lines ~27-30) — the CLI surface documented as "the way every test and every live section goes through" the binary; if colour verification needs a hook (e.g., dumping styles/attributes, not just text), this file and its `--help` text change, cascading to `README.md` (kept identical by `tests/documents.rs` via `make readme`) and `scripts/readme-help.py`.
- `README.md` "What it shows" / help text block — must stay byte-identical to `hostscope --help` (enforced by `tests/documents.rs`); any new CLI flag or any new documented colour rule change here follows from a cli.rs/render.rs change.
- `scripts/oracle.py`, `scripts/pidstat-check.py`, `scripts/netlink-check.py` — these verify VALUES (the arithmetic), not presentation; not directly touched, but flagged because the request's "necykual value" concept might imply the oracle's role (measuring the "right" number) could be asked to also state a threshold — not currently in scope for these scripts.
- `docs/tasks/` directory — the accepted workflow (`write-gods-tasks` skill) requires any new task/decision to be filed here once scoped; no existing task file found for colour thresholds (`grep` for theme/color/bar found only `init-context.md`, unrelated).
- Non-functional budget, D-16/D-40 (`docs/requirements.md` section 9, `docs/testing.md` section 9) — the accepted CPU cost budget (3.0–4/5 percent of one core) is measured against the current per-tick work; adding a per-row/per-value threshold computation (however cheap) is new per-tick work that the existing budget test would need to re-measure against, since D-40's numbers were taken before this feature existed.

## Queries used
- Directory listing: top-level project tree, `src/`, `src/collect/`, `tests/`, `scripts/`, `docs/`.
- Read `src/theme.rs`, `src/render.rs` in full.
- `grep -n "^## \|^### "` over `docs/requirements.md` and `docs/testing.md` to map section numbers (the numbers in `CLAUDE.md` do not match; verified against the actual headings).
- Read `docs/requirements.md` lines 1686-1791 (State of the documents / Mandatory rules of the screen).
- Read `docs/requirements.md` lines 41-57 (Scope: in/out).
- `grep -i "threshold|danger|alert|unusual|problem|highlight|warn"` over `docs/requirements.md`, `docs/testing.md`, `README.md`.
- `grep "pub struct|pub enum|pub fn"` over `src/model.rs`, `src/app.rs`.
- `grep "color|Color|style|Style|theme"` over `scripts/frame-lint.py`, `src/dump.rs`, `README.md`, `src/cli.rs`.
- `grep "fn test|^fn "` over `tests/frame_invariants.rs`.
- Read `docs/testing.md` lines 622-663 (Mapping to the requirements table).
- Read `README.md` lines 1-50.
- Search `docs/tasks/` for existing theme/color/bar-related task files.

## Not examined (budget)
- `src/collect/procs.rs`, `src/collect/cgroup.rs`, `src/collect/host.rs`, `src/sample.rs` — not opened in full; flagged only by inference (they produce the `Metrics` the colouring would judge) rather than confirmed to contain any existing threshold-like logic.
- `src/enrich/docker.rs`, `src/enrich/json.rs` — not examined; unlikely to be affected (they supply container identity/name, not numeric metrics) but not verified by reading.
- `src/util.rs`, `src/logging.rs` — not read; `util.rs` holds `bar()`, `cores_opt`, `mem_str`, etc. referenced by `render.rs`, worth checking for where a value-string is formatted, in case colour needs to attach at formatting time rather than render time.
- `tests/environment_shapes.rs`, `tests/model_over_snapshot.rs`, `tests/documents.rs`, `tests/support/` — not opened; may contain fixtures relevant to a new induced "problem" state.
- Full text of `docs/requirements.md` sections 5 (Functional requirements, FR-1…FR-20) and 6 (Non-functional) — only grepped, not read end to end; a full read might surface an existing FR touching visual emphasis that grep missed.
- `docs/testing.md` sections 2-6 (rigs, feedback loop, verification layers) — not read; may describe a mechanism for style/colour verification I am not aware of.

## Could not rule out
- Whether a "problem row" concept overlaps with the existing `(self)` dim-row styling (`Kind::Own` branch in `table_lines`, `src/render.rs:524-526`) or the selected-row styling — these are the only two non-default row styles today, and a third ("problem row") competing for the same `base` variable needs explicit precedence rules I have not seen settled anywhere.
- Whether "necычные" (unusual) values implies a moving/statistical baseline (e.g., "unusual for this host" vs. a fixed threshold) — nothing in the codebase computes a historical baseline per metric beyond the CPU sparkline history (`host.history` in `render.rs:310`) and the average mode (`Mode::Average`); I cannot rule out that this request wants outlier detection rather than fixed thresholds, which would be a materially larger surface (new state, new sampling) not enumerated above.
```

- `src/render.rs` `bar_style` and `table_lines` - covered-by the Solution of the
  selected variant; this is where a value span takes its colour.
- `src/render.rs` `marked` and the selected-row / `(self)` styles - covered-by
  the precedence rule the selected variant fixes.
- `src/render.rs` `card_lines` - covered-by the Solution; the card shows the
  same figures and must not disagree with the table.
- `src/theme.rs` roles and the two invariant tests - covered-by the Solution;
  the reading reuses `signal`, `accent` and `calm`.
- `src/model.rs` `Metrics` - covered-by the Solution where the reading is
  computed once per row rather than per draw.
- `docs/requirements.md` sections 9 and 11 - covered-by the DoD item that adds
  FR-21, the new mandatory rule and the numbered decision.
- `docs/testing.md` section 13 - covered-by the DoD item that names how FR-21 is
  verified.
- `--dump-frame` and the frame linter - covered-by step 6 of the Solution: the
  new `--dump-style` hook gives the linter a role map to read.
- `src/cli.rs`, `README.md`, `scripts/readme-help.py` - covered-by step 6; the
  hook is a new flag, so the help text moves and `make readme` copies it.
- `/sys/class/net/<if>/speed` and FR-10a - covered-by step 2 and by the DoD item
  that amends the declared read surface; the security section's allow-list
  widens with it.
- `src/app.rs` row state and a key to jump between flagged rows - deferred -
  human choice; the request asks for marking, not for navigation.
- `scripts/oracle.py`, `scripts/pidstat-check.py`, `scripts/netlink-check.py` -
  not affected: they compare values against other readers and never look at
  presentation.
- Section 2 of the requirements - covered-by the DoD item that records how the
  "alerts and thresholds" line is read against the colouring `bar_style`
  already does.
- The budget of section 6 - covered-by the DoD item that re-measures the tick.

## Definition of Done

The first item is a precondition, not work: nothing below it starts until the
human has ruled on it.

- [x] FR-21: the human rules on how section 2 of the requirements is read. That
      section puts "alerts and thresholds" out of scope; this task reads the
      line as excluding the monitoring function - alert rules, notification,
      history - and not the colouring of a reading, which section 11 already
      mandates for the bar. The reading was put to the human in chat on
      2026-08-30 and is recorded here so the ruling precedes the code rather
      than arriving inside it.
  - Test: manual - korchasa
  - Evidence: the ruling is written into D-42 before the first edit to `src/`
- [x] FR-21: a figure the tool reads as alarming is drawn in a different colour
      from a calm one, in every palette.
  - Test: `src/render.rs::a_figure_past_its_threshold_is_drawn_apart_from_a_calm_one`
  - Evidence: `cargo test --bin hostscope a_figure_past_its_threshold`
- [x] FR-21: a figure the tool reads as unusual is drawn apart from both the
      calm and the alarming one, by the definition the selected variant fixes.
  - Test: `src/render.rs::an_unusual_figure_is_drawn_apart_from_calm_and_alarm`
  - Evidence: `cargo test --bin hostscope an_unusual_figure`
- [x] FR-21: a row carrying a problem is marked as a whole, and the mark
      survives the selected row, the `(self)` row and a filter match, in that
      settled precedence.
  - Test: `src/render.rs::a_flagged_row_keeps_its_mark_under_selection_and_filter`
  - Evidence: `cargo test --bin hostscope a_flagged_row_keeps_its_mark`
- [x] FR-21: in every palette the three readings are told apart from each other,
      not only from the ground of the selected row. The existing test already
      holds `calm`, `accent` and `signal` against `sel_bg` and passes today, so
      it proves nothing about this task; what is unproven is that amber and red
      are distinguishable on a dim palette.
  - Test: `src/theme.rs::the_three_readings_are_told_apart_in_every_palette`
  - Evidence: `cargo test --bin hostscope the_three_readings_are_told_apart`
- [x] FR-21: the bar keeps the colour and the rule it has today, on every
      sorting. The figures gain a reading; the bar does not change (D-20, D-27).
  - Test: `src/render.rs::the_bar_keeps_reading_the_level_and_not_the_machine`
  - Evidence: `cargo test --bin hostscope the_bar_keeps_reading_the_level`
- [x] FR-21: add the FR-21 section to `docs/requirements.md` with its
      `Acceptance:` filled, the new mandatory rule in section 11, and the
      numbered decision in section 9 that fixes every threshold with the
      measurement it came from and states how the "alerts and thresholds" line
      of section 2 is read.
  - Test: manual - korchasa
  - Evidence: `grep -n 'FR-21' docs/requirements.md docs/testing.md`
- [x] FR-21: `docs/testing.md` section 13 names how FR-21 is verified.
  - Test: manual - korchasa
  - Evidence: `grep -n '| FR-21' docs/testing.md`
- [x] FR-21: the network figures read against the speed of the link, and where
      no interface reports a speed the reading is unavailable rather than
      guessed.
  - Test: `src/collect/host.rs::a_link_that_reports_no_rate_is_left_out_rather_than_guessed`, `src/model.rs::a_link_that_reports_no_speed_leaves_the_network_reading_out`
  - Evidence: `cargo test --bin hostscope a_link_that_reports_no_speed`
- [x] FR-10a: `/sys/class/net/<if>/speed` is named in the declared read surface
      and the security section allows it, so the trace still fails on anything
      else.
  - Test: `scripts/host-check.sh::security`
  - Evidence: `make live SECTIONS="prepare security cleanup"`
- [x] FR-21: `--dump-style N` prints, beside each frame, a role map of the same
      width and height, one character per cell.
  - Test: `tests/frame_invariants.rs::the_style_map_covers_the_frame_cell_for_cell`
  - Evidence: `cargo test --test frame_invariants the_style_map_covers_the_frame`
- [x] FR-21: the frame linter reports the alarm cells of every frame, and holds
      them under a ceiling only in the induced states the check raises itself,
      where the expected answer is known. On an arbitrary host it reports and
      never fails: a busy machine is the case the tool exists for, and a ceiling
      fitted to two rigs cannot tell a loud palette from a host in trouble.
  - Test: `scripts/frame-lint.py::invariant 18`, `scripts/host-check.sh::induced_alarm`
  - Evidence: `make live SECTIONS="prepare induced_alarm linter cleanup"`
- [x] FR-21: the tick still costs what section 6 allows, measured after the
      reading is computed.
  - Test: `scripts/host-check.sh::measurements`
  - Evidence: `make live SECTIONS="prepare measurements cleanup"`

## Solution

Chosen: the reading is computed once per row in the model, every figure takes
its own colour, a flagged row carries a glyph, and the colours get a dump
channel so the frame linter can count them. The heuristics are the absolute set
including the network against the speed of the link, which widens the declared
read surface.

### The criterion the heuristics answer to

A figure is drawn apart from calm only when the tool can name the whole it is a
share of, and that whole is a property of this machine rather than of the rows
on screen; or when the kernel itself reports a state, in which case there is no
threshold. A heuristic that cannot name a measured denominator is not admitted -
which is why disk read and write stay calm always: nothing readable says what
the device can do.

### Step 1 - the reading in the model

`src/model.rs`: `enum Reading { Calm, Unusual, Alarm }` and a `Readings` struct
holding one per metric plus the row's own flag. A `Limits` struct carries the
denominators: cores, `mem_total`, `swap_total`, `pid_max`, the summed link
speed. `Limits` is built once per tick in `src/collect/mod.rs` from the host
summary and the two files read at start.

Computed in the model rather than in the renderer so the table and the card
read one value and cannot disagree, and so `--dump-model json` carries it.

Facts, no threshold:

- process state `Z` - the row is flagged `Alarm`;
- process state `D` - the row is flagged `Unusual`.

Rejected during the critique: "a metric the row could not read is `Unusual`".
FR-11 leaves the network counters unavailable for every process in the host
namespace, so on an ordinary machine that rule would turn the whole `NET`
column amber on every row. Worse, it recolours what D-13 settled: an
unavailable value means there is no reading, and `n/a` already says so. It
stays as it is.

Shares of a measured whole:

- CPU: > 1.0 core `Alarm`, > 0.1 `Unusual` - strictly greater, which is what
  `bar_style` compares today; a `>=` here would disagree with the bar at the
  boundary;
- memory of the row against `mem_total`: >= 25 percent `Alarm`, >= 10
  `Unusual`;
- host load[0] against `cores`: >= 2.0 `Alarm`, >= 1.0 `Unusual`;
- host memory used: >= 90 percent `Alarm`, >= 75 `Unusual`;
- host swap used against `swap_total`: >= 50 percent `Alarm`, >= 10
  `Unusual` - not "any use", which on a host that has swapped fires on most
  long-lived processes and says nothing;
- swap of the row against `swap_total`: >= 10 percent `Unusual`;
- tasks of the subtree against `pid_max`: >= 10 percent `Unusual`;
- network of the HOST ROW against the summed speed of the physical links:
  >= 80 percent `Alarm`, >= 50 `Unusual`. Rows below the host get no network
  reading at all - see step 2 for why.

Every percentage above is a convention taken from operational practice, not a
measurement, and the project does not accept an unmeasured figure in a numbered
decision. Step 7 measures how often each of them fires, on both rigs and under
the induced states, and D-42 carries every threshold with that figure beside
it. A threshold that ships without its measured firing rate is the defect this
paragraph exists to prevent - including one that produced a quiet screen and
therefore looked settled.

### Step 2 - the two new denominators

`src/collect/host.rs`: read `/proc/sys/kernel/pid_max` once at start, and sum
`/sys/class/net/<if>/speed` over the PHYSICAL interfaces only - those carrying a
`device` symlink under `/sys/class/net/<if>/`.

Not the set `parse_net_dev` sums. That one skips `lo` and takes everything
else, which on the Docker rig means `docker0` and one `veth` per container:
the denominator would then grow with the number of containers on screen, and
the criterion of this task forbids exactly that.

And the reading applies to the host row only. A container's `rx`/`tx` are read
inside its own namespace, so comparing them against a host-wide sum of link
speeds compares two different things. A per-container denominator would be the
speed of its veth, which is nominal rather than real; there is no honest
reading there, so there is none.

Error handling, and no invented values: the speed file answers `-1` for a link
that is down and fails outright on an interface with no fixed rate, so only a
positive reading is summed. A sum of zero means the reading is unavailable and
the network figures stay `Calm` - never a guessed denominator. The same for
`pid_max`: unreadable means the tasks heuristic does not fire.

`/sys/class/net` is outside `/proc` and `/sys/fs/cgroup`, so FR-10a gains it by
name and the allow-list of the `security` section widens to match. The trace
must still fail on anything else.

### Step 3 - the process state, which costs nothing

`src/collect/procs.rs`: `parse_stat` already splits the tail of
`/proc/<pid>/stat` and its own comment records that `rest[0]` is field 3, the
state. Take it into `StatFields` and carry it to the row. One index into a
buffer already parsed - no new file and no new read.

### Step 4 - the colours

`src/render.rs`: one `style_for(Reading)` returning the existing theme roles -
`calm`, `accent`, `signal`. No new role, so the eight palettes stand as they
are. `table_lines` gives every value span its own reading's style, and
`card_lines` uses the same function.

`bar_style` is NOT touched. It reads the share of the largest row of the level
for memory, disk and network, and section 11 makes that colour mandatory on
every sorting; routing it through the new reading would draw every bar calm
under a disk sorting, because disk has no absolute reading at all. So the bar
and the figure beside it can disagree, and they mean different things when they
do: the bar says large next to this level, the figure says large for this
machine. D-42 records that in those words, because a reader who sees a red bar
against a calm figure will otherwise read it as a fault.

### Step 5 - the mark on the row

A glyph in the name column, before the mark of the node, drawn in the worst
reading the row carries. A glyph and not a ground, because the ground is
already spoken for three times over - the selected row, the `(self)` row and
the filter match - and a fourth claimant would have to win or lose against each
of them. A glyph composes with all three by construction, and that is the
reason the decision records.

### Step 6 - the dump channel

`src/cli.rs`, `src/dump.rs`, `src/render.rs`: `--dump-style N` prints each frame
as it does today and then a role map of the same width and height, one
character per cell - `.` default, `c` calm, `u` unusual, `a` alarm, `s` the
ground of the selected row, `m` the filter mark. Cell for cell with the text, so
the linter reads it the way it already reads a frame.

`--help` gains the flag, `make readme` copies it into `README.md`, and
`tests/documents.rs` fails until it has.

### Step 7 - the measurement that replaces the conventions

`scripts/frame-lint.py` gains invariant 18: it reports the alarm cells of every
frame, and fails only in the induced states the check raises itself - the known
CPU quota, the disk load, the many-scopes state - where the right answer is
known in advance. On an arbitrary host it reports and never fails. A ceiling
fitted to two rigs cannot tell a palette that is too loud from a host that is
genuinely in trouble, and the second is the case this tool exists for.

Invariant 17 - that the map covers the frame cell for cell - belongs to the
linter. `tests/frame_invariants.rs` owns the other half: that `--dump-style`
emits a map of the right shape at all, over a snapshot, with no host involved.

The measurement that fixes step 1: both rigs and every induced state are run
with the linter reporting, and for each heuristic the run says how many rows it
fired on. D-42 carries every threshold with that number beside it. Where a
heuristic fires on almost every row or on none, the threshold moves before it
is written down.

### Step 8 - the documents

`docs/requirements.md`: FR-21 with its acceptance; FR-10a amended with
`/sys/class/net/<if>/speed`; one new mandatory rule in section 11 saying that a
figure carries its own reading and a flagged row carries a glyph; decision D-42
with the criterion, every heuristic and its denominator, the precedence of the
glyph against the three grounds, the rejected relative and historical readings,
and why disk has no reading at all.

`docs/testing.md`: the two new invariants, the `--dump-style` hook in the list,
and rows for FR-21 and D-42 in section 13.

### Verification

- `make fast` after every edit.
- `make live` once the batch is settled, and twice in step 7 - the second run is
  what fixes the ceiling.
- `make live SECTIONS="prepare security cleanup"` for the widened read surface.

## After the operator read the screen

Two rounds of feedback landed after the first version was on the host, and both
became decisions rather than edits to the old ones.

D-43: the card of a marked row says why. One line per rule that fired in either
column, named after the card row its figure stands on, and naming the figure of
both columns, the whole it was read against and the threshold it crossed. The
first version was unreadable on a live host for three separate reasons: the
sentence named a figure that stood in neither visible column, the swap reason
compared the subtree against the card row `own swap` beside it, and the names of
the heuristics matched no card label at all.

D-44: a mark says whether the row is the source of its reading or only the way
down to it. Every figure is the sum of its subtree (FR-5), so one busy process
marked every row above it up to the root, and the mark on pid 1 was a tautology.
`!` and `*` now stand where the reading survives on the row's own contribution -
the `(self)` remainder of FR-14 - and the arrow stands where it does not. The
mark is computed in `src/app.rs`, where the row is built, because the row keeps
a shallow copy of the node without the children the remainder needs.

## Follow-ups

- The `index` role of the planning workflow is not bound in this project. The
  documentation hierarchy in `AGENTS.md` names no index document, and the
  project states plainly that it does not use the SALP anchor grammar and that
  traceability runs the other way, through section 13 of `docs/testing.md`. The
  index and back-pointer steps are therefore not applicable here rather than
  skipped.
