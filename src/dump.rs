//! `--dump-model json`: the tree as numbers, on standard output.
//!
//! Comparison against an independent oracle runs over this, not over screen
//! text: text cannot tell an arithmetic error from a layout error. Nothing that
//! changes between two runs over the same snapshot is printed, so the FR-17
//! acceptance - two runs, one identical dump - holds.

use crate::model::{Limits, Metrics, Mode, Node, Readings, Snapshot};

pub fn model_json(snap: &Snapshot, tick_ms: u64) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"ticks\": {},\n", snap.ticks));
    out.push_str(&format!("  \"tick_ms\": {},\n", tick_ms));
    out.push_str(&format!(
        "  \"root_privileges\": {},\n  \"docker_available\": {},\n",
        snap.root_privileges, snap.docker_available
    ));
    out.push_str("  \"host\": {\n");
    let h = &snap.host;
    out.push_str(&format!("    \"hostname\": {},\n", string(&h.hostname)));
    out.push_str(&format!("    \"cores\": {},\n", num(h.cores)));
    out.push_str(&format!("    \"busy_cores\": {},\n", num(h.busy_cores)));
    out.push_str(&format!(
        "    \"busy_cores_avg\": {},\n",
        num(h.busy_cores_avg)
    ));
    out.push_str(&format!("    \"mem_used\": {},\n", num(h.mem_used)));
    out.push_str(&format!("    \"mem_total\": {},\n", num(h.mem_total)));
    out.push_str(&format!("    \"swap_used\": {},\n", num(h.swap_used)));
    out.push_str(&format!("    \"swap_total\": {},\n", num(h.swap_total)));
    out.push_str(&format!("    \"net_rx\": {},\n", num(h.net_rx)));
    out.push_str(&format!("    \"net_tx\": {},\n", num(h.net_tx)));
    out.push_str(&format!(
        "    \"load\": [{}, {}, {}]\n",
        num(h.load[0]),
        num(h.load[1]),
        num(h.load[2])
    ));
    out.push_str("  },\n");
    // The denominators every reading was taken against. A reading cannot be
    // checked without them, and they are what separates a threshold that fired
    // on the right row from one that fired on the wrong machine (D-42).
    let l = &snap.limits;
    out.push_str("  \"limits\": {\n");
    out.push_str(&format!("    \"cores\": {},\n", num(l.cores)));
    out.push_str(&format!("    \"mem_total\": {},\n", num(l.mem_total)));
    out.push_str(&format!("    \"swap_total\": {},\n", num(l.swap_total)));
    out.push_str(&format!("    \"pid_max\": {},\n", opt(l.pid_max)));
    out.push_str(&format!("    \"link_speed\": {}\n", opt(l.link_speed)));
    out.push_str("  },\n");
    out.push_str("  \"tree\": ");
    node_json(&snap.root, 1, &snap.limits, &mut out);
    out.push('\n');
    out.push_str("}\n");
    out
}

fn node_json(node: &Node, depth: usize, limits: &Limits, out: &mut String) {
    let pad = "  ".repeat(depth + 1);
    let inner = "  ".repeat(depth + 2);
    out.push_str("{\n");
    out.push_str(&format!("{pad}\"id\": {},\n", string(&node.id)));
    out.push_str(&format!("{pad}\"name\": {},\n", string(&node.name)));
    out.push_str(&format!("{pad}\"kind\": {},\n", string(node.kind.label())));
    if let Some(pid) = node.detail.pid {
        out.push_str(&format!("{pad}\"pid\": {pid},\n"));
    }
    if let Some(path) = &node.detail.cgroup_path {
        out.push_str(&format!("{pad}\"cgroup\": {},\n", string(path)));
    }
    if let Some(id) = &node.detail.short_id {
        out.push_str(&format!("{pad}\"container_id\": {},\n", string(id)));
    }
    if let Some(info) = &node.detail.container {
        out.push_str(&format!("{pad}\"image\": {},\n", string(&info.image)));
        out.push_str(&format!("{pad}\"state\": {},\n", string(&info.state)));
    }
    // Every link of a glued chain, the first one included, and each with the
    // name that belongs to it: a check that the card names them all cannot be
    // made out of pids alone - a list that pairs each pid with its neighbour's
    // name has the same pids (D-25).
    if !node.detail.glued.is_empty() {
        let links: Vec<String> = node
            .detail
            .glued
            .iter()
            .map(|(pid, name)| string(&format!("{pid} {name}")))
            .collect();
        out.push_str(&format!("{pad}\"chain\": [{}],\n", links.join(", ")));
    }
    if let Some(owner) = &node.detail.owner {
        out.push_str(&format!("{pad}\"owner\": {},\n", string(&owner.name)));
        out.push_str(&format!(
            "{pad}\"owner_kind\": {},\n",
            string(owner.kind.label())
        ));
    }
    out.push_str(&format!("{pad}\"instant\": "));
    metrics_json(&node.instant, out);
    out.push_str(",\n");
    out.push_str(&format!("{pad}\"avg\": "));
    metrics_json(&node.avg, out);
    out.push_str(",\n");
    // The instant figures, because those are what the screen opens on. Disk
    // has no field here, exactly as it has none in the type (D-42).
    out.push_str(&format!("{pad}\"reading\": "));
    readings_json(&node.readings(limits, Mode::Instant), out);
    out.push_str(",\n");
    if node.children.is_empty() {
        out.push_str(&format!("{pad}\"children\": []\n"));
    } else {
        out.push_str(&format!("{pad}\"children\": [\n"));
        for (i, c) in node.children.iter().enumerate() {
            out.push_str(&inner);
            node_json(c, depth + 2, limits, out);
            if i + 1 < node.children.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&format!("{pad}]\n"));
    }
    out.push_str(&format!("{}}}", "  ".repeat(depth)));
}

fn readings_json(r: &Readings, out: &mut String) {
    out.push_str(&format!(
        "{{\"cpu\": {}, \"mem\": {}, \"swap\": {}, \"tasks\": {}, \"rx\": {}, \"tx\": {}, \"flag\": {}}}",
        string(r.cpu.label()),
        string(r.mem.label()),
        string(r.swap.label()),
        string(r.tasks.label()),
        string(r.rx.label()),
        string(r.tx.label()),
        string(r.flag.label())
    ));
}

fn metrics_json(m: &Metrics, out: &mut String) {
    out.push_str(&format!(
        "{{\"cpu\": {}, \"mem\": {}, \"swap\": {}, \"tasks\": {}, \"rd\": {}, \"wr\": {}, \"rx\": {}, \"tx\": {}}}",
        opt(m.cpu),
        opt(m.mem),
        opt(m.swap),
        opt(m.tasks),
        opt(m.rd),
        opt(m.wr),
        opt(m.rx),
        opt(m.tx)
    ));
}

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    // Six decimals: enough for a rate of a few bytes per second, and stable
    // between runs, which a shortest-roundtrip print is not across platforms.
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-0" {
        "0".to_string()
    } else {
        s
    }
}

fn opt(v: Option<f64>) -> String {
    match v {
        Some(v) => num(v),
        None => "null".to_string(),
    }
}

fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Kind, Metrics, Node, Snapshot};

    #[test]
    fn an_unavailable_value_is_null_rather_than_zero() {
        let mut snap = Snapshot::empty();
        let mut n = Node::new("p:1", "a", Kind::Process);
        n.instant = Metrics {
            cpu: Some(0.5),
            rx: None,
            ..Metrics::default()
        };
        snap.root.children.push(n);
        let text = model_json(&snap, 1000);
        assert!(text.contains("\"rx\": null"), "{text}");
        assert!(text.contains("\"cpu\": 0.5"), "{text}");
    }

    /// The comparison against the oracle runs over this text, so a threshold
    /// that fires on the wrong row has to be visible here rather than only on
    /// screen (D-42). The denominators go with it: a reading cannot be checked
    /// without the whole it is a share of.
    #[test]
    fn the_dump_carries_the_reading_and_what_it_was_read_against() {
        use crate::model::Limits;
        let mut snap = Snapshot::empty();
        snap.limits = Limits {
            cores: 4.0,
            mem_total: 8.0 * 1024.0 * 1024.0 * 1024.0,
            swap_total: 0.0,
            pid_max: Some(32768.0),
            link_speed: None,
        };
        let mut n = Node::new("p:1", "a", Kind::Process);
        n.instant = Metrics {
            cpu: Some(2.5),
            ..Metrics::default()
        };
        n.detail.state = Some('Z');
        snap.root.children.push(n);
        let text = model_json(&snap, 1000);
        assert!(text.contains("\"cpu\": \"alarm\""), "{text}");
        assert!(text.contains("\"flag\": \"alarm\""), "{text}");
        assert!(
            !text.contains("\"rd\": \"calm\""),
            "disk has no reading: {text}"
        );
        assert!(text.contains("\"pid_max\": 32768"), "{text}");
        assert!(text.contains("\"link_speed\": null"), "{text}");
        assert!(crate::enrich::json::parse(&text).is_some(), "{text}");
    }

    #[test]
    fn the_same_snapshot_prints_the_same_text() {
        let snap = Snapshot::empty();
        assert_eq!(model_json(&snap, 1000), model_json(&snap, 1000));
    }

    #[test]
    fn the_dump_is_valid_json() {
        let mut snap = Snapshot::empty();
        let mut a = Node::new("p:1", "a", Kind::Process);
        a.children.push(Node::new("p:2", "b", Kind::Process));
        snap.root.children.push(a);
        let text = model_json(&snap, 1000);
        assert!(crate::enrich::json::parse(&text).is_some(), "{text}");
    }
}
