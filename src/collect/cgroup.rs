//! Reading the cgroup v2 hierarchy - not for its counters, but for the one
//! thing only it knows: which process belongs to which container, service or
//! login session (FR-20).
//!
//! Every process on a Linux host sits in exactly one cgroup, and the path of
//! that cgroup says what put it there. Nothing else here is read: the walk
//! opens `cgroup.procs` and no other file, which is also what makes a captured
//! snapshot a valid substitute for the live tree (FR-17, `--cgroup-root`).

use std::fs;
use std::path::Path;

use crate::model::{Owner, OwnerKind};

/// One cgroup directory: where it is, and which processes it holds.
#[derive(Clone, Debug, Default)]
pub struct RawCgroup {
    pub rel: String,
    pub procs: Vec<i32>,
    pub children: Vec<RawCgroup>,
}

/// Reads the whole hierarchy under `root`. Unreadable directories are skipped:
/// without root some nodes are simply not visible, and that must not stop the
/// walk (FR-8).
pub fn read_tree(root: &Path) -> RawCgroup {
    let mut node = read_one(root, "/");
    node.children = read_children(root, "");
    node
}

fn read_children(dir: &Path, rel: &str) -> Vec<RawCgroup> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    // `file_type` comes from the directory entry itself, while `path().is_dir()`
    // asks the kernel about every name again. Measured on the test host: 110
    // cgroups hold about 5500 entries between them, and that second question
    // cost more than reading all the counters.
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let child_rel = format!("{rel}/{name}");
        let mut child = read_one(&path, &child_rel);
        child.children = read_children(&path, &child_rel);
        out.push(child);
    }
    out
}

fn read_one(dir: &Path, rel: &str) -> RawCgroup {
    let mut c = RawCgroup {
        rel: rel.to_string(),
        ..RawCgroup::default()
    };
    if let Some(text) = read_file(&dir.join("cgroup.procs")) {
        c.procs = text
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect();
    }
    c
}

/// Every cgroup of the tree as a flat list of `(path, processes)`.
pub fn flatten(node: &RawCgroup, out: &mut Vec<(String, Vec<i32>)>) {
    if !node.procs.is_empty() {
        out.push((node.rel.clone(), node.procs.clone()));
    }
    for c in &node.children {
        flatten(c, out);
    }
}

/// A kernel file, read with the fewest syscalls it can be read with.
///
/// `fs::read_to_string` asks for the size first and then reads twice, because a
/// file in `sysfs` or `procfs` reports a size of zero. These files are small
/// and generated on the spot, so one open and one read is the whole story.
/// Measured on the test host: the walk over 110 cgroups went from 20 ms to 13 ms.
pub fn read_file(p: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    let mut file = fs::File::open(p).ok()?;
    let mut len = file.read(&mut buf).ok()?;
    // A file that filled the buffer may have more to give: fall back to the
    // straightforward read rather than truncating a value.
    if len == buf.len() {
        let mut rest = Vec::new();
        if file.read_to_end(&mut rest).is_ok() && !rest.is_empty() {
            let mut all = buf.to_vec();
            all.extend(rest);
            return Some(String::from_utf8_lossy(&all).into_owned());
        }
        len = buf.len();
    }
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

/// `key value` on its own line, the shape of `/proc/<pid>/io`.
pub fn field(text: &str, key: &str) -> Option<f64> {
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() == Some(key) {
            return it.next().and_then(|v| v.parse().ok());
        }
    }
    None
}

/// What the path of a cgroup says about the processes inside it, before the
/// container daemon is asked for a name.
#[derive(Clone, Debug, PartialEq)]
pub enum Ownership {
    Container(String),
    Service(String),
    User(u32),
    System,
}

/// Reads the whole path, not just its last component: a container may be three
/// directories below the name that says it is one, and a login session holds a
/// service manager of its own.
///
/// Where several apply, the innermost thing a person would name it by wins: a
/// container first, then the user whose session it is, then the service. That
/// order is what puts `user@1000.service` under the user rather than among the
/// services of the host - it is how a login session is run, not a service the
/// host offers.
pub fn ownership(rel: &str) -> Ownership {
    let mut under_runtime = false;
    let mut container = None;
    let mut service = None;
    let mut user = None;
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        if let Some(id) = container_id(part, under_runtime) {
            container = Some(id);
        } else if let Some(uid) = user_slice(part) {
            user = Some(uid);
        } else if let Some(name) = part.strip_suffix(".service") {
            service = Some(name.to_string());
        }
        if is_runtime_dir(part) {
            under_runtime = true;
        }
    }
    match (container, user, service) {
        (Some(id), _, _) => Ownership::Container(id),
        (None, Some(uid), _) => Ownership::User(uid),
        (None, None, Some(name)) => Ownership::Service(name),
        _ => Ownership::System,
    }
}

/// The owner a path with no container to enrich resolves to.
pub fn owner_of(rel: &str, users: &std::collections::HashMap<u32, String>) -> Owner {
    match ownership(rel) {
        Ownership::Container(id) => Owner {
            kind: OwnerKind::Container,
            name: short_id(&id),
        },
        Ownership::Service(name) => Owner {
            kind: OwnerKind::Service,
            name,
        },
        Ownership::User(uid) => Owner {
            kind: OwnerKind::User,
            name: users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
        },
        Ownership::System => Owner {
            kind: OwnerKind::System,
            name: String::new(),
        },
    }
}

/// `user-1000.slice`, the slice systemd gives one logged-in user.
fn user_slice(name: &str) -> Option<u32> {
    name.strip_suffix(".slice")?
        .strip_prefix("user-")?
        .parse()
        .ok()
}

/// The directories a container runtime keeps its containers in. A bare
/// identifier is a container only under one of these; anywhere else a long
/// hexadecimal name is just a name, and claiming it would label processes as
/// belonging to a container that does not exist.
pub fn is_runtime_dir(name: &str) -> bool {
    // OrbStack writes `.lxc` beside `docker`, and the systemd driver adds the
    // `.slice` suffix to everything, so neither decoration may decide.
    let stem = name.trim_start_matches('.');
    let stem = stem.strip_suffix(".slice").unwrap_or(stem);
    matches!(
        stem,
        "docker" | "containerd" | "crio" | "podman" | "machine"
    ) || stem.starts_with("kubepods")
}

/// The container identifier carried by the cgroup name, when there is one.
///
/// `under_runtime` is true when some directory on the way down was one a
/// container runtime keeps its containers in. Without it the name alone
/// decides, and the name alone is not enough: only the systemd cgroup driver
/// writes `docker-<id>.scope`, while the cgroupfs driver names the directory
/// with the bare identifier - measured on OrbStack (`/docker/<id>`) and on k3s
/// (`/kubepods/besteffort/pod<uid>/<id>`), where the earlier name-only rule
/// recognised none of the containers that were running (D-23).
pub fn container_id(name: &str, under_runtime: bool) -> Option<String> {
    let stem = name.strip_suffix(".scope").unwrap_or(name);
    for prefix in [
        "docker-",
        "cri-containerd-",
        "containerd-",
        "crio-",
        "libpod-",
    ] {
        if let Some(id) = stem.strip_prefix(prefix) {
            if is_hex_id(id, 12) {
                return Some(id.to_string());
            }
        }
    }
    // The cgroupfs driver leaves nothing but the identifier, so what the
    // directory is can only be read from the runtime directory above it.
    if under_runtime && is_hex_id(stem, 32) {
        return Some(stem.to_string());
    }
    None
}

fn is_hex_id(s: &str, least: usize) -> bool {
    s.len() >= least && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// The pod a cgroup path belongs to, when it belongs to one (D-31).
///
/// Both cgroup drivers write the pod into the path, in their own shape: the
/// cgroupfs driver as a directory of its own, `/kubepods/burstable/pod<uuid>/`,
/// and the systemd driver as one flattened name,
/// `kubepods-burstable-pod<uuid>.slice`, with the dashes of the UUID written as
/// underscores because a dash is what systemd builds the name out of.
///
/// The shape of the UUID is what decides, not the position of the component: a
/// directory called `pod` followed by anything else is not a pod, and reading
/// one would put a name on rows that have nothing to do with Kubernetes.
pub fn pod_id(rel: &str) -> Option<String> {
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        let stem = part.strip_suffix(".slice").unwrap_or(part);
        let rest = stem
            .strip_prefix("pod")
            .or_else(|| stem.split_once("-pod").map(|(_, r)| r));
        if let Some(rest) = rest {
            let id = rest.replace('_', "-");
            if is_pod_uuid(&id) {
                return Some(id);
            }
        }
    }
    None
}

fn is_pod_uuid(s: &str) -> bool {
    s.len() == 36
        && s.chars().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// The first group of a pod's UUID, which is what the row shows (D-31). It is
/// not a name, and the decision says why there is none to show on such a host.
pub fn short_pod(id: &str) -> String {
    id.split('-').next().unwrap_or(id).to_string()
}

/// The short form an engineer sees when the daemon cannot be reached (D-13).
pub fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_key_and_its_value() {
        let text = "read_bytes 105627599590933\nwrite_bytes 1\n";
        assert_eq!(field(text, "read_bytes"), Some(105627599590933.0));
        assert_eq!(field(text, "nope"), None);
    }

    /// Both cgroup drivers carry the pod, and neither writes it the same way
    /// (D-31). A directory that merely begins with `pod` is not one.
    #[test]
    fn the_pod_is_read_from_either_driver_layout() {
        let uuid = "49ccade5-8a0b-4389-99ae-0c74a2533472";
        assert_eq!(
            pod_id(&format!("/kubepods/burstable/pod{uuid}/460e0f0eceb5")),
            Some(uuid.to_string())
        );
        assert_eq!(
            pod_id(&format!(
                "/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod{}.slice/cri-containerd-460e.scope",
                uuid.replace('-', "_")
            )),
            Some(uuid.to_string())
        );
        assert_eq!(pod_id("/system.slice/podman.service"), None);
        assert_eq!(pod_id("/kubepods/besteffort"), None);
        assert_eq!(short_pod(uuid), "49ccade5");
    }

    /// The three layouts the captured environments showed, side by side (D-23).
    #[test]
    fn a_container_is_recognised_in_all_three_driver_layouts() {
        let bare = "3c9e0142ab76d5f8e29b47c03a15d6802f4e91b7c8a03d5f6e2b19c740a83e5b";
        // systemd driver: the name carries the prefix and says it itself.
        assert_eq!(
            ownership("/system.slice/docker-8f21c70b3d95e46a2c.scope"),
            Ownership::Container("8f21c70b3d95e46a2c".into())
        );
        // cgroupfs driver, as OrbStack runs it: the name says nothing.
        assert_eq!(
            ownership(&format!("/docker/{bare}")),
            Ownership::Container(bare.into())
        );
        // Kubernetes, three levels under the only name that means anything.
        assert_eq!(
            ownership(&format!(
                "/kubepods/besteffort/pod0b5d84c1-27ae-49f3-8c60-1d7e93af5620/{bare}"
            )),
            Ownership::Container(bare.into())
        );
        // A name that only looks like an identifier stays what it is.
        assert_eq!(ownership("/docker/buildkit"), Ownership::System);
    }

    #[test]
    fn the_innermost_name_a_person_knows_wins() {
        assert_eq!(
            ownership("/system.slice/ssh.service"),
            Ownership::Service("ssh".into())
        );
        // The service manager of a session belongs to the session, not to the
        // list of services the host offers.
        assert_eq!(
            ownership("/user.slice/user-1000.slice/user@1000.service"),
            Ownership::User(1000)
        );
        assert_eq!(ownership("/init.scope"), Ownership::System);
        assert_eq!(ownership("/"), Ownership::System);
        assert_eq!(ownership("/dev-mqueue.mount"), Ownership::System);
    }

    #[test]
    fn the_runtime_directory_is_recognised_in_both_spellings() {
        assert!(is_runtime_dir("docker"));
        assert!(is_runtime_dir("kubepods"));
        assert!(is_runtime_dir("kubepods-burstable.slice"));
        assert!(!is_runtime_dir("system.slice"));
        assert!(!is_runtime_dir("buildkit"));
    }
}
