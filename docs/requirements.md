# hostscope: requirements

Date: 2026-08-14.
Status: implemented and checked on the Docker rig. Every decision of
section 9 is settled. D-24, of 2026-08-15, made the tree the process
forest of the host and turned what runs a process into a word on its
row: it withdrew FR-16 and FR-19, closed D-22, and kept the container
rule D-23 established on snapshots of three real environments. D-25 and
D-26, the same day, took out everything that stood between the reader
and a row: the second CPU unit, the second key for going down, the
levels that held one row each, and the switch on the `OWNER` column.
D-27 - D-29, also that day, put four pieces of state back on the screen:
the bar moved to the sorted column, the refresh interval became a pair
of keys with the value written between them, the page keys started
working, and a filter now says what it is, marks where it matched and
goes away on `Esc`.

## 1. Why

An engineer opens an SSH session and has to answer "who is eating the
resources right now" within a minute. Existing tools answer in pieces,
and this was measured on 2026-08-14 on a live production host with 128
cores, called the reference host throughout this document:

- `htop` shows a flat process list. It has a tree (`F5`), but it does
  not sum subtrees. The request for that has been open in the project
  since January 2022 and is still unimplemented. On a host with 128
  cores and 2311 threads a flat list is unreadable.
- `systemd-cgtop` does sum by cgroup, but on this host the CPU column
  stayed empty for three passes in a row, while `cpu.stat` reads fine
  directly. Only memory, tasks and disk remain.
- `atop` does sum by cgroup, but its purpose is different: it is a black
  box for reconstructing the past (continuous recording, accounting for
  finished processes, parsing daily files). Live viewing is secondary
  there, and the interface serves the recording.

None of them connects resource usage with what the container actually
does: you see `docker-8f21c70b3d95018`, not the service name, image and
state.

## 2. Scope

In scope:

- The current state of the host the tool runs on.
- A single process: no daemons, no services, no ports.

Out of scope:

- History and post-mortem analysis. `atop` does that, no need to
  duplicate it.
- Alerts and thresholds. A monitoring stack does that.
- Dashboards and charts for the team. The dashboards that are already in
  place do that.
- Control: stopping, restarting, changing limits. Read only.
- Collecting from several hosts at once.

## 3. Scenarios

S1. Load went up, the cause is unknown. The engineer sees the top level,
finds the group with the highest usage, drills into it and reaches the
specific process in 3-4 keystrokes.

S2. The container is known, the details are not. The engineer finds it
by service name (not by identifier), sees the total usage, the processes
inside it and the container parameters.

S3. Memory is running out. The engineer switches sorting to memory and
repeats S1 without changing the way they navigate.

S4. A short spike. The engineer freezes the screen to read the values
instead of fighting the redraw.

## 4. Core concept: drill-down

The same navigation on every level. The level is chosen by the position
in the tree, not by a mode switch.

- L0. Host: CPU, memory, swap, disk, network, load average. The rows are
  the processes that stand at the root of the forest - on an ordinary
  host, `init` and the kernel thread daemon.
- L1 and below. The processes each of them started, with totals over the
  whole subtree under each row (FR-5). The depth is the depth of the
  forest, not a fixed number of levels.
- The card of a row: threads, I/O (`/proc/<pid>/io`), the user, the
  start time, the full command line, and what runs the process - the
  container with its image and ports, the service, the login session
  (FR-20). The environment is not read at all (FR-9).

Navigation rules:

- `Enter` goes deeper, `Backspace` goes back, the path is visible in the
  navigation line at the top. The right arrow also goes deeper, and in
  the list view it puts a row among its neighbours instead.
- `PageUp` and `PageDown` move by what the table holds: the cursor and
  the window move together, so the next screen is the rows that stood
  after this one rather than the same rows shifted by a line (D-29).
- `Esc` undoes the narrowing nearest at hand, in that order: the card,
  then the filter, then the level. The way back up a level stays
  `Backspace`, which carries the filter with it (FR-2), and the filter is
  the one state that used to have no key to drop it (D-29).
- `i` opens the card of the selected row, any row: a process, the host,
  and `(self)`. The card lies on top of the table, `Esc` closes it,
  `Enter` from it drills into the same node. On a row with nothing under
  it `Enter` opens the card too: there is nowhere to go, and the detail
  is what the reader wanted (D-25).
- A chain of processes where each started only the next, all with the
  same owner, is one row named for the whole chain. Its figures are the
  first link's, which already cover the chain, and its card names every
  link with its pid. A change of owner ends a chain: that boundary - a
  shim stepping into a container - is worth the keystroke, and gluing
  across it would put a name in the `OWNER` column that half the row
  does not belong to (D-25).
- A node with children carries the mark `>` in front of its name, a node
  without children carries two spaces there. The mark says where
  descending makes sense before a key is pressed.
- `v` switches the level between the tree and the list (FR-18). The
  position in the tree is the same in both; what changes is how many
  levels of it are on screen at once.
- Sorting, search and filter work the same on every level and survive
  the transition.
- The value in the parent row always equals the sum of the rows shown.
  Where a node consumes resources itself, the difference is carried by
  the `(self)` row (see FR-14). A mismatch after accounting for it is a
  defect. This holds in both views: the list shows the ends of the
  subtree - the leaves and the remainders above them - and those are the
  parent split up, not a second reading of it (FR-18).

What it looks like on screen is fixed by the rules of section 11: one
column set on every level, the detail of a row on a line under the
table, the mark on a node with children.

## 5. Functional requirements

FR-1. A tree of the host with CPU, memory, disk and network totals in
every node. The tree is the process forest (D-24): a row is a process,
its children are the processes it started.
Acceptance: on a host with running containers the total of a node
matches the total of its children within 1 percent; verified by a test
over a captured `/proc` snapshot.

FR-1a. CPU is measured in busy cores and in nothing else. The header
shows `<busy> of <total> cores`, a row shows the number of cores with
three decimals. There is no percentage view and no switch (D-25).
Rationale: on 128 cores a percentage without a base reads two ways - 13%
is either 16.6 cores or 0.13 cores, a hundredfold difference. A core
count is comparable to load average, which uses the same unit. A switch
between units only put that ambiguity back on the screen one keystroke
away, and the label under it was the only thing keeping it honest.
Acceptance: on a host with 128 cores the sum of cores over the root
nodes differs from `1 - idle` per `/proc/stat` by no more than 5 percent
over the same window; every CPU figure on screen is a core count.

FR-2. Drill-down down the forest preserving sorting and filter.
The detail card opens on any row and is not a level: it changes neither
the path nor the selected row, so `Esc` returns exactly where it was
opened from.
Acceptance: a scenario test walks down the forest and back, checking
sorting and filter state at every step; opening and closing the card on
every level changes neither the path nor the index of the selected row.

FR-3. Container enrichment: name, image, state, creation time, restart
count, labels, published ports. It reaches the screen through the owner
of a process row (FR-20).
Acceptance: for a process whose cgroup names a container the output
carries the container name; when the Docker socket is unavailable the
row shows the short identifier and a "data unavailable" marker, and the
application does not crash.

FR-4. WITHDRAWN 2026-08-14 by D-17. It required systemd enrichment: the
unit description and its load, active and sub states, read over the
system bus. A unit row still carries the unit name, which is what the
cgroup directory is called; nothing is asked of systemd. The number is
left in place rather than reused, because the other documents and the
checks refer to these requirements by number.

FR-5. Every row carries the totals of its whole subtree: a process plus
every process it started, however deep. This is what makes a branch
readable from the top - the figure on a row is what that branch costs
the host, not what one process of it costs.
Acceptance: the total over a process subtree is checked against the sum
of `/proc/<pid>` values of its descendants.

FR-6. Sorting by any resource and substring search (process name,
command line, and the owner of the row - container, service or user).
The sorted column carries the bar, so the order on screen and the
lengths on screen are the same reading (D-27). A filter that is on says
so on the path line, with what was typed and how many rows it left; the
match is marked where it was found; and `Esc` drops it (D-29).
Acceptance: a test over a set of 500 nodes checks the order and the
search result; a frame scenario types a filter, keeps it, and drops it
again, and checks the line and the rows at each step.

FR-7. Pause: freezing the screen without stopping collection. The pause
is the near end of the refresh interval (D-28), which `-` and `+` move
between a pause, 1, 2, 3, 5, 10, 30 and 60 seconds; the interval stands
on the key line beside the two keys that change it. Three seconds is what the
application opens at: a second of screen movement costs a tick of
collection on a tree of hundreds of processes, and a screen an engineer
watches for a minute does not need twenty of them.
Acceptance: after the pause key the values on screen stop changing;
after releasing it they show the current state; the frame says which
interval it is being renewed at, and the two keys walk the list to both
of its ends.

FR-8. The main mode is running as root through `sudo` (D-5). Without
root the application keeps working in a reduced form: unavailable fields
are marked as such, not replaced with zeros.
Acceptance: under root the container name, process I/O and network
connections are shown; without root the same fields are marked
unavailable and the application does not crash.

FR-9. The environment of a process is never read. Under root
`/proc/<pid>/environ` of any process is readable, and that is where
tokens, passwords and keys live. Showing the variable names was allowed
until 2026-08-14 and is now withdrawn by D-17: the names answer no
question about resource usage, while reading the file at all is the one
way this tool could leak a secret.
Acceptance: a process is started with a variable holding a known secret;
neither its value nor its name appears in any frame or log line, and an
`openat` trace of a run over the live host shows no `environ` opened at
all.

FR-10. Read only. The application runs no external commands, writes
nothing outside the log named on the command line and has no action that
changes the
state of the host: no killing processes, no changing limits, no
restarting containers. Running as root makes this requirement mandatory
rather than desirable.
Acceptance: the build fails if process-spawning calls
(`std::process::Command`) appear in the code; checked by a dedicated
build step.

FR-11. Network is shown on every level (operator decision 2026-08-14).
cgroup v2 has no network counters: the host has the controllers
`cpuset cpu io memory hugetlb pids rdma misc` enabled, and network is
not among them. So there are two sources:

- a container with its own network namespace - counters are taken from
  `/proc/<pid>/net/dev` inside that namespace, with no kernel programs;
- processes in the host namespace (systemd units, user sessions) - only
  through eBPF, otherwise their traffic cannot be attributed.

Acceptance: for a container the traffic total matches the increment of
the interface counters in its namespace; for host processes with eBPF
disabled the field is marked unavailable rather than zero.

Consequences for the plan: eBPF adds a kernel object compilation step
(clang) to the build and a dependency on the kernel version. DECIDED
2026-08-14: the first stage is built without eBPF - a container gives
its traffic right away from the interface counters of its own namespace
- and eBPF moves to the second one. Until then a process in the host
namespace carries `n/a` in the network columns and the card says why:
`host netns, not attributable without eBPF`.

FR-12. The interface language is English, entirely and without a switch:
column headers, header labels, units, hints, the key line, the level
number on the path line, unavailability texts (`n/a`, `image and state unavailable
without sudo`). Data is a different matter and is shown as it is: a
container name, a unit name and a command line keep their own letters,
whatever the script (D-17 replaces D-15 here). What is removed from data
is only what would drive the terminal rather than be drawn by it -
control characters become a space, zero-width and direction-changing
characters are dropped.
Column widths are counted in terminal cells and not in characters, so a
name in a wide script pushes no column aside. The cells come from the
Unicode tables through `unicode-width`, not from a list of ranges kept
in this project (D-18).
Acceptance: the interface text of every level and every mode is English;
a process whose command line is not ASCII shows those letters in the
frame, no frame carries a control character, and every line of the frame
takes exactly the width of the terminal in cells.

FR-13. Two measurement modes, switched by a single key on any level:
instant (the last collection interval) and average (since the
application started). The mode applies to every number on screen at
once - header, columns, sorting - and is labelled with the window the
values were taken over: `INSTANT 1.0s` or `AVG over <duration>`. The
application opens in average mode (D-19).

- CPU in average mode: accumulated time from `/proc/<pid>/stat` over the
  window, divided by the window length. Disk and network are accumulated bytes
  over the window, divided by its length. Memory and task count are
  averaged over samples.
- The averaging window is counted from application start, not from the
  moment the process appeared: otherwise rows cannot be compared with
  each other and tree totals stop adding up. A process that lived a
  small part of the window looks modest in average mode, so the process
  card must show both values side by side.
- Sorting applies to the values of the current mode, so row order
  differs between modes. That is not a defect, that is the point of the
  mode.
Acceptance: a scenario test over a tree with an artificial spike checks
that in instant mode the spike node comes first and in average mode the
steadily loaded node does; the sum of averages over children matches the
parent for CPU, disk and network.

FR-14. The `(self)` row is the node's own usage: the node value minus
the sum of its direct children, per value and in both measurement modes.
It is computed at render time, so the equality "children plus `(self)`
equals the parent" holds by construction rather than by data.

- Sources of the difference: the work the process does itself rather
  than through the processes it started, and children that appeared and
  disappeared while the application was running - their work stays in
  the counter of the parent, and their own row no longer exists.
- On the host row the difference is large by construction: the machine
  reports its own memory, the processes report theirs, and the page
  cache is the gap between the two.
- The row appears only when the difference is non-zero for at least one
  value. A node whose children cover it entirely does not have it.
- The row is drawn in a dimmed colour and does not expand: it is a
  computed remainder, not an entity.
- Its position is fixed - always the first row of the level, excluded
  from sorting (operator decision 2026-08-14). Otherwise it wanders
  across the screen on every change of the sort resource, and there is
  no need to hunt for the remainder: it is either small and harmless or
  large and must catch the eye immediately.
- The row offers a card on `Enter` like any other: it names the parent,
  the number of subtracted children and both sources of the difference.
Acceptance: on a `/proc` snapshot where a parent process holds more than
its children, the sum of the rows shown matches the parent for every
value; the row is first under each of the four sortings; on a node whose
children cover it entirely the row is absent.

FR-15. Cumulative kernel counters (`/proc/<pid>/stat`, `/proc/<pid>/io`,
`/proc/stat`) are sampled at start and from then on shown ONLY as
deltas: instant mode is the difference over the interval, average mode
is the difference over the window since start. No value on screen
contains anything accumulated before the application started.
Rationale: `system.slice` on the reference host holds 1024 CPU-days in
`cpu.stat` while its live children hold 198 - the rest was brought by
containers that are long gone. That is history, not state, and it must
not be shown as usage (see scope: the application does not replace
atop). Gauge counters (resident memory, thread count) are taken as they
are - they do describe the current state.
Acceptance: right after start every value derived from cumulative
counters equals zero; a test over a substituted `/proc` snapshot with a
non-zero base checks that the base never reaches the output.

FR-16. WITHDRAWN by D-24 on 2026-08-15. It required that a grouping the
kernel does not have be marked as assembled by the application. There is
no such grouping any more: the tree is the process forest, every row in
it is a real process, and what runs a process is a word on the row
rather than a node above it (FR-20).

FR-17. The application provides hooks for verification: substitution of
data sources, dumping of the model and of the frame, a key program, a
fixed collection tick and a log to file. Without them verification comes
down to parsing screen text, which cannot tell an arithmetic error from
a layout error; how the hooks are used is described in
[testing.md](testing.md).

- `--cgroup-root DIR` and `--proc-root DIR` - read a captured snapshot
  instead of the live `/sys/fs/cgroup` and `/proc`. Gives repeatable
  checks without a host.
- `--docker-socket PATH|none` - substituting and disabling the socket,
  which is how the degradation from FR-3 and D-13 is checked.
- `--dump-model json` - print the tree model as numbers and exit.
  Comparison against an independent oracle runs over this output, not
  over screen text.
- `--dump-frame N` - render N frames as text and exit. Gives a layout
  check with no terminal involved.
- `--keys "Right Right a Escape"` - run a key program and stop, so that
  a scenario is a single command.
- `--tick MS` - a fixed collection tick, so the averaging window of
  FR-13 and FR-15 is known exactly.
- `--log FILE` - the log goes to a file only, never to the terminal. The
  log also carries the collection time and the render time of every
  frame: there is no way to measure them from outside, and without them
  the 50 ms threshold of section 6 is unverifiable.

Dumps go to standard output rather than to files: FR-10 forbids writing
outside the log named on the command line, and a verification hook is no
reason to make an exception.
Acceptance: two runs with `--cgroup-root` over the same snapshot produce
an identical `--dump-model json`; `--dump-frame` works with no terminal,
writing to a file; an `openat` trace of a run with any set of hooks
shows no write anywhere except standard output and the file from
`--log`.

FR-18. The list view: `v` lays the subtree of the current level out as a
flat list of its ends, and `v` again returns to the tree. It answers the
question the tree cannot answer in one screen - which process on this
host is eating the resource, whatever level it sits on.

- A row is an end of the tree: a node with no children, or the `(self)`
  remainder of a node that has them (FR-14). Nodes in the middle are not
  rows - their numbers are the sum of what stands under them, and the
  tree view is where a total is read. The remainders are what keeps the
  middle of the host on screen all the same: the page cache of a
  container, or the work of children that came and went, is charged to
  the node itself and to no leaf under it.
- The rows therefore add up to the level exactly as in the tree: the
  leaves of a subtree plus the remainder of every node above them are
  the parent, by the construction of FR-14. The equality is the same
  one, and it is checked the same way.
- The rows are ordered by the current sort and taken over the current
  measurement mode, exactly as in the tree.
- In front of the name a row carries the chain of names between the
  level and itself, drawn pale. When the name column is too narrow the
  chain is cut from its front and the name is kept whole: the row is
  read by its name, and the chain only says where it came from.
- The filter reaches every level at once. A node the filter rejects is
  left out, but its children are still shown - otherwise a process could
  not be found by its own name.
- `→` on a row goes to the level the row lives on, however deep that
  is, and the view stays as it was. Nothing sits under a row of the
  list, so the useful move is the one that puts the row among its
  neighbours; `Esc` then walks back up one level at a time.
- The `(self)` row of the current level keeps its place at the top,
  excluded from sorting (FR-14). The deeper remainders sort among the
  leaves: they are rows of the subtree like any other, and their place
  is where their number puts them.
- The path line names the view - `view: tree` or `view: list` - because
  the rows of one are not the rows of the other.
- The per-process files that the tree view reads only for the level in
  view - `/proc/<pid>/io` - are read for the whole tree while the list
  is on screen. In the list every process is on screen, and an `n/a` in
  the disk column has to mean the host cannot report the value, not that
  the application did not look (FR-8). That is what the view costs, and
  the figures of section 6a were measured in the tree view: they have to
  be taken again in the list view on a live host.
Acceptance: over a captured snapshot, `v` from the root shows a process
of a container without descending; a filter typed in the list finds it
by name; `→` on it puts the path at its own level; every invariant of
the frame linter holds in the list view as it does in the tree.

FR-19. WITHDRAWN by D-24 on 2026-08-15. It made the opening level four
groups - `containers`, `services`, `users`, `system` - assembled by the
application, with the cgroup hierarchy one keystroke away under `g`.
What it was answering is now answered by FR-20 without a second tree:
the row says what runs the process, and the filter reaches that word.
The container rule the groups were built on survives unchanged and is
where D-23 left it.

FR-20. Every process row says what runs it. The cgroup hierarchy is the
one place a host records that a process belongs to a container, to a
service or to a login session, so it is read for that and for nothing
else - `cgroup.procs` and no other file.

- The `OWNER` column carries the name and nothing else (D-26): the
  container name from the runtime, or its short identifier when the
  socket cannot be reached (D-13); the unit name of a service, without
  the `.service` suffix; the user of a login session. The column is
  always drawn; where nothing runs a row - the host, the `(self)`
  remainder, a process outside every cgroup - it is blank.
- The kind of an owner - `container`, `service`, `user`, `kernel`,
  `system` - is on the card and in the filter, not in the column. It
  answers "show me everything in a container", which is a question the
  filter takes, and it says nothing new on a row that already names the
  container.
- Where several apply, the innermost thing a person would name it by
  wins: a container first, then the user whose session it is, then the
  service. That is what keeps `user@1000.service` with the user rather
  than among the services the host offers - it is how a login session is
  run, not something the host offers.
- A container is recognised by its name together with the runtime
  directory above it, exactly as D-23 established it on the three
  captured environments.
- A kernel thread is `kthreadd` or something it started. It belongs to
  no container, service or user, and calling it `system` would mix it in
  with the work of the host, so it is its own kind.
- The filter reads the owner as it reads the name and the command line,
  so `grafana` finds every process of that container wherever its
  runtime hung them in the forest.
Acceptance: over captured snapshots of the three environments - Docker
with the cgroupfs driver, a Kubernetes node, a plain server with the
systemd driver - every running container is named on every process it
runs; a filter on the container name shows those processes and no
others; a filter on the kind of an owner shows every row of that kind;
every owner the model names is on a row of the drawn level.

## 6. Non-functional requirements

Measure on the reference host: 128 cores, 125 GB of memory, about 287
tasks and 2311 threads, Ubuntu 22.04, cgroup v2.

- Start to first screen: under 300 ms.
- Redraw: under 50 ms, to hold 10 updates per second.
- Default collection interval: 3 seconds, moved by `-` and `+` between a
  pause and a minute, and set at start by `--tick` (D-28).
- Own usage: under 4 percent of one core and under 100 MB of memory on a
  tree of about 110 cgroups and 370 processes at a one second interval -
  the interval the figure was measured at, and a third of what the
  application now opens at (D-28). The figure is stated with the tree it
  was measured on,
  because the cost of a tick follows the number of cgroups and processes
  and not the number of cores; the first version of this line said 2
  percent with no tree attached and was unverifiable (D-16).
- Enrichment (Docker) runs separately from collection and does
  not block the redraw: a slow socket response must not freeze the
  screen. Data is filled in as it arrives and is cached.
- Working over SSH on a laggy link: the output volume per frame is
  bounded by the visible area, and a full screen repaint does not happen
  on every frame.

### 6a. What was measured, and where

The reference host was not available for this work, so every figure
below comes from the Docker rig: 4 cores, 16 GB, kernel 6.8, Ubuntu, cgroup v2, 110
cgroups, 366 processes, 19 containers. The tree there is comparable in
node count to the target host, and the cost of a tick follows the number
of cgroups and processes rather than the number of cores.

Every figure is produced by `scripts/host-check.sh`, section
`measurements`, which runs the application once under a scope of its own
and reads the timings from its log and the CPU and memory from that
scope:

```
bash host-check.sh measurements
```

| Requirement | Target | Measured on the Docker rig |
| --- | --- | --- |
| Start to first screen | under 300 ms | 29.6 ms |
| Redraw | under 50 ms | 0.7 ms at the 95th percentile |
| Collection per tick | (no target) | 47.3 ms at the 95th percentile |
| Output per frame over SSH | bounded by the screen | 454 bytes per frame, no full clear in 20 s |
| Own memory | under 100 MB | 1.8 MB |
| Own CPU | under 4 percent of one core | 3.11 percent |

Every line of that table now comes from one twenty second window: the
application runs once, under a scope of its own, and the timings are read
from its log while the CPU and the memory are read from that scope. It
used to be two runs, which meant the top four rows and the bottom two
described different windows.

The figures above are from the run of 2026-08-15 that checked D-26: 30
checks, none failed, 289 processes agreeing with the oracle over ten
seconds. A tree of 695 nodes - 200 extra processes raised on purpose -
costs 82 ms per tick, so the cost grows roughly with the number of
processes read rather than with the number of rows drawn.

The figures have been taken after every change that could move them, on
the same host and with the same commands: after D-17 removed three
features, after D-23 regrouped the opening level, after D-24 made the
tree the process forest, after D-25 glued the chains, and after D-26 made
the owner column a name and nothing else. Everything moved within its own
noise.

Two lines are worth reading rather than glancing at. The bytes per frame
follow what happened to be on the screen rather than any change. And the
cost of a tick did move with D-24, in both directions at once: the
cgroup walk stopped reading five files per cgroup and now reads one,
while every process is now read in full on every tick, where before that
was done only for the level in view. The two nearly cancelled - 44.0 ms
before, 47.7 ms after - and the second is the one that will grow with
the host. D-25 and D-26 do not touch the cost: the chains are glued
after everything has been read, and the owner column draws a string that
was already collected, so both take work off the screen and none off the
tick. What it did take off is the tree: 368 nodes became 295 on
an idle host, and 966 became 699 with the 200 induced processes running,
because each of those was a `timeout` and its child.

## 7. Delivery constraints

- A single static binary with no external dependencies, copied to the
  host with plain `scp` into the home directory. This follows directly
  from the session of 2026-08-14: building atop required a container of
  the right distribution version, because the binary links against
  glibc, ncurses, glib and pcre.
- Target: Linux x86_64, kernel 5.4 and up, cgroup v2. Support for
  cgroup v1 - proposal to postpone until the first host that needs it.
- No systemd services, no ports, no files outside the home directory.
- Started through `sudo` (D-5). Root is needed for container data,
  process I/O and network connections. Privileges are taken once for the
  whole process: there are no repeated `sudo` calls and no external
  commands per frame, so the 50 ms requirement of section 6 holds.
- Root widens the data beyond FR-3 as well: `/proc/<pid>/io`, the list
  of open files and connections, PSS instead of RSS and per-cgroup disk
  figures become available. This affects the contents of the card.

## 8. Review of existing solutions

Done on 2026-08-14 on the reference host (Ubuntu 22.04, 128 cores,
cgroup v2).
Static builds were downloaded and run on the host itself; the TUI screen
was captured through a pseudo-terminal (`ssh -tt` plus stripping of
control sequences). FR-1 (tree with totals), FR-2 (drill-down) and FR-3
(container data) were checked.

**bottom (btm) 0.14.8 - the closest candidate.**

- FR-1 partially. A PROCESS tree with totals on a collapsed branch is
  confirmed by measurement: the row `+ systemd` (pid 1) showed 2.0% CPU
  and 7.9% memory, while the own usage of pid 1 on the same host is zero
  (`procs --tree`: `1 root 0.0 0.0`). So a collapsed parent carries the
  subtree total.
- But that is a process tree, not a cgroup tree: there is no "container"
  node in it, and grouping by `docker-<id>` or by systemd unit is
  impossible.
- FR-2 partially: collapsing and expanding exist, levels with a
  navigation path and a filter carried between them do not.
- FR-3 no: no container name, no image, no state.
- Grouping by name (`-g`) does sum ("usage is added together when
  displayed"), but per the documentation it is mutually exclusive with
  the tree.
- Command used: `btm -T --tree_collapse`.

**btop 1.4.7 - not suitable.** The tree is there (`proc_tree`), the
totals are not: the `systemd` row showed 13M - the own memory of the
process, not the subtree total. FR-1 no, FR-2 no, FR-3 no.

**ctop 0.7.7 - not suitable by coverage.** Its own description: "ctop -
interactive container viewer". It shows containers only; processes
outside containers and systemd units do not exist for it. FR-1 no, FR-2
partially (entering a container), FR-3 yes - but that is all it can do.

**procs 0.14.12 - not suitable.** It draws a tree but does not sum
(verified: `multipathd` pid 1581 shows 0.0 with six children). It is not
interactive, there is no drill-down. A useful detail: its column set
includes `Cgroup`, `Docker` and `container`, so the data source for FR-3
is already implemented there and worth a look.

**Not checked, with reasons.** `glances` - written in Python, needs an
environment installed rather than a binary copied, which contradicts
section 7. `csysdig` - requires a kernel driver or eBPF, which is a
system change. `lazydocker` - by its description works with Docker only,
so it is narrower than FR-1 by construction. `nvtop` - about GPUs, not
related to the task.

**Conclusion.** No candidate covers FR-1 together with FR-2 and FR-3.
The closest is bottom, and what it lacks is exactly the grouping axis:
it lives in the process tree, while the task is stated in the cgroup
tree, where the node is a container or a unit. Building our own tool is
justified.

Before the start it is worth looking at two other implementations: how
bottom sums a collapsed subtree, and how procs obtains its `Docker`
column.

## 8a. The access limitation found during the review

The account this work runs under is not a member of the `docker` group,
and the socket is owned `srw-rw---- root docker`. Verified: `docker ps` answers
`permission denied while trying to connect to the Docker daemon socket`.

Consequence: with the current privileges FR-3 is unreachable, neither
for our tool nor for any of the candidates. The container name, image
and state can only be obtained through the Docker daemon, and the files
in `/var/lib/docker` are readable by root only. From the cgroup path
alone only the identifier can be extracted.

This makes D-5 mandatory to settle before development starts.

## 8b. How cgroup is arranged: what was measured on the host

Captured on 2026-08-14 on the reference host by reading `/sys/fs/cgroup`
(Ubuntu 22.04, 128 cores, 1091 days of uptime). This is the evidence
base for FR-14, FR-15 and FR-16 - without it they look like matters of
taste. The numbers are given exactly as the kernel returned them.

It can be reproduced from the root of the node in question (example for
`system.slice`):

```sh
cd /sys/fs/cgroup/system.slice
cat memory.current                     # the counter of the node itself
cat */memory.current | paste -sd+ | bc # children total, difference is (self)
cat cpu.stat | head -1                 # usage_usec, accumulated since boot
wc -l < cgroup.procs                   # own processes: 0 for a node with children
ls -d docker-*.scope | wc -l           # containers sit right here
```

- **Containers are siblings of `docker.service`, not its children.** The
  kernel keeps every container as a separate `docker-<id>.scope` node
  directly inside `system.slice`, interleaved with `ssh.service` and the
  rest of the units. `docker.service` has five own processes
  (`dockerd`, `containerd` and helpers) and not a single child node.
- **A node with children has no processes of its own.** For
  `system.slice` and `user.slice` the file `cgroup.procs` is empty even
  though children exist. This is not a quirk of the host: cgroup v2
  forbids holding both own processes and children with enabled
  controllers.
- **The parent's memory exceeds the sum of its children.**
  `system.slice`: `memory.current` 54678M against 36518M for the
  children, a difference of 18.2G. `user.slice`: 34554M against 31850M,
  a difference of 2.7G. The cause is that pages of deleted cgroups are
  moved to the parent, and the node's own page cache also sits in its
  counter.
- **What accumulated since boot is an order of magnitude above what is
  live.** `system.slice`, `cpu.stat usage_usec`: 105627599590933 against
  17174657731130 for the children - about 1024 CPU-days from cgroups
  that no longer exist, against 198 days for the live ones.
  `user.slice`: 54368729408 against 37517468230. Showing such a number
  on screen means lying about the current load, hence the delta rule
  (FR-15).
- **"Parent equals the sum of its children" does not hold for every
  value.** For CPU, disk and network it does (the counters are
  recursive). For memory it does not, because of the page transfer. That
  is why the requirements state the equality through the `(self)` row
  rather than as a property of the data.

## 9. Open decisions

D-1. Delivery form. DECIDED 2026-08-14: a full-screen terminal
application. Rationale: the work happens over SSH, a web interface would
need a port and an authentication layer in front of it, and would
overlap with the dashboards that are already in place.

D-2. Language. DECIDED 2026-08-14: Rust with ratatui. Rationale:
rendering is faster and cleaner, which works directly towards the sub-50
ms frame of section 6. Consequences for the plan: a static build for the
target is done through `x86_64-unknown-linux-musl`, and access to the
Docker API is taken through a separate library (candidate - `bollard`,
to be checked before the start). "No dependencies" is a budget on the
binary, not a count of lines in `Cargo.toml`. Before writing a table, a
parser or a formula by hand, run `cargo tree` and look: a crate already
pulled in by `ratatui` or `crossterm` costs nothing to name directly,
and the hand-written version costs the bugs it will have. This is how
`unicode-width` was reached, and only after the hand-written table had
already shipped a wrong width (D-18).

D-3. Source of container data. DECIDED 2026-08-14: the Docker socket
directly, degrading to the short identifier from the cgroup path when
access is missing. The alternative of calling `docker` as an external
command is closed by FR-10: under root the application runs no external
commands.

D-4. Area of use. DECIDED 2026-08-14: non-production hosts at the first
stage; the Docker rig is the host every check of this work ran on.
Production needs a separate decision, because direct SSH access there is
closed. Nothing in the application depends on which host it runs on: it
writes nothing, opens no port and runs no command (FR-10), so widening
the area later is a matter of access policy, not of code.

D-5. Access to container data (see section 8a). DECIDED 2026-08-14: run
through `sudo`. The operator's rationale: root is needed anyway for most
of the requested data, not only for the Docker socket.

Rejected: membership in the `docker` group (equivalent to root rights,
but a system change instead of a one-off elevation), a read-only proxy
to the socket (covers Docker only and gives neither process I/O nor
connections), dropping FR-3 (without container names the tool is barely
different from bottom).

The concern about the 50 ms is removed by how the start works: under
root the whole process is elevated, `sudo` is called once at start,
there are no external calls per frame. FR-9 and FR-10 were added
precisely because the application runs as root.

D-6. CPU unit. DECIDED 2026-08-14: busy cores (FR-1a). Percentages
remain a switchable view and are always labelled with their base. The
reason for rejecting percent by default: on 128 cores "13%" reads two
ways with a hundredfold difference.

D-7. Interface language. DECIDED 2026-08-14: English entirely, without a
switch (FR-12). The project documents were kept in Russian at first and
translated to English on 2026-08-14 by operator decision, so the project
is now single-language throughout.

D-8. Instant values or averages. DECIDED 2026-08-14: both modes,
switched by the `a` key on any level (FR-13). The averaging window is
counted from application start, not from host boot.

D-9. The `(self)` row. DECIDED 2026-08-14: show it, always as the first
row of the level, excluded from sorting (FR-14). Rejected: sorting it
along with the other rows - the remainder then wanders across the screen
on every change of the sort resource.

D-10. Details and drill-down are different keys. DECIDED 2026-08-14:
`Enter` opens the card of any row, `→` goes deeper (section 4, FR-2).
Rejected: `Enter` going deeper, as in the first design - the card is
then reachable only on tree leaves, while its content is needed for a
container, for a cgroup slice and for `(self)` as well.

D-11. Memory in the table: `memory.current` or PSS. DECIDED 2026-08-14
by measurement: `memory.current` in the table, PSS in the process card
only. Reading `/proc/<pid>/smaps_rollup` for the 238 processes of the
Docker rig as root cost 175 ms - on its own more than seven times the
whole tick, which is 23 ms. The card reads it for one process, where the
cost is a fraction of a millisecond and the honest split of shared pages
is exactly what the reader came for.

D-12. The block of environment variable names in the process card.
DECIDED 2026-08-14: keep the names. SUPERSEDED the same day by D-17: the
block is gone and `/proc/<pid>/environ` is not opened at all. The check
still raises a process carrying a marker value, and now expects neither
the value nor the name to appear anywhere (`host-check.sh security`).

D-13. Behaviour without `sudo`. DECIDED 2026-08-14: work in a reduced
form. Refusing to start was rejected because most of the tree - every
cgroup counter and every process of the current user - is readable
without root, and a tool that refuses to open is worth less than one
that says what it cannot see. Checked live in both directions: without
the Docker socket a container row degrades to `docker-<short id>` and
the card says the data is unavailable; without root the application
keeps working and the fields root would have provided are listed as
unavailable rather than shown as zero (`host-check.sh degraded`).

D-14. The parent of a process living in a container. DECIDED
2026-08-14: the card shows the parent pid as the host sees it, taken
from field 4 of `/proc/<pid>/stat`. Everything else on the screen is the
host's view - the pid, the cgroup path, the counters - so a parent
numbered inside the container's own namespace would be the single value
that does not match `ps` on the same host. The namespace pid is not
shown at all: it cannot be read without entering the namespace, which
the read-only rule of FR-10 rules out.

The tree keeps the same view: a process row is nested under its parent
row when both live in the same cgroup, and the process forest of a
cgroup is rooted at the processes whose parent is outside it.

D-15. Non-ASCII characters coming from data. DECIDED 2026-08-14:
escape them as `\uXXXX`. SUPERSEDED the same day by D-17, which shows
data in its own script and counts column widths in cells. The reasoning
below is kept because D-17 answers it point by point.

- The notation is `\uXXXX` with four uppercase hexadecimal digits: a
  three-letter Cyrillic word prints as `\u0431\u043E\u0442`. The
  notation is fixed here, because otherwise the acceptance of FR-12 is
  unverifiable: the test has to know what exactly it expects to see.
- Every character outside printable ASCII is escaped, not only Cyrillic.
  A rule stated per alphabet would diverge from the acceptance at the
  first Chinese name or emoji in a container label.
- The row is marked in the details line under the table: `name shown
  escaped`. Otherwise the engineer sees an unreadable row and cannot
  tell whether the name is like that or the tool is broken.
- Truncation to column width and substring search (FR-6) operate on the
  already escaped string. One letter turns into six characters, and a
  ten-letter name takes sixty: without this rule the columns fall apart
  and the search matches something other than what is shown.
- Transliteration is rejected: it changes the name the engineer later
  searches for in `docker ps` and in the logs.

The acceptance this decision carried is replaced by the one in FR-12.

Found while raising the induced state on 2026-08-14: Docker refuses to
create a container whose name is not ASCII, so the half of the state
that the document describes as "a container named `hs-имя-по-русски`"
cannot be raised on this host at all. The check records that and raises
the same case on a process argument instead, which reaches the screen
through the command line of the card. That is still how the state is
raised under D-17.

D-16. Own CPU usage against the budget of section 6. DECIDED
2026-08-14 by the operator: accept the measured figure and restate the
budget with the tree size it was taken on - under 4 percent of one core
on about 110 cgroups and 370 processes at a one second interval.

Measured on the Docker rig: 3.0 - 3.5 percent, memory 3 MB against a
budget of 100 MB. The old line asked for under 2 percent without saying
on what tree, which made it unverifiable: the cost of a tick follows the
number of cgroups and processes, so the same code is under 2 percent on
a small host and over it on a large one.

What the tick is spent on, from the application's own log
(`cgroup_ms`, `proc_ms` per frame): 11 ms reading the counters of 110
cgroups, 7.5 ms reading `/proc` for 366 processes, about 4.5 ms
assembling the tree, and 3 ms drawing the frame. The figure started at
7.8 percent and came down to 3 percent by four changes, each measured:
reading directory entries by their type instead of asking the kernel
about every name again (20 ms to 13 ms on the cgroup walk), taking the
user of a process from the owner of `/proc/<pid>` instead of reading
`status` (8.5 ms to 0.9 ms), reading the command line and the I/O
counters only for the cgroup whose rows are on the screen, and reading
every kernel file with one open and one read instead of the four
syscalls `read_to_string` needs.

What is left is close to the syscall floor of the current design: about
550 file reads for the cgroups and 730 for the processes. Two ways
further down were considered and are not taken now, but stay written for
whoever needs the room later:

- read the process rows only for the cgroup in view, which takes the
  tick to about 15 ms but leaves a level empty for up to one interval
  after descending into it, unless the sampler learns a per-key time
  base;
- raise the default interval, which divides the cost by the same factor
  and multiplies the age of the numbers by it. D-28 took this one for
  another reason - the interval is now a key rather than a setting, and
  the application opens at three seconds.

Neither is worth its cost while the tool is under four percent of one
core: it runs for the minute an engineer looks at it, not around the
clock.

D-17. Three features removed. DECIDED 2026-08-14 by the operator, after
a review of what each feature costs against what it adds to the one
question of section 1 - who is eating the resources right now.

Removed: the systemd enrichment of FR-4, the environment variable names
of D-12, and the escaping of data of D-15.

- systemd enrichment cost a hand-written D-Bus client: the EXTERNAL
  handshake, message marshalling with its alignment rules, a reader for
  the reply, timeouts, and a fallback that read `Description=` out of the
  unit files in four directories. All of it for one `ListUnits` call.
  What it added was a description and three state words. The unit name
  is already the name of the cgroup directory, and a unit that is eating
  the processor is visibly active without being asked.
- The environment variable names were the only feature that needed a
  requirement to restrain it (FR-9), a section of the live check, and a
  canary process to prove the restraint held. A list of names diagnoses
  nothing about resource usage. Removing it removes the risk and the
  evidence burden together; FR-9 is now the stronger statement that the
  file is never opened.
- Escaping turned data into `\u0431\u043E\u0442`, which is unreadable,
  and made FR-6 search match a string the engineer cannot type. It was
  the wrong answer to a real problem: what breaks a frame is a control
  character, not a letter. Control characters are now removed, letters
  are kept, and widths are counted in terminal cells so a wide script
  pushes no column aside. Transliteration stays rejected for the reason
  D-15 gave: it changes the name the engineer searches for elsewhere.

The alternative to each was to keep it in a reduced form, and each was
rejected on the same ground: a feature that answers the wrong question
does not get better by answering it more cheaply.

Checked on the Docker rig on 2026-08-14 with the full `host-check.sh`: 27
checks passed and one failed. The three sections that cover this
decision all passed. The environment is not read - the canary process
put neither the value nor the name `HS_CANARY` on any screen or in the
log, and the `openat` trace of a live run shows no `/proc/<pid>/environ`
opened at all. A process argument outside ASCII reaches the frame in its
own script, and the frame linter passes on those frames, so the cell
arithmetic holds where the escaping used to.

The one failure was the oracle, on the memory of a single container:
2237693952 bytes against the model's 2170445824, a difference of exactly
64 MiB. It is a jump between the two readings, not an arithmetic error.
Four further oracle runs - two with this build and two with the build
from before the removal - found nothing on any of the 112 nodes, and the
code that reads `memory.current` is untouched by the change.

D-18. Where the width of a character comes from. DECIDED 2026-08-15
during review of D-17: from the Unicode tables through the
`unicode-width` crate, not from ranges written out in this project.

The first version of the cell counter carried its own table. It covered
the two emoji blocks it had been written from and silently dropped the
rest: a rocket (U+1F680) counted as one cell while the terminal draws
two, so a process whose command line held one produced a frame line of
101 cells in a 100 cell terminal. The frame linter did not catch it,
because its copy of the table had been written from the same list and
agreed with the mistake.

- The crate costs nothing: `ratatui` already depends on it, so naming it
  in `Cargo.toml` adds no download, no build step and no bytes. The
  delivery constraint of section 7 - one static binary with nothing
  installed on the host - is untouched.
- The two linters now read different sources on purpose. The Rust one
  uses the same crate as the application, so what it still judges from
  outside is how the frame USES a width - truncation, padding, the
  column plan. `scripts/frame-lint.py` counts with Python's own
  `unicodedata`, which is the independent second opinion on the table
  itself, and a frame is accepted only when both agree.
- Ambiguous-width characters - the box drawing of the frame, Cyrillic -
  count as one cell, which is what a terminal does unless configured
  otherwise.

Acceptance: a command line holding characters from the transport,
dingbat, miscellaneous-symbol and Unicode 12 emoji blocks keeps every
frame line at exactly the terminal width; checked by
`every_block_of_wide_characters_counts_as_two_cells` and by
`an_emoji_in_a_command_line_keeps_the_frame_the_width_of_the_terminal`,
both of which fail when the width of those blocks is forced back to one.

D-19. The mode the application opens in. DECIDED 2026-08-15 by the
operator, after using it: the average since start (FR-13). Rejected: the
last interval, which is what it opened in until now - a single second of
a live host jumps far enough that the first screen says more about the
moment of the keystroke than about the host, and the average is what an
engineer is looking for on arrival. `a` still switches to the interval
and back, and the window label in the right corner still names which of
the two every number was taken over.

D-20. How a row is drawn. DECIDED 2026-08-15 by the operator, after
using it: the selected row takes a background rather than the reverse
attribute, and the mark of a node with children is `>` rather than `▸`.

- Reversing swapped the colour of the bar with the colour of the line,
  so the filled blocks of the bar read as a hole in the very row the
  engineer was looking at. The bar now keeps its own colour - teal,
  amber or red by load - and only takes the background of the selection.
  The background is drawn to the right edge of the line, or the
  selection would stop at the last column.
- `>` is a character every terminal font has, and it does not depend on
  the same width tables as D-18. `▸` (U+25B8) is ambiguous-width, which
  is exactly the class of character the frame counts as one cell while
  some terminals draw two.

D-21. The list view (FR-18). DECIDED 2026-08-15 by the operator, in two
steps the same day: `v` flattens the level, and what it flattens to is
the ends of the tree - a node with no children, plus the `(self)`
remainder of a node that has them. Rejected: a permanently expanded tree
with indentation.

- The first version listed every descendant, the middle of the tree
  included. The operator called that back: a row whose number is the sum
  of the rows under it is a second reading of the same resource, and in
  one flat list the two stand side by side and are added up by the eye.
- The objection to a list of leaves is what the remainders answer. A
  slice or a container that holds usage in its own accounting rather
  than in its processes - the page cache is the usual case - would be
  missing from a list of leaves alone. Carrying its `(self)` row puts it
  back, under the name of the node it belongs to.
- With that, the equality of FR-14 holds in the list as it does in the
  tree: the leaves of a subtree plus the remainder of every node above
  them are the parent, by construction. The first version had to give
  the equality up; this one does not, and the linter checks the same
  invariant on both views.
- A row of the list has nothing under it, so `→` there means the level
  the row lives on rather than the row itself. The card of a deeper
  remainder names the node it was computed from, not the level the
  reader stands on.
- The rows carry the node without its subtree. A row draws itself and
  never its children, while a flat list holds a row per node of the
  tree: copying the subtree into every one of them would cost the square
  of the tree on every frame, against the 50 ms of section 6.
- The list withdraws the collection shortcut of section 6, which reads
  the per-process files only for the level in view. It was justified by
  "nowhere else can they reach the screen", and in the list they can.
  Kept as it was, the disk column of every process outside the level
  read `n/a` - a statement about the host that would have been false.
  The cost of the honest reading is one small file per process per tick
  while the list is open, and it stays unmeasured until the check runs
  on a live host: this machine has no `/proc` to measure it on.

D-22. CLOSED by D-24 and then answered by D-26, both on 2026-08-15.
Raised the same day by the operator: how much of the `TYPE` column an
engineer reads when they are looking for what eats the host. D-24 took
the column away - every row is a process now, so the kind of a row says
nothing - and moved the switch to the `OWNER` column. D-26 answered the
original question for that column too: the name, always. What the
question was worth is recorded below as it stood.

- `full` - the ten kinds as collected, which is what the column has
  shown until now.
- `short` - the five that come from systemd naming (`slice`, `session`,
  `unit`, `scope`, `mount`) folded into one word, `cgroup`. Nothing in
  the application branches on which of the five a node is: they differ
  only as five words in one column, and the name of the row already ends
  in `.service`, `.slice`, `.scope` or `.mount`. What is left are the
  four kinds that do change what a row is - `container`, `process`,
  `group`, `self` - plus `host`, which is never a row.
- `off` - no column at all. Its ten cells go to the name, which at a
  hundred columns takes it from 27 to 37. The list view is where that is
  felt: a row there carries the chain of names it came from, and it is
  the first thing the column runs out of room for.

What the switch cost while it was there: the column was not demanded by
either linter, only checked where it was drawn, and the column plan took
the width of the column as an argument. Both are gone with it - the
linters demand the column on every frame again (invariant 8 of the
testing document), and the plan carries the width itself.

Look at it with a snapshot and no terminal:

```
hostscope --cgroup-root DIR --proc-root DIR --dump-frame 1 --keys "v t t"
```

D-23. What decides the grouping. DECIDED 2026-08-15 by the operator,
after the question "what is the grouping determined by at the moment,
and by what criterion" and the answer that a scheme is needed which an
engineer of the first year can read without knowing cgroups, and which
works everywhere.

The decision was taken on measurements rather than on argument. Three
environments were captured with the six files per cgroup that the
collector reads, and the rule of the day was run over them:

| Environment | Layout | Containers recognised |
| --- | --- | --- |
| OrbStack, Docker 29.4, cgroupfs driver | `/docker/<64 hex>` | 0 of 4 |
| k3s 1.31, Kubernetes | `/kubepods/besteffort/pod<uuid>/<64 hex>` | 0 of 4 |
| the Docker rig, systemd driver | `/system.slice/docker-<id>.scope` | 19 of 19 |

Two environments out of three showed no containers at all, because the
rule read the name and only the systemd driver writes a name that says
anything. What was on screen instead: four directories typed `slice`
with hexadecimal names under `docker`, and on k3s the same four
levels down, under a pod identifier that names nothing a person knows.
On the server, where the rule did work, the containers were still two
keystrokes away inside `system.slice`, and the opening level - sorted by
memory - was three useful rows followed by seven `.mount` rows of zeros.

What was decided (the first of these was withdrawn by D-24 four hours
later; everything below it stands, and is what FR-20 rests on):

- The opening level is the four groups of FR-19 rather than the children
  of the root cgroup.
- A directory is a container when its name carries a known prefix
  (`docker-`, `cri-containerd-`, `containerd-`, `crio-`, `libpod-`) or
  when it is a bare identifier of at least 32 hexadecimal characters and
  a runtime directory stands above it - `docker`, `containerd`, `crio`,
  `podman`, `machine`, or anything beginning with `kubepods`, in either
  driver's spelling of the name. The name alone is not enough and the
  ancestry alone is not enough; both together recognised every container
  in all three environments.
- A pod is `pod<uuid>` or `...-pod<uuid>.slice`, and it is a level
  inside `containers`. Its name comes from the `io.kubernetes.pod.name`
  label of any container in it when the runtime provides labels, and
  from the shortened uuid otherwise. The classes of service
  (`besteffort`, `burstable`, `guaranteed`) are not levels: they hold
  nothing of their own and disappear.
- A container whose name cannot be enriched degrades to the bare short
  identifier, not to `docker-<id>` as before. Only one of the five
  runtimes recognised here is Docker, and the prefix would be a claim
  rather than a placeholder (this narrows D-13).
- A `.service` unit is called a `service` rather than a `unit`: the word
  is read by people who do not run `systemctl`. The `.service` suffix is
  dropped, where it says nothing the word has not said and costs the
  eight cells that cut `cloudflare_exporter.service` short.

What is not covered, and is known not to be: LXC and systemd-nspawn name
their containers rather than numbering them, so the bare-identifier rule
does not reach them. The `.lxc` directory of the OrbStack capture was
empty, and a rule written against no evidence is a rule written against
a guess. Such a container shows as an ordinary cgroup under `system`.

The three shapes are kept as fixtures built in code
(`tests/support/mod.rs`), not as the 432 KB, 432 KB and 6.6 MB of files
they were captured as. What has to be checked is which directory holds
which, and that is readable next to the assertion about it.

D-24. What the tree is. DECIDED 2026-08-15 by the operator, hours after
D-23, with the instruction to drop every grouping and keep the hierarchy
of process parents.

D-23 had made the opening level four assembled groups and left the
cgroup hierarchy under `g`. Using it showed that the application then
carried two trees and neither was the one an engineer asks for. The
cgroup hierarchy is a statement about resource control, and reading it
needs knowledge of cgroups. The four groups were readable but invented:
a `containers` row is a thing the application made up, and a reader who
descends into it is not finding out what started what. The question
"what is this process, and what started it" has one answer on a Linux
host, and it is the process forest.

What was decided:

- The tree is the process forest of the host, read from `/proc`. A row
  is a process; its children are the processes it started; a row carries
  the totals of its whole subtree (FR-5). Nothing else is a level.
- The cgroup hierarchy is still read, for the one thing only it knows:
  which process belongs to which container, service or login session.
  The walk opens `cgroup.procs` and no other file.
- That belonging is shown on the row, in the `OWNER` column, and the
  filter reaches it (FR-20). This is what the four groups were for, at
  the cost of a word per row instead of a tree.
- `g` is gone, with `src/shape.rs` and both shapes it switched between.
  FR-16 and FR-19 are withdrawn; D-22 is closed, its switch now cycling
  the `OWNER` column.

What this costs, stated rather than hidden:

- Memory over a subtree is a sum of RSS, and RSS counts a shared page in
  full for every process that maps it. A branch of the forest therefore
  reads higher than the memory those processes really hold. A cgroup
  reported `memory.current`, which does not double count; that reading
  is no longer on screen. The check on sums in the oracle covers what
  accumulates - CPU and disk - and leaves memory out for this reason.
- The processes of a container are not neighbours in this tree. A
  container's work hangs under whatever `containerd-shim` or the
  runtime started, scattered among the rest. The `OWNER` column and the
  filter over it are what answer that; the operator accepted the trade
  when it was put to them.

D-25. What stands between the reader and a row. DECIDED 2026-08-15 by
the operator, after the first look at the process forest, in three
instructions: change the navigation, drop every CPU unit but cores, glue
away the levels that hold one element.

- **The way down is `Enter`, the way back is `Backspace`.** Before, the
  way down was `→` and `Enter` opened the card. Two keys for two intents
  read well in the requirement and badly under the hand: going down is
  what the reader does over and over, and it was on the key that is not
  the obvious one. `Enter` on a row with nothing under it opens the card,
  because there is nowhere to go and the detail is what was wanted. `i`
  opens the card on any row at all. `→` still descends, and in the list
  view it still puts a row among its neighbours, which no other key does.
- **CPU is busy cores and nothing else.** The percentage views and the
  `u` switch are gone. FR-1a always said a percentage without a base is
  ambiguous on a large machine; keeping the ambiguous form one keystroke
  away, with a label to make it safe, was carrying the problem rather
  than solving it.
- **A chain of single children is one row.** On the Docker rig a quarter of
  the forest was pass-through: 93 of 368 nodes were the only child of the
  node above them, and
  `supervisor/app/python3/npm exec chrome/sh/chrome-devtools/node`
  was seven levels of one row each. Such a chain is drawn as one row
  named for the whole of it. The figures are the first link's - a row
  already carries its whole subtree (FR-5), so nothing moves and FR-14
  still holds. The row is identified by the first link and its card names
  every link with its pid, so the two are never confused.

  Gluing stops where the owner changes. Of the 93 links, 73 have the same
  owner all the way down; the other 20 cross a boundary - `sudo` into a
  scope, `containerd-shim` into a container - and the `OWNER` column
  would then carry a name that half the row does not belong to. The
  keystroke is what that boundary costs, and it buys a true column.

D-26. What the `OWNER` column shows. DECIDED 2026-08-15 by the operator,
in one instruction: take the switch away, always the name.

D-22 had asked how much of a type column an engineer reads, and the
answer built for it was a switch between three states. D-24 moved that
switch from the withdrawn `TYPE` column to `OWNER`. Using it settled the
question: the name is what the column is for. `grafana` and `ssh` and
`deploy` are what a person is looking for; `container` and `service`
say what the name already says, and an empty column says nothing at all.

- The column is always drawn and always the name. `t` is gone, and so is
  `owner:` from the path line, which only existed to say which of the
  three states was on.
- The kind keeps the two places where it answers something: the card,
  where there is room to say what a row belongs to, and the filter,
  where `container` means "show me everything running in one" - a
  question no name can be typed for.
- Blank is a state of the column, not a missing value. The host row, the
  `(self)` remainder, a kernel thread and a process systemd runs outside
  any unit have nothing that runs them under a name, and the column is
  empty on those rows rather than filled with a word.

A switch is worth building when a question cannot be settled by
argument, and worth removing the moment it has been. This one was open
for the length of a working session and answered by using it, which is
what it was for.

D-27 - D-29 came out of one sitting with the application on 2026-08-15
and are written as three decisions because they touch three different
things. What they have in common is the complaint: each is a piece of
state that was on the screen without being on the screen.

D-27. The bar belongs to the sorting, not to `CORES`. DECIDED
2026-08-15 by the operator. The strip was drawn beside the CPU column
whatever the rows were ordered by, so under `m` the rows stood in one
order and the lengths beside them in another, and the eye followed the
lengths. It now stands beside the column the sorting names and moves
with it - which also makes the longest bar the top row, at every
sorting. Beside `CORES` its colour is still read off the value, because
a busy core is a fixed amount of host; beside the other three it follows
the share of the largest row, because a megabyte is large or small only
next to the rest of the level. Where a narrow terminal has already
dropped the sorted column, the bar goes with it and the cells return to
the name.

D-28. The refresh interval is a key, not a setting. DECIDED 2026-08-15
by the operator. `-` and `+` walk a pause, 1, 2, 3, 5, 10, 30 and 60
seconds, and the interval is written between the two keys that move it.
Three seconds is what the application opens at: a second was chosen when
the tick was the only pace there was, and it renews the screen three
times for every look an engineer takes at it. The pause is the near end
of that list rather than a mode of its own - holding the screen and
renewing it less often are the same act - and `space` still reaches it
in one key. `--tick` now sets where the list is entered rather than a
rate that cannot be changed.

A default that changes reaches further than the screen it changes. The
checks on the live host send a key and read the screen 1.3 seconds
later, which is a tick and a half at one second and half a tick at
three: the capture then returns the frame from before the key, and the
failure reads as a defect in what the key does rather than as a check
arriving early. That cost four runs on the host the day this was
decided.

D-29. The page keys, and a filter that says what it is. DECIDED
2026-08-15 by the operator.

- `PageUp` and `PageDown` did nothing at all. A page is what the table
  holds, and the window moves with the cursor: the next screen is the
  rows that stood after this one, which is what these keys do in every
  pager and every process viewer.
- A filter left no trace of itself: the reader could not see what the
  filter was, where in the row it had matched, or how to be rid of it.
  The filter now stands on the path line with the level and the sorting,
  with what was typed and how many rows it left; the match is marked
  wherever it is drawn - the name, the path in front of it, the owner,
  and the command line under the table; and `Esc` drops it. `Esc` was
  free for that because the way back up a level is `Backspace`, and it
  now undoes the narrowing nearest at hand in one order: the card, then
  the filter, then the level.

D-30. The row a shim stands on says which container is under it. DECIDED
2026-08-29 by the operator, after reading a level of the Docker rig where
twenty rows in a row read `containerd-shim`.

- Measured on the Docker rig the same day: the level under `systemd`
  held 75 rows, and 20 of them were a service process with exactly one
  child whose owner was a container. All 20 containers of the host stood
  behind such a row, and sorted by memory they gather at the top - so the
  first screen of the level was a wall of identical names that answered
  nothing.
- What D-25 decided still stands. The shim belongs to `containerd` and
  the work inside the container does not, so gluing the two into one row
  would put a name in `OWNER` that half the row does not belong to. What
  was missing at that boundary was not gluing but a name.
- The row therefore keeps its own name, its own owner and its own place,
  and says in parentheses which container is under it:
  `containerd-shim (web-frontend)`. The rule is narrow - exactly one
  child, and that child's owner a container other than the row's own.
  Nothing on the Docker rig fell outside it: every one of the 20 cases
  had a single child, and no row had two different containers under it.
- The parenthesised name is part of what the row shows, so the filter
  reaches it and the match is marked there like anywhere else (D-29).
  Filtering by a container name now finds the row that leads into the
  container as well as the processes inside it.
- The list view does not change. Its rows are the ends of the subtree and
  already carry the container as their owner.
- The name is truncated from the right like any other name (section 11),
  so in a column too narrow for both halves what is cut is the
  parenthesised one. The alternative - cutting the process name to keep
  the container - would make two rows of different processes read alike,
  which is the confusion the row's own name is there to prevent.

D-31. On a Kubernetes node the row names the pod. DECIDED 2026-08-29 by
the operator, after the same level on the Kubernetes rig came back with
six rows all reading `containerd-shim`.

- D-30 left this case unnamed on purpose: a row with two containers
  under it named neither, because one row cannot name two. Measured on
  the Kubernetes rig the same day, that is every shim on the host - all 6
  had exactly two children, the pod's `pause` sandbox and the workload
  container, and all 6 had both children inside one pod.
- What the two children have in common is the pod, so that is what the
  row says: `containerd-shim (pod 49ccade5)`. The rule applies where the
  children are containers and all of them sit in the same pod; where they
  sit in different pods the row still names nothing, which is what D-30
  decided and what has not changed.
- The name is the first group of the pod's UUID, read out of the cgroup
  path. It is not a name a person recognises, and it was chosen knowing
  that: on this host no readable name exists at all. Every container
  owner there is a 12-character identifier, because microk8s runs
  containerd and the enrichment of FR-3 speaks only to a Docker socket.
  What the UUID buys is that six identical rows become six different
  ones, and that `kubectl` finds the pod by it.
- Where a row has exactly one container under it, D-30 still decides and
  the row names the container. That rule is the more precise of the two:
  it says which container, and the pod rule only says which pod.
- The pod is read from the cgroup path, which both cgroup drivers write
  in a form that carries it: `/kubepods/burstable/pod<uuid>/<container>`
  from the cgroupfs driver, and `kubepods-burstable-pod<uuid>.slice` with
  underscores for dashes from the systemd one.

D-32. The card is read down a column, not along a line. DECIDED
2026-08-29 by the operator, after looking for the average of one quantity
on a card and having to read every line to find it.

- The four rows of figures wrote both modes as prose - `0.000 cores now
  0.003 avg over 3s` - so the word `avg` began at a different cell on
  every row, because the value before it has a different width on every
  row. Comparing four averages meant reading four lines instead of
  running the eye down one column.
- The modes now stand in fixed columns with one heading above them, `now`
  and `avg over <window>`, written once instead of eight times. What is
  left of the line is a third column that says something about the row -
  the totals since start, the network namespace - and never a figure that
  belongs in the first two.
- Memory was four figures on one line, and two of them - the virtual size
  and the PSS - belong to the process alone while the RSS carries the
  whole subtree. Each is a row of its own now, labelled `own virtual` and
  `own PSS`, so the scope is in the label rather than in a note halfway
  along the line.
- A fact gets a label of its own. `sockets` used to sit inside the value
  of `files`, and the two limits inside the value of `limits`, so the
  reader searched for a word inside a line while everything else was
  found by running down the left edge. One way of reading a card is
  enough.
- The card grows by about five lines. It already says how many lines it
  hid when it does not fit (D-25), so a short terminal loses the tail
  rather than losing it in silence.

D-33. Nothing on the card is cut off the edge, and nothing shares a
label. DECIDED 2026-08-29 by the operator, continuing D-32 into the rest
of the card.

- The rows above the figures packed several facts each: `process` held
  the name, the pid and the parent, and `user` held the user, the thread
  count and the start time. They are identity rather than measurement, so
  they are read once rather than compared - but a card read two ways is
  still read two ways, and the reader who wants the pid should find it
  where every other label is, on the left edge.
- A value too wide for the card was cut with an ellipsis, which loses the
  end of a command line - the part that says which configuration file a
  process was started with. Values wrap instead, onto as many lines as
  they need, with the label written once and the continuation lines
  under it. The card already says how many lines it hid when it does not
  fit (D-25), so what wrapping costs is visible rather than silent.
- The command line is the exception, and it is capped at three lines with
  the cut marked. A command of five hundred characters is six lines on a
  narrow terminal, and the card exists for the figures below it: letting
  one value push them off the screen answers a question nobody asked at
  the price of the one they did.
- A pid is never wrapped. Four digits of a pid name a different process,
  so where the room cannot hold one whole the value goes on a single line
  and the cut is marked, the way it was before wrapping existed.
- The explanations wrap too. The note beside a figure - what PSS counts,
  why the network is attributed to a namespace - stood to the right of
  the two columns and was cut against the border on a narrow terminal.
  It moves under the figures, aligned with the value column, when the
  room beside them is too small for it.

D-34. The card says how much of a process sits in swap. DECIDED
2026-08-29 by the operator, after the memory rows were given their
explanations.

- A host that reads from disk while its memory looks free is a host
  swapping, and the card had no figure for it. `VmSwap` in
  `/proc/<pid>/status` is that figure, and it costs one file read when a
  card opens - the card is drawn for the selected row alone, so this
  never reaches the tick budget of section 6.
- `status` is readable by any user, unlike the `smaps_rollup` the PSS
  comes from. Measured on the Kubernetes rig on 2026-08-29: without root
  `smaps_rollup` answered `Permission denied` while `VmSwap` was there
  for a process of another owner. The row therefore answers in a reduced
  run where the row above it cannot (D-13).
- The row is drawn at zero as well. Nothing in swap is the answer the
  reader opened the card for, and a row that disappears at zero reads as
  a figure that could not be read.
- Only the process rows. A container or a service has
  `memory.swap.current` in its cgroup, which is a second source and a
  second read; it is left for the host that needs it.

D-35. The swap column is drawn only on a host that has swapped
something. DECIDED 2026-08-29 by the operator, after the swap figure
reached the card (D-34).

- The figure is `VmSwap` from `/proc/<pid>/status`, one file per process
  per tick, and a parent row carries the sum of its subtree like every
  other column (FR-5).
- Measured on the Kubernetes rig on 2026-08-29 with a program that makes
  exactly these reads, 221 processes, 21 rounds, median: the `stat` pass
  the collector already makes costs 2.69 ms, the same pass with `status`
  beside it costs 7.22 ms. The column therefore costs 4.5 ms a tick.
  Against the own-CPU requirement of section 6 that is about 0.45 percent
  of one core at the one second interval the measurements use, and a
  third of that at the three second interval the application opens at.
- On a host whose swap device is untouched those 4.5 ms buy a column of
  zeros: none of the 221 processes on that rig had a byte in swap. So the
  machine is asked first - `SwapTotal` and `SwapFree` are read for the
  header anyway - and the per-process reads happen only where the answer
  can be something.
- The column is not sortable. The key map is a settled thing and adding a
  letter to it is its own decision; what the column gives without one is
  which row is in swap, on a screen the reader is already looking at.
- The cgroup route was rejected. `memory.swap.current` costs 0.77 ms for
  the 90 files of that rig and the collector already walks the tree, but
  it names a cgroup rather than a process, so the process rows - which
  are most of the table (D-24) - would stay empty.

## 10. Definition of done

The work is finished when:

- The step from section 8 is done and written down.
- FR-1 - FR-20 are closed with the stated acceptance, except FR-4, which
  D-17 withdrew, and FR-16 and FR-19, which D-24 withdrew. FR-18 was
  added on 2026-08-15 by D-21, FR-19 the same day by D-23, and FR-20 the
  same day by D-24. FR-1a was narrowed by D-25 the same day: cores are
  the only unit. FR-20 was narrowed by D-26, also the same day: the name
  is the only thing the column shows.
- The open decisions D-4, D-11 - D-15 are settled and written down here.
  Done on 2026-08-14, together with D-16, which the measurement of the
  section 6 figures opened and the operator closed the same day, and
  D-17, which removed three features the same day, and D-18, which the
  review of D-17 opened and closed on 2026-08-15, and D-19 - D-21 and
  D-23, D-24, D-25, D-26 and D-27 - D-29, which the operator opened and
  closed on 2026-08-15 after using the application.
- The figures of section 6 are measured and written down together with
  the measurement command. Done in section 6a, on the Docker rig rather
  than on the reference host, which this work had no access to.
- The binary is copied to a clean host and starts with no package
  installation. Done: a static `x86_64-unknown-linux-musl` build, 918 KB,
  copied with `scp` and run on the Docker rig with nothing installed.

Two facts about the numbers on the screen, found during verification and
worth carrying next to the requirements:

- The `(self)` row can be negative in the memory column. The row is the
  figure of the node minus the sum of its children, and RSS counts
  shared pages in full for every process that maps them, so a parent
  that shares its pages with its children subtracts more than it holds.
  The subtraction is honest and the sign is information, so it is shown
  as it comes out.
- The host row is not a sum. Its CPU and memory are what the machine
  reports for itself - `/proc/stat` and `/proc/meminfo` - while tasks,
  disk and network are summed from the processes under it, because only
  a process reports those. The gap between the machine's memory and the
  sum of the processes is exactly what the `(self)` row of the host
  carries, and on any real host it is large: the page cache is in it.

## 11. State of the documents

- Requirements - this file. Decisions live in section 9, their evidence
  base in sections 8, 8a and 8b.
- Verification procedure - [testing.md](testing.md): the two rigs, the
  feedback loop through `tmux`, the frame invariants, the
  induced states and the mapping of every FR to the way it is checked.
  Its mechanics were verified on the server on 2026-08-14, and the hooks
  of FR-17 are its requirement towards the code.
- Screen mockup - removed on 2026-08-21. It was a working TUI mockup in
  a browser, and the interface was designed against it before the
  application existed: the layout defects of 2026-08-14 were found by
  walking its keys, and its note blocks carried the analysis behind
  D-11 - D-13. What it drew stopped being true when D-24 made the tree
  the process forest, so it showed the cgroup tree, a `TYPE` column and
  the keys of before D-25, and a reader who opened it saw an interface
  the application no longer has. What it fixed as mandatory is the list
  below, which stands on its own.
- README - what the application shows and how to run it. The description
  of the screen there is not written by hand: it is what `hostscope
  --help` prints, copied in by `make readme` and held identical by a test
  (`tests/documents.rs`). The help text is the one place that describes
  the interface, so the two cannot drift apart and leave a reader
  trusting the older of them.
- The code is under git.

### Mandatory rules of the screen

- The column set and order are the same on every level: name, owner,
  tasks, cores, memory, swap, disk read/write, network down/up. Changing
  the level does not change the columns - otherwise the eye has to find
  the column again every time.
- Swap is the one column the host decides about. It is drawn only where
  the machine has moved something out of RAM, and it is then the same on
  every level like the rest; where the swap device is untouched the
  column is not drawn at all and the cells go to the name (D-35).
- The bar is not a column of its own but a strip beside the column the
  rows are ordered by, and it moves with the sorting (D-27).
- The details of the selected row go on a separate line under the table,
  not into an extra column. A container image takes a third of the width
  and is needed for one row only.
- A node with children is marked in its name, a node without children is
  not. The reader must see where drilling makes sense before pressing a
  key.
- A row whose single child belongs to a container names that container in
  parentheses after its own name and keeps its own owner. The shim that
  starts a container is a real process of the runtime; what the reader is
  looking for is what it leads to (D-30).
- A row whose children are several containers of one pod names the pod
  there instead, by the first group of its UUID (D-31). Where they belong
  to different pods the row names nothing: one row cannot name two.
- A non-zero value draws at least one tick of the bar. An empty cell
  reads as zero, and 0 and 0.004 cores are different things.
- A name longer than its column is truncated with an ellipsis, the path
  line is truncated from the left. Columns never merge, whatever the
  name.
- On the card a value too wide for the room wraps onto the next line
  under the same label, rather than being cut. The command line wraps at
  most three times and marks the cut, so that one value cannot push the
  figures off the screen (D-33).
- The card of any row carries CPU, memory, disk and network in both
  modes at once, in two fixed columns under one heading. The gap between
  instant and average is the diagnosis: a spike shows as a divergence, a
  steady load as a match. Every figure has a label of its own on the left
  edge, and no line holds a second labelled value (D-32).
- The card of a process carries `own swap` - what the kernel has moved
  out of RAM for it - and carries it at zero as well (D-34).
- Every memory row on the card says what its number counts. RSS, virtual
  and PSS stand under one another and measure three different things, so
  a reader who takes them for one number reads the card wrong: RSS counts
  a shared page in full for every process that maps it, virtual is
  address space rather than memory held, and PSS divides a shared page
  among those that map it. The swap row under them says the pages are
  out of RAM altogether (D-32).
- On the card a value too wide for the room wraps under its own label,
  and the cgroup path wraps with the rest: a path with its tail cut off
  names no unit `systemctl` would answer about (D-33).
- The card prints only what this node actually has. The host row has no
  command line and no container, and filling the screen with empty
  labels is not allowed.
- There is no virtual memory in the table, only in the process card: for
  `containerd` on the reference host it is 6097M against 33M resident.
- A card that does not fit says how many lines it hid. The card does not
  scroll, so on a short terminal something has to go; what may not happen
  is that it goes in silence. Found on 2026-08-15: a glued chain of
  twenty links took every line of a card 18 rows tall, and the reader saw
  a full screen with no sign that the figures had been cut off it.
- A pid the card cannot hold whole is marked as cut, like any other value
  that will not fit. A pid is the one value where a silent cut names a
  different process rather than shortening a word: 4194303, the largest
  an ordinary `pid_max` hands out, cut to `4194` reads as a process that
  exists on the same host. Found on 2026-08-15: the card reserved a fixed
  number of cells for the pid, which is a reserve only while the terminal
  is wide enough to hold one - below twenty cells plus the digits, the
  pid ran past its room and was cut without a mark.
