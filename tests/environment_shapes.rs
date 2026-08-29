//! The three environments hostscope was measured against, and what a process
//! looks like on each of them (FR-20, D-23).
//!
//! Layer V3 of the testing document: the whole binary over a captured tree.
//! What is checked here is not a number but a shape - that the tree is the
//! process forest of the host on all three, and that every process says what
//! runs it, whichever way its runtime lays its cgroups out.

mod support;

use support::{frames, run, Fixture};

fn walk(f: &Fixture, keys: &str) -> Vec<Vec<String>> {
    walk_at(f, keys, "100x30")
}

/// The same walk at a stated terminal size. Width is what decides how much of a
/// name is drawn, so a test about a name has to say which width it means.
fn walk_at(f: &Fixture, keys: &str, size: &str) -> Vec<Vec<String>> {
    frames(&run(&[
        "--cgroup-root",
        f.cgroup_root().to_str().unwrap(),
        "--proc-root",
        f.proc_root().to_str().unwrap(),
        "--docker-socket",
        "none",
        "--dump-frame",
        "1",
        "--tick",
        "60",
        "--size",
        size,
        "--keys",
        keys,
    ]))
}

fn model(f: &Fixture) -> String {
    run(&[
        "--cgroup-root",
        f.cgroup_root().to_str().unwrap(),
        "--proc-root",
        f.proc_root().to_str().unwrap(),
        "--docker-socket",
        "none",
        "--dump-model",
        "json",
    ])
}

/// The table of the last frame a key program produced, without the blank rows.
fn table(frames: &[Vec<String>]) -> Vec<String> {
    let last = frames.last().expect("at least one frame");
    last[7..last.len() - 4]
        .iter()
        .filter(|l| !l.chars().skip(1).take(97).all(|c| c == ' '))
        .cloned()
        .collect()
}

/// The drawn row of a named process, name column and owner column together.
fn row(frames: &[Vec<String>], name: &str) -> String {
    table(frames)
        .into_iter()
        .find(|l| l.contains(name))
        .unwrap_or_else(|| panic!("no row named {name} in:\n{}", table(frames).join("\n")))
}

fn owners(model: &str, kind: &str) -> usize {
    model
        .matches(&format!("\"owner_kind\": \"{kind}\""))
        .count()
}

/// The children of the one process at the root of these fixtures, with no
/// filter left over: the filter picks the root out by name, the arrow descends
/// into it, and the empty filter that follows puts the whole level back on
/// screen - the filter survives a descent by design (FR-2).
fn under(root: &str) -> String {
    let letters: Vec<String> = root.chars().map(|c| c.to_string()).collect();
    format!("/ {} Enter Right / Enter", letters.join(" "))
}

#[test]
fn docker_with_the_cgroupfs_driver_is_read_the_same_as_any_other_host() {
    let f = Fixture::new("shape-cgroupfs");
    f.shape_docker_cgroupfs();
    // The name of such a cgroup carries no prefix and no suffix, which is why
    // the earlier name-only rule found none of these containers at all.
    assert_eq!(owners(&model(&f), "container"), 2);

    let inside = walk(&f, &under("systemd"));
    assert!(
        row(&inside, "nginx").contains("3c9e0142ab76"),
        "{}",
        row(&inside, "nginx")
    );
    assert!(
        row(&inside, "redis-server").contains("4a1b76d29e05"),
        "{}",
        row(&inside, "redis-server")
    );
}

#[test]
fn a_kubernetes_node_names_the_container_behind_every_process() {
    let f = Fixture::new("shape-k8s");
    f.shape_kubernetes();
    // Three directories below the only name that means anything, and the
    // classes of service - besteffort, burstable - name nothing themselves.
    assert_eq!(owners(&model(&f), "container"), 2);

    let inside = walk(&f, &under("init"));
    assert!(
        row(&inside, "coredns").contains("5e8c31a70b49"),
        "{}",
        row(&inside, "coredns")
    );
    assert!(
        row(&inside, "traefik").contains("6b3d92f05a81"),
        "{}",
        row(&inside, "traefik")
    );
    // The server that runs the node is not in a container and must not be
    // labelled as if it were.
    assert!(
        !row(&inside, "k3s-server").contains("pod"),
        "{}",
        row(&inside, "k3s-server")
    );
}

#[test]
fn a_plain_server_names_the_service_the_container_or_the_user() {
    let f = Fixture::new("shape-systemd");
    f.shape_systemd_docker();
    let m = model(&f);
    assert_eq!(owners(&m, "container"), 2);
    assert_eq!(owners(&m, "service"), 2);
    assert_eq!(owners(&m, "user"), 2);

    let inside = walk(&f, &under("systemd"));
    assert!(
        row(&inside, "sshd").contains("ssh"),
        "{}",
        row(&inside, "sshd")
    );
    assert!(
        row(&inside, "dockerd").contains("docker"),
        "{}",
        row(&inside, "dockerd")
    );
    assert!(
        row(&inside, "postgres").contains("1d4c8f2b6a30"),
        "{}",
        row(&inside, "postgres")
    );
    assert!(
        row(&inside, "bash").contains("1000"),
        "{}",
        row(&inside, "bash")
    );
    // The service manager of a login session belongs to the session, not to the
    // list of services the host offers.
    assert!(
        !m.contains("\"owner\": \"user@1000\""),
        "a session service joined the host's services"
    );
}

/// The tree is the process forest and nothing else: the level below a process
/// holds the processes it started, and the path line says whose level it is.
#[test]
fn the_level_under_a_process_holds_the_processes_it_started() {
    let f = Fixture::new("shape-forest");
    f.shape_systemd_docker();
    let top = table(&walk(&f, ""));
    assert_eq!(
        top.len(),
        2,
        "the host opens on its one root process:\n{top:?}"
    );
    assert!(top.iter().any(|l| l.contains("(self)")));
    assert!(top.iter().any(|l| l.contains("systemd")));

    let inside = walk(&f, &under("systemd"));
    let path = &inside.last().unwrap()[4];
    assert!(path.contains("host \u{203a} systemd"), "{path}");
    assert!(path.contains("L1"), "{path}");
    for child in ["sshd", "dockerd", "postgres", "grafana", "bash"] {
        row(&inside, child);
    }
}

/// The owner is a property of a row, so the filter has to reach it: an engineer
/// who knows only the name of the container must find its processes wherever
/// their runtime hung them in the forest (FR-2, FR-20).
#[test]
fn the_filter_reaches_what_runs_a_process() {
    let f = Fixture::new("shape-filter");
    f.shape_systemd_docker();

    let by_kind = table(&walk(
        &f,
        &format!("{} / c o n t a i n e r Enter", under("systemd")),
    ));
    assert_eq!(by_kind.len(), 2, "{by_kind:?}");
    assert!(
        by_kind.iter().any(|l| l.contains("postgres")),
        "{by_kind:?}"
    );
    assert!(by_kind.iter().any(|l| l.contains("grafana")), "{by_kind:?}");

    let by_id = table(&walk(
        &f,
        &format!("{} / 1 d 4 c 8 f 2 b 6 a 3 0 Enter", under("systemd")),
    ));
    assert_eq!(by_id.len(), 1, "{by_id:?}");
    assert!(by_id[0].contains("postgres"), "{by_id:?}");
}

/// D-30: a shim is a real process of the runtime and keeps `containerd` in its
/// `OWNER` column, so the level it stands on used to be a wall of rows all
/// reading `containerd-shim` - 20 of the 75 rows under `systemd` on the Docker
/// rig, one for every container of the host. The row now says in parentheses
/// what it leads into, and a row that leads into two containers says nothing:
/// one row cannot name two, and half a name is worse than none.
#[test]
fn a_row_leading_into_one_container_names_it() {
    let f = Fixture::new("shim-boundary");
    f.shape_shim_boundary();

    let level = walk_at(&f, &under("systemd"), "160x30");
    let leading = table(&level)
        .into_iter()
        .find(|l| l.contains("containerd-shim ("))
        .unwrap_or_else(|| {
            panic!(
                "no row names the container it leads into in:\n{}",
                table(&level).join("\n")
            )
        });

    assert!(
        leading.contains("(5b21e4f70a9c)"),
        "the row must name the container under it: {leading:?}"
    );
    assert!(
        leading.contains("containerd"),
        "and keep its own owner, which is the runtime: {leading:?}"
    );
    let two: Vec<String> = table(&level)
        .into_iter()
        .filter(|l| l.contains("containerd-shim"))
        .collect();
    assert_eq!(two.len(), 2, "{two:?}");
    assert_eq!(
        two.iter().filter(|l| l.contains('(')).count(),
        1,
        "the shim with two containers under it names neither: {two:?}"
    );

    // A column too narrow for both halves cuts the parenthesised one, with the
    // ellipsis that marks every other cut (D-30, section 11). The row still
    // reads as the process it is: cutting the process name instead would make
    // two rows of different processes read alike.
    let narrow = table(&walk(&f, &under("systemd")))
        .into_iter()
        .find(|l| l.contains("containerd-shim ("))
        .unwrap_or_else(|| panic!("nothing leads into a container at 100 cells"));
    assert!(narrow.contains('\u{2026}'), "the cut is marked: {narrow:?}");
    assert!(
        !narrow.contains("(5b21e4f70a9c)"),
        "and it is the container half that is cut: {narrow:?}"
    );
}

/// D-31: on a Kubernetes node a shim carries two children - the pod's `pause`
/// sandbox and the workload container - so D-30 named nothing and the level came
/// back as rows all reading `containerd-shim`. What the two have in common is
/// the pod, so the row says the pod. Two containers of two different pods still
/// name nothing: one row cannot name two.
#[test]
fn a_row_leading_into_one_pod_names_the_pod() {
    let f = Fixture::new("pod-sandbox");
    f.shape_pod_sandbox();

    let level = table(&walk_at(&f, &under("systemd"), "160x30"));
    let shims: Vec<String> = level
        .into_iter()
        .filter(|l| l.contains("containerd-shim"))
        .collect();
    assert_eq!(shims.len(), 2, "{shims:?}");

    let named: Vec<&String> = shims.iter().filter(|l| l.contains('(')).collect();
    assert_eq!(
        named.len(),
        1,
        "only the shim of one pod is named: {shims:?}"
    );
    assert!(
        named[0].contains("(pod 49ccade5)"),
        "the row names the pod its containers share: {named:?}"
    );
}

/// The name in parentheses is part of what the row shows, so the filter reaches
/// it and the row that leads into a container is found by that container's name
/// (D-30, D-29). Without this the reader types the name, the row says it, and
/// the row disappears.
#[test]
fn the_filter_reaches_the_container_a_row_leads_into() {
    let f = Fixture::new("shim-filter");
    f.shape_shim_boundary();

    let found = table(&walk(
        &f,
        &format!("{} / 5 b 2 1 e 4 f 7 0 a 9 c Enter", under("systemd")),
    ));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("containerd-shim"), "{found:?}");
}

/// The owner column labels rows, it does not build them. Whatever the shape of
/// the host, a row still carries its whole subtree and no more (FR-5, FR-14),
/// and every process the model gives an owner shows that owner on its row.
///
/// The sum is checked over the dump rather than over the drawn text: a cell is
/// rounded to the width it is drawn in, and four rounded cells do not add up to
/// one rounded total (invariant 6 of the testing document).
#[test]
fn the_owner_column_labels_rows_and_does_not_move_values() {
    for (name, build) in [("col-cgroupfs", 0usize), ("col-k8s", 1), ("col-systemd", 2)] {
        let f = Fixture::new(name);
        match build {
            0 => f.shape_docker_cgroupfs(),
            1 => f.shape_kubernetes(),
            _ => f.shape_systemd_docker(),
        }
        let m = model(&f);
        // A check that reads nothing agrees with everything, which is how the
        // earlier version of this test passed while measuring blank space.
        let sums = nodes_with_children(&m);
        assert!(
            sums.len() >= 2,
            "{name}: the dump gave {} nodes to sum",
            sums.len()
        );
        assert!(
            sums.iter().any(|(_, c)| *c > 0.0),
            "{name}: every child sums to nothing"
        );
        for (node, children) in sums {
            assert!(
                children <= node + 1.0,
                "{name}: the children carry {children} bytes under a row of {node}"
            );
        }
        let root = if build == 1 { "init" } else { "systemd" };
        let level = table(&walk(&f, &under(root)));
        assert!(
            level.iter().any(|l| l.contains("(self)")),
            "{name}: no remainder row:\n{level:?}"
        );
        // Every owner the model names is on a row, cut to the column but never
        // absent from it.
        let names = owner_names(&m);
        assert!(!names.is_empty(), "{name}: the dump names no owner at all");
        for owner in names {
            let short: String = owner.chars().take(12).collect();
            assert!(
                level.iter().any(|l| l.contains(&short)),
                "{name}: the owner {owner} is on no row:\n{level:?}"
            );
        }
    }
}

/// Every node of the dump that has children, as `(its memory, their memory)`.
fn nodes_with_children(model: &str) -> Vec<(f64, f64)> {
    // The dump is read as text on purpose: a parser shared with the application
    // would repeat the application's error (section 5 of the testing document).
    let mut out = Vec::new();
    let mem_of = |chunk: &str| -> Option<f64> {
        let at = chunk.find("\"instant\": {\"cpu\": ")?;
        let tail = &chunk[at..];
        let at = tail.find("\"mem\": ")? + 7;
        let rest = &tail[at..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))?;
        rest[..end].parse().ok()
    };
    for (i, _) in model.match_indices("\"children\": [\n") {
        // The node this list belongs to starts at the `{` its own line opens.
        let before = &model[..i];
        let start = match before
            .rfind("\n      {\n")
            .or_else(|| before.rfind("\"tree\": "))
        {
            Some(p) => p,
            None => continue,
        };
        let node = match mem_of(&model[start..i]) {
            Some(v) => v,
            None => continue,
        };
        // Its children are the nodes one nesting level in, which the dump
        // indents further than the node itself; summing every `"instant"` after
        // the opening bracket would reach grandchildren too, so the depth is
        // taken from the indent of the closing bracket.
        let mut sum = 0.0;
        let mut depth = 0i32;
        let rest = &model[i + "\"children\": [\n".len()..];
        let mut chunk_start = 0usize;
        for (j, c) in rest.char_indices() {
            match c {
                '{' | '[' => {
                    if depth == 0 {
                        chunk_start = j;
                    }
                    depth += 1;
                }
                '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(v) = mem_of(&rest[chunk_start..j]) {
                            sum += v;
                        }
                    }
                    if depth < 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push((node, sum));
    }
    out
}

fn owner_names(model: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, _) in model.match_indices("\"owner\": \"") {
        let rest = &model[i + "\"owner\": \"".len()..];
        if let Some(end) = rest.find('"') {
            let name = &rest[..end];
            if !name.is_empty() && !out.iter().any(|o| o == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}
