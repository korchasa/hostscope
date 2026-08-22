//! The host summary of level L0 and the network counters of an interface set.

use std::fs;
use std::path::Path;

/// Raw host counters of one tick. Cumulative ones go through the sampler like
/// every other counter (FR-15).
#[derive(Clone, Debug, Default)]
pub struct RawHost {
    pub hostname: String,
    pub kernel: String,
    pub uptime: f64,
    pub cores: f64,
    pub cpu_total: f64,
    pub cpu_idle: f64,
    pub mem_total: f64,
    pub mem_available: f64,
    pub swap_total: f64,
    pub swap_free: f64,
    pub load: [f64; 3],
    pub net_rx: f64,
    pub net_tx: f64,
}

pub fn read(proc_root: &Path) -> RawHost {
    let mut h = RawHost {
        hostname: read_trim(&proc_root.join("sys/kernel/hostname"))
            .unwrap_or_else(|| "host".into()),
        kernel: read_trim(&proc_root.join("sys/kernel/osrelease")).unwrap_or_default(),
        ..RawHost::default()
    };
    if let Some(t) = read_trim(&proc_root.join("uptime")) {
        h.uptime = t
            .split_whitespace()
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
    }
    if let Ok(text) = fs::read_to_string(proc_root.join("stat")) {
        let (total, idle, cores) = cpu_line(&text);
        h.cpu_total = total;
        h.cpu_idle = idle;
        h.cores = cores;
    }
    if let Ok(text) = fs::read_to_string(proc_root.join("meminfo")) {
        h.mem_total = meminfo(&text, "MemTotal");
        h.mem_available = meminfo(&text, "MemAvailable");
        h.swap_total = meminfo(&text, "SwapTotal");
        h.swap_free = meminfo(&text, "SwapFree");
    }
    if let Some(t) = read_trim(&proc_root.join("loadavg")) {
        let f: Vec<f64> = t
            .split_whitespace()
            .take(3)
            .filter_map(|v| v.parse().ok())
            .collect();
        for (i, v) in f.iter().enumerate().take(3) {
            h.load[i] = *v;
        }
    }
    if let Some((rx, tx)) = net_dev(&proc_root.join("net/dev")) {
        h.net_rx = rx;
        h.net_tx = tx;
    }
    h
}

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|t| t.trim().to_string())
}

/// Returns total jiffies, idle jiffies (idle plus iowait) and the number of
/// cores, counted from the per-core lines.
pub fn cpu_line(text: &str) -> (f64, f64, f64) {
    let mut total = 0.0;
    let mut idle = 0.0;
    let mut cores = 0.0f64;
    for line in text.lines() {
        if line.starts_with("cpu ") {
            let f: Vec<f64> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|v| v.parse().ok())
                .collect();
            total = f.iter().sum();
            idle = f.get(3).copied().unwrap_or(0.0) + f.get(4).copied().unwrap_or(0.0);
        } else if line.starts_with("cpu") {
            cores += 1.0;
        }
    }
    (total, idle, cores.max(1.0))
}

fn meminfo(text: &str, key: &str) -> f64 {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(rest) = rest.strip_prefix(':') {
                return rest
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0)
                    * 1024.0;
            }
        }
    }
    0.0
}

/// Sums the byte counters of every interface except the loopback. This is the
/// only network source the kernel offers without eBPF, and inside a container's
/// own namespace it is exactly that container's traffic (FR-11).
pub fn net_dev(path: &Path) -> Option<(f64, f64)> {
    let text = fs::read_to_string(path).ok()?;
    Some(parse_net_dev(&text))
}

pub fn parse_net_dev(text: &str) -> (f64, f64) {
    let (mut rx, mut tx) = (0.0, 0.0);
    for line in text.lines().skip(2) {
        let (name, rest) = match line.split_once(':') {
            Some(p) => p,
            None => continue,
        };
        let name = name.trim();
        if name == "lo" {
            continue;
        }
        let f: Vec<f64> = rest
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        rx += f.first().copied().unwrap_or(0.0);
        tx += f.get(8).copied().unwrap_or(0.0);
    }
    (rx, tx)
}

/// The inode of a process's network namespace. Equal to our own means the
/// process shares the host namespace, and its traffic cannot be attributed
/// without eBPF (FR-11).
pub fn netns_ino(proc_root: &Path, pid: i32) -> Option<u64> {
    let link = fs::read_link(proc_root.join(pid.to_string()).join("ns/net")).ok()?;
    let s = link.to_string_lossy().to_string();
    let inner = s.strip_prefix("net:[")?.strip_suffix(']')?;
    inner.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_cores_and_idle() {
        let text = "cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 0 1 1\ncpu1 1 0 1 1\nintr 0\n";
        let (total, idle, cores) = cpu_line(text);
        assert_eq!(total, 1000.0);
        assert_eq!(idle, 800.0);
        assert_eq!(cores, 2.0);
    }

    #[test]
    fn skips_the_loopback_in_the_interface_total() {
        let text = "Inter-|   Receive\n face |bytes\n    lo: 999 0 0 0 0 0 0 0 999 0\n  eth0: 100 0 0 0 0 0 0 0 200 0\n";
        assert_eq!(parse_net_dev(text), (100.0, 200.0));
    }
}
