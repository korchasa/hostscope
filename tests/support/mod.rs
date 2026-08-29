//! A captured `/sys/fs/cgroup` and `/proc` written out as files, which is what
//! `--cgroup-root` and `--proc-root` exist for (FR-17). Building the fixture in
//! code rather than storing a dump keeps every number in the test visible next
//! to the assertion that depends on it.

// The module serves several test binaries, and no one of them uses all of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new(name: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("hostscope-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    pub fn cgroup_root(&self) -> PathBuf {
        self.root.join("cgroup")
    }

    pub fn proc_root(&self) -> PathBuf {
        self.root.join("proc")
    }

    /// One cgroup node. Only the list of processes matters: the counters of a
    /// cgroup are not read at all any more, and the hierarchy is here for the
    /// one thing only it knows - what runs a process (FR-20).
    pub fn cgroup(&self, rel: &str, procs: &[i32]) {
        let dir = if rel.is_empty() {
            self.cgroup_root()
        } else {
            self.cgroup_root().join(rel)
        };
        fs::create_dir_all(&dir).unwrap();
        let list: Vec<String> = procs.iter().map(|p| p.to_string()).collect();
        write_file(&dir.join("cgroup.procs"), &(list.join("\n") + "\n"));
    }

    /// The bytes a process has read and written since it started, as
    /// `/proc/<pid>/io` reports them. Without root this file cannot be opened
    /// for a process of another user, so a fixture that leaves it out is the
    /// ordinary case, not a broken one (FR-8).
    pub fn process_io(&self, pid: i32, read: u64, write: u64) {
        let dir = self.proc_root().join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        write_file(
            &dir.join("io"),
            &format!("rchar: 1\nwchar: 2\nread_bytes: {read}\nwrite_bytes: {write}\n"),
        );
    }

    /// One process, in the shape `/proc/<pid>/stat` has on a live host.
    pub fn process(
        &self,
        pid: i32,
        ppid: i32,
        comm: &str,
        cpu_ticks: u64,
        rss_pages: u64,
        cmdline: &str,
    ) {
        let dir = self.proc_root().join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        write_file(
            &dir.join("stat"),
            &format!(
                "{pid} ({comm}) S {ppid} {pid} {pid} 0 -1 4194560 100 0 0 0 {} 0 0 0 20 0 1 0 5000 12345678 {rss_pages} 18446744073709551615\n",
                cpu_ticks
            ),
        );
        write_file(
            &dir.join("status"),
            "Name:\tx\nUid:\t0\t0\t0\t0\nThreads:\t1\n",
        );
        write_file(&dir.join("cmdline"), &cmdline.replace(' ', "\0"));
    }

    /// The three layouts the captured environments turned out to have (D-23).
    /// They are written out here rather than stored as a dump of the real
    /// hosts: what has to be checked is the shape - which directory holds
    /// which - and a shape written in code can be read next to the assertion
    /// about it. The originals were 432 KB, 432 KB and 6.6 MB.
    ///
    /// Docker with the cgroupfs driver, as OrbStack runs it: the containers sit
    /// under a `docker` directory, named with the identifier and nothing else.
    pub fn shape_docker_cgroupfs(&self) {
        self.host(1_000_000, 900_000, 4);
        self.cgroup("", &[]);
        self.cgroup("init.scope", &[1]);
        self.cgroup(".lxc", &[]);
        self.cgroup("docker", &[]);
        self.cgroup("docker/buildkit", &[]);
        self.cgroup(
            "docker/3c9e0142ab76d5f8e29b47c03a15d6802f4e91b7c8a03d5f6e2b19c740a83e5b",
            &[201],
        );
        self.cgroup(
            "docker/4a1b76d29e05c83f1d7a04b6e39c25f81074e6a3b95d02c8f16e4a70d38b91c2",
            &[301],
        );
        self.process(1, 0, "systemd", 20, 100, "/sbin/init");
        self.process(201, 1, "nginx", 120, 2000, "nginx: master process");
        self.process(301, 1, "redis-server", 80, 1000, "redis-server *:6379");
    }

    /// A Kubernetes node, as k3s lays it out: a class of service, then one
    /// directory per pod, then the containers of that pod.
    pub fn shape_kubernetes(&self) {
        self.host(1_000_000, 900_000, 4);
        self.cgroup("", &[]);
        self.cgroup("init", &[1]);
        self.cgroup("k3s", &[101]);
        self.cgroup("kubepods", &[]);
        self.cgroup("kubepods/besteffort", &[]);
        self.cgroup(
            "kubepods/besteffort/pod0b5d84c1-27ae-49f3-8c60-1d7e93af5620",
            &[],
        );
        self.cgroup("kubepods/besteffort/pod0b5d84c1-27ae-49f3-8c60-1d7e93af5620/5e8c31a70b492df6c05e83b71a2d94f60837c25e91b04a6df3c72e08b195d43a", &[201]);
        self.cgroup("kubepods/burstable", &[]);
        self.cgroup(
            "kubepods/burstable/pod7c62e0a9-4f13-42db-95ae-3b80d6c17f45",
            &[],
        );
        self.cgroup("kubepods/burstable/pod7c62e0a9-4f13-42db-95ae-3b80d6c17f45/6b3d92f05a81c47e2f90d6b83e15a72c04918d3f6a25c07be49f1d820c63a75e", &[301]);
        self.process(1, 0, "init", 20, 100, "/sbin/init");
        self.process(101, 1, "k3s-server", 100, 5000, "k3s server");
        self.process(
            201,
            1,
            "coredns",
            100,
            1800,
            "/coredns -conf /etc/coredns/Corefile",
        );
        self.process(301, 1, "traefik", 70, 900, "traefik");
    }

    /// Docker with the systemd driver on a plain server: the containers stand
    /// among the services of `system.slice`, and there is a login session.
    pub fn shape_systemd_docker(&self) {
        self.host(1_000_000, 900_000, 4);
        self.cgroup("", &[]);
        self.cgroup("init.scope", &[1]);
        self.cgroup("dev-mqueue.mount", &[]);
        self.cgroup("sys-kernel-debug.mount", &[]);
        self.cgroup("system.slice", &[]);
        self.cgroup("system.slice/ssh.service", &[101]);
        self.cgroup("system.slice/docker.service", &[102]);
        self.cgroup("system.slice/docker-1d4c8f2b6a30e75c94b1d0e3f28a6c517b93d4e0a6c821f5b7e340d9c2a8b16f.scope", &[201]);
        self.cgroup("system.slice/docker-2f7a30b9c81d465e0a3b72c9d1e846f05c9b28a7d3e610f4b85c2d97e03a1b6d.scope", &[301]);
        self.cgroup("user.slice", &[]);
        self.cgroup("user.slice/user-1000.slice", &[]);
        self.cgroup("user.slice/user-1000.slice/session-15324.scope", &[401]);
        self.cgroup("user.slice/user-1000.slice/user@1000.service", &[402]);
        self.process(1, 0, "systemd", 20, 100, "/sbin/init");
        self.process(101, 1, "sshd", 40, 200, "/usr/sbin/sshd -D");
        self.process(102, 1, "dockerd", 30, 300, "/usr/bin/dockerd");
        self.process(201, 1, "postgres", 120, 2000, "postgres");
        self.process(
            301,
            1,
            "grafana",
            80,
            1500,
            "/usr/share/grafana/bin/grafana",
        );
        self.process(401, 1, "bash", 40, 600, "-bash");
        self.process(402, 1, "systemd", 20, 300, "/lib/systemd/systemd --user");
    }

    /// A Kubernetes node as microk8s lays it out, which is where D-30 named
    /// nothing: every shim carries two children, the pod's `pause` sandbox and
    /// the workload container, and both sit in one pod (D-31). The third shim
    /// here holds two containers of two different pods, which is the case that
    /// still has nothing to name.
    pub fn shape_pod_sandbox(&self) {
        self.host(1_000_000, 900_000, 4);
        self.cgroup("", &[]);
        self.cgroup("init.scope", &[1]);
        self.cgroup("system.slice", &[]);
        self.cgroup(
            "system.slice/snap.microk8s.daemon-containerd.service",
            &[101, 102],
        );
        self.cgroup("kubepods", &[]);
        self.cgroup("kubepods/burstable", &[]);
        let pod_a = "kubepods/burstable/pod49ccade5-8a0b-4389-99ae-0c74a2533472";
        self.cgroup(pod_a, &[]);
        self.cgroup(
            &format!("{pod_a}/9f9149fcb4804719f61ed48794583a2eeb6735da282cdee85652fb76bdd7e09b"),
            &[201],
        );
        self.cgroup(
            &format!("{pod_a}/460e0f0eceb53803fb111b7d7df22fb036ad4ac357b825cd601c1c131cd83145"),
            &[202],
        );
        self.cgroup("kubepods/besteffort", &[]);
        let pod_b = "kubepods/besteffort/pod41810b2b-14eb-47f1-b216-ace571bb8132";
        let pod_c = "kubepods/besteffort/podad57a2a9-a796-4079-a42e-0e8fbefdd054";
        self.cgroup(pod_b, &[]);
        self.cgroup(
            &format!("{pod_b}/0ab5a48ea28a1a80f064701f33d0c4dd0635aa26e25a421444f63c5256bbce9c"),
            &[301],
        );
        self.cgroup(pod_c, &[]);
        self.cgroup(
            &format!("{pod_c}/08732113efbe5aa5e6031eba814a5657151d7b685dffb4029f76d5a0fc1d390f"),
            &[302],
        );
        self.process(1, 0, "systemd", 20, 100, "/sbin/init");
        self.process(
            101,
            1,
            "containerd-shim",
            12,
            300,
            "containerd-shim-runc-v2",
        );
        self.process(
            102,
            1,
            "containerd-shim",
            12,
            300,
            "containerd-shim-runc-v2",
        );
        self.process(201, 101, "pause", 4, 200, "/pause");
        self.process(
            202,
            101,
            "coredns",
            40,
            900,
            "/coredns -conf /etc/coredns/Corefile",
        );
        self.process(301, 102, "pause", 4, 200, "/pause");
        self.process(
            302,
            102,
            "hostpath-provis",
            30,
            700,
            "/hostpath-provisioner",
        );
    }

    /// The boundary D-30 is about: a runtime shim of `containerd.service` with
    /// its whole work inside a container, which is where the process forest
    /// stops gluing because the owner changes. `system.slice` also holds a shim
    /// with two containers under it, which is the case the parentheses must
    /// stay away from - one row cannot name two.
    pub fn shape_shim_boundary(&self) {
        self.host(1_000_000, 900_000, 4);
        self.cgroup("", &[]);
        self.cgroup("init.scope", &[1]);
        self.cgroup("system.slice", &[]);
        self.cgroup("system.slice/containerd.service", &[101, 102]);
        self.cgroup("system.slice/docker-5b21e4f70a9c36d81e40b7a2c95df0361847e2b9d05c7a61f3e802b4d97c16a8.scope", &[201]);
        self.cgroup("system.slice/docker-7d09b3a62e15c48f0b7d29e63a840c157f2b96d4e01a83c7b52d4f096e18a3b7.scope", &[301]);
        self.cgroup("system.slice/docker-9f42c807b1d63e59a20c48f7d3b915e64087a2c9f5b31d80e647c2a91b05d3f8.scope", &[302]);
        self.process(1, 0, "systemd", 20, 100, "/sbin/init");
        self.process(
            101,
            1,
            "containerd-shim",
            12,
            300,
            "containerd-shim-runc-v2",
        );
        self.process(
            102,
            1,
            "containerd-shim",
            12,
            300,
            "containerd-shim-runc-v2",
        );
        self.process(
            201,
            101,
            "s6-svscan",
            60,
            1500,
            "/bin/s6-svscan /run/service",
        );
        self.process(301, 102, "nginx", 40, 900, "nginx: master process");
        self.process(302, 102, "redis-server", 40, 800, "redis-server *:6379");
    }

    pub fn host(&self, cpu_total: u64, cpu_idle: u64, cores: usize) {
        let dir = self.proc_root();
        fs::create_dir_all(dir.join("sys/kernel")).unwrap();
        fs::create_dir_all(dir.join("net")).unwrap();
        write_file(&dir.join("sys/kernel/hostname"), "fixture\n");
        write_file(&dir.join("sys/kernel/osrelease"), "6.8.0-test\n");
        write_file(&dir.join("uptime"), "1000.0 2000.0\n");
        write_file(&dir.join("loadavg"), "0.10 0.20 0.30 1/100 999\n");
        let mut stat = format!(
            "cpu  {} 0 0 {} 0 0 0 0 0 0\n",
            cpu_total.saturating_sub(cpu_idle),
            cpu_idle
        );
        for i in 0..cores {
            stat.push_str(&format!("cpu{i} 1 0 0 1 0 0 0 0 0 0\n"));
        }
        write_file(&dir.join("stat"), &stat);
        write_file(
            &dir.join("meminfo"),
            "MemTotal:       16000000 kB\nMemAvailable:    8000000 kB\nSwapTotal:       2000000 kB\nSwapFree:        1000000 kB\n",
        );
        write_file(
            &dir.join("net/dev"),
            "Inter-|   Receive\n face |bytes\n    lo: 5 0 0 0 0 0 0 0 5 0\n  eth0: 1000 0 0 0 0 0 0 0 2000 0\n",
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_file(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

/// Runs the built binary and returns its standard output.
pub fn run(args: &[&str]) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_hostscope"))
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "hostscope {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The cells a drawn line takes. The application counts them with the same
/// Unicode tables, so this is no longer an independent source for the table
/// itself - what it still judges from outside is how the frame USES the width:
/// truncation, padding, the column plan. The independent second opinion on the
/// table lives in `scripts/frame-lint.py`, which reads Python's own tables.
pub fn width(s: &str) -> usize {
    s.chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

/// A frame as the terminal would show it: `--dump-frame` prints frames
/// separated by a blank line.
pub fn frames(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
