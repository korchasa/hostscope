//! Assembling one tick into the tree the interface works over: the process
//! forest of the host, rooted at the processes nothing on the host started.
//!
//! Everything expensive is bounded here: the cgroup hierarchy is walked once
//! and only for `cgroup.procs`, `/proc` is read only for the processes that
//! walk listed, and the container data is taken from the last cached
//! enrichment rather than fetched inline (section 6).

pub mod cgroup;
pub mod host;
pub mod procs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::enrich::Enrichment;
use crate::model::{
    Ceilings, Detail, HostSummary, Kind, Limits, Metrics, Node, Owner, OwnerKind, Snapshot,
};
use crate::sample::Sampler;
use crate::util::clean;

pub struct Collector {
    cgroup_root: PathBuf,
    proc_root: PathBuf,
    sampler: Sampler,
    users: HashMap<u32, String>,
    self_netns: Option<u64>,
    history: Vec<f64>,
    ebpf: bool,
    root_privileges: bool,
    boot_time: f64,
    cost: TickCost,
    cmd_cache: procs::CmdCache,
    link_speed: Option<f64>,
}

/// What the last tick cost, split by source. Section 6 puts a budget on the
/// whole tick, and a single number cannot say which half to work on.
#[derive(Clone, Copy, Debug, Default)]
pub struct TickCost {
    pub cgroup_ms: f64,
    pub proc_ms: f64,
    pub processes: usize,
}

impl Collector {
    pub fn new(cgroup_root: PathBuf, proc_root: PathBuf, now: f64, etc_passwd: bool) -> Collector {
        let self_netns = host::netns_ino(&proc_root, std::process::id() as i32);
        // `/sys/class/net` sits beside `/sys/fs/cgroup` rather than under
        // `/proc`, and a captured snapshot lays the two out the same way, so
        // the interface set is found from the cgroup root instead of being
        // hard-coded. Without that, a run over a snapshot would read the links
        // of whatever machine happened to be running it.
        let link_speed = host::link_speed(
            &cgroup_root
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(Path::new("/sys"))
                .join("class/net"),
        );
        Collector {
            cgroup_root,
            proc_root,
            sampler: Sampler::new(now),
            // Without the table every login session is a number on the screen.
            // That is a worse reading and not a wrong one, which is why the
            // choice belongs to whoever runs the tool (D-41).
            users: if etc_passwd {
                procs::user_table()
            } else {
                std::collections::HashMap::new()
            },
            self_netns,
            history: Vec::new(),
            ebpf: false,
            root_privileges: self_uid() == 0,
            boot_time: 0.0,
            cost: TickCost::default(),
            cmd_cache: procs::CmdCache::new(),
            link_speed,
        }
    }

    pub fn cost(&self) -> TickCost {
        self.cost
    }

    pub fn proc_root(&self) -> &Path {
        &self.proc_root
    }

    pub fn tick(&mut self, now: f64, enrichment: &Enrichment, docker_enabled: bool) -> Snapshot {
        self.sampler.begin(now);
        let raw_host = host::read(&self.proc_root);
        self.boot_time = unix_now() - raw_host.uptime;
        let summary = self.host_summary(&raw_host);

        self.cost = TickCost::default();
        let started = std::time::Instant::now();
        let raw = cgroup::read_tree(&self.cgroup_root);
        let mut cgroups = Vec::new();
        cgroup::flatten(&raw, &mut cgroups);
        let held = self.ceilings(&raw);
        self.cost.cgroup_ms = started.elapsed().as_secs_f64() * 1000.0;

        let root = self.build_forest(&cgroups, enrichment, &summary, &held);

        self.history.push(summary.busy_cores);
        if self.history.len() > 120 {
            self.history.remove(0);
        }
        let mut summary = summary;
        summary.history = self.history.clone();

        self.sampler.sweep();
        Snapshot {
            root,
            limits: Limits {
                cores: summary.cores,
                mem_total: summary.mem_total,
                swap_total: summary.swap_total,
                link_speed: self.link_speed,
            },
            host: summary,
            interval: self.sampler.interval(),
            window: self.sampler.window(),
            ticks: self.sampler.ticks(),
            root_privileges: self.root_privileges,
            docker_available: enrichment.docker_ok,
            ebpf: self.ebpf,
        }
        .with_docker_flag(docker_enabled)
    }

    fn host_summary(&mut self, raw: &host::RawHost) -> HostSummary {
        let total = self.sampler.counter("host:cpu_total", raw.cpu_total);
        let idle = self.sampler.counter("host:cpu_idle", raw.cpu_idle);
        let rx = self.sampler.counter("host:net_rx", raw.net_rx);
        let tx = self.sampler.counter("host:net_tx", raw.net_tx);
        let mem = self.sampler.gauge(
            "host:mem_used",
            (raw.mem_total - raw.mem_available).max(0.0),
        );
        let swap = self
            .sampler
            .gauge("host:swap_used", (raw.swap_total - raw.swap_free).max(0.0));
        HostSummary {
            // Read fresh each tick and not sampled: the kernel already averages
            // it over ten and sixty seconds, and averaging an average again
            // would say nothing about either window (D-46).
            pressure: host::pressure(&self.proc_root),
            hostname: clean(&raw.hostname),
            kernel: clean(&raw.kernel),
            uptime_secs: raw.uptime,
            cores: raw.cores,
            busy_cores: ((total.instant - idle.instant) / procs::USER_HZ).max(0.0),
            busy_cores_avg: ((total.average - idle.average) / procs::USER_HZ).max(0.0),
            mem_used: mem.instant,
            mem_used_avg: mem.average,
            mem_total: raw.mem_total,
            swap_used: swap.instant,
            swap_used_avg: swap.average,
            swap_total: raw.swap_total,
            net_rx: rx.instant,
            net_tx: tx.instant,
            net_rx_avg: rx.average,
            net_tx_avg: tx.average,
            load: raw.load,
            history: Vec::new(),
        }
    }

    /// The whole host as one forest under the host node.
    ///
    /// The host row does not sum its children: its CPU and memory are what the
    /// kernel reports for the machine, so the `(self)` row of FR-14 is what no
    /// process accounts for - the kernel's own time, and the memory that is
    /// cache and slab rather than any process's pages.
    fn build_forest(
        &mut self,
        cgroups: &[(String, Vec<i32>)],
        enrichment: &Enrichment,
        summary: &HostSummary,
        held: &HashMap<String, Ceilings>,
    ) -> Node {
        let started = std::time::Instant::now();
        let mut owners: HashMap<i32, (String, Owner)> = HashMap::new();
        for (rel, pids) in cgroups {
            let owner = self.owner_of(rel, enrichment);
            for pid in pids {
                owners.insert(*pid, (rel.clone(), owner.clone()));
            }
        }

        // Processes come and go all day; the cache of their command lines must
        // not follow the churn upwards without a bound.
        if self.cmd_cache.len() > 8192 {
            self.cmd_cache.clear();
        }
        // Reading `VmSwap` for every process costs about 4.5 ms a tick on a
        // tree of 221, and on a host whose swap device is untouched it buys a
        // column of zeros. So the machine is asked first (D-35).
        let swap_in_use = summary.swap_used > 0.0;
        let mut samples: HashMap<i32, procs::ProcSample> = HashMap::new();
        for pid in owners.keys() {
            // Every row is read in full: a parent row carries the totals of its
            // whole subtree (FR-5), so the disk of any row on screen needs the
            // disk of every process under it.
            if let Some(s) = procs::read(
                &self.proc_root,
                *pid,
                true,
                swap_in_use,
                &mut self.cmd_cache,
            ) {
                samples.insert(*pid, s);
            }
        }
        self.cost.proc_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.cost.processes = samples.len();

        let mut kids: HashMap<i32, Vec<i32>> = HashMap::new();
        let mut roots: Vec<i32> = Vec::new();
        for (pid, s) in &samples {
            if samples.contains_key(&s.ppid) && s.ppid != *pid {
                kids.entry(s.ppid).or_default().push(*pid);
            } else {
                roots.push(*pid);
            }
        }
        roots.sort();
        for v in kids.values_mut() {
            v.sort();
        }
        // A kernel thread belongs to no container, service or user, and saying
        // `system` about it would put it in with the work of the host. What it
        // is can only be read from the forest: it is `kthreadd` or something
        // `kthreadd` started.
        let mut kernel: std::collections::HashSet<i32> = std::collections::HashSet::new();
        if samples.contains_key(&2) {
            mark_kernel(2, &kids, &mut kernel);
        }

        let mut node = Node::new("cg:/", "host", Kind::Host);
        node.detail.cgroup_path = Some("/".to_string());
        node.children = roots
            .iter()
            .map(|pid| self.process_node(*pid, &samples, &kids, &owners, &kernel, held))
            .collect();
        node.detail.child_count = node.children.len();

        let mut totals = Metrics::default();
        let mut avg_totals = Metrics::default();
        for c in &node.children {
            totals.add(&c.instant);
            avg_totals.add(&c.avg);
        }
        // Tasks, disk and network are only known where a process reports them,
        // so for those the host is the sum of what stands under it. CPU and
        // memory the machine reports for itself, and the difference is the
        // point of the `(self)` row.
        node.instant = Metrics {
            cpu: Some(summary.busy_cores),
            mem: Some(summary.mem_used),
            swap: swap_in_use.then_some(summary.swap_used),
            ..totals
        };
        node.avg = Metrics {
            cpu: Some(summary.busy_cores_avg),
            mem: Some(summary.mem_used_avg),
            swap: swap_in_use.then_some(summary.swap_used_avg),
            ..avg_totals
        };
        node
    }

    /// The ceilings every cgroup was held against during this tick, keyed by
    /// path (D-45).
    ///
    /// Each of the four is a running total, so the fact is the growth between
    /// two ticks and never the value: a group throttled an hour ago is not a
    /// group being throttled now. The first tick of a run therefore reports
    /// nothing, which is the same rule FR-15 already holds every rate to.
    ///
    /// A group inherits what its ancestors were held against, because the
    /// limit is usually not set where the process is listed: a runtime puts the
    /// memory limit on the pod and runs the process one directory below it, and
    /// `cpu.stat` counts the throttling of the group whose quota it was.
    fn ceilings(&mut self, raw: &cgroup::RawCgroup) -> HashMap<String, Ceilings> {
        let mut flat = Vec::new();
        cgroup::flatten_ceilings(raw, &mut flat);
        let mut grew: Vec<(String, Ceilings)> = Vec::with_capacity(flat.len());
        for (rel, c) in &flat {
            let mut hit = |what: &str, v: Option<f64>| match v {
                Some(v) => self.sampler.counter(&format!("cg:{rel}:{what}"), v).instant > 0.0,
                None => false,
            };
            grew.push((
                rel.clone(),
                Ceilings {
                    throttled: hit("thr", c.throttled),
                    oom_kill: hit("oom", c.oom_kill),
                    mem_ceiling: hit("mem", c.mem_ceiling),
                    pid_ceiling: hit("pid", c.pid_ceiling),
                },
            ));
        }
        // The walk is depth-first from the root, so every ancestor of a path
        // has already been folded by the time the path itself is reached.
        let mut out: HashMap<String, Ceilings> = HashMap::new();
        for (rel, own) in grew {
            let mut all = own;
            if let Some(cut) = rel.rfind('/') {
                let parent = if cut == 0 { "/" } else { &rel[..cut] };
                if let Some(up) = out.get(parent) {
                    all.throttled |= up.throttled;
                    all.oom_kill |= up.oom_kill;
                    all.mem_ceiling |= up.mem_ceiling;
                    all.pid_ceiling |= up.pid_ceiling;
                }
            }
            out.insert(rel, all);
        }
        out
    }

    /// The owner of every process in a cgroup, named once per cgroup rather
    /// than once per process (FR-20).
    fn owner_of(&self, rel: &str, enrichment: &Enrichment) -> Owner {
        let mut owner = cgroup::owner_of(rel, &self.users);
        if owner.kind != OwnerKind::Container {
            return owner;
        }
        let full = match cgroup::ownership(rel) {
            cgroup::Ownership::Container(id) => id,
            _ => return owner,
        };
        if let Some(info) = enrichment.containers.get(&full) {
            if !info.name.is_empty() {
                owner.name = clean(&info.name);
            }
        }
        owner
    }

    fn process_node(
        &mut self,
        pid: i32,
        samples: &HashMap<i32, procs::ProcSample>,
        kids: &HashMap<i32, Vec<i32>>,
        owners: &HashMap<i32, (String, Owner)>,
        kernel: &std::collections::HashSet<i32>,
        held: &HashMap<String, Ceilings>,
    ) -> Node {
        let s = &samples[&pid];
        let id = format!("p:{}:{}", pid, s.starttime);
        let mut node = Node::new(id.clone(), &s.comm, Kind::Process);

        let cpu = self.sampler.counter(&format!("{id}:cpu"), s.cpu_seconds);
        let mem = self.sampler.gauge(&format!("{id}:mem"), s.rss);
        let tasks = self.sampler.gauge(&format!("{id}:tasks"), s.threads as f64);
        let (rd, wr) = match s.io {
            Some((r, w)) => (
                Some(self.sampler.counter(&format!("{id}:rd"), r)),
                Some(self.sampler.counter(&format!("{id}:wr"), w)),
            ),
            None => (None, None),
        };
        let net = self.namespace_net(&id, pid, s.ppid, samples);

        let mut own_instant = Metrics {
            cpu: Some(cpu.instant),
            mem: Some(mem.instant),
            swap: s.swap,
            tasks: Some(tasks.instant),
            rd: rd.map(|r| r.instant),
            wr: wr.map(|r| r.instant),
            rx: net.map(|n| n.0.instant),
            tx: net.map(|n| n.1.instant),
        };
        let mut own_avg = Metrics {
            cpu: Some(cpu.average),
            mem: Some(mem.average),
            swap: s.swap,
            tasks: Some(tasks.average),
            rd: rd.map(|r| r.average),
            wr: wr.map(|r| r.average),
            rx: net.map(|n| n.0.average),
            tx: net.map(|n| n.1.average),
        };

        let (cgroup_path, owner) = match owners.get(&pid) {
            Some((rel, _)) if kernel.contains(&pid) => (
                Some(rel.clone()),
                Owner {
                    kind: OwnerKind::Kernel,
                    name: String::new(),
                },
            ),
            Some((rel, owner)) => (Some(rel.clone()), owner.clone()),
            // The forest is read out of the cgroup listing, so a process with
            // no entry here never became a node at all. The arm is the type
            // system's, not the host's: what it must not do is invent a cgroup
            // path for a row that has none.
            None => (
                None,
                Owner {
                    kind: OwnerKind::System,
                    name: String::new(),
                },
            ),
        };
        let container = match (&owner.kind, &cgroup_path) {
            (OwnerKind::Container, Some(rel)) => match cgroup::ownership(rel) {
                cgroup::Ownership::Container(id) => Some(cgroup::short_id(&id)),
                _ => None,
            },
            _ => None,
        };

        node.detail = Detail {
            pid: Some(pid),
            ppid: Some(s.ppid),
            user: Some(
                self.users
                    .get(&s.uid)
                    .cloned()
                    .unwrap_or_else(|| s.uid.to_string()),
            ),
            state: Some(s.state),
            cmdline: s.cmdline.as_deref().map(clean),
            threads: Some(s.threads),
            started: Some(crate::enrich::stamp(
                self.boot_time + s.starttime as f64 / procs::USER_HZ,
            )),
            vsz: Some(s.vsz),
            ceilings: cgroup_path
                .as_deref()
                .and_then(|rel| held.get(rel).copied())
                .unwrap_or_default(),
            cgroup_path,
            short_id: container,
            owner: Some(owner),
            own_netns: net.is_some(),
            io_total: s.io.map(|(r, w)| {
                (
                    self.sampler.total_since_start(&format!("{id}:rd"), r),
                    self.sampler.total_since_start(&format!("{id}:wr"), w),
                )
            }),
            restricted: if s.io.is_none() {
                vec!["process I/O"]
            } else {
                Vec::new()
            },
            ..Detail::default()
        };

        let children: Vec<Node> = kids
            .get(&pid)
            .map(|list| {
                list.iter()
                    .map(|c| self.process_node(*c, samples, kids, owners, kernel, held))
                    .collect()
            })
            .unwrap_or_default();
        for c in &children {
            own_instant.add(&c.instant);
            own_avg.add(&c.avg);
        }
        node.instant = own_instant;
        node.avg = own_avg;
        node.children = children;
        glue_single_child(&mut node);
        node.detail.leads_into = leads_into(&node);
        node.detail.child_count = node.children.len();
        node
    }

    /// Traffic of a process that holds a network namespace of its own (FR-11).
    /// There is no per-process network counter, so the figures come from
    /// `/proc/<pid>/net/dev` read inside that namespace, and they belong to the
    /// process that entered it - the first one on its branch whose namespace
    /// differs from its parent's. Anywhere else traffic cannot be attributed
    /// and stays absent.
    fn namespace_net(
        &mut self,
        id: &str,
        pid: i32,
        ppid: i32,
        samples: &HashMap<i32, procs::ProcSample>,
    ) -> Option<(crate::sample::Rates, crate::sample::Rates)> {
        let ino = host::netns_ino(&self.proc_root, pid)?;
        if Some(ino) == self.self_netns {
            return None;
        }
        // A child of a process already in the namespace adds nothing: the
        // counters are the namespace's, and counting them twice would make the
        // branch report traffic it does not have.
        if samples.contains_key(&ppid) && host::netns_ino(&self.proc_root, ppid) == Some(ino) {
            return None;
        }
        let (rx, tx) = host::net_dev(&self.proc_root.join(pid.to_string()).join("net/dev"))?;
        // Keyed by the namespace, not by the process: a restarted container
        // keeps its own history and a shared namespace is not counted twice.
        let key = format!("{id}:ns{ino}");
        Some((
            self.sampler.counter(&format!("{key}:rx"), rx),
            self.sampler.counter(&format!("{key}:tx"), tx),
        ))
    }
}

fn mark_kernel(pid: i32, kids: &HashMap<i32, Vec<i32>>, out: &mut std::collections::HashSet<i32>) {
    if !out.insert(pid) {
        return;
    }
    if let Some(list) = kids.get(&pid) {
        for c in list {
            mark_kernel(*c, kids, out);
        }
    }
}

impl Snapshot {
    fn with_docker_flag(mut self, enabled: bool) -> Snapshot {
        if !enabled {
            self.docker_available = false;
        }
        self
    }
}

pub fn self_uid() -> u32 {
    if let Ok(text) = std::fs::read_to_string("/proc/self/status") {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(v) = rest.split_whitespace().next().and_then(|v| v.parse().ok()) {
                    return v;
                }
            }
        }
    }
    // Outside Linux (the tests run on a developer machine) there is no
    // /proc/self/status; a non-root answer is the safe one.
    1000
}

pub fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A row with exactly one child says nothing the child does not say, and costs
/// a keystroke to get past: `supervisor/app/python3/npm exec chrome/sh/
/// chrome-devtools/node` was seven levels of one row each on the test host. Such
/// a chain is drawn as one row named for the whole of it (D-25).
///
/// The figures are the top link's, which already cover the chain - a row
/// carries its whole subtree (FR-5) - so nothing moves. What the row is
/// identified by stays the top link too: its pid, its user, its cgroup. The
/// command line comes from the last link, because that is what the chain is
/// doing, and the card prints the chain so the two are never confused.
///
/// Only where the owner is the same all the way down. A change of owner is a
/// boundary worth a keystroke - a shim stepping into a container - and gluing
/// across it would put a name in the `OWNER` column that half the row does not
/// belong to.
///
/// The forest is built from the leaves up, so the child being glued may be a
/// glued chain itself. Its own name is then no longer in `name` - that holds
/// the whole of its chain - and taking the name from there would attribute
/// every link below it to the one pid. The links it already recorded are moved
/// across instead, which is what keeps the card naming each of them with the
/// pid it belongs to.
///
/// That same order means the body runs at most once today: the child has
/// already been glued, so what is left under it is either nothing, or more than
/// one node, or one node of another owner. The loop and the guard on the first
/// link are what would keep this correct if the order ever changed - they are
/// not there because it runs twice.
///
/// The command line is taken from the last link unconditionally, and a last
/// link that has none leaves the row with none. That is the honest reading:
/// showing the first link's command under a label naming the last link's pid is
/// the confusion the chain list exists to prevent.
/// The container a row leads into (D-30). A runtime shim belongs to
/// `containerd.service` and its whole work is inside the container it started,
/// so the row keeps `containerd` in `OWNER` and gluing stops at that boundary -
/// which left a level of the Docker rig showing 20 rows all named
/// `containerd-shim`, one per container of the host.
///
/// Read after gluing, so what is looked at is the children the row really has.
/// Exactly one of them, and it a container other than the row's own: a row with
/// two containers under it names neither, because one row cannot name two and
/// half a name is worse than none. An owner with no name of its own gives
/// nothing to put in the parentheses.
fn leads_into(node: &Node) -> Option<String> {
    if let [child] = node.children.as_slice() {
        if let Some(owner) = child.detail.owner.as_ref() {
            if owner.kind == OwnerKind::Container
                && !owner.name.is_empty()
                && node.detail.owner.as_ref() != Some(owner)
            {
                return Some(owner.name.clone());
            }
        }
        return None;
    }
    pod_below(node).map(|id| format!("pod {}", cgroup::short_pod(&id)))
}

/// The pod several containers under one row share (D-31). On a Kubernetes node
/// a shim carries two children - the pod's `pause` sandbox and the workload
/// container - so the rule above names nothing, and what the two have in common
/// is the pod. Every child must be a container and every one of them in the
/// same pod: children of two pods leave a row that would have to name two, and
/// a single non-container child means the row leads somewhere else as well.
fn pod_below(node: &Node) -> Option<String> {
    if node.children.len() < 2 {
        return None;
    }
    let mut pod: Option<String> = None;
    for child in &node.children {
        let owner = child.detail.owner.as_ref()?;
        if owner.kind != OwnerKind::Container {
            return None;
        }
        let here = cgroup::pod_id(child.detail.cgroup_path.as_deref()?)?;
        match &pod {
            Some(seen) if *seen != here => return None,
            _ => pod = Some(here),
        }
    }
    pod
}

fn glue_single_child(node: &mut Node) {
    while node.children.len() == 1 && node.children[0].detail.owner == node.detail.owner {
        let mut child = node.children.remove(0);
        if node.detail.glued.is_empty() {
            node.detail
                .glued
                .push((node.detail.pid.unwrap_or(0), node.name.clone()));
        }
        node.name = format!("{}/{}", node.name, child.name);
        node.children = std::mem::take(&mut child.children);
        node.detail.cmdline = child.detail.cmdline.take();
        if child.detail.glued.is_empty() {
            node.detail
                .glued
                .push((child.detail.pid.unwrap_or(0), child.name));
        } else {
            node.detail.glued.append(&mut child.detail.glued);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(rel: &str, throttled: Option<f64>) -> cgroup::RawCgroup {
        cgroup::RawCgroup {
            rel: rel.to_string(),
            ceilings: cgroup::CeilingCounters {
                throttled,
                ..cgroup::CeilingCounters::default()
            },
            ..cgroup::RawCgroup::default()
        }
    }

    /// The ceiling a row is held against is the growth of a counter between two
    /// ticks, and it is inherited downwards (D-45).
    ///
    /// Both halves matter and neither is obvious. The first tick of a run knows
    /// no previous value, so it must report nothing rather than everything the
    /// group was ever held against - the same rule FR-15 holds every rate to.
    /// And the quota is set where the runtime put it, which is the pod and not
    /// the container below it, so a container whose own `cpu.stat` reads zero
    /// is still a container that was held back.
    #[test]
    fn a_ceiling_is_the_growth_between_ticks_and_reaches_the_groups_below_it() {
        let dir = std::env::temp_dir().join(format!("hs-ceil-{}", std::process::id()));
        let mut c = Collector::new(dir.join("cgroup"), dir.join("proc"), 0.0, false);

        let mut tree = group("/", None);
        let mut pod = group("/pod", Some(5.0));
        pod.children.push(group("/pod/container", Some(0.0)));
        tree.children.push(pod);

        let first = c.ceilings(&tree);
        assert!(
            !first["/pod"].throttled,
            "the first tick has nothing to compare against"
        );

        c.sampler.begin(1.0);
        let same = c.ceilings(&tree);
        assert!(!same["/pod"].throttled, "a counter that stood still");

        c.sampler.begin(2.0);
        let mut tree = group("/", None);
        let mut pod = group("/pod", Some(7.0));
        pod.children.push(group("/pod/container", Some(0.0)));
        tree.children.push(pod);
        let grew = c.ceilings(&tree);
        assert!(grew["/pod"].throttled, "the quota was hit twice more");
        assert!(
            grew["/pod/container"].throttled,
            "the quota is the pod's, and the process runs one level below it"
        );
        assert!(
            !grew["/"].throttled,
            "the root was not held against anything"
        );
    }
}
