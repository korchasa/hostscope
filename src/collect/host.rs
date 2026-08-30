//! The host summary of level L0 and the network counters of an interface set.

use std::fs;
use std::path::Path;

use crate::model::{Pressure, Stall};

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

/// Reads `<root>/pressure/{cpu,memory,io}`. A kernel built without the feature,
/// or booted with it off, has no such directory - and then there is no reading
/// at all rather than a machine that never waited for anything (D-13).
pub fn pressure(proc_root: &Path) -> Pressure {
    let one = |name: &str| {
        fs::read_to_string(proc_root.join("pressure").join(name))
            .map(|t| stall(&t))
            .unwrap_or_default()
    };
    Pressure {
        cpu: one("cpu"),
        mem: one("memory"),
        io: one("io"),
    }
}

/// `some avg10=0.00 avg60=0.00 avg300=0.00 total=0`, two lines of it.
fn stall(text: &str) -> Stall {
    let mut s = Stall::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let which = it.next();
        let (a, b) = match which {
            Some("some") => (&mut s.some10, &mut s.some60),
            Some("full") => (&mut s.full10, &mut s.full60),
            _ => continue,
        };
        for f in it {
            if let Some(v) = f.strip_prefix("avg10=") {
                *a = v.parse().ok();
            } else if let Some(v) = f.strip_prefix("avg60=") {
                *b = v.parse().ok();
            }
        }
    }
    s
}

/// The summed speed of the PHYSICAL links, in bytes per second, so the network
/// figures of the host row can be read as a share of what the wire can carry
/// (D-42).
///
/// Physical means an interface carrying a `device` entry. Not the set
/// `parse_net_dev` sums: that one takes `docker0` and one `veth` per
/// container, so the denominator would grow with the number of rows on screen.
///
/// `speed` answers `-1` for a link that is down and fails outright on an
/// interface with no fixed rate, so only a positive reading is summed. Nothing
/// summed means the reading is unavailable, never a guess.
pub fn link_speed(class_net: &Path) -> Option<f64> {
    let mut bits = 0.0;
    for entry in fs::read_dir(class_net).ok()?.flatten() {
        let d = entry.path();
        if fs::symlink_metadata(d.join("device")).is_err() {
            continue;
        }
        if let Some(mbit) = read_trim(&d.join("speed")).and_then(|t| t.parse::<f64>().ok()) {
            if mbit > 0.0 {
                bits += mbit * 1_000_000.0;
            }
        }
    }
    (bits > 0.0).then_some(bits / 8.0)
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

    /// The one quantity that means the same on a machine with one core and on
    /// one with two hundred: the share of time something waited (D-46). The
    /// kernel writes two lines - `some`, where at least one task was stalled,
    /// and `full`, where every non-idle task was stalled at once, which its own
    /// documentation calls thrashing.
    #[test]
    fn pressure_is_read_as_a_share_of_time_and_not_of_the_machine() {
        let text = "some avg10=1.23 avg60=4.56 avg300=0.07 total=14765441644\n\
                    full avg10=0.00 avg60=2.50 avg300=0.00 total=0\n";
        let s = stall(text);
        assert_eq!(s.some10, Some(1.23));
        assert_eq!(s.some60, Some(4.56));
        assert_eq!(s.full10, Some(0.0));
        assert_eq!(s.full60, Some(2.50));
        // A kernel built without pressure leaves no file at all, and an empty
        // one is not a machine that never waited (D-13).
        assert_eq!(stall(""), Stall::default());
    }

    #[test]
    fn the_three_resources_are_read_and_a_missing_one_stays_unavailable() {
        let dir = std::env::temp_dir().join(format!("hs-psi-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("pressure")).unwrap();
        fs::write(
            dir.join("pressure/cpu"),
            "some avg10=9.00 avg60=1.00 avg300=0.00 total=1\n",
        )
        .unwrap();
        fs::write(
            dir.join("pressure/io"),
            "some avg10=3.00 avg60=1.00 avg300=0.00 total=1\nfull avg10=2.00 avg60=0.00 avg300=0.00 total=1\n",
        )
        .unwrap();
        let p = pressure(&dir);
        assert_eq!(p.cpu.some10, Some(9.0));
        assert_eq!(p.io.full10, Some(2.0));
        assert_eq!(p.mem.some10, None, "the file was not there");
    }

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

    /// Builds a `/sys/class/net` the way the kernel lays one out: a physical
    /// interface carries a `device` entry, a virtual one does not.
    fn a_class_net(case: &str, ifs: &[(&str, &str, bool)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hs-net-{}-{}", std::process::id(), case));
        let _ = fs::remove_dir_all(&dir);
        for (name, speed, physical) in ifs {
            let d = dir.join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("speed"), speed).unwrap();
            if *physical {
                fs::write(d.join("device"), "").unwrap();
            }
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_link_speed_sums_the_physical_interfaces_and_nothing_else() {
        // The bridge and the veth of a container report a speed too, and
        // summing them would grow the denominator with the rows on screen.
        let dir = a_class_net(
            "physical",
            &[
                ("eth0", "1000\n", true),
                ("eth1", "1000\n", true),
                ("docker0", "10000\n", false),
                ("veth1a2b", "10000\n", false),
            ],
        );
        // 2000 Mbit/s is 250 MB/s.
        assert_eq!(link_speed(&dir), Some(250_000_000.0));
    }

    #[test]
    fn a_link_that_reports_no_rate_is_left_out_rather_than_guessed() {
        // A link that is down answers -1, and one with no fixed rate fails the
        // read outright. Neither is a number to divide by.
        let dir = a_class_net("down", &[("eth0", "-1\n", true), ("eth1", "\n", true)]);
        assert_eq!(link_speed(&dir), None);
    }

    #[test]
    fn a_machine_with_no_sysfs_reports_no_link_speed() {
        assert_eq!(
            link_speed(std::path::Path::new("/nonexistent/class/net")),
            None
        );
    }
}
