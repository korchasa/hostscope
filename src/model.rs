//! The tree the whole application works over: the process forest of the host,
//! one node per process, with the same set of values everywhere.
//!
//! There is no grouping in it. What runs a process - a container, a service, a
//! login session - is written on the row as its [`Owner`] rather than made
//! into a level of its own (FR-20).

use crate::util::{clean, mem_str};

/// What the node is. Since the tree is the process forest of the host, there
/// are only three: the host at the root, a process, and the computed remainder
/// of a process. What a process belongs to - a container, a service, a user -
/// is not a kind of row but a property of one; see [`Owner`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Host,
    Process,
    /// The computed remainder of a node (FR-14).
    Own,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Host => "host",
            Kind::Process => "process",
            Kind::Own => "self",
        }
    }
}

/// What a process belongs to. Read from the cgroup the kernel put the process
/// in, and shown in a column of its own: the process tree scatters the
/// processes of one container across the branches of the shims that started
/// them, and without the label a row does not say whose work it is (FR-20).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnerKind {
    Container,
    Service,
    User,
    /// A kernel thread: a descendant of `kthreadd`, which belongs to no
    /// container, service or user and is not a program on disk at all.
    Kernel,
    /// The host itself: the init scope, the mounts, everything systemd runs
    /// outside a unit.
    System,
}

impl OwnerKind {
    pub fn label(self) -> &'static str {
        match self {
            OwnerKind::Container => "container",
            OwnerKind::Service => "service",
            OwnerKind::User => "user",
            OwnerKind::Kernel => "kernel",
            OwnerKind::System => "system",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Owner {
    pub kind: OwnerKind,
    /// The name a person knows it by: the container name from the daemon, the
    /// unit without its suffix, the login name. Empty where the owner has no
    /// name of its own, which is the host and the kernel.
    pub name: String,
}

/// The seven values every row carries. Rates are per second; `mem` and `tasks`
/// are gauges. Every one of them is optional, because every one of them can be
/// genuinely unavailable: a controller that is not enabled for the node leaves
/// no file to read, `/proc/<pid>/io` needs root, and there is no per-process
/// network counter at all - traffic is only attributable where a process holds
/// a network namespace of its own. FR-8 says such a field is marked, not
/// replaced with a zero - a zero is a statement about the host, and it would be
/// a false one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    /// What the kernel has moved out of RAM, summed over the subtree. Read
    /// only on a host that has swapped something (D-35).
    pub swap: Option<f64>,
    pub tasks: Option<f64>,
    pub rd: Option<f64>,
    pub wr: Option<f64>,
    pub rx: Option<f64>,
    pub tx: Option<f64>,
}

impl Metrics {
    /// Adds a child into a total. A value the child cannot report leaves the
    /// total as it is: a partial sum of what is known beats refusing to add.
    pub fn add(&mut self, o: &Metrics) {
        self.cpu = opt_add(self.cpu, o.cpu);
        self.mem = opt_add(self.mem, o.mem);
        self.swap = opt_add(self.swap, o.swap);
        self.tasks = opt_add(self.tasks, o.tasks);
        self.rd = opt_add(self.rd, o.rd);
        self.wr = opt_add(self.wr, o.wr);
        self.rx = opt_add(self.rx, o.rx);
        self.tx = opt_add(self.tx, o.tx);
    }

    pub fn sub(&mut self, o: &Metrics) {
        self.cpu = opt_sub(self.cpu, o.cpu);
        self.mem = opt_sub(self.mem, o.mem);
        self.swap = opt_sub(self.swap, o.swap);
        self.tasks = opt_sub(self.tasks, o.tasks);
        self.rd = opt_sub(self.rd, o.rd);
        self.wr = opt_sub(self.wr, o.wr);
        self.rx = opt_sub(self.rx, o.rx);
        self.tx = opt_sub(self.tx, o.tx);
    }

    /// Rounds away the residue of adding fractions. A remainder that is
    /// genuinely negative is kept: memory is RSS, and a page two processes
    /// share is counted in full for each of them, so the sum of a branch can
    /// stand above the figure of the node above it. Hiding that would break
    /// the equality FR-14 states and hide a real property of the reading.
    pub fn clamp_small(&mut self) {
        for v in [
            &mut self.cpu,
            &mut self.mem,
            &mut self.swap,
            &mut self.tasks,
            &mut self.rd,
            &mut self.wr,
            &mut self.rx,
            &mut self.tx,
        ]
        .into_iter()
        .flatten()
        {
            if v.abs() < 0.0005 {
                *v = 0.0;
            }
        }
    }

    pub fn any_nonzero(&self) -> bool {
        [
            self.cpu, self.mem, self.swap, self.tasks, self.rd, self.wr, self.rx, self.tx,
        ]
        .iter()
        .any(|v| v.unwrap_or(0.0) != 0.0)
    }

    pub fn net_total(&self) -> f64 {
        self.rx.unwrap_or(0.0) + self.tx.unwrap_or(0.0)
    }

    pub fn disk_total(&self) -> f64 {
        self.rd.unwrap_or(0.0) + self.wr.unwrap_or(0.0)
    }

    pub fn cpu_or_zero(&self) -> f64 {
        self.cpu.unwrap_or(0.0)
    }
}

fn opt_add(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// A remainder is only known when both sides are. If the children cannot
/// report the value at all, how the parent's figure splits between them is
/// unknown, and an unknown is marked rather than charged to `(self)` (FR-8).
fn opt_sub(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a - b),
        _ => None,
    }
}

/// How a figure reads against the machine it was taken on. Three steps only:
/// the eye has to sort a screen of rows in a second, and a scale with more
/// steps than that is read by counting rather than at a glance (D-42).
/// The order of the variants is the order of severity, and `derive` turns it
/// into the comparison the row flag folds over.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Reading {
    #[default]
    Calm,
    Unusual,
    Alarm,
}

impl Reading {
    pub fn label(self) -> &'static str {
        match self {
            Reading::Calm => "calm",
            Reading::Unusual => "unusual",
            Reading::Alarm => "alarm",
        }
    }
}

/// What the leading cell of the name column says about a row: nothing, that
/// this row is where the reading comes from, or that the reading only passes
/// through it on its way up from a child (D-44).
///
/// Every figure on a row is the sum of its subtree (FR-5), so one busy process
/// paints every row above it up to the root. Without this distinction the mark
/// on pid 1 is a tautology - the whole machine is in its subtree - and the
/// reader who follows it finds a `(self)` remainder of nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Calm,
    Own(Reading),
    Below(Reading),
}

impl Mark {
    /// The reading behind the mark, whichever kind it is.
    pub fn reading(self) -> Reading {
        match self {
            Mark::Calm => Reading::Calm,
            Mark::Own(r) | Mark::Below(r) => r,
        }
    }
}

/// What the machine can do, so a figure can be read as a share of it. Every
/// denominator here is read from the host itself, never guessed: a threshold
/// against an invented whole is a decoration, not a reading (D-42).
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub cores: f64,
    pub mem_total: f64,
    pub swap_total: f64,
    /// `/proc/sys/kernel/pid_max`. Absent where the file cannot be read, and
    /// then the task count carries no reading at all.
    pub pid_max: Option<f64>,
    /// The summed speed of the physical links, in bytes per second. Absent
    /// where no interface reports one - a virtual machine often reports
    /// nothing - and then the network carries no reading.
    pub link_speed: Option<f64>,
}

/// One reading per figure a row shows, plus the row's own. Disk has no field:
/// nothing readable says what the device underneath can do, so any threshold
/// on it would be a number this project invented (D-42).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Readings {
    pub cpu: Reading,
    pub mem: Reading,
    pub swap: Reading,
    pub tasks: Reading,
    pub rx: Reading,
    pub tx: Reading,
    /// The worst of the figures above, and of what the kernel says the process
    /// is doing. This is what marks the row itself.
    pub flag: Reading,
}

/// One heuristic that fired, with the sentence that says why. The name is the
/// label of a card line and fits the sixteen cells every other label there has
/// (D-32); the reason names the figure, the whole it was read against and the
/// threshold it crossed, so the reader can disagree with the tool rather than
/// only obey it (D-43).
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// The label of the card row the figure stands on, so the reader can find
    /// the number this sentence is about. A name nobody can look up is what
    /// made the first version of this block unreadable on a live host.
    pub name: &'static str,
    /// How the figure reads in each of the two columns the card prints, and
    /// the worse of the two, which is what colours the name.
    pub now: Reading,
    pub avg: Reading,
    pub reading: Reading,
    pub why: String,
}

impl Readings {
    /// Reads one row. `state` is field 3 of the process `stat`; `is_host` is
    /// true for the root, which is the only row whose network can be compared
    /// against the link of the machine - every row below it counts bytes
    /// inside its own namespace (D-42).
    pub fn of(m: &Metrics, l: &Limits, state: char, is_host: bool) -> Readings {
        Readings::read(m, l, state, is_host).0
    }

    /// The heuristics that fired on this row, in the order the card prints
    /// them, read over BOTH columns the card shows. A heuristic that fired in
    /// either of them is a reason, and the sentence names the figure of each,
    /// so the reader can find the number on the card instead of hunting for it
    /// (operator, 2026-08-30). Empty where everything reads calm, which is what
    /// makes the block disappear from a card with nothing to explain (D-43).
    pub fn findings(
        now: &Metrics,
        avg: &Metrics,
        l: &Limits,
        state: char,
        is_host: bool,
    ) -> Vec<Finding> {
        let a = Readings::read(now, l, state, is_host).1;
        let b = Readings::read(avg, l, state, is_host).1;
        let mut names: Vec<&'static str> = Vec::new();
        for r in a.iter().chain(b.iter()) {
            if !names.contains(&r.name) {
                names.push(r.name);
            }
        }
        // The order is the order of the card rows these names point at, so a
        // reader running down the block finds each name again further down the
        // card.
        names.sort_by_key(|n| ORDER.iter().position(|o| o == n).unwrap_or(ORDER.len()));

        names
            .into_iter()
            .map(|name| {
                let x = a.iter().find(|r| r.name == name);
                let y = b.iter().find(|r| r.name == name);
                let now_r = x.map(|r| r.reading).unwrap_or(Reading::Calm);
                let avg_r = y.map(|r| r.reading).unwrap_or(Reading::Calm);
                // The rule explained is the one that marks the row, so a spike
                // that reads alarm in one column is explained as an alarm.
                let worst = x.into_iter().chain(y).max_by_key(|r| r.reading).unwrap();
                let why = match &worst.text {
                    // A fact the kernel reports has no figure to put in two
                    // columns; the sentence is the whole of it.
                    None => worst.rule.clone(),
                    Some(_) => {
                        let a_text = x.and_then(|r| r.text.clone()).unwrap_or("-".into());
                        let b_text = y.and_then(|r| r.text.clone()).unwrap_or("-".into());
                        // A steady figure said twice is the same number twice,
                        // and it pushed the sentence onto a second line.
                        let figures = if a_text == b_text {
                            format!("{a_text} now and avg")
                        } else {
                            format!("{a_text} now, {b_text} avg")
                        };
                        format!("{figures}{}; {}", worst.scope, worst.rule)
                    }
                };
                Finding {
                    name,
                    now: now_r,
                    avg: avg_r,
                    reading: now_r.max(avg_r),
                    why,
                }
            })
            .collect()
    }

    /// The reading and its reasons come out of one function, so a threshold
    /// cannot move in the figure and stay in the sentence - which is the way a
    /// card goes stale without anybody noticing (D-43).
    fn read(m: &Metrics, l: &Limits, state: char, is_host: bool) -> (Readings, Vec<Raw>) {
        let mut found = Vec::new();

        // Two of the letters the kernel writes are readings a figure cannot
        // give: a zombie holds a slot its parent is not reaping, and an
        // uninterruptible wait is a process stuck inside the kernel. They come
        // first, because they are facts rather than comparisons.
        let from_state = match state {
            'Z' => {
                found.push(fact(
                    "zombie",
                    Reading::Alarm,
                    "the process is dead and the parent has not reaped it, \
                     so it still holds a slot in the process table",
                ));
                Reading::Alarm
            }
            'D' => {
                found.push(fact(
                    "stuck in kernel",
                    Reading::Unusual,
                    "in an uninterruptible wait inside the kernel, where a signal \
                     cannot reach it - usually storage or a network filesystem \
                     that is not answering",
                ));
                Reading::Unusual
            }
            _ => Reading::Calm,
        };

        // The host row is a whole machine, not a process on it. Two busy cores
        // out of four is half a machine and an ordinary afternoon; on a process
        // the same figure is what this reading exists to catch. So the root
        // takes the thresholds of the summary line and every row below it takes
        // the row rules (D-42).
        let cores = &|v: f64| format!("{v:.3} cores");
        let (cpu, mem, swap) = if is_host {
            (
                share_of(&mut found, "cpu", "", m.cpu, l.cores, 0.90, 0.75, cores),
                share_of(
                    &mut found,
                    "memory",
                    "",
                    m.mem,
                    l.mem_total,
                    0.90,
                    0.75,
                    &mem_str,
                ),
                share_of(
                    &mut found,
                    "swap",
                    "",
                    m.swap,
                    l.swap_total,
                    0.50,
                    0.10,
                    &mem_str,
                ),
            )
        } else {
            (
                // The same two steps the bar has always drawn on the CPU
                // column, so a busy core reads the same whichever way the
                // screen is sorted.
                above(&mut found, "cpu", m.cpu, 1.0, 0.1),
                // "with children", because the card row beside it is `memory
                // RSS`, which is the subtree too - and RSS counts a shared page
                // in full for every process that maps it, so this share can
                // stand above what the machine reports for itself.
                share_of(
                    &mut found,
                    "memory",
                    ", with children",
                    m.mem,
                    l.mem_total,
                    0.25,
                    0.10,
                    &mem_str,
                ),
                // Swap holds one step on a row: what matters there is that this
                // process is the one being swapped out, not how far the device
                // has filled. It is the swap of the whole subtree, and the card
                // row `own swap` beside it is not - so the sentence says which.
                share_of(
                    &mut found,
                    "swap",
                    ", over the subtree",
                    m.swap,
                    l.swap_total,
                    f64::INFINITY,
                    0.10,
                    &mem_str,
                ),
            )
        };
        let tasks = share_of(
            &mut found,
            "tasks",
            "",
            m.tasks,
            l.pid_max.unwrap_or(0.0),
            f64::INFINITY,
            0.10,
            &|v| format!("{v:.0}"),
        );
        // Only the host row can be compared against the link of the machine
        // (D-42), and one card row holds both directions, so both take its
        // label.
        let link = if is_host { l.link_speed } else { None };
        let rate = &|v: f64| format!("{}/s", crate::util::bytes_str(v));
        let rx = share_of(
            &mut found,
            "net",
            ", down",
            m.rx,
            link.unwrap_or(0.0),
            0.80,
            0.50,
            rate,
        );
        let tx = share_of(
            &mut found,
            "net",
            ", up",
            m.tx,
            link.unwrap_or(0.0),
            0.80,
            0.50,
            rate,
        );

        let r = Readings {
            cpu,
            mem,
            swap,
            tasks,
            rx,
            tx,
            flag: [cpu, mem, swap, tasks, rx, tx]
                .into_iter()
                .fold(from_state, Reading::max),
        };
        (r, found)
    }
}

/// The order the card prints its rows in, which is the order the reasons take:
/// a reader running down the block finds each name again further down the card.
const ORDER: [&str; 7] = [
    "zombie",
    "stuck in kernel",
    "cpu",
    "memory",
    "swap",
    "tasks",
    "net",
];

/// One heuristic as a single column read it: the figure it saw, the scope that
/// figure covers and the rule it crossed. The sentence is built from a pair of
/// these, because the card shows two columns and a reason that named one of
/// them without saying which sends the reader looking for a number that is in
/// neither (operator, 2026-08-30).
struct Raw {
    name: &'static str,
    reading: Reading,
    /// The figure as the card writes it, or `None` for a fact the kernel
    /// reports, which has no figure at all.
    text: Option<String>,
    scope: &'static str,
    rule: String,
}

fn fact(name: &'static str, reading: Reading, rule: &str) -> Raw {
    Raw {
        name,
        reading,
        text: None,
        scope: "",
        rule: rule.into(),
    }
}

/// A figure read as a share of a measured whole, with the sentence appended to
/// `found` where it fired. A whole of zero means the machine did not report
/// one, and then there is no reading at all rather than a division (D-42).
#[allow(clippy::too_many_arguments)]
fn share_of(
    found: &mut Vec<Raw>,
    name: &'static str,
    scope: &'static str,
    v: Option<f64>,
    whole: f64,
    alarm: f64,
    unusual: f64,
    show: &dyn Fn(f64) -> String,
) -> Reading {
    let reading = share(v, whole, alarm, unusual);
    if reading != Reading::Calm {
        let v = v.unwrap_or(0.0);
        let step = if reading == Reading::Alarm {
            alarm
        } else {
            unusual
        };
        found.push(Raw {
            name,
            reading,
            text: Some(format!("{} ({})", show(v), pct(v / whole))),
            scope,
            rule: format!(
                "{} starts at {} of {}",
                reading.label(),
                pct(step),
                show(whole)
            ),
        });
    }
    reading
}

/// A figure read against a threshold it stands above, rather than as a share.
/// Busy cores on a process row are the one such figure: a core is a core, and
/// what a row holds says nothing about how many the machine has (D-42).
fn above(
    found: &mut Vec<Raw>,
    name: &'static str,
    v: Option<f64>,
    alarm: f64,
    unusual: f64,
) -> Reading {
    let reading = match v {
        Some(v) if v > alarm => Reading::Alarm,
        Some(v) if v > unusual => Reading::Unusual,
        _ => Reading::Calm,
    };
    if reading != Reading::Calm {
        let step = if reading == Reading::Alarm {
            alarm
        } else {
            unusual
        };
        found.push(Raw {
            name,
            reading,
            text: Some(format!("{:.3} cores", v.unwrap_or(0.0))),
            scope: "",
            rule: format!("{} starts above {step:.3} cores", reading.label()),
        });
    }
    reading
}

fn pct(f: f64) -> String {
    format!("{:.0}%", f * 100.0)
}

/// How the machine's own summary line reads. Separate from [`Readings`]
/// because these three figures are the machine's, not a row's: the load and
/// the used memory the kernel reports for the whole host (D-42).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HostReadings {
    pub load: Reading,
    pub mem: Reading,
    pub swap: Reading,
}

impl HostReadings {
    pub fn of(load1: f64, mem_used: f64, swap_used: f64, l: &Limits) -> HostReadings {
        HostReadings {
            // Load is a queue length, so it is read per core: on a machine of
            // four, a load of four is a full machine and not a fault.
            load: share(Some(load1), l.cores, 2.0, 1.0),
            mem: share(Some(mem_used), l.mem_total, 0.90, 0.75),
            // Not "any swap at all": on a host that has swapped once, that
            // fires forever and tells the reader nothing.
            swap: share(Some(swap_used), l.swap_total, 0.50, 0.10),
        }
    }
}

/// A figure as a share of a whole. A whole of zero means the machine did not
/// report one, and then there is no reading at all rather than a division.
fn share(v: Option<f64>, whole: f64, alarm: f64, unusual: f64) -> Reading {
    match v {
        Some(v) if whole > 0.0 => {
            let f = v / whole;
            if f >= alarm {
                Reading::Alarm
            } else if f >= unusual {
                Reading::Unusual
            } else {
                Reading::Calm
            }
        }
        _ => Reading::Calm,
    }
}

/// What a container's cgroup node is enriched with (FR-3). Absent when the
/// Docker socket cannot be reached - then the row degrades to the short
/// identifier and carries the unavailability marker (D-13).
#[derive(Clone, Debug, Default)]
pub struct ContainerInfo {
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub created: String,
    pub restarts: Option<u64>,
    pub labels: Vec<(String, String)>,
    pub ports: Vec<String>,
}

/// Everything a card may print, filled only where it exists.
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub cgroup_path: Option<String>,
    pub short_id: Option<String>,
    pub pid: Option<i32>,
    pub ppid: Option<i32>,
    pub user: Option<String>,
    pub cmdline: Option<String>,
    pub threads: Option<u64>,
    pub started: Option<String>,
    pub container: Option<ContainerInfo>,
    /// Field 3 of `/proc/<pid>/stat`: what the kernel says the process is
    /// doing. Two of the letters are readings a figure cannot give (D-42).
    /// Absent on the host row and on a `(self)` remainder, neither of which is
    /// a process.
    pub state: Option<char>,
    /// What runs this process, read from its cgroup (FR-20).
    pub owner: Option<Owner>,
    /// The chain of processes this row stands for, in order down it: each link
    /// was the only child of the one above it (D-25). The first link is in the
    /// list too, because the card has to name every one of them with its own
    /// pid and its own name, and once a chain is glued the row's `name` is the
    /// whole of it. Empty where nothing was glued, which is the ordinary row.
    pub glued: Vec<(i32, String)>,
    /// The container this row leads into, when the row itself belongs to
    /// something else: a runtime shim keeps `containerd` in its `OWNER` column
    /// and its whole work is one level down, inside the container (D-30). Set
    /// only where the row has exactly one child and that child's owner is a
    /// container other than the row's own - one row cannot name two. It is not
    /// part of `name`: gluing builds `name` from the links it joins, and a
    /// name that already carried the container would be repeated into every
    /// row glued above it.
    pub leads_into: Option<String>,
    pub own_netns: bool,
    pub io_total: Option<(f64, f64)>,
    pub vsz: Option<f64>,
    pub child_count: usize,
    /// Fields that root would have provided and this run could not read.
    pub restricted: Vec<&'static str>,
}

/// A node of the tree. `name` is already cleaned of control characters, so
/// truncation, search and the width check all operate on what is drawn.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub instant: Metrics,
    pub avg: Metrics,
    pub detail: Detail,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(id: impl Into<String>, name: &str, kind: Kind) -> Node {
        Node {
            id: id.into(),
            name: clean(name),
            kind,
            instant: Metrics::default(),
            avg: Metrics::default(),
            detail: Detail::default(),
            children: Vec::new(),
        }
    }

    /// How this row's figures read against the machine (D-42). Computed here
    /// rather than in the renderer so the table, the card and the dump read
    /// one value and cannot disagree, and against the figures of `mode`, so
    /// switching to the average switches the colours with it.
    pub fn readings(&self, limits: &Limits, mode: Mode) -> Readings {
        Readings::of(
            self.metrics(mode),
            limits,
            self.detail.state.unwrap_or('?'),
            self.kind == Kind::Host,
        )
    }

    /// What the leading cell of this row says (D-44). The row is the source
    /// when the reading survives on its own contribution - the figures minus
    /// the sum of the children, which is the `(self)` remainder FR-14 already
    /// computes - or when there are no children to have carried it up. A state
    /// the kernel reports is always the row's own.
    pub fn mark(&self, limits: &Limits, mode: Mode) -> Mark {
        let whole = self.readings(limits, mode).flag;
        if whole == Reading::Calm {
            return Mark::Calm;
        }
        if self.children.is_empty() {
            return Mark::Own(whole);
        }
        let mut own = *self.metrics(mode);
        for c in &self.children {
            own.sub(c.metrics(mode));
        }
        own.clamp_small();
        let alone = Readings::of(
            &own,
            limits,
            self.detail.state.unwrap_or('?'),
            self.kind == Kind::Host,
        )
        .flag;
        if alone == Reading::Calm {
            Mark::Below(whole)
        } else {
            Mark::Own(whole)
        }
    }

    pub fn metrics(&self, mode: Mode) -> &Metrics {
        match mode {
            Mode::Instant => &self.instant,
            Mode::Average => &self.avg,
        }
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// A copy of the node without its subtree. A drawn row shows itself and
    /// never its children, and the list view holds one row per node of the
    /// whole subtree: cloning the children into every one of them would cost
    /// the square of the tree on each frame.
    pub fn shallow(&self) -> Node {
        Node {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            instant: self.instant,
            avg: self.avg,
            detail: self.detail.clone(),
            children: Vec::new(),
        }
    }

    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.id == id {
            return Some(self);
        }
        for c in &self.children {
            if let Some(n) = c.find(id) {
                return Some(n);
            }
        }
        None
    }

    /// The node reached by walking the ids of the path, stopping at the last
    /// one that still exists. A vanished node must not throw the view away.
    pub fn at_path<'a>(&'a self, path: &[String]) -> &'a Node {
        let mut node = self;
        for step in path {
            match node.children.iter().find(|c| &c.id == step) {
                Some(next) => node = next,
                None => break,
            }
        }
        node
    }

    /// How many steps of the path still resolve. The view trims itself to this
    /// when a container or a process disappears under it.
    pub fn resolved_depth(&self, path: &[String]) -> usize {
        let mut node = self;
        let mut depth = 0;
        for step in path {
            match node.children.iter().find(|c| &c.id == step) {
                Some(next) => {
                    node = next;
                    depth += 1;
                }
                None => break,
            }
        }
        depth
    }
}

/// Instant values over the last interval, or averages since the application
/// started (FR-13). The mode applies to every number on screen at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Instant,
    Average,
}

/// The host summary drawn in the header. Its network comes from the interface
/// counters, so it exceeds the tree total whenever traffic cannot be
/// attributed.
#[derive(Clone, Debug, Default)]
pub struct HostSummary {
    pub hostname: String,
    pub kernel: String,
    pub uptime_secs: f64,
    pub cores: f64,
    pub busy_cores: f64,
    pub busy_cores_avg: f64,
    pub mem_used: f64,
    pub mem_used_avg: f64,
    pub mem_total: f64,
    pub swap_used: f64,
    pub swap_used_avg: f64,
    pub swap_total: f64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub net_rx_avg: f64,
    pub net_tx_avg: f64,
    pub load: [f64; 3],
    pub history: Vec<f64>,
}

/// One collected tick: the tree, the host summary and the two windows the
/// values were taken over.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub root: Node,
    pub host: HostSummary,
    /// What this machine can do, so every figure on screen can be read as a
    /// share of it (D-42).
    pub limits: Limits,
    pub interval: f64,
    pub window: f64,
    pub ticks: u64,
    pub root_privileges: bool,
    pub docker_available: bool,
    pub ebpf: bool,
}

impl Snapshot {
    #[allow(dead_code)] // the tests build trees from an empty snapshot
    pub fn empty() -> Snapshot {
        Snapshot {
            root: Node::new("/", "host", Kind::Host),
            host: HostSummary::default(),
            limits: Limits::default(),
            interval: 0.0,
            window: 0.0,
            ticks: 0,
            root_privileges: false,
            docker_available: false,
            ebpf: false,
        }
    }
}

/// The resource rows are ordered by (FR-6). The order applies to the values of
/// the current mode, so it differs between modes - that is the point of the
/// mode, not a defect (FR-13).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Cpu,
    Mem,
    Disk,
    Net,
}

impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Sort::Cpu => "cores",
            Sort::Mem => "memory",
            Sort::Disk => "disk",
            Sort::Net => "net",
        }
    }

    pub fn key(self, m: &Metrics) -> f64 {
        match self {
            Sort::Cpu => m.cpu_or_zero(),
            Sort::Mem => m.mem.unwrap_or(0.0),
            Sort::Disk => m.disk_total(),
            Sort::Net => m.net_total(),
        }
    }
}

/// The `(self)` row: the node value minus the sum of its direct children, per
/// value and in both modes (FR-14). Computed here rather than collected, so the
/// equality "children plus (self) equals the parent" holds by construction.
pub fn self_row(node: &Node) -> Option<Node> {
    if node.children.is_empty() {
        return None;
    }
    let mut inst = node.instant;
    let mut avg = node.avg;
    for c in &node.children {
        inst.sub(&c.instant);
        avg.sub(&c.avg);
    }
    inst.clamp_small();
    avg.clamp_small();
    if !inst.any_nonzero() && !avg.any_nonzero() {
        return None;
    }
    let mut row = Node::new(format!("{}#self", node.id), "(self)", Kind::Own);
    row.instant = inst;
    row.avg = avg;
    row.detail.child_count = node.children.len();
    row.detail.cgroup_path = node.detail.cgroup_path.clone();
    Some(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, cpu: f64, mem: f64) -> Node {
        let mut n = Node::new(id, id, Kind::Process);
        n.instant = Metrics {
            cpu: Some(cpu),
            mem: Some(mem),
            ..Metrics::default()
        };
        n.avg = n.instant;
        n
    }

    #[test]
    fn self_row_is_the_remainder() {
        let mut parent = node("p", 1.0, 100.0);
        parent.children.push(node("a", 0.4, 30.0));
        parent.children.push(node("b", 0.2, 20.0));
        let s = self_row(&parent).unwrap();
        assert!((s.instant.cpu.unwrap() - 0.4).abs() < 1e-9);
        assert!((s.instant.mem.unwrap() - 50.0).abs() < 1e-9);
    }

    /// Swap is subtracted like every other value. Seen on a third host on
    /// 2026-08-29: the host row said 8.0G of swap and the branch under it
    /// summed to 8.2G, and the `(self)` row repeated the 8.0G instead of the
    /// remainder - it was left out of the subtraction.
    #[test]
    fn self_row_takes_the_swap_of_its_children_off_too() {
        let mut parent = node("p", 1.0, 100.0);
        parent.instant.swap = Some(8000.0);
        parent.avg.swap = Some(8000.0);
        let mut child = node("a", 0.4, 30.0);
        child.instant.swap = Some(8200.0);
        child.avg.swap = Some(8200.0);
        parent.children.push(child);
        let s = self_row(&parent).unwrap();
        assert!(
            (s.instant.swap.unwrap() + 200.0).abs() < 1e-9,
            "the swap remainder is {:?}",
            s.instant.swap
        );
    }

    #[test]
    fn self_row_absent_when_children_cover_the_parent() {
        let mut parent = node("p", 0.6, 50.0);
        parent.children.push(node("a", 0.4, 30.0));
        parent.children.push(node("b", 0.2, 20.0));
        assert!(self_row(&parent).is_none());
    }

    #[test]
    fn children_plus_self_equal_the_parent() {
        let mut parent = node("p", 1.0, 100.0);
        parent.children.push(node("a", 0.4, 30.0));
        let s = self_row(&parent).unwrap();
        let total = parent.children[0].instant.cpu.unwrap() + s.instant.cpu.unwrap();
        assert!((total - parent.instant.cpu.unwrap()).abs() < 1e-9);
    }

    #[test]
    fn a_name_keeps_its_letters_and_loses_its_control_characters() {
        let n = Node::new("x", "hs-\u{0438}\u{043C}\u{044F}", Kind::Process);
        assert_eq!(n.name, "hs-\u{0438}\u{043C}\u{044F}");
        let n = Node::new("y", "hs-\u{1B}[2Jname", Kind::Process);
        assert_eq!(n.name, "hs- [2Jname");
    }

    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    fn a_machine() -> Limits {
        // Four cores, 8 GB of RAM, a 2 GB swap device, a gigabit link.
        Limits {
            cores: 4.0,
            mem_total: 8.0 * GIB,
            swap_total: 2.0 * GIB,
            pid_max: Some(32768.0),
            link_speed: Some(125_000_000.0),
        }
    }

    #[test]
    fn a_figure_is_read_against_the_machine_and_not_against_its_neighbours() {
        let l = a_machine();
        let m = |cpu: f64, mem: f64| Metrics {
            cpu: Some(cpu),
            mem: Some(mem),
            ..Metrics::default()
        };
        // The CPU rule is the one the bar already applies, strictly greater.
        assert_eq!(
            Readings::of(&m(1.35, 0.0), &l, 'S', false).cpu,
            Reading::Alarm
        );
        assert_eq!(
            Readings::of(&m(1.0, 0.0), &l, 'S', false).cpu,
            Reading::Unusual
        );
        assert_eq!(
            Readings::of(&m(0.06, 0.0), &l, 'S', false).cpu,
            Reading::Calm
        );
        // Memory is a share of the RAM this machine has.
        assert_eq!(
            Readings::of(&m(0.0, 4.6 * GIB), &l, 'S', false).mem,
            Reading::Alarm
        );
        assert_eq!(
            Readings::of(&m(0.0, 1.2 * GIB), &l, 'S', false).mem,
            Reading::Unusual
        );
        assert_eq!(
            Readings::of(&m(0.0, 0.4 * GIB), &l, 'S', false).mem,
            Reading::Calm
        );
    }

    #[test]
    fn the_kernel_reports_a_state_and_that_needs_no_threshold() {
        let l = a_machine();
        let quiet = Metrics::default();
        assert_eq!(Readings::of(&quiet, &l, 'Z', false).flag, Reading::Alarm);
        assert_eq!(Readings::of(&quiet, &l, 'D', false).flag, Reading::Unusual);
        assert_eq!(Readings::of(&quiet, &l, 'S', false).flag, Reading::Calm);
    }

    #[test]
    fn a_row_that_reads_alarm_anywhere_carries_the_flag() {
        let l = a_machine();
        let hot = Metrics {
            cpu: Some(1.35),
            ..Metrics::default()
        };
        assert_eq!(Readings::of(&hot, &l, 'S', false).flag, Reading::Alarm);
    }

    #[test]
    fn the_host_row_is_read_as_a_whole_machine_and_not_as_a_process() {
        let l = a_machine();
        let busy = Metrics {
            // Two busy cores out of four is half a machine. On a process that
            // is alarming; on the machine itself it is an ordinary afternoon.
            cpu: Some(2.0),
            mem: Some(4.0 * GIB),
            ..Metrics::default()
        };
        assert_eq!(Readings::of(&busy, &l, '?', true).cpu, Reading::Calm);
        assert_eq!(Readings::of(&busy, &l, '?', true).mem, Reading::Calm);
        // The same figures on a process are what the row rules are for.
        assert_eq!(Readings::of(&busy, &l, 'S', false).cpu, Reading::Alarm);
        assert_eq!(Readings::of(&busy, &l, 'S', false).mem, Reading::Alarm);
        // A machine with nothing left is still read, by its own thresholds.
        let full = Metrics {
            cpu: Some(3.8),
            mem: Some(7.5 * GIB),
            ..Metrics::default()
        };
        assert_eq!(Readings::of(&full, &l, '?', true).cpu, Reading::Alarm);
        assert_eq!(Readings::of(&full, &l, '?', true).mem, Reading::Alarm);
    }

    #[test]
    fn only_the_host_row_reads_its_network_against_the_link() {
        let l = a_machine();
        let loud = Metrics {
            rx: Some(108_000_000.0),
            tx: Some(94_000_000.0),
            ..Metrics::default()
        };
        // 108 MB/s of a 125 MB/s link is 86 percent.
        assert_eq!(Readings::of(&loud, &l, 'S', true).rx, Reading::Alarm);
        // A row below the host counts bytes inside its own namespace, so the
        // link of the machine is not the whole it is a share of (D-42).
        assert_eq!(Readings::of(&loud, &l, 'S', false).rx, Reading::Calm);
    }

    #[test]
    fn a_link_that_reports_no_speed_leaves_the_network_reading_out() {
        let l = Limits {
            link_speed: None,
            ..a_machine()
        };
        let loud = Metrics {
            rx: Some(108_000_000.0),
            ..Metrics::default()
        };
        assert_eq!(Readings::of(&loud, &l, 'S', true).rx, Reading::Calm);
    }

    #[test]
    fn a_row_that_only_carries_a_readings_of_its_children_is_marked_as_the_way_down() {
        let l = a_machine();
        // pid 1 of an ordinary host: everything is in the subtree and nothing
        // is its own. The mark there is a tautology, and following it into the
        // row finds nothing (D-44).
        let mut parent = Node::new("p:1", "systemd", Kind::Process);
        parent.instant = Metrics {
            cpu: Some(2.5),
            mem: Some(4.6 * GIB),
            ..Metrics::default()
        };
        parent.avg = parent.instant;
        parent.detail.state = Some('S');
        let mut child = Node::new("p:2", "worker", Kind::Process);
        child.instant = parent.instant;
        child.avg = parent.instant;
        child.detail.state = Some('S');
        parent.children.push(child);

        assert_eq!(parent.mark(&l, Mode::Instant), Mark::Below(Reading::Alarm));
        assert_eq!(
            parent.children[0].mark(&l, Mode::Instant),
            Mark::Own(Reading::Alarm),
            "a row with no children is always its own source"
        );
    }

    #[test]
    fn a_row_that_burns_its_own_core_is_the_source_even_with_children() {
        let l = a_machine();
        let mut parent = Node::new("p:1", "server", Kind::Process);
        parent.instant = Metrics {
            cpu: Some(2.5),
            ..Metrics::default()
        };
        parent.avg = parent.instant;
        parent.detail.state = Some('S');
        let mut child = Node::new("p:2", "helper", Kind::Process);
        child.instant = Metrics {
            cpu: Some(0.02),
            ..Metrics::default()
        };
        child.avg = child.instant;
        child.detail.state = Some('S');
        parent.children.push(child);
        // 2.48 cores are left after the child is taken off, and that is still
        // alarm - so this row is where the work is.
        assert_eq!(parent.mark(&l, Mode::Instant), Mark::Own(Reading::Alarm));
    }

    #[test]
    fn a_state_the_kernel_reports_is_always_the_rows_own() {
        let l = a_machine();
        let mut parent = Node::new("p:1", "shell", Kind::Process);
        parent.detail.state = Some('Z');
        parent
            .children
            .push(Node::new("p:2", "child", Kind::Process));
        assert_eq!(parent.mark(&l, Mode::Instant), Mark::Own(Reading::Alarm));
    }

    #[test]
    fn a_calm_row_carries_no_mark_at_all() {
        let l = a_machine();
        let mut n = Node::new("p:1", "idle", Kind::Process);
        n.detail.state = Some('S');
        n.instant.cpu = Some(0.01);
        assert_eq!(n.mark(&l, Mode::Instant), Mark::Calm);
    }

    #[test]
    fn a_row_is_read_through_the_node_so_the_table_and_the_card_cannot_disagree() {
        let l = a_machine();
        let mut n = Node::new("p:9", "worker", Kind::Process);
        n.instant.cpu = Some(1.35);
        n.avg.cpu = Some(0.02);
        n.detail.state = Some('S');
        // The reading follows the figure the screen is showing, so switching
        // to the average switches the colour with it.
        assert_eq!(n.readings(&l, Mode::Instant).cpu, Reading::Alarm);
        assert_eq!(n.readings(&l, Mode::Average).cpu, Reading::Calm);
    }

    #[test]
    fn only_the_root_is_read_as_the_host() {
        let l = a_machine();
        let mut host = Node::new("cg:/", "host", Kind::Host);
        host.instant.rx = Some(108_000_000.0);
        assert_eq!(host.readings(&l, Mode::Instant).rx, Reading::Alarm);

        let mut proc = Node::new("p:9", "worker", Kind::Process);
        proc.instant.rx = Some(108_000_000.0);
        assert_eq!(proc.readings(&l, Mode::Instant).rx, Reading::Calm);
    }

    #[test]
    fn the_machine_reads_its_own_summary_against_what_it_has() {
        let l = a_machine();
        // Load is per core, so 4 cores carry 4.0 without a word.
        assert_eq!(HostReadings::of(9.0, 0.0, 0.0, &l).load, Reading::Alarm);
        assert_eq!(HostReadings::of(5.0, 0.0, 0.0, &l).load, Reading::Unusual);
        assert_eq!(HostReadings::of(2.0, 0.0, 0.0, &l).load, Reading::Calm);
        assert_eq!(
            HostReadings::of(0.0, 7.5 * GIB, 0.0, &l).mem,
            Reading::Alarm
        );
        assert_eq!(
            HostReadings::of(0.0, 6.2 * GIB, 0.0, &l).mem,
            Reading::Unusual
        );
        assert_eq!(HostReadings::of(0.0, 4.0 * GIB, 0.0, &l).mem, Reading::Calm);
        // Any swap at all is not the rule: on a host that has swapped once it
        // fires forever and says nothing.
        assert_eq!(
            HostReadings::of(0.0, 0.0, 1.5 * GIB, &l).swap,
            Reading::Alarm
        );
        assert_eq!(
            HostReadings::of(0.0, 0.0, 0.5 * GIB, &l).swap,
            Reading::Unusual
        );
        assert_eq!(
            HostReadings::of(0.0, 0.0, 0.05 * GIB, &l).swap,
            Reading::Calm
        );
    }

    #[test]
    fn a_reading_carries_the_sentence_that_says_why_it_fired() {
        let l = a_machine();
        let now = Metrics {
            cpu: Some(2.5),
            mem: Some(2.4 * GIB),
            ..Metrics::default()
        };
        let found = Readings::findings(&now, &now, &l, 'Z', false);
        let names: Vec<&str> = found.iter().map(|f| f.name).collect();
        // The name of a heuristic is the label of the card row its figure
        // stands on, so the reader can find the number the sentence is about
        // (D-43). A name nobody can look up is what made the first block
        // unreadable on a live host.
        assert_eq!(names, vec!["zombie", "cpu", "memory"]);
        for f in &found {
            assert!(f.name.len() <= 16, "{} is too wide for the label", f.name);
            assert!(f.reading != Reading::Calm, "{} did not fire", f.name);
        }
        assert!(found[1].why.contains("2.500"), "{}", found[1].why);
        assert!(found[1].why.contains("1.000"), "{}", found[1].why);
        assert!(found[2].why.contains("30%"), "{}", found[2].why);
        assert!(found[2].why.contains("25%"), "{}", found[2].why);
    }

    #[test]
    fn a_reason_names_the_figure_of_both_columns_the_card_shows() {
        // The card prints now and avg side by side. A sentence that named one
        // of them without saying which sends the reader looking for a number
        // that is in neither column (operator, 2026-08-30).
        let l = a_machine();
        let now = Metrics {
            cpu: Some(0.203),
            ..Metrics::default()
        };
        let avg = Metrics {
            cpu: Some(0.343),
            ..Metrics::default()
        };
        let found = Readings::findings(&now, &avg, &l, 'S', false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].now, Reading::Unusual);
        assert_eq!(found[0].avg, Reading::Unusual);
        assert!(found[0].why.contains("0.203 cores now"), "{}", found[0].why);
        assert!(found[0].why.contains("0.343 cores avg"), "{}", found[0].why);
    }

    #[test]
    fn a_figure_that_fires_in_one_column_only_says_which() {
        let l = a_machine();
        let now = Metrics {
            cpu: Some(2.5),
            ..Metrics::default()
        };
        let avg = Metrics {
            cpu: Some(0.02),
            ..Metrics::default()
        };
        let found = Readings::findings(&now, &avg, &l, 'S', false);
        assert_eq!(found.len(), 1, "a spike is still a reason");
        assert_eq!(found[0].now, Reading::Alarm);
        assert_eq!(found[0].avg, Reading::Calm);
        assert_eq!(found[0].reading, Reading::Alarm, "the worse of the two");
    }

    #[test]
    fn a_figure_that_did_not_move_is_written_once_rather_than_twice() {
        // "8.5G (55%) now, 8.5G (55%) avg" is the same number said twice, and
        // it pushed the sentence onto a second line on a live host.
        let l = a_machine();
        let m = Metrics {
            mem: Some(2.4 * GIB),
            ..Metrics::default()
        };
        let found = Readings::findings(&m, &m, &l, 'S', false);
        assert!(
            found[0].why.starts_with("2.4G (30%) now and avg,"),
            "{}",
            found[0].why
        );
        // Short enough to stand on one line of a hundred-cell terminal beside
        // a label column of sixteen (D-32).
        assert!(found[0].why.len() <= 80, "{}", found[0].why);
    }

    #[test]
    fn the_swap_reason_says_it_is_the_subtree_and_not_the_process() {
        // The card of a process carries `own swap`, which is the swap of that
        // one process. The heuristic reads the swap of the whole subtree, so a
        // sentence that did not say so named 1.9G beside a card row saying 0M
        // (found on the reference host, 2026-08-30).
        let l = a_machine();
        let m = Metrics {
            swap: Some(0.9 * GIB),
            ..Metrics::default()
        };
        let found = Readings::findings(&m, &m, &l, 'S', false);
        assert_eq!(found[0].name, "swap");
        assert!(found[0].why.contains("subtree"), "{}", found[0].why);
    }

    #[test]
    fn a_row_where_everything_is_calm_has_nothing_to_explain() {
        let l = a_machine();
        let quiet = Metrics {
            cpu: Some(0.01),
            mem: Some(1024.0),
            ..Metrics::default()
        };
        assert!(Readings::findings(&quiet, &quiet, &l, 'S', false).is_empty());
    }

    #[test]
    fn the_reading_and_its_reason_come_from_one_place() {
        // A threshold that moved in the figure and stayed in the sentence is
        // the way a card goes stale without anybody noticing (D-43).
        let l = a_machine();
        for state in ['S', 'D', 'Z'] {
            for cpu in [0.05, 0.5, 2.0] {
                let m = Metrics {
                    cpu: Some(cpu),
                    ..Metrics::default()
                };
                let r = Readings::of(&m, &l, state, false);
                let found = Readings::findings(&m, &m, &l, state, false);
                let worst = found.iter().map(|f| f.now).max().unwrap_or(Reading::Calm);
                assert_eq!(worst, r.flag, "state {state}, cpu {cpu}");
            }
        }
    }

    #[test]
    fn disk_has_no_reading_at_all() {
        // Nothing readable says what the device can do, so the type carries no
        // field for it (D-42). This test stands as the record of that: it fails
        // to compile the day somebody adds one.
        let r = Readings::default();
        let _ = (r.cpu, r.mem, r.swap, r.tasks, r.rx, r.tx, r.flag);
    }
}
