//! The tree the whole application works over: the process forest of the host,
//! one node per process, with the same set of values everywhere.
//!
//! There is no grouping in it. What runs a process - a container, a service, a
//! login session - is written on the row as its [`Owner`] rather than made
//! into a level of its own (FR-20).

use crate::util::clean;

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
        self.tasks = opt_add(self.tasks, o.tasks);
        self.rd = opt_add(self.rd, o.rd);
        self.wr = opt_add(self.wr, o.wr);
        self.rx = opt_add(self.rx, o.rx);
        self.tx = opt_add(self.tx, o.tx);
    }

    pub fn sub(&mut self, o: &Metrics) {
        self.cpu = opt_sub(self.cpu, o.cpu);
        self.mem = opt_sub(self.mem, o.mem);
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
            self.cpu, self.mem, self.tasks, self.rd, self.wr, self.rx, self.tx,
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
}
