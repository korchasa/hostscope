//! Layer V1 of the testing document: the application run over a captured
//! snapshot, with no live host involved. Deterministic, runs anywhere.

mod support;

use support::{run, FakeDocker, Fixture};

/// A host running a service, two containers and a login shell. `tick` moves
/// every cumulative counter forward, which is how a second snapshot is made.
fn build(f: &Fixture, tick: u64) {
    // The CPU counter of a process is cumulative and huge on purpose: FR-15
    // says nothing that accumulated before the start may reach the output.
    let base = 100_000_000u64 + tick * 100;
    f.host(1_000_000 + tick * 400, 900_000 + tick * 360, 4);
    f.cgroup("", &[]);
    f.cgroup("init.scope", &[1]);
    f.cgroup("system.slice/ssh.service", &[101]);
    f.cgroup("system.slice/docker-aaaaaaaaaaaa1111.scope", &[201]);
    f.cgroup("system.slice/docker-bbbbbbbbbbbb2222.scope", &[301]);
    f.process(1, 0, "systemd", 20, 100, "/sbin/init");
    f.process(101, 1, "sshd", base, 1000, "/usr/sbin/sshd -D");
    f.process(201, 1, "nginx", base / 2, 2000, "nginx: master process");
    f.process(301, 1, "redis-server", 50, 500, "redis-server *:6379");
    // One process can be read and one cannot: without root `/proc/<pid>/io`
    // belongs to its owner alone (FR-8).
    f.process_io(101, 1_000_000 + tick * 1000, 2_000_000 + tick * 2000);
}

fn dump(f: &Fixture) -> String {
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

fn number_after(text: &str, anchor: &str, key: &str) -> f64 {
    let start = text
        .find(anchor)
        .unwrap_or_else(|| panic!("no {anchor} in the dump"));
    let rest = &text[start..];
    let at = rest
        .find(key)
        .unwrap_or_else(|| panic!("no {key} after {anchor}"));
    let tail = &rest[at + key.len()..];
    let end = tail
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(tail.len());
    tail[..end]
        .parse()
        .unwrap_or_else(|_| panic!("not a number after {key}"))
}

/// The bytes a count of pages stands for.
fn pages(n: f64) -> f64 {
    n * 4096.0
}

#[test]
fn nothing_accumulated_before_the_start_reaches_the_output() {
    // FR-15: sshd has burned a million seconds of CPU before hostscope started,
    // and the first tick must still print zero.
    let f = Fixture::new("fr15");
    build(&f, 0);
    let text = dump(&f);
    assert!(text.contains("\"name\": \"sshd\""), "{text}");
    assert_eq!(
        number_after(&text, "\"name\": \"sshd\"", "\"instant\": {\"cpu\": "),
        0.0
    );
    assert_eq!(
        number_after(&text, "\"name\": \"sshd\"", "\"avg\": {\"cpu\": "),
        0.0
    );
    // A gauge does describe the current state and is shown as it is.
    assert_eq!(
        number_after(
            &text,
            "\"name\": \"sshd\"",
            "\"instant\": {\"cpu\": 0, \"mem\": "
        ),
        pages(1000.0)
    );
}

#[test]
fn a_container_without_the_socket_degrades_to_its_short_identifier() {
    // D-13: the socket is not readable here, so the only name there is comes
    // from the cgroup - and it is the identifier alone. Only one of the
    // runtimes hostscope recognises is Docker, so a `docker-` prefix would be a
    // claim rather than a fallback.
    let f = Fixture::new("degraded");
    let text = {
        build(&f, 0);
        dump(&f)
    };
    assert!(text.contains("\"owner\": \"aaaaaaaaaaaa\""), "{text}");
    assert!(text.contains("\"owner_kind\": \"container\""), "{text}");
    // The service beside them keeps its own name and its own kind.
    let ssh = text.find("\"name\": \"sshd\"").unwrap();
    let after = &text[ssh..ssh + 300];
    assert!(after.contains("\"owner\": \"ssh\""), "{after}");
    assert!(after.contains("\"owner_kind\": \"service\""), "{after}");
}

/// The container list as the daemon answers it, for the first of the two
/// containers the fixture runs. The identifier is the one the cgroup name
/// carries, because that is the key the row is looked up by.
const DAEMON_LIST: &str = r#"[{"Id":"aaaaaaaaaaaa1111","Names":["/web"],"Image":"nginx:1.27","State":"running","Status":"Up 3 days","Created":1723635000,"Labels":{"com.docker.compose.project":"site"},"Ports":[{"PrivatePort":80,"PublicPort":8080,"Type":"tcp"}]}]"#;

#[test]
fn the_daemon_answer_reaches_the_row() {
    // FR-3 asks for the image, the state, the creation time and the restart
    // count, and the name is only the first of them. The name arrives because
    // the owner is named from the daemon's answer; the rest has to arrive on
    // the row itself, or the card has nothing to print but a guess about the
    // socket.
    let f = Fixture::new("daemon");
    build(&f, 0);
    let daemon = FakeDocker::new("daemon", DAEMON_LIST, 3);
    let text = run(&[
        "--cgroup-root",
        f.cgroup_root().to_str().unwrap(),
        "--proc-root",
        f.proc_root().to_str().unwrap(),
        "--docker-socket",
        daemon.arg(),
        "--dump-model",
        "json",
    ]);
    // The socket answered, so the row is named for the container and not for
    // its identifier.
    assert!(text.contains("\"owner\": \"web\""), "{text}");
    assert!(text.contains("\"image\": \"nginx:1.27\""), "{text}");
    assert!(text.contains("\"state\": \"running\""), "{text}");
    // The second container is not in the answer, so it stays on its short
    // identifier - the degraded case of FR-3 and the enriched one on one host.
    assert!(text.contains("\"owner\": \"bbbbbbbbbbbb\""), "{text}");
}

#[test]
fn a_process_totals_its_descendants_exactly() {
    // FR-1 and FR-5: the row of a process carries the whole subtree below it,
    // so the memory of pid 1 is its own plus that of every process it started.
    let f = Fixture::new("fr1");
    build(&f, 0);
    let text = dump(&f);
    let root = number_after(
        &text,
        "\"name\": \"systemd\"",
        "\"instant\": {\"cpu\": 0, \"mem\": ",
    );
    assert_eq!(root, pages(100.0 + 1000.0 + 2000.0 + 500.0));
}

#[test]
fn two_runs_over_one_snapshot_produce_one_dump() {
    // The FR-17 acceptance, word for word.
    let f = Fixture::new("fr17");
    build(&f, 0);
    let cgroup = f.cgroup_root();
    let proc = f.proc_root();
    let args = [
        "--cgroup-root",
        cgroup.to_str().unwrap(),
        "--proc-root",
        proc.to_str().unwrap(),
        "--docker-socket",
        "none",
        "--dump-model",
        "json",
    ];
    assert_eq!(run(&args), run(&args));
}

#[test]
fn a_delta_between_two_snapshots_becomes_the_rate() {
    // FR-13 and FR-15 together: the snapshot is captured twice with a known
    // pause, and what the application shows is the difference over it.
    let f = Fixture::new("delta");
    build(&f, 0);
    let cgroup = f.cgroup_root().to_str().unwrap().to_string();
    let proc = f.proc_root().to_str().unwrap().to_string();
    let root = f.root.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let second = Fixture { root };
        build(&second, 1);
        std::mem::forget(second); // the original owns the directory
    });
    let text = run(&[
        "--cgroup-root",
        &cgroup,
        "--proc-root",
        &proc,
        "--docker-socket",
        "none",
        "--dump-model",
        "json",
        "--dump-frame",
        "2",
        "--tick",
        "700",
    ]);
    writer.join().unwrap();
    // sshd gained a hundred ticks - one CPU-second - over a 0.7 s window.
    let cpu = number_after(&text, "\"name\": \"sshd\"", "\"instant\": {\"cpu\": ");
    assert!(cpu > 1.0, "expected a rate above one core, got {cpu}");
    let avg = number_after(&text, "\"name\": \"sshd\"", "\"avg\": {\"cpu\": ");
    assert!(avg > 1.0, "expected an average above one core, got {avg}");
}

#[test]
fn the_process_subtree_carries_its_descendants() {
    // FR-5: a worker started by a master is a row of its own, and the master
    // carries it too.
    let f = Fixture::new("fr5");
    build(&f, 0);
    f.process(202, 201, "nginx-worker", 10, 4000, "nginx: worker process");
    f.cgroup("system.slice/docker-aaaaaaaaaaaa1111.scope", &[201, 202]);
    let text = dump(&f);
    // The worker is the master's only child and sits in the same container, so
    // the two are one row named for both of them (D-25) - and that row carries
    // the memory of both: (2000 + 4000) pages of 4096 bytes.
    let anchor = "\"name\": \"nginx/nginx-worker\"";
    let mem = number_after(&text, anchor, "\"instant\": {\"cpu\": 0, \"mem\": ");
    assert_eq!(mem, pages(2000.0 + 4000.0));
    // The row is identified by the first link and names every link of the chain.
    assert_eq!(number_after(&text, anchor, "\"pid\": "), 201.0);
    let node = &text[text.find(anchor).unwrap()..];
    assert!(
        node[..300].contains("\"chain\": [\"201 nginx\", \"202 nginx-worker\"]"),
        "{}",
        &node[..300]
    );
    assert!(
        node[..400].contains("\"owner\": \"aaaaaaaaaaaa\""),
        "{}",
        &node[..400]
    );
}

#[test]
fn a_chain_names_every_link_however_long_it_is() {
    // D-25 was decided over a chain of seven, not of two, and a chain of two is
    // the one length at which naming only the last link looks right: there is
    // nothing between the ends. The forest is built from the leaves up, so each
    // link is glued into a row that is already a chain, and that is where the
    // links in the middle are lost.
    let f = Fixture::new("chain");
    build(&f, 0);
    f.cgroup(
        "system.slice/docker-aaaaaaaaaaaa1111.scope",
        &[201, 202, 203, 204],
    );
    f.process(201, 1, "supervisor", 10, 100, "supervisor app");
    f.process(202, 201, "app", 10, 100, "app");
    f.process(203, 202, "python3", 10, 100, "python3 app.py");
    f.process(204, 203, "node", 10, 100, "node server.js");
    let text = dump(&f);
    let anchor = "\"name\": \"supervisor/app/python3/node\"";
    assert!(text.contains(anchor), "the chain is not one row: {text}");
    let node = &text[text.find(anchor).unwrap()..];
    assert!(
        node[..400].contains(
            "\"chain\": [\"201 supervisor\", \"202 app\", \"203 python3\", \"204 node\"]"
        ),
        "every link must be named with its own pid and its own name: {}",
        &node[..400]
    );
    // The row is still identified by the first link. What the card makes of the
    // list is checked over a drawn frame, in `tests/frame_invariants.rs`.
    assert_eq!(number_after(&text, anchor, "\"pid\": "), 201.0);
}

#[test]
fn gluing_stops_at_the_owner_and_keeps_what_stands_under_the_chain() {
    // Two halves of D-25 that no fixture reached: the boundary that ends a
    // chain, and a chain that does not end at a leaf. Removing the owner check,
    // or dropping the subtree while gluing, left the whole suite green.
    let f = Fixture::new("boundary");
    build(&f, 0);
    // A shim of the host steps into a container. One single-child link on each
    // side of the boundary, and the boundary itself is worth a keystroke.
    f.cgroup("system.slice/containerd.service", &[501, 502]);
    f.cgroup(
        "system.slice/docker-cccccccccccc3333.scope",
        &[503, 504, 505, 506],
    );
    f.process(501, 1, "containerd", 10, 100, "containerd");
    f.process(
        502,
        501,
        "containerd-shim",
        10,
        100,
        "containerd-shim -id x",
    );
    f.process(503, 502, "entrypoint", 10, 100, "/entrypoint.sh");
    f.process(504, 503, "app", 10, 200, "app --serve");
    f.process(505, 504, "worker-a", 10, 300, "app worker a");
    f.process(506, 504, "worker-b", 10, 400, "app worker b");
    let text = dump(&f);

    // Two rows, not one: the shim keeps the service it belongs to, the
    // entrypoint keeps the container, and neither name reaches across.
    assert!(
        !text.contains("containerd-shim/entrypoint"),
        "the chain was glued across a change of owner: {text}"
    );
    let shim = "\"name\": \"containerd/containerd-shim\"";
    let node = &text[text
        .find(shim)
        .unwrap_or_else(|| panic!("no shim row: {text}"))..];
    assert!(
        node[..400].contains("\"chain\": [\"501 containerd\", \"502 containerd-shim\"]"),
        "{}",
        &node[..400]
    );
    assert!(
        node[..400].contains("\"owner\": \"containerd\""),
        "{}",
        &node[..400]
    );

    // The chain inside the container ends on a process that started two others,
    // and both of them are still under it.
    let app = "\"name\": \"entrypoint/app\"";
    let node = &text[text
        .find(app)
        .unwrap_or_else(|| panic!("no app row: {text}"))..];
    assert!(
        node[..400].contains("\"chain\": [\"503 entrypoint\", \"504 app\"]"),
        "{}",
        &node[..400]
    );
    assert!(
        text.contains("\"name\": \"worker-a\""),
        "the subtree was dropped: {text}"
    );
    assert!(
        text.contains("\"name\": \"worker-b\""),
        "the subtree was dropped: {text}"
    );
    // And the row still carries them: its memory is the whole chain plus both.
    assert_eq!(
        number_after(&text, app, "\"instant\": {\"cpu\": 0, \"mem\": "),
        pages(100.0 + 200.0 + 300.0 + 400.0)
    );
}

#[test]
fn a_name_keeps_its_letters_and_loses_what_would_move_the_cursor() {
    // FR-12 over a snapshot: a name in any script is shown as it is, and only
    // the characters that would drive the terminal are removed.
    let f = Fixture::new("names");
    build(&f, 0);
    f.process(
        101,
        1,
        "hs-\u{0431}\u{043E}\u{0442}\u{1B}[2J",
        100,
        1000,
        "/usr/sbin/sshd -D",
    );
    let text = dump(&f);
    assert!(
        text.contains("\"name\": \"hs-\u{0431}\u{043E}\u{0442} [2J\""),
        "the letters must survive and the escape must not: {text}"
    );
    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "a control character reached the dump"
    );
}

#[test]
fn a_file_that_cannot_be_read_leaves_the_value_unavailable() {
    // FR-8: an unavailable field is marked, not replaced with a zero. Only
    // sshd has an `io` file here, which is the ordinary case on a host where
    // hostscope runs without root.
    let f = Fixture::new("noio");
    build(&f, 0);
    let text = dump(&f);
    let start = text.find("\"name\": \"redis-server\"").unwrap();
    let node = &text[start..start + 400];
    assert!(
        node.contains("\"rd\": null"),
        "disk must be unknown, not zero: {node}"
    );
    assert!(node.contains("\"wr\": null"), "{node}");
    // The one that can be read is a rate, and on the first tick a rate is zero.
    let start = text.find("\"name\": \"sshd\"").unwrap();
    let node = &text[start..start + 400];
    assert!(node.contains("\"rd\": 0"), "{node}");
    assert!(
        node.contains(&format!("\"mem\": {}", pages(1000.0))),
        "the other values are still read: {node}"
    );
}
