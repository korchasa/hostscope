//! Layer V4 of the testing document: the frame linter. Every invariant of
//! section 7 that can be decided from the drawn text is checked here, and it is
//! run over every frame any scenario produces rather than as a test of its own.

mod support;

use support::{frames, run, width, Fixture};

fn build(f: &Fixture) {
    f.host(1_000_000, 900_000, 4);
    f.cgroup("", &[]);
    f.cgroup("init.scope", &[1]);
    f.cgroup("system.slice/ssh.service", &[101]);
    f.cgroup(
        "system.slice/docker-aaaaaaaaaaaa1111.scope",
        &[201, 202, 203],
    );
    f.cgroup("system.slice/docker-bbbbbbbbbbbb2222.scope", &[301, 302]);
    f.cgroup("user.slice/user-1000.slice/session-15324.scope", &[401]);
    f.process(1, 0, "systemd", 20, 100, "/sbin/init");
    f.process(101, 1, "sshd", 100, 1000, "/usr/sbin/sshd -D");
    f.process(
        201,
        1,
        "a-very-long-process-name-that-will-not-fit-the-column",
        200,
        2000,
        "nginx",
    );
    f.process(202, 201, "nginx-worker", 20, 400, "nginx: worker process");
    f.process(
        203,
        201,
        "nginx-cache",
        10,
        200,
        "nginx: cache manager process",
    );
    // One child and one owner all the way down, which is the chain the row
    // glues into a single line (D-25).
    f.process(301, 1, "redis-server", 50, 500, "redis-server *:6379");
    f.process(302, 301, "redis-io", 5, 100, "redis-server io thread");
    // Only one process can be read: without root `/proc/<pid>/io` belongs to
    // its owner alone, so the disk cell of the others has to say so (FR-8).
    f.process_io(101, 1_000_000, 2_000_000);
    // A name outside ASCII, which the frame now shows as it is (FR-12). It
    // carries Cyrillic, which takes one cell per letter, and two Han letters,
    // which take two - so the name column is the one place in the fixture where
    // a cell and a character are different things, and the linter's column
    // arithmetic is exercised rather than assumed.
    // The command line carries an emoji from a block the first hand-written
    // width table did not cover (D-18). It reaches the hint line under the
    // table, which is where a miscounted cell shows up as a short frame.
    f.process(
        401,
        1,
        "hs-\u{0431}\u{043E}\u{0442}-\u{4E2D}\u{6587}",
        40,
        400,
        "/usr/bin/hs-\u{0431}\u{043E}\u{0442} --serve \u{1F680}",
    );
}

fn scenario(f: &Fixture, keys: &str, size: &str) -> Vec<Vec<String>> {
    let text = run(&[
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
    ]);
    frames(&text)
}

/// The character offset of a word in a line, counted in characters.
fn char_find(line: &[char], needle: &str, from: usize) -> Option<usize> {
    let want: Vec<char> = needle.chars().collect();
    (from..line.len().saturating_sub(want.len() - 1)).find(|&i| line[i..i + want.len()] == want[..])
}

/// The index into a row at which a cell column begins. A column taken from the
/// header is a count of cells, and a row may carry letters two cells wide, so
/// the two are the same number only while the row stays inside ASCII.
fn cell_index(chars: &[char], column: usize) -> usize {
    let mut cells = 0usize;
    for (i, c) in chars.iter().enumerate() {
        if cells >= column {
            return i;
        }
        cells += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
    }
    chars.len()
}

/// The invariants of section 7 that a frame can be judged by on its own.
fn lint(frame: &[String], cells: usize) -> Vec<String> {
    let mut bad = Vec::new();

    // 1. Every line takes the width of the terminal, counted in cells.
    for (i, line) in frame.iter().enumerate() {
        if width(line) != cells {
            bad.push(format!(
                "line {i} is {} wide, not {cells}: {line:?}",
                width(line)
            ));
        }
    }
    // 2. Nothing that drives the terminal reaches the frame (FR-12). A letter
    // of any script is fine; a control character is what breaks the drawing.
    for (i, line) in frame.iter().enumerate() {
        if line.chars().any(|c| c.is_control()) {
            bad.push(format!("line {i} carries a control character: {line:?}"));
        }
    }
    // 11. No panic anywhere in the frame.
    for line in frame {
        if line.contains("panicked") || line.contains("RUST_BACKTRACE") {
            bad.push(format!("a panic reached the frame: {line:?}"));
        }
    }
    // 9. The path line is present and labelled with its level.
    let path = frame.get(4).cloned().unwrap_or_default();
    if !["L0", "L1", "L2", "L3"].iter().any(|l| path.contains(l)) {
        bad.push(format!("the path line carries no level: {path:?}"));
    }
    // 10. The measurement mode is labelled with its window.
    let mode = frame.get(2).cloned().unwrap_or_default();
    if !(mode.contains("INSTANT") || mode.contains("AVG over") || mode.contains("PAUSED")) {
        bad.push(format!("the window is not labelled: {mode:?}"));
    }

    let header = frame.get(6).cloned().unwrap_or_default();
    if !header.contains("NAME") {
        // A card is open: the table invariants do not apply to it.
        return bad;
    }
    // Every offset below is a cell column. The header is ASCII, so there a cell
    // is a character and `char_find` gives the column directly; a row is not,
    // because a name in a wide script takes two cells per letter, and slicing a
    // row by the header's character index lands one character short per wide
    // letter - which reports a name leaving its column on a frame that is
    // exactly the width of the terminal, and hides a real overrun just as
    // easily. `cell_index` is what turns a column back into an index into a row.
    let head: Vec<char> = header.chars().collect();
    // 8. The column set and order are the same on every level, and every level
    // draws all of them.
    let mut at = 0usize;
    let mut whole = true;
    for column in ["NAME", "OWNER", "TASKS", "CORES", "MEM"] {
        match char_find(&head, column, at) {
            Some(pos) => at = pos + column.chars().count(),
            None => {
                whole = false;
                bad.push(format!(
                    "column {column} is missing or out of order: {header:?}"
                ))
            }
        }
    }
    // Every check below slices a row at a column read off the header, so a
    // header missing one of them has no offsets to slice by: the missing column
    // reads as 0 and the slice runs backwards, which panicked the linter on a
    // 24-column terminal instead of reporting anything at all. The finding is
    // already recorded above; what is left is to stop, not to guess.
    if !whole {
        return bad;
    }
    let owner_col = char_find(&head, "OWNER", 0).unwrap_or(0);
    // CPU is written in busy cores and in nothing else (D-25), so the column
    // has one header and the linter needs no fallback for another.
    let cpu_end = char_find(&head, "CORES", 0).map(|i| i + 5).unwrap_or(0);
    // The bar belongs to the column the rows are ordered by (D-27), so where it
    // is expected is read off the path line rather than fixed. The region runs
    // from the end of that column to the start of the next heading; the bar is
    // the only thing in the frame drawn out of blocks, so it is found by
    // character rather than by arithmetic over the widths.
    let sorted = ["cores", "memory", "disk", "net"]
        .into_iter()
        .find(|s| path.contains(&format!("sort: {s}")))
        .unwrap_or("cores");
    let headings = ["CORES", "MEM", "DISK R/W", "NET"];
    let sorted_col = match sorted {
        "memory" => "MEM",
        "disk" => "DISK R/W",
        "net" => "NET",
        _ => "CORES",
    };
    let bar_from = match char_find(&head, sorted_col, 0) {
        Some(i) => i + sorted_col.chars().count(),
        None => {
            bad.push(format!(
                "the table is sorted by {sorted} and has no such column: {header:?}"
            ));
            return bad;
        }
    };
    let bar_to = headings
        .iter()
        .filter_map(|h| char_find(&head, h, 0).filter(|i| *i > bar_from))
        .min()
        .unwrap_or(head.len());

    let rows: Vec<&String> = frame[7..frame.len() - 4].iter().collect();
    let mut seen_self = false;
    for (i, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        let owner_at = cell_index(&chars, owner_col);
        // The row is drawn as one leading space, then the name column.
        let name: String = chars[cell_index(&chars, 2)..owner_at].iter().collect();
        if name.trim().is_empty() {
            continue;
        }
        // 4. A name never leaves its column.
        if owner_at.checked_sub(1).and_then(|k| chars.get(k)) != Some(&' ') {
            bad.push(format!("row {i} runs into the owner column: {row:?}"));
        }
        // 12. The mark of a node with children is a column of its own.
        if !(name.starts_with("> ") || name.starts_with("  ")) {
            bad.push(format!("row {i} has no marker column: {row:?}"));
        }
        // 5. The (self) row, when present, is the first row of the level.
        if name.trim() == "(self)" {
            seen_self = true;
            if i != 0 {
                bad.push(format!("the (self) row is at {i}, not first"));
            }
        } else if seen_self && i == 0 {
            bad.push("the (self) row lost its place".to_string());
        }
        // 7. A non-zero value draws at least one tick of the bar, and the bar is
        // drawn beside the column the rows are ordered by - nowhere else.
        fn is_block(c: char) -> bool {
            ('\u{2588}'..='\u{258F}').contains(&c)
        }
        let bar: String = chars[cell_index(&chars, bar_from)..cell_index(&chars, bar_to)]
            .iter()
            .collect();
        let outside: String = chars
            .iter()
            .take(cell_index(&chars, bar_from))
            .chain(chars.iter().skip(cell_index(&chars, bar_to)))
            .filter(|c| is_block(**c))
            .collect();
        if !outside.is_empty() {
            bad.push(format!(
                "row {i} draws a bar away from the {sorted} column: {row:?}"
            ));
        }
        if sorted == "cores" {
            let cpu_at = cell_index(&chars, cpu_end);
            let cpu: String = chars[cell_index(&chars, cpu_end.saturating_sub(8))..cpu_at]
                .iter()
                .collect();
            let value: f64 = cpu.trim().parse().unwrap_or(0.0);
            if value > 0.0 && !bar.chars().any(is_block) {
                bad.push(format!("row {i} shows {value} and draws no tick: {row:?}"));
            }
        }
    }
    bad
}

fn check_all(frames: &[Vec<String>], cells: usize) {
    assert!(!frames.is_empty(), "no frames were produced");
    for (n, frame) in frames.iter().enumerate() {
        let bad = lint(frame, cells);
        assert!(bad.is_empty(), "frame {n}: {bad:#?}\n{}", frame.join("\n"));
    }
}

#[test]
fn a_walk_down_the_forest_and_back_keeps_every_invariant() {
    let f = Fixture::new("walk");
    build(&f);
    // Down into the one process at the root of the forest, into a process it
    // started, into the worker that one started, and back out again, opening a
    // card on every level.
    let frames = scenario(
        &f,
        "i Escape Down Enter i Escape Down Enter Backspace Down Enter Backspace Backspace",
        "100x30",
    );
    check_all(&frames, 100);
}

#[test]
fn the_invariants_hold_under_every_sorting_and_unit() {
    let f = Fixture::new("sorts");
    build(&f);
    let frames = scenario(&f, "c m d n u u u a a Down Down", "100x30");
    check_all(&frames, 100);
}

#[test]
fn the_invariants_hold_on_a_narrow_and_on_a_wide_terminal() {
    let f = Fixture::new("sizes");
    build(&f);
    for (size, cells) in [("60x20", 60usize), ("200x60", 200), ("80x24", 80)] {
        let frames = scenario(&f, "Down Right Down Enter", size);
        check_all(&frames, cells);
    }
}

#[test]
fn the_self_row_stays_first_under_each_of_the_four_sortings() {
    let f = Fixture::new("selffirst");
    build(&f);
    for key in ["c", "m", "d", "n"] {
        let frames = scenario(&f, key, "100x30");
        let last = frames.last().unwrap();
        let rows: Vec<&String> = last[7..last.len() - 4].iter().collect();
        let first = rows[0].chars().skip(1).take(28).collect::<String>();
        assert!(
            first.trim() == "(self)",
            "under sort {key} the first row is {first:?}"
        );
        check_all(&frames, 100);
    }
}

#[test]
fn the_card_opens_on_every_kind_of_row_and_is_not_a_level() {
    let f = Fixture::new("cards");
    build(&f);
    // The (self) remainder, a process with children, and a process without
    // them - where `Enter` has nowhere to go and opens the card instead.
    let frames = scenario(
        &f,
        "i Escape Down Enter Down i Escape Enter Escape",
        "100x30",
    );
    check_all(&frames, 100);
    let text: String = frames
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Esc closes the card"), "no card was drawn");
    assert!(text.contains("own usage of"), "the (self) card is missing");
    assert!(text.contains("pid "), "the process card is missing");
}

/// D-32: the card is read down a column. Both modes stand under one heading in
/// fixed columns, and every figure has a label of its own on the left edge -
/// the average of a quantity is found by looking, not by reading each line to
/// see where the word `avg` fell this time.
#[test]
fn the_card_puts_every_figure_under_a_label_and_a_column() {
    let f = Fixture::new("card-columns");
    build(&f);
    f.process_extras(101, 20, 2, 4400);

    let frames = scenario(&f, "v / s s h d Enter i", "110x36");
    check_all(&frames, 110);
    let card = frames.last().unwrap().clone();
    let line = |label: &str| -> String {
        card.iter()
            .find(|l| {
                l.starts_with(&format!("\u{2502}  {label} "))
                    || l.trim_end() == format!("\u{2502}  {label}")
            })
            .unwrap_or_else(|| panic!("no line labelled {label} in:\n{}", card.join("\n")))
            .clone()
    };

    // The heading is written once, and it is what fixes the two columns.
    let head = card
        .iter()
        .find(|l| {
            let text = l.trim_start_matches('\u{2502}').trim_start();
            text.starts_with("now") && text.contains("avg over")
        })
        .unwrap_or_else(|| {
            panic!(
                "the figure columns have no heading in:\n{}",
                card.join("\n")
            )
        })
        .clone();
    // In characters, not in bytes: the frame border is three bytes wide.
    let head_chars: Vec<char> = head.chars().collect();
    let now_at = char_find(&head_chars, "now", 0).unwrap();
    let avg_at = char_find(&head_chars, "avg over", 0).unwrap();

    // Every figure starts where its column starts, on all four rows.
    for label in ["cpu", "memory RSS", "disk r/w", "net \u{2193}/\u{2191}"] {
        let row: Vec<char> = line(label).chars().collect();
        for (name, at) in [("now", now_at), ("avg", avg_at)] {
            assert_eq!(
                row[at - 1],
                ' ',
                "the {name} column of {label} does not start where the heading says"
            );
            assert_ne!(
                row[at], ' ',
                "the {name} column of {label} is empty where the heading says it is"
            );
        }
    }

    // A fact with a label of its own is found by running down the left edge.
    for label in [
        "own virtual",
        "own PSS",
        "files",
        "sockets",
        "nofile",
        "nproc",
    ] {
        line(label);
    }
    assert!(
        !line("files").contains("sockets"),
        "the socket count is still inside the value of another label"
    );
}

/// D-33: every fact of the card has a label on the left edge, and a value too
/// wide for the room wraps under that label instead of losing its end. A
/// command line is where the end matters: it is the part that says which
/// configuration file the process was started with.
#[test]
fn the_card_labels_every_fact_and_wraps_what_does_not_fit() {
    let f = Fixture::new("card-wrap");
    build(&f);
    let long = "/usr/sbin/sshd -D -f /etc/ssh/sshd_config.d/50-cloud-init.conf \
-o LogLevel=VERBOSE -o PermitRootLogin=no -o PasswordAuthentication=no";
    f.process(101, 1, "sshd", 100, 1000, long);
    f.process_extras(101, 20, 2, 4400);

    let frames = scenario(&f, "v / s s h d Enter i", "100x40");
    check_all(&frames, 100);
    let card = frames.last().unwrap().clone();
    let text = card.join("\n");

    // Identity is read by label, like everything else on the card.
    for label in ["process", "pid", "parent", "user", "threads", "started"] {
        assert!(
            card.iter()
                .any(|l| l.starts_with(&format!("\u{2502}  {label} "))),
            "no line labelled {label} in:\n{text}"
        );
    }

    // The command is all there, across as many lines as it takes.
    let drawn: String = card
        .iter()
        .skip_while(|l| !l.starts_with("\u{2502}  command "))
        .take_while(|l| !l.trim_end_matches([' ', '\u{2502}']).is_empty())
        .map(|l| l.trim_start_matches('\u{2502}').trim())
        .collect::<Vec<_>>()
        .join(" ");
    for part in ["50-cloud-init.conf", "PasswordAuthentication=no"] {
        assert!(drawn.contains(part), "the command lost {part}: {drawn:?}");
    }
    assert!(
        !drawn.contains('\u{2026}'),
        "the command was cut although it fits in three lines: {drawn:?}"
    );
}

/// The explanations on the card are text like any other, and a narrow terminal
/// used to cut them mid-word against the border - the reader saw "shared pages
/// divid" and had nothing to do with it. They wrap now (D-33).
#[test]
fn the_card_wraps_its_explanations_instead_of_cutting_them() {
    let f = Fixture::new("card-notes");
    build(&f);
    f.process(101, 1, "sshd", 100, 1000, "/usr/sbin/sshd -D");
    f.process_extras(101, 20, 2, 4400);

    for cells in [70usize, 60] {
        let size = format!("{cells}x40");
        let frames = scenario(&f, "v / s s h d Enter i", &size);
        let card = frames.last().unwrap().clone();
        // Wrapped text reads as one sentence again once the lines are joined.
        let flat = card
            .iter()
            .map(|l| l.trim_matches(|c| c == '\u{2502}' || c == ' '))
            .collect::<Vec<_>>()
            .join(" ");
        let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
        for phrase in [
            "shared pages divided between those that map them",
            "attributed to the namespace, not to the process",
        ] {
            assert!(
                flat.contains(phrase),
                "{size}: the explanation was cut: {phrase:?} is missing from:\n{}",
                card.join("\n")
            );
        }
    }

    // The cgroup path is how the reader finds the unit in `systemctl`, and a
    // path with its tail cut off names nothing. It wraps too (D-33).
    let card = scenario(&f, "v / h s - Enter i", "60x40")
        .last()
        .unwrap()
        .clone();
    let flat = card
        .iter()
        .map(|l| l.trim_matches(|c| c == '\u{2502}' || c == ' '))
        .collect::<Vec<_>>()
        .join("");
    assert!(
        flat.contains("/user.slice/user-1000.slice/session-15324.scope"),
        "the cgroup path was cut at 60 cells:\n{}",
        card.join("\n")
    );
}

#[test]
fn the_list_view_holds_the_invariants_and_reaches_a_process_from_the_root() {
    let f = Fixture::new("list");
    build(&f);
    // `v` flattens the forest: every leaf process is on screen without
    // descending, and the filter finds one of them by name.
    let frames = scenario(&f, "v / r e d i s Enter", "100x30");
    check_all(&frames, 100);
    let last = frames.last().unwrap().join("\n");
    assert!(
        last.contains("view: list"),
        "the view is not labelled:\n{last}"
    );
    assert!(
        last.contains("redis-server"),
        "the list did not reach the process:\n{last}"
    );
    // The row says where it came from, in front of its own name.
    assert!(
        last.contains("/redis-server"),
        "the row carries no path:\n{last}"
    );
    // The middle of the tree is not a row of the list: a process that started
    // others holds them and stands nowhere in it, while the ends do - the
    // leaves, and the remainders of the processes above them.
    let listed = frames[1].join("\n");
    for row in ["redis-server", "sshd", "nginx-worker", "(self)"] {
        assert!(listed.contains(row), "the list lost {row}:\n{listed}");
    }
}

#[test]
fn the_card_of_a_glued_chain_names_every_link_with_its_pid() {
    // The acceptance of D-25, over the length the decision was made for: on
    // the test host the chain was seven links, and a card that names only the
    // last of them attributes the work of the whole chain to one pid. A chain
    // of two is the one length at which that mistake cannot be seen.
    // Seven links, the length D-25 was decided over, and the last of them under
    // a six-digit pid, which is what the ordinary `pid_max` of 4194304 hands
    // out. Four links and three-digit pids are the two sizes at which both ways
    // of losing a link stay invisible.
    let f = Fixture::new("chain");
    build(&f);
    f.cgroup(
        "system.slice/docker-bbbbbbbbbbbb2222.scope",
        &[301, 302, 303, 304, 305, 306, 999999],
    );
    f.process(302, 301, "app", 5, 100, "app");
    f.process(303, 302, "python3", 5, 100, "python3 app.py");
    f.process(304, 303, "npm exec chrome", 5, 100, "npm exec chrome");
    f.process(305, 304, "sh", 5, 100, "sh -c start");
    f.process(306, 305, "chrome-devtools", 5, 100, "chrome-devtools");
    f.process(999999, 306, "node", 5, 100, "node server.js");
    // The chain is a leaf of the forest, so the flat list reaches it in one
    // step and the filter picks it out by the name of its first link.
    //
    // Two widths, and the narrow one is the point: at a hundred columns the
    // value column is eighty cells and a single line holds a short chain, which
    // proves nothing. The card has to name every link at any width it is drawn
    // at, or the line below it credits the whole chain to one pid.
    for (size, cells) in [("100x40", 100usize), ("70x40", 70)] {
        let frames = scenario(&f, "v / r e d i s Enter i", size);
        check_all(&frames, cells);
        let card = frames.last().unwrap().join("\n");
        assert!(
            card.contains("Esc closes the card"),
            "{size}: no card was drawn:\n{card}"
        );
        for link in [
            "301 redis-server",
            "302 app",
            "303 python3",
            "304 npm exec chrome",
            "305 sh",
            "306 chrome-devtools",
            "999999 node",
        ] {
            assert!(
                card.contains(link),
                "{size}: the card does not name {link}:\n{card}"
            );
        }
        // The command line is the last link's, and the card says so by naming
        // that link's pid whole - a pid cut short names another process, and a
        // pid that touches the value merges two columns. The command itself is
        // asserted too: the first link's is `redis-server *:6379`, so a card
        // that kept it would read as if 999999 were running redis.
        assert!(
            card.contains("999999   node server.js"),
            "{size}: the command is misattributed or the columns merged:\n{card}"
        );
    }
}

#[test]
fn a_pid_the_card_cannot_hold_whole_is_marked_as_cut() {
    // The same substitution as above, from the other end: not a pid moved into
    // a label that cuts it, but a pid too wide for the room the label leaves.
    // The value used to reserve a fixed number of cells for it, which is a
    // reserve only while the terminal is wide enough to hold one; narrower, the
    // pid ran past the room and was cut by `pad`, which leaves no mark. Pid
    // 4194303 - the largest an ordinary `pid_max` hands out - then read as
    // `4194`, a shorter pid that exists on the same host.
    let f = Fixture::new("narrowpid");
    build(&f);
    f.cgroup(
        "system.slice/docker-bbbbbbbbbbbb2222.scope",
        &[301, 4194303],
    );
    f.process(4194303, 301, "node", 5, 100, "node server.js");
    for (size, cells) in [
        ("24x40", 24usize),
        ("26x40", 26),
        ("30x40", 30),
        ("40x40", 40),
    ] {
        let frames = scenario(&f, "v / r e d i s Enter i", size);
        // Not `check_all`: below about sixty cells the table header cannot hold
        // its five columns, so the column invariants have nothing to hold on to
        // and report their absence on every table frame. What still has to hold
        // at any width is the shape of the frame.
        for (n, frame) in frames.iter().enumerate() {
            for (i, line) in frame.iter().enumerate() {
                assert_eq!(width(line), cells, "{size}: frame {n} line {i}: {line:?}");
                assert!(
                    !line.chars().any(|c| c.is_control()),
                    "{size}: frame {n} line {i} carries a control character: {line:?}"
                );
            }
        }
        let card = frames.last().unwrap();
        let line = card
            .iter()
            .find(|l| l.contains("command of"))
            .unwrap_or_else(|| {
                panic!("{size}: the card has no command line:\n{}", card.join("\n"))
            });
        let value = line.split("command of").nth(1).unwrap().trim();
        assert!(
            value.starts_with("4194303") || value.contains('\u{2026}'),
            "{size}: the pid is cut with no sign of it: {line:?}"
        );
    }
}

/// The number of lines a card produced. `frame` draws six lines above the card
/// and four below it, and a card that fits is followed by blank lines, so the
/// count is the position of the last line with anything on it.
fn card_height(frame: &[String]) -> usize {
    let content = &frame[6..frame.len() - 4];
    content
        .iter()
        .rposition(|l| !l.trim_matches(|c| c == '\u{2502}' || c == ' ').is_empty())
        .map(|i| i + 1)
        .expect("the card drew nothing")
}

#[test]
fn a_card_that_does_not_fit_says_how_much_of_it_is_missing() {
    // The card has no scrolling, so on a short terminal something has to go.
    // What may not happen is that it goes in silence: a long chain used to push
    // every figure off the card, and the reader saw a full screen with no sign
    // that anything had been cut.
    let f = Fixture::new("overflow");
    build(&f);
    let pids: Vec<i32> = (0..20).map(|i| 601 + i).collect();
    f.cgroup("system.slice/docker-bbbbbbbbbbbb2222.scope", &pids);
    for (i, pid) in pids.iter().enumerate() {
        let parent = if i == 0 { 1 } else { pids[i - 1] };
        f.process(
            *pid,
            parent,
            &format!("link{i:02}"),
            5,
            100,
            &format!("link{i:02} --run"),
        );
    }
    // The same card at a height that holds all of it. It is the measurement the
    // count is checked against, and on its own it says the other half: a card
    // that fits says nothing. A sentinel that is always on cannot be told from
    // one that works, and here it would cut the last real line to make room for
    // itself - the very loss it is drawn to announce.
    let tall = scenario(&f, "v / l i n k 0 0 Enter i", "60x60");
    let whole = tall.last().unwrap();
    let full = card_height(whole);
    assert!(
        !whole.join("\n").contains("more lines"),
        "a card that fits still says lines are missing:\n{}",
        whole.join("\n")
    );

    // The two heights either side of the boundary. Room to spare proves little:
    // the guard is a comparison, and a comparison is wrong by one line at a
    // time. At the height that holds the card exactly the guard must stay
    // silent, and one line below it must speak - and say two, because the line
    // it speaks on costs the card another.
    for (height, want) in [(full + 10, None), (full + 9, Some(2))] {
        let frames = scenario(&f, "v / l i n k 0 0 Enter i", &format!("60x{height}"));
        let card = frames.last().unwrap().join("\n");
        match want {
            None => assert!(
                !card.contains("more lines"),
                "the card fits in {height} lines and still says it does not:\n{card}"
            ),
            Some(n) => assert!(
                card.contains(&format!("\u{2026} {n} more lines")),
                "at {height} lines the card should hide {n}:\n{card}"
            ),
        }
    }

    let frames = scenario(&f, "v / l i n k 0 0 Enter i", "60x18");
    check_all(&frames, 60);
    let card = frames.last().unwrap().join("\n");
    assert!(
        card.contains("Esc closes the card"),
        "no card was drawn:\n{card}"
    );
    // The number, not merely the sentence. The line that carries the count is
    // itself one of the lines the card has room for, so it displaces one more:
    // of the `full` lines the card wanted, `content - 1` are left.
    let hidden = full - (18 - 10) + 1;
    assert!(
        card.contains(&format!("\u{2026} {hidden} more lines")),
        "the card hid {hidden} of its {full} lines and says otherwise:\n{card}"
    );
}

#[test]
fn a_filter_that_matches_nothing_says_so_and_keeps_the_frame_whole() {
    let f = Fixture::new("filter");
    build(&f);
    let frames = scenario(&f, "/ z z z Enter", "100x30");
    check_all(&frames, 100);
    let last = frames.last().unwrap().join("\n");
    assert!(last.contains("no rows match the filter"), "{last}");
}

/// The blocks a row draws, by the cell they start at. The bar is the only thing
/// in a table row made of blocks, so this is where the bar is.
fn blocks_at(row: &str) -> Option<usize> {
    let chars: Vec<char> = row.chars().collect();
    let mut cells = 0usize;
    for c in chars {
        if ('\u{2588}'..='\u{258F}').contains(&c) {
            return Some(cells);
        }
        cells += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
    }
    None
}

#[test]
fn the_bar_stands_beside_the_column_the_rows_are_sorted_by() {
    // The fixture is still, so no row spends any CPU over an interval and the
    // bar beside `CORES` has nothing to draw. Memory is a gauge and is there on
    // the first reading, which is what makes this fixture able to tell the two
    // columns apart at all: under `m` the bars appear, and they appear beside
    // `MEM` rather than where they used to be.
    let f = Fixture::new("sortedbar");
    build(&f);
    let frames = scenario(&f, "m d", "100x30");
    check_all(&frames, 100);
    // Counted in characters, not in bytes: the frame border is one character
    // of three bytes, so a byte offset lands two cells to the right.
    // The header of the same frame: the bar slot moves with the sorting, so
    // the columns of a frame sorted by cores stand elsewhere.
    let head: Vec<char> = frames[1][6].chars().collect();
    let mem_end = char_find(&head, "MEM", 0).expect("the header names MEM") + 3;
    let disk_at = char_find(&head, "DISK", 0).expect("the header names DISK");
    let sorted_by_memory = &frames[1];
    let bars: Vec<usize> = sorted_by_memory[7..sorted_by_memory.len() - 4]
        .iter()
        .filter_map(|r| blocks_at(r))
        .collect();
    assert!(
        !bars.is_empty(),
        "no row drew a bar under sorting by memory:\n{}",
        sorted_by_memory.join("\n")
    );
    for at in bars {
        assert!(
            at >= mem_end && at < disk_at,
            "the bar is at {at}, not beside MEM ({mem_end}..{disk_at}):\n{}",
            sorted_by_memory.join("\n")
        );
    }
    // The heading of the sorted column says which one it is, and the path line
    // agrees with it.
    assert!(sorted_by_memory[4].contains("sort: memory"));
    assert!(frames[2][4].contains("sort: disk"));
}

#[test]
fn the_page_keys_move_the_table_by_a_screenful() {
    let f = Fixture::new("paging");
    f.host(1_000_000, 900_000, 4);
    f.cgroup("", &[]);
    f.cgroup("init.scope", &[1]);
    f.process(1, 0, "systemd", 20, 100, "/sbin/init");
    // More rows than one screen holds: at 100x24 the table draws 13 of them.
    let pids: Vec<i32> = (0..40).map(|i| 500 + i).collect();
    f.cgroup("system.slice/many.service", &pids);
    for (i, pid) in pids.iter().enumerate() {
        f.process(
            *pid,
            1,
            &format!("proc{i:02}"),
            5,
            (100 + i * 10) as u64,
            &format!("proc{i:02} --run"),
        );
    }
    // Sorted by memory, which the fixture has from the first reading, so the
    // order of the rows is known and the page keys can be checked by name.
    let frames = scenario(&f, "Down Right m NPage NPage PPage", "100x24");
    check_all(&frames, 100);
    // Every row of the level, in the order the sorting put them: the remainder
    // of systemd first, then the forty processes from the largest down. Thirteen
    // of them are on screen at this height.
    let names = |frame: &Vec<String>| -> Vec<String> {
        frame[7..frame.len() - 4]
            .iter()
            .filter_map(|r| r.split_whitespace().nth(1).map(|s| s.to_string()))
            .collect()
    };
    let first = names(&frames[3]);
    let second = names(&frames[4]);
    let third = names(&frames[5]);
    let back = names(&frames[6]);
    assert_eq!(first.len(), 13, "the screen holds thirteen rows: {first:?}");
    assert_eq!(first[0], "(self)");
    assert_eq!(first[1], "proc39");
    // A page is what the table holds, so the screen after the key is the rows
    // that came after this one - not the same rows shifted by a line.
    assert_eq!(
        second[0],
        "proc27",
        "PageDown did not move a screenful:\n{}",
        frames[4].join("\n")
    );
    assert_eq!(
        third[0],
        "proc14",
        "the second PageDown did not move another:\n{}",
        frames[5].join("\n")
    );
    assert_eq!(back, second, "PageUp did not undo one PageDown");
}

#[test]
fn the_filter_says_what_it_is_how_much_it_left_and_how_to_drop_it() {
    let f = Fixture::new("filterstate");
    build(&f);
    // The filter is typed, kept with Enter, and then dropped with Escape.
    let frames = scenario(&f, "v / r e d i s Enter Escape", "100x30");
    check_all(&frames, 100);
    let typing = frames[6][frames[6].len() - 2].clone();
    assert!(
        typing.contains("filter: redi"),
        "the line does not show what is being typed: {typing:?}"
    );
    let kept = frames[8].join("\n");
    // What the filter is stands on the path line, with the level and the
    // sorting, and the count says how much of the level it left.
    let kept_state = frames[8][4].clone();
    let kept_keys = frames[8][frames[8].len() - 2].clone();
    assert!(
        kept_state.contains("filter: redis (1)"),
        "a filter that is still on is not on screen: {kept_state:?}"
    );
    assert!(
        kept_keys.contains("Esc clears"),
        "the keys do not say how to drop the filter: {kept_keys:?}"
    );
    assert!(kept.contains("redis-server"), "the filter lost its row");
    // Escape drops the filter and the level comes back whole.
    let dropped = frames[9].join("\n");
    assert!(
        !dropped.contains("filter:"),
        "Escape did not drop the filter:\n{dropped}"
    );
    assert!(
        frames[9].join("\n").contains("sshd"),
        "the rows the filter hid did not come back:\n{}",
        frames[9].join("\n")
    );
}

#[test]
fn the_refresh_interval_is_on_screen_and_the_two_keys_move_it() {
    let f = Fixture::new("interval");
    build(&f);
    // `--tick 60` is what the scenarios run at, and the frame says so rather
    // than rounding it to a step it is not.
    let frames = scenario(&f, "+ + - - -", "100x30");
    check_all(&frames, 100);
    let keys = |n: usize| frames[n][frames[n].len() - 2].clone();
    assert!(keys(0).contains("- + 60ms"), "{:?}", keys(0));
    assert!(keys(1).contains("- + 1s"), "{:?}", keys(1));
    assert!(keys(2).contains("- + 2s"), "{:?}", keys(2));
    assert!(keys(3).contains("- + 1s"), "{:?}", keys(3));
    // The near end of the list is the pause, and the frame is held there.
    assert!(keys(4).contains("- + paused"), "{:?}", keys(4));
    assert!(frames[4][2].contains("PAUSED"), "the pause is not labelled");
    assert!(keys(5).contains("paused"), "the pause is not the end of it");
}

#[test]
fn pause_holds_the_frame_and_says_so() {
    let f = Fixture::new("pause");
    build(&f);
    let frames = scenario(&f, "Space Space", "100x30");
    check_all(&frames, 100);
    assert!(
        frames[1].join("\n").contains("PAUSED"),
        "the pause is not labelled"
    );
}

#[test]
fn an_unavailable_column_says_so_instead_of_showing_a_zero() {
    let f = Fixture::new("na");
    build(&f);
    // Only sshd has an `io` file, so every other row has to say that its disk
    // figure is unknown rather than draw a zero.
    let frames = scenario(&f, "Down Right m", "100x30");
    check_all(&frames, 100);
    let last = frames.last().unwrap();
    let row = last
        .iter()
        .find(|l| l.contains("redis-server"))
        .expect("the row is drawn");
    assert!(row.contains("n/a"), "the disk cell must say n/a: {row:?}");
}

#[test]
fn a_name_outside_ascii_is_shown_as_it_is_and_keeps_its_column() {
    let f = Fixture::new("nonascii");
    build(&f);
    // One step down from the root of the forest is the level the process with
    // the non-ASCII name lives on.
    let frames = scenario(&f, "Down Right", "100x30");
    check_all(&frames, 100);
    // The table row, not the hint line: the name column carries the letters and
    // the owner column still starts where the header says it does. The name is
    // two cells wider than it is long in characters, so the column has to be
    // found by cell and not by character - the one difference this fixture
    // exists to make visible.
    let last = frames.last().unwrap();
    let full = "hs-\u{0431}\u{043E}\u{0442}-\u{4E2D}\u{6587}";
    let row = last
        .iter()
        .find(|l| l.contains(full) && l.contains("1000"))
        .unwrap_or_else(|| panic!("no row shows the name as it is:\n{}", last.join("\n")));
    let head = last
        .iter()
        .find(|l| l.contains("NAME"))
        .expect("the header is drawn");
    let owner_col = char_find(&head.chars().collect::<Vec<char>>(), "OWNER", 0).unwrap();
    let chars: Vec<char> = row.chars().collect();
    let name: String = chars[..cell_index(&chars, owner_col)].iter().collect();
    assert!(name.contains(full), "the name left its column: {row:?}");
    assert_eq!(
        width(&name),
        owner_col,
        "the name column is not where the header says"
    );
}

#[test]
fn an_emoji_in_a_command_line_keeps_the_frame_the_width_of_the_terminal() {
    let f = Fixture::new("emoji");
    build(&f);
    // Down to the process whose command line holds the rocket, then its card:
    // the hint line shows the command under the table, the card shows it in
    // full. Both are linted, and the linter counts cells with Python-grade
    // tables rather than with the application's own.
    let frames = scenario(&f, "Down Right / h s Enter Enter", "100x30");
    check_all(&frames, 100);
    let text: String = frames
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("\u{1F680}"),
        "the emoji never reached the frame:\n{text}"
    );
}
