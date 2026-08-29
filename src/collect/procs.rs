//! Reading `/proc`. The cheap part runs every tick for every process of a
//! cgroup; the expensive part (open files, connections) runs only when a card
//! is opened, so it never enters the 50 ms frame budget.
//!
//! FR-9: `/proc/<pid>/environ` is not opened at all, by this module or by any
//! other. Under root it holds the tokens and keys of every service on the host,
//! and nothing on screen needs it.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::util::clean;

/// The kernel measures process time in USER_HZ, which is 100 on every Linux
/// port this tool targets.
pub const USER_HZ: f64 = 100.0;
pub const PAGE_SIZE: f64 = 4096.0;

#[derive(Clone, Debug)]
pub struct ProcSample {
    pub ppid: i32,
    pub comm: String,
    pub cmdline: Option<String>,
    pub uid: u32,
    pub threads: u64,
    pub starttime: u64,
    pub cpu_seconds: f64,
    pub rss: f64,
    pub vsz: f64,
    pub io: Option<(f64, f64)>,
    /// `VmSwap` from `/proc/<pid>/status`, read only where the host has
    /// swapped something (D-35). `None` where the file could not be read or
    /// carries no such line, which is every kernel thread.
    pub swap: Option<f64>,
}

/// Everything a process card adds on top of the sample. Fields root would have
/// provided and this run could not read are listed in `restricted` rather than
/// shown as zero (FR-8).
#[derive(Clone, Debug, Default)]
pub struct ProcExtras {
    pub files: Option<usize>,
    pub sockets: Option<usize>,
    pub conns: Vec<String>,
    /// The limits as named values, one per row of the card: a line that
    /// carried both made the reader look for a word inside it (D-32).
    pub limits: Vec<(&'static str, String)>,
    pub pss: Option<f64>,
    /// What the kernel has moved out of RAM for this process. It comes from
    /// `/proc/<pid>/status`, which any user may read - unlike `smaps_rollup`
    /// beside it, so a run without root still answers the question the card
    /// was opened for.
    pub swap: Option<f64>,
    pub restricted: Vec<&'static str>,
}

/// What a command line costs to read again, kept between ticks. The key is the
/// pid together with its start time, so a reused pid never inherits the line of
/// the process that held the number before it.
pub type CmdCache = std::collections::HashMap<i32, (u64, Option<String>, u32)>;

/// One process. `deep` asks for the files only a visible row needs - the
/// command line and the I/O counters. Off the visible level the command line
/// comes from the cache and the I/O is not read at all: neither can reach the
/// screen from there, and both cost a syscall per process per tick.
pub fn read(
    proc_root: &Path,
    pid: i32,
    deep: bool,
    want_swap: bool,
    cache: &mut CmdCache,
) -> Option<ProcSample> {
    let dir = proc_root.join(pid.to_string());
    let stat = crate::collect::cgroup::read_file(&dir.join("stat"))?;
    let parsed = parse_stat(&stat)?;
    let cached = cache
        .get(&pid)
        .filter(|(start, _, _)| *start == parsed.starttime)
        .cloned();
    // The owner of `/proc/<pid>` is the user of the process, so one `stat` on
    // the directory replaces reading `status`, a file the kernel generates line
    // by line. Measured on the test host: `status` for 366 processes cost 8.5 ms,
    // the `stat` calls cost 0.9 ms. The user of a process never changes, so the
    // answer is kept next to the command line.
    let (cmdline, uid) = match cached {
        Some((_, line, uid)) if !deep => (line, uid),
        _ => {
            let line = read_cmdline(&dir);
            let uid = fs::metadata(&dir).map(|m| m.uid()).unwrap_or(0);
            cache.insert(pid, (parsed.starttime, line.clone(), uid));
            (line, uid)
        }
    };
    let io = if !deep {
        None
    } else {
        crate::collect::cgroup::read_file(&dir.join("io")).map(|t| {
            (
                crate::collect::cgroup::field(&t.replace(':', ""), "read_bytes").unwrap_or(0.0),
                crate::collect::cgroup::field(&t.replace(':', ""), "write_bytes").unwrap_or(0.0),
            )
        })
    };
    // `status` is generated line by line by the kernel and costs about twice
    // what `stat` costs: measured on the Kubernetes rig on 2026-08-29, 2.7 ms
    // for `stat` over 221 processes against 7.2 ms for both. That is why it is
    // read only where its one useful line can be non-zero (D-35).
    let swap = if want_swap {
        crate::collect::cgroup::read_file(&dir.join("status")).and_then(|t| vm_swap(&t))
    } else {
        None
    };
    Some(ProcSample {
        ppid: parsed.ppid,
        comm: parsed.comm.clone(),
        cmdline,
        uid,
        threads: parsed.threads,
        starttime: parsed.starttime,
        cpu_seconds: (parsed.utime + parsed.stime) / USER_HZ,
        rss: parsed.rss_pages * PAGE_SIZE,
        vsz: parsed.vsize,
        io,
        swap,
    })
}

/// The one line of `/proc/<pid>/status` this tool reads. A process without an
/// address space - every kernel thread - has no such line, and the answer is
/// then absent rather than zero (D-13).
pub fn vm_swap(text: &str) -> Option<f64> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmSwap:") {
            return rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .map(|kb| kb * 1024.0);
        }
    }
    None
}

fn read_cmdline(dir: &Path) -> Option<String> {
    fs::read(dir.join("cmdline")).ok().and_then(|b| {
        let joined: Vec<u8> = b
            .split(|c| *c == 0)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(&b' ');
        if joined.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&joined).to_string())
        }
    })
}

struct StatFields {
    ppid: i32,
    comm: String,
    utime: f64,
    stime: f64,
    threads: u64,
    starttime: u64,
    vsize: f64,
    rss_pages: f64,
}

/// `/proc/<pid>/stat` cannot be split on spaces: the command sits in
/// parentheses and may contain both spaces and parentheses, so the tail is
/// taken after the last `)`.
fn parse_stat(text: &str) -> Option<StatFields> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = text.get(close + 2..)?.split_whitespace().collect();
    // rest[0] is field 3 (state), so field N is rest[N - 3].
    let get = |n: usize| rest.get(n - 3).and_then(|v| v.parse::<f64>().ok());
    Some(StatFields {
        ppid: get(4).unwrap_or(0.0) as i32,
        comm,
        utime: get(14).unwrap_or(0.0),
        stime: get(15).unwrap_or(0.0),
        threads: get(20).unwrap_or(1.0) as u64,
        starttime: get(22).unwrap_or(0.0) as u64,
        vsize: get(23).unwrap_or(0.0),
        rss_pages: get(24).unwrap_or(0.0),
    })
}

/// The user table, read once at start. `/etc/passwd` is the only file outside
/// `/proc` and `/sys` this tool opens for data.
pub fn user_table() -> std::collections::HashMap<u32, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(text) = fs::read_to_string("/etc/passwd") {
        for line in text.lines() {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() > 2 {
                if let Ok(uid) = f[2].parse::<u32>() {
                    map.insert(uid, clean(f[0]));
                }
            }
        }
    }
    map
}

/// The card extras. Every failure degrades to a marker rather than to a zero.
pub fn extras(proc_root: &Path, pid: i32) -> ProcExtras {
    let dir = proc_root.join(pid.to_string());
    let mut e = ProcExtras::default();

    let mut inodes = Vec::new();
    match fs::read_dir(dir.join("fd")) {
        Ok(entries) => {
            let mut files = 0usize;
            let mut socks = 0usize;
            for entry in entries.flatten() {
                files += 1;
                if let Ok(target) = fs::read_link(entry.path()) {
                    let t = target.to_string_lossy().to_string();
                    if let Some(ino) = t.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']'))
                    {
                        socks += 1;
                        if let Ok(n) = ino.parse::<u64>() {
                            inodes.push(n);
                        }
                    }
                }
            }
            e.files = Some(files);
            e.sockets = Some(socks);
        }
        Err(_) => e.restricted.push("open files"),
    }

    e.conns = connections(&dir, &inodes);
    if e.conns.is_empty() && e.sockets.is_none() {
        e.restricted.push("connections");
    }

    if let Ok(text) = fs::read_to_string(dir.join("limits")) {
        for line in text.lines() {
            if let Some(v) = limit_line(line, "Max open files") {
                e.limits.push(("nofile", v));
            } else if let Some(v) = limit_line(line, "Max processes") {
                e.limits.push(("nproc", v));
            }
        }
    }

    match fs::read_to_string(dir.join("status")) {
        Ok(text) => e.swap = vm_swap(&text),
        // A kernel thread has no address space and no `VmSwap` line at all, so
        // an absent value is left absent rather than turned into a zero (D-13).
        Err(_) => e.restricted.push("swap"),
    }

    match fs::read_to_string(dir.join("smaps_rollup")) {
        Ok(text) => {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("Pss:") {
                    if let Some(kb) = rest
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        e.pss = Some(kb * 1024.0);
                    }
                }
            }
        }
        Err(_) => e.restricted.push("PSS"),
    }

    e
}

fn limit_line(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?;
    let f: Vec<&str> = rest.split_whitespace().collect();
    if f.len() >= 2 {
        Some(format!("{}/{}", f[0], f[1]))
    } else {
        None
    }
}

/// Connections of the process, matched to its sockets by inode. The tables are
/// read inside the process's own network namespace, so a container's
/// connections are its own.
fn connections(dir: &Path, inodes: &[u64]) -> Vec<String> {
    if inodes.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (file, v6) in [("net/tcp", false), ("net/tcp6", true)] {
        let text = match fs::read_to_string(dir.join(file)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let ino: u64 = f[9].parse().unwrap_or(0);
            if !inodes.contains(&ino) {
                continue;
            }
            let state = tcp_state(f[3]);
            let local = hex_addr(f[1], v6);
            let remote = hex_addr(f[2], v6);
            out.push(if state == "LISTEN" {
                format!("{local}  LISTEN")
            } else {
                format!("{local} -> {remote}  {state}")
            });
        }
    }
    out.sort();
    out.dedup();
    out.truncate(8);
    out
}

fn tcp_state(code: &str) -> &'static str {
    match code {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}

fn hex_addr(s: &str, v6: bool) -> String {
    let (addr, port) = match s.split_once(':') {
        Some(p) => p,
        None => return s.to_string(),
    };
    let port: u16 = u16::from_str_radix(port, 16).unwrap_or(0);
    if v6 {
        let mut groups = Vec::new();
        for chunk in addr.as_bytes().chunks(8) {
            let word = std::str::from_utf8(chunk).unwrap_or("0");
            let v = u32::from_str_radix(word, 16).unwrap_or(0).swap_bytes();
            groups.push(format!("{:x}:{:x}", v >> 16, v & 0xffff));
        }
        format!("[{}]:{}", groups.join(":"), port)
    } else {
        let v = u32::from_str_radix(addr, 16).unwrap_or(0);
        format!(
            "{}.{}.{}.{}:{}",
            v & 0xff,
            (v >> 8) & 0xff,
            (v >> 16) & 0xff,
            (v >> 24) & 0xff,
            port
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_command_with_spaces_and_parentheses() {
        let line = "42 (my (odd) name) S 7 42 42 0 -1 4194560 100 0 0 0 11 22 0 0 20 0 3 0 999 12345 678 18446744073709551615";
        let p = parse_stat(line).unwrap();
        assert_eq!(p.comm, "my (odd) name");
        assert_eq!(p.ppid, 7);
        assert_eq!(p.utime, 11.0);
        assert_eq!(p.stime, 22.0);
        assert_eq!(p.threads, 3);
        assert_eq!(p.starttime, 999);
        assert_eq!(p.vsize, 12345.0);
        assert_eq!(p.rss_pages, 678.0);
    }

    #[test]
    fn decodes_an_address_the_way_the_kernel_wrote_it() {
        assert_eq!(hex_addr("0100007F:1F90", false), "127.0.0.1:8080");
        assert_eq!(hex_addr("00000000:0016", false), "0.0.0.0:22");
    }
}
