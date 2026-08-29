//! The interface state and what a keystroke does to it.
//!
//! Navigation is the same on every level (section 4): the level comes from the
//! position in the tree, not from a mode switch, and sorting, search and the
//! measurement mode survive every transition (FR-2, FR-6, FR-13).

use crate::collect::procs::ProcExtras;
use crate::model::{self_row, Kind, Mode, Node, Snapshot, Sort};

/// A key of the program, named the way `--keys` and `tmux send-keys` name them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Right,
    Left,
    PageUp,
    PageDown,
    Enter,
    Escape,
    Backspace,
    Space,
    Char(char),
}

impl Key {
    /// Parses one name of a key program (`--keys "Right Right a Escape"`).
    pub fn parse(name: &str) -> Option<Key> {
        Some(match name {
            "Up" => Key::Up,
            "Down" => Key::Down,
            "Right" => Key::Right,
            "Left" => Key::Left,
            // `PPage` and `NPage` are what `tmux send-keys` calls these two,
            // and a scenario is written in the names tmux takes (FR-17).
            "PageUp" | "PgUp" | "PPage" => Key::PageUp,
            "PageDown" | "PgDn" | "NPage" => Key::PageDown,
            "Enter" => Key::Enter,
            "Escape" | "Esc" => Key::Escape,
            "Backspace" => Key::Backspace,
            "Space" => Key::Space,
            other => {
                let mut chars = other.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Key::Char(c)
            }
        })
    }

    pub fn program(text: &str) -> Result<Vec<Key>, String> {
        text.split_whitespace()
            .map(|n| Key::parse(n).ok_or_else(|| format!("unknown key name: {n}")))
            .collect()
    }
}

/// The interval as a word: the pause by name, a whole number of seconds
/// without a decimal, and a tick below a second in the unit it was given in -
/// `--tick 60` is a verification hook, and rounding it to `0s` would say the
/// screen stands still when it is renewed sixteen times a second.
pub fn interval_label(ms: u64) -> String {
    if ms == 0 {
        "paused".to_string()
    } else if ms < 1000 {
        format!("{ms}ms")
    } else if ms % 1000 == 0 {
        format!("{}s", ms / 1000)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

/// How a level is laid out. The tree shows one level at a time (section 4);
/// the list flattens the subtree into its ends - the leaves, and the `(self)`
/// remainder of every node that has children - which is how a process is found
/// when the level it sits on is not known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Tree,
    List,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Tree => "tree",
            View::List => "list",
        }
    }
}

/// One row of the table: the node it draws, how many children it has, how to
/// reach it and which level it lives on.
///
/// The node is carried without its subtree, because a row never draws one.
/// `trail` is the way down to the node itself, which is what descending needs;
/// `home` is the way down to the level the row sits on, which is where `→`
/// goes when the row has nothing under it. In the tree view both are a single
/// step and an empty path; in the list view they are as long as the depth of
/// the row. `prefix` is the chain of names between the current level and the
/// row, drawn pale in front of the name so a flattened row still says where it
/// came from.
#[derive(Clone, Debug)]
pub struct Row {
    pub node: Node,
    pub children: usize,
    pub trail: Vec<String>,
    pub home: Vec<String>,
    pub prefix: String,
}

impl Row {
    fn of(node: &Node, trail: Vec<String>, home: Vec<String>, prefix: String) -> Row {
        Row {
            node: node.shallow(),
            children: node.children.len(),
            trail,
            home,
            prefix,
        }
    }
}

/// The steps the refresh interval moves between, in milliseconds, with a pause
/// at the near end of them (D-28): `-` walks down this list and `+` walks up
/// it. The values are the ones an engineer asks for - a second while watching
/// something move, a minute while leaving the screen on - and nothing in
/// between, because a free number is a number to choose rather than a key to
/// press.
pub const STEPS: [u64; 8] = [0, 1000, 2000, 3000, 5000, 10_000, 30_000, 60_000];

/// The interval the application opens at.
pub const DEFAULT_INTERVAL_MS: u64 = 3000;

pub struct App {
    pub snapshot: Snapshot,
    /// The frame shown while paused. Collection carries on behind it (FR-7).
    pub frozen: Option<Snapshot>,
    pub path: Vec<String>,
    pub cursor: usize,
    pub card: Option<Row>,
    pub card_extras: Option<ProcExtras>,
    pub sort: Sort,
    pub mode: Mode,
    pub layout: View,
    pub filter: String,
    pub filtering: bool,
    /// How often the frame is renewed, in milliseconds; zero is the pause.
    /// Collection keeps its own pace behind a pause (FR-7), so the value the
    /// screen returns to is remembered rather than recomputed.
    interval_ms: u64,
    resume_ms: u64,
    pub flash: String,
    pub quit: bool,
    pub table_rows: usize,
    pub scroll: usize,
    /// Which palette the screen is drawn in. An index into `theme::THEMES`
    /// rather than the palette itself: the renderer reads it once a frame, and
    /// `t` walks it.
    pub theme: usize,
}

/// When the screen is written whole rather than as a difference against the
/// last frame.
///
/// The drawing library writes only the cells that changed, which is what keeps
/// the output per frame inside the bound of section 6. That is right as long as
/// what is on the terminal is what the program last wrote - and it is not
/// always. Found on 2026-08-29: `sudo` runs the program in a pseudo-terminal of
/// its own and relays the bytes to the terminal the reader is looking at, and
/// what the relay loses stays on the screen until every cell is written again.
/// The reader saw a frame of nonsense and had to find a key that happened to
/// rewrite every row (D-38).
pub struct Repaint {
    every: std::time::Duration,
    last: Option<std::time::Instant>,
}

impl Repaint {
    pub fn new(every: std::time::Duration) -> Repaint {
        Repaint { every, last: None }
    }

    /// True when the span has passed since the last time it was true. The
    /// clock is passed in rather than read here, so the rule can be tested
    /// without waiting for it.
    pub fn due(&mut self, now: std::time::Instant) -> bool {
        match self.last {
            None => {
                self.last = Some(now);
                false
            }
            Some(last) if now.duration_since(last) >= self.every => {
                self.last = Some(now);
                true
            }
            Some(_) => false,
        }
    }
}

impl App {
    pub fn new(snapshot: Snapshot) -> App {
        App {
            snapshot,
            frozen: None,
            path: Vec::new(),
            cursor: 0,
            card: None,
            card_extras: None,
            sort: Sort::Cpu,
            // The average since start is the mode to open in: a single interval
            // jumps around too much to judge a host by, and `a` switches to it.
            mode: Mode::Average,
            layout: View::Tree,
            filter: String::new(),
            filtering: false,
            interval_ms: DEFAULT_INTERVAL_MS,
            resume_ms: DEFAULT_INTERVAL_MS,
            flash: String::new(),
            quit: false,
            table_rows: 12,
            scroll: 0,
            theme: 0,
        }
    }

    /// The screen is held (FR-7).
    pub fn paused(&self) -> bool {
        self.interval_ms == 0
    }

    /// How often collection runs. A pause holds the screen and not the
    /// collector, so behind a paused frame the tick is the one the screen will
    /// return to (FR-7).
    pub fn tick_ms(&self) -> u64 {
        if self.interval_ms == 0 {
            self.resume_ms
        } else {
            self.interval_ms
        }
    }

    /// The interval as the key line writes it.
    pub fn interval_label(&self) -> String {
        interval_label(self.interval_ms)
    }

    /// Sets the interval to any value in milliseconds. `--tick` comes through
    /// here, so a tick that is not one of the [`STEPS`] is kept as it is and
    /// shown as it is; `-` and `+` move from it to the nearest step.
    pub fn set_interval_ms(&mut self, ms: u64) {
        self.interval_ms = ms;
        if ms > 0 {
            self.resume_ms = ms;
            self.frozen = None;
        } else if self.frozen.is_none() {
            self.frozen = Some(self.snapshot.clone());
        }
    }

    /// `-`: the next step towards the near end of the list, where the pause is.
    fn step_down(&mut self) {
        let next = STEPS
            .iter()
            .rev()
            .copied()
            .find(|s| *s < self.interval_ms)
            .unwrap_or(0);
        self.set_interval_ms(next);
    }

    /// `+`: the next step towards the far end of the list, which is a minute.
    fn step_up(&mut self) {
        let next = STEPS
            .iter()
            .copied()
            .find(|s| *s > self.interval_ms)
            .unwrap_or(STEPS[STEPS.len() - 1]);
        self.set_interval_ms(next);
    }

    fn toggle_pause(&mut self) {
        if self.paused() {
            let back = self.resume_ms;
            self.set_interval_ms(back);
        } else {
            self.set_interval_ms(0);
        }
    }

    /// The snapshot the frame is drawn from: the frozen one while paused, the
    /// live one otherwise.
    pub fn view(&self) -> &Snapshot {
        self.frozen.as_ref().unwrap_or(&self.snapshot)
    }

    /// The process forest the screen navigates.
    pub fn tree(&self) -> &Node {
        &self.view().root
    }

    pub fn update(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot;
        // A path whose tail has vanished is trimmed to what still resolves, so
        // a process that exited does not leave the view on a dead node.
        let depth = self.tree().resolved_depth(&self.path);
        if depth < self.path.len() && self.frozen.is_none() {
            self.path.truncate(depth);
            self.cursor = 0;
            self.scroll = 0;
        }
    }

    pub fn current(&self) -> &Node {
        self.tree().at_path(&self.path)
    }

    /// The rows of the current level: the children in the tree view, the
    /// endpoints of the subtree in the list view, filtered and sorted, with
    /// the `(self)` row pinned first and excluded from sorting (FR-14).
    pub fn rows(&self) -> Vec<Row> {
        let node = self.current();
        let mut kids: Vec<Row> = Vec::new();
        match self.layout {
            View::Tree => {
                for c in &node.children {
                    if self.matches(c) {
                        kids.push(Row::of(c, vec![c.id.clone()], Vec::new(), String::new()));
                    }
                }
            }
            View::List => self.flatten(node, &mut Vec::new(), "", &mut kids),
        }
        let sort = self.sort;
        let mode = self.mode;
        kids.sort_by(|a, b| {
            let ka = sort.key(a.node.metrics(mode));
            let kb = sort.key(b.node.metrics(mode));
            kb.partial_cmp(&ka)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.node.name.cmp(&b.node.name))
        });
        if let Some(row) = self_row(node) {
            if self.matches(&row) {
                kids.insert(0, Row::of(&row, Vec::new(), Vec::new(), String::new()));
            }
        }
        kids
    }

    /// The endpoints of the subtree as one row each: a node with no children,
    /// and the `(self)` remainder of a node that has them. Both are ends of
    /// the tree - the remainder never expands either (FR-14) - and together
    /// they carry the whole of the level: what the leaves use, plus what their
    /// parents hold themselves. A node in the middle is not a row; its numbers
    /// are the sum of the rows under it, and the tree view is where they are
    /// read.
    ///
    /// A node the filter rejects is left out, but its children are still
    /// walked: in the list view the filter is how a process is found without
    /// knowing which level it sits on, and a parent that does not match must
    /// not hide it.
    fn flatten(&self, node: &Node, trail: &mut Vec<String>, prefix: &str, out: &mut Vec<Row>) {
        for c in &node.children {
            trail.push(c.id.clone());
            if c.children.is_empty() {
                if self.matches(c) {
                    let home = trail[..trail.len() - 1].to_vec();
                    out.push(Row::of(c, trail.clone(), home, prefix.to_string()));
                }
            } else {
                let deeper = format!("{prefix}{}/", c.name);
                if let Some(own) = self_row(c) {
                    if self.matches(&own) {
                        // The remainder lives on the level of the node it was
                        // computed from, which is where `→` takes it.
                        out.push(Row::of(&own, Vec::new(), trail.clone(), deeper.clone()));
                    }
                }
                self.flatten(c, trail, &deeper, out);
            }
            trail.pop();
        }
    }

    /// Substring search over what is shown: the name, the type, the command
    /// line and the container image (FR-6).
    fn matches(&self, node: &Node) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let needle = self.filter.to_lowercase();
        if node.name.to_lowercase().contains(&needle) {
            return true;
        }
        // The container a row leads into is drawn on the row, so the filter
        // reaches it: typing a container name finds the row that leads into it
        // as well as the processes inside it (D-30).
        if let Some(container) = &node.detail.leads_into {
            if container.to_lowercase().contains(&needle) {
                return true;
            }
        }
        if node.kind.label().contains(&needle) {
            return true;
        }
        // The owner is on the row, so the filter reaches it: `grafana` finds
        // every process of that container whatever the processes are called
        // (FR-20).
        if let Some(owner) = &node.detail.owner {
            if owner.name.to_lowercase().contains(&needle) || owner.kind.label().contains(&needle) {
                return true;
            }
        }
        if let Some(cmd) = &node.detail.cmdline {
            if cmd.to_lowercase().contains(&needle) {
                return true;
            }
        }
        if let Some(info) = &node.detail.container {
            if info.image.to_lowercase().contains(&needle) {
                return true;
            }
        }
        false
    }

    pub fn selected(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.cursor)
    }

    /// The card node re-resolved against the live tree, so its numbers keep
    /// moving while it is open; the copy taken when it was opened is the
    /// fallback for a node that disappeared under it.
    pub fn card_node(&self) -> Option<Node> {
        let row = self.card.as_ref()?;
        let card = &row.node;
        if card.kind == Kind::Own {
            return self_row(self.own_parent(row)).or_else(|| Some(card.clone()));
        }
        Some(
            self.tree()
                .find(&card.id)
                .cloned()
                .unwrap_or_else(|| card.clone()),
        )
    }

    /// The node a `(self)` row was computed from. In the tree view that is the
    /// level in view; in the list view the row can be the remainder of a node
    /// several levels down, and its card must name that node rather than the
    /// level the reader happens to stand on.
    pub fn own_parent(&self, row: &Row) -> &Node {
        self.current().at_path(&row.home)
    }

    pub fn level(&self) -> usize {
        self.path.len().min(3)
    }

    pub fn on_key(&mut self, key: Key, proc_root: &std::path::Path) {
        if self.filtering {
            match key {
                Key::Escape => {
                    self.filtering = false;
                    self.filter.clear();
                }
                Key::Enter => self.filtering = false,
                Key::Backspace => {
                    self.filter.pop();
                }
                Key::Space => self.filter.push(' '),
                Key::Char(c) => {
                    self.filter.push(c);
                    self.cursor = 0;
                }
                _ => {}
            }
            self.clamp();
            return;
        }

        self.flash.clear();
        match key {
            Key::Down | Key::Char('j') => {
                if self.card.is_none() {
                    let len = self.rows().len();
                    if len > 0 {
                        self.cursor = (self.cursor + 1).min(len - 1);
                    }
                }
            }
            Key::Up | Key::Char('k') => {
                if self.card.is_none() {
                    self.cursor = self.cursor.saturating_sub(1);
                }
            }
            // A page is what the table holds at once, which is what a pager
            // and a process viewer both move by (D-29). At the ends the move is short
            // rather than refused: the key takes the cursor to the last row
            // instead of doing nothing, so a long level is walked to its end
            // with one key rather than with two.
            Key::PageDown => {
                if self.card.is_none() {
                    let len = self.rows().len();
                    if len > 0 {
                        let page = self.page();
                        self.cursor = (self.cursor + page).min(len - 1);
                        // The window moves with the cursor, so the next screen
                        // is the rows that came after this one rather than the
                        // same rows shifted by a line. `clamp` pulls it back
                        // where the level ends.
                        self.scroll += page;
                    }
                }
            }
            Key::PageUp => {
                if self.card.is_none() {
                    let page = self.page();
                    self.cursor = self.cursor.saturating_sub(page);
                    self.scroll = self.scroll.saturating_sub(page);
                }
            }
            // Enter is the way down (D-25). On a row with nothing under it -
            // the end of a branch, the `(self)` remainder - there is nowhere to
            // go, and what the reader wants there is the detail, so the same
            // key opens the card. `i` opens it on any row at all.
            Key::Enter => {
                let target = self.card.clone().or_else(|| self.selected());
                match target {
                    Some(row) if row.children > 0 => {
                        let trail = row.trail.clone();
                        self.descend(trail);
                    }
                    Some(row) => self.open_card(row, proc_root),
                    None => {}
                }
            }
            // The card of any row, (self) included. It is not a level: neither
            // the path nor the selected row changes (D-10).
            Key::Char('i') => {
                if self.card.is_none() {
                    if let Some(row) = self.selected() {
                        self.open_card(row, proc_root);
                    }
                }
            }
            Key::Right | Key::Char('l') => {
                let target = self.card.clone().or_else(|| self.selected());
                match target {
                    // In the list view the row can sit several levels down, so
                    // descending walks the whole trail at once rather than one
                    // step of it.
                    Some(row) if row.children > 0 => {
                        let trail = row.trail.clone();
                        self.descend(trail);
                    }
                    // Every row of the list is an end of the tree, so there is
                    // nothing under it. What the key is for there is the level
                    // the row lives on: it puts the row among its neighbours.
                    Some(row) if !row.home.is_empty() => {
                        let home = row.home.clone();
                        self.descend(home);
                    }
                    Some(_) => self.flash = "no children to descend into".into(),
                    None => {}
                }
            }
            // Escape undoes the narrowing, from the nearest one outwards
            // (D-29): the card, then the filter, then the level. A filter left on after the
            // reader stopped typing is the state that made the table look empty
            // for no reason, so it goes before the level does - and `Backspace`
            // still walks the levels with the filter kept, which is what FR-2
            // asks for.
            Key::Escape => {
                if self.card.is_some() {
                    self.card = None;
                    self.card_extras = None;
                } else if !self.filter.is_empty() {
                    self.filter.clear();
                    self.cursor = 0;
                    self.scroll = 0;
                } else if self.path.pop().is_some() {
                    self.cursor = 0;
                    self.scroll = 0;
                }
            }
            Key::Left | Key::Backspace | Key::Char('h') => {
                if self.card.is_some() {
                    self.card = None;
                    self.card_extras = None;
                } else if self.path.pop().is_some() {
                    self.cursor = 0;
                    self.scroll = 0;
                }
            }
            Key::Char('/') => {
                if self.card.is_none() {
                    self.filtering = true;
                    self.filter.clear();
                    self.cursor = 0;
                }
            }
            Key::Char('c') => self.sort = Sort::Cpu,
            Key::Char('m') => self.sort = Sort::Mem,
            Key::Char('d') => self.sort = Sort::Disk,
            Key::Char('n') => self.sort = Sort::Net,
            Key::Char('v') => {
                self.layout = match self.layout {
                    View::Tree => View::List,
                    View::List => View::Tree,
                };
                // The two views hold different rows, so the index of the old
                // one means nothing in the new one.
                self.cursor = 0;
                self.scroll = 0;
            }
            Key::Char('a') => {
                self.mode = match self.mode {
                    Mode::Instant => Mode::Average,
                    Mode::Average => Mode::Instant,
                }
            }
            Key::Space => self.toggle_pause(),
            // The refresh interval walks the list of steps, and the pause is
            // its near end: holding the screen is the same act as renewing it
            // less often, so it is the same pair of keys rather than a mode of
            // its own. `=` is the unshifted key `+` sits on.
            Key::Char('-') | Key::Char('_') => self.step_down(),
            Key::Char('+') | Key::Char('=') => self.step_up(),
            Key::Char('t') => {
                self.theme = (self.theme + 1) % crate::theme::THEMES.len();
                let t = &crate::theme::THEMES[self.theme];
                self.flash = format!("theme: {} - {}", t.name, t.about);
            }
            Key::Char('q') => self.quit = true,
            _ => {}
        }
        self.clamp();
    }

    fn open_card(&mut self, row: Row, proc_root: &std::path::Path) {
        self.card_extras = row
            .node
            .detail
            .pid
            .map(|pid| crate::collect::procs::extras(proc_root, pid));
        self.card = Some(row);
    }

    /// Walks the path down by a whole trail at once and puts the cursor at the
    /// top of the new level. The card belongs to the level it was opened on,
    /// so it closes with the move.
    fn descend(&mut self, trail: Vec<String>) {
        self.path.extend(trail);
        self.cursor = 0;
        self.scroll = 0;
        self.card = None;
        self.card_extras = None;
    }

    /// The rows one page key moves by: what the table draws at once.
    fn page(&self) -> usize {
        self.table_rows.max(1)
    }

    fn clamp(&mut self) {
        let len = self.rows().len();
        if len == 0 {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        if self.cursor >= len {
            self.cursor = len - 1;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        let rows = self.table_rows.max(1);
        if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
        if self.scroll + rows > len {
            self.scroll = len.saturating_sub(rows);
        }
    }

    /// The path line: names rather than identifiers, so an engineer reads where
    /// they are.
    pub fn crumbs(&self) -> String {
        let mut out = String::from("host");
        let mut node = self.tree();
        for step in &self.path {
            match node.children.iter().find(|c| &c.id == step) {
                Some(next) => {
                    out.push_str(" \u{203a} ");
                    out.push_str(&next.name);
                    node = next;
                }
                None => break,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metrics, Node};

    fn tree() -> Snapshot {
        let mut snap = Snapshot::empty();
        let mut a = Node::new("cg:/a", "alpha", Kind::Process);
        a.instant = Metrics {
            cpu: Some(1.0),
            mem: Some(100.0),
            ..Metrics::default()
        };
        a.avg = a.instant;
        let mut inner = Node::new("cg:/a/x", "inner", Kind::Process);
        inner.instant = Metrics {
            cpu: Some(0.4),
            mem: Some(40.0),
            ..Metrics::default()
        };
        inner.avg = inner.instant;
        a.children.push(inner);
        let mut b = Node::new("cg:/b", "beta", Kind::Process);
        b.instant = Metrics {
            cpu: Some(2.0),
            mem: Some(10.0),
            ..Metrics::default()
        };
        b.avg = b.instant;
        snap.root.children = vec![a, b];
        // The root holds more than its children, so the level has a (self) row.
        snap.root.instant = Metrics {
            cpu: Some(3.5),
            mem: Some(120.0),
            ..Metrics::default()
        };
        snap.root.avg = snap.root.instant;
        snap
    }

    #[test]
    fn sorting_changes_the_order_but_not_the_place_of_self() {
        let mut app = App::new(tree());
        assert_eq!(app.rows()[0].node.kind, Kind::Own);
        assert_eq!(app.rows()[1].node.name, "beta");
        app.on_key(Key::Char('m'), std::path::Path::new("/proc"));
        assert_eq!(app.rows()[0].node.kind, Kind::Own);
        assert_eq!(app.rows()[1].node.name, "alpha");
    }

    #[test]
    fn the_card_is_not_a_level() {
        let mut app = App::new(tree());
        app.cursor = 1;
        let proc = std::path::Path::new("/proc");
        app.on_key(Key::Enter, proc);
        assert!(app.card.is_some());
        assert_eq!(app.path.len(), 0);
        assert_eq!(app.cursor, 1);
        app.on_key(Key::Escape, proc);
        assert!(app.card.is_none());
        assert_eq!(app.cursor, 1);
        assert_eq!(app.path.len(), 0);
    }

    #[test]
    fn drill_down_keeps_sorting_and_filter() {
        let mut app = App::new(tree());
        let proc = std::path::Path::new("/proc");
        app.on_key(Key::Char('m'), proc);
        // The filter hides the (self) row too, so alpha is the first row.
        app.filter = "a".into();
        app.cursor = 0;
        app.on_key(Key::Right, proc);
        assert_eq!(app.path, vec!["cg:/a".to_string()]);
        assert_eq!(app.sort, crate::model::Sort::Mem);
        assert_eq!(app.filter, "a");
        // The way back up is `Backspace`, and it carries the filter with it
        // (FR-2). `Escape` is the key that drops the filter, which is why the
        // two are no longer the same key.
        app.on_key(Key::Backspace, proc);
        assert!(app.path.is_empty());
        assert_eq!(app.filter, "a");
    }

    #[test]
    fn escape_undoes_the_card_then_the_filter_then_the_level() {
        let mut app = App::new(tree());
        let proc = std::path::Path::new("/proc");
        // Down into alpha, then a filter over the level under it, then a card:
        // three narrowings, one inside the next.
        app.cursor = 2;
        app.on_key(Key::Right, proc);
        assert_eq!(app.path, vec!["cg:/a".to_string()]);
        app.filter = "inner".into();
        app.cursor = 0;
        app.on_key(Key::Char('i'), proc);
        assert!(app.card.is_some());
        app.on_key(Key::Escape, proc);
        assert!(app.card.is_none(), "the card is nearest to hand");
        assert_eq!(app.filter, "inner", "the card cost the filter");
        app.on_key(Key::Escape, proc);
        assert_eq!(app.filter, "", "the filter did not clear");
        assert_eq!(app.path.len(), 1, "clearing the filter left the level too");
        app.on_key(Key::Escape, proc);
        assert!(app.path.is_empty(), "the level did not come back");
    }

    #[test]
    fn the_page_keys_move_by_what_the_table_holds() {
        let mut snap = Snapshot::empty();
        for i in 0..40 {
            let mut n = Node::new(format!("p:{i}"), &format!("proc{i:02}"), Kind::Process);
            n.instant = Metrics {
                cpu: Some(40.0 - i as f64),
                ..Metrics::default()
            };
            n.avg = n.instant;
            snap.root.children.push(n);
        }
        let mut app = App::new(snap);
        app.table_rows = 12;
        let proc = std::path::Path::new("/proc");
        app.on_key(Key::PageDown, proc);
        assert_eq!(app.cursor, 12);
        // The rows the cursor moved onto are on screen with it.
        assert!(app.scroll <= app.cursor && app.cursor < app.scroll + 12);
        app.on_key(Key::PageDown, proc);
        assert_eq!(app.cursor, 24);
        app.on_key(Key::PageUp, proc);
        assert_eq!(app.cursor, 12);
        // At the ends the move is short rather than refused.
        for _ in 0..10 {
            app.on_key(Key::PageDown, proc);
        }
        assert_eq!(app.cursor, app.rows().len() - 1);
        for _ in 0..10 {
            app.on_key(Key::PageUp, proc);
        }
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn the_interval_walks_the_steps_and_ends_at_the_pause() {
        let mut app = App::new(tree());
        let proc = std::path::Path::new("/proc");
        assert_eq!(app.interval_label(), "3s");
        app.on_key(Key::Char('+'), proc);
        assert_eq!(app.interval_label(), "5s");
        for _ in 0..10 {
            app.on_key(Key::Char('+'), proc);
        }
        assert_eq!(app.interval_label(), "60s", "the far end is a minute");
        for _ in 0..10 {
            app.on_key(Key::Char('-'), proc);
        }
        assert!(app.paused(), "the near end is the pause");
        assert_eq!(app.interval_label(), "paused");
        // Collection keeps the pace the screen will return to (FR-7).
        assert_eq!(app.tick_ms(), 1000);
        assert!(app.frozen.is_some(), "a pause holds the frame");
        app.on_key(Key::Char('+'), proc);
        assert_eq!(app.interval_label(), "1s");
        assert!(app.frozen.is_none(), "the frame was not let go");
        // A tick that is not one of the steps is kept as it was given, and the
        // keys move from it to the nearest step.
        app.set_interval_ms(1500);
        assert_eq!(app.interval_label(), "1.5s");
        app.on_key(Key::Char('-'), proc);
        assert_eq!(app.interval_label(), "1s");
        // Space is the same pause, from either side of it.
        app.on_key(Key::Space, proc);
        assert!(app.paused());
        app.on_key(Key::Space, proc);
        assert_eq!(app.interval_label(), "1s");
    }

    #[test]
    fn the_list_view_holds_the_ends_of_the_tree_and_nothing_in_the_middle() {
        let mut snap = tree();
        let mut leaf = Node::new("cg:/a/x/y", "leaf", Kind::Process);
        leaf.instant = Metrics {
            cpu: Some(0.1),
            ..Metrics::default()
        };
        leaf.avg = leaf.instant;
        snap.root.children[0].children[0].children.push(leaf);
        let mut app = App::new(snap);
        let proc = std::path::Path::new("/proc");
        // The tree view of the root holds its two children and the (self) row.
        assert_eq!(app.rows().len(), 3);
        app.on_key(Key::Char('v'), proc);
        let names: Vec<String> = app
            .rows()
            .iter()
            .map(|r| format!("{}{}", r.prefix, r.node.name))
            .collect();
        // alpha and inner are in the middle: their numbers are the sum of what
        // stands under them. What is left are the ends - the leaves, and the
        // remainder of every node that has children.
        assert_eq!(
            names,
            vec![
                "(self)",
                "beta",
                "alpha/(self)",
                "alpha/inner/(self)",
                "alpha/inner/leaf",
            ]
        );
        // The rows still add up to the level: 2.0 + 0.6 + 0.3 + 0.1 = 3.0, and
        // the (self) row of the root carries the rest of its 3.5.
        let total: f64 = app.rows().iter().filter_map(|r| r.node.instant.cpu).sum();
        assert!(
            (total - app.current().instant.cpu.unwrap()).abs() < 1e-9,
            "the rows sum to {total}"
        );
        // A filter reaches a node two levels down, and the arrow puts it among
        // its neighbours: the level it lives on, in one step.
        app.filter = "leaf".into();
        assert_eq!(app.rows().len(), 1);
        app.cursor = 0;
        app.on_key(Key::Right, proc);
        assert_eq!(app.path, vec!["cg:/a".to_string(), "cg:/a/x".to_string()]);
    }

    #[test]
    fn filter_narrows_the_level() {
        let mut app = App::new(tree());
        app.filter = "bet".into();
        let names: Vec<String> = app.rows().iter().map(|r| r.node.name.clone()).collect();
        assert!(names.contains(&"beta".to_string()));
        assert!(!names.contains(&"alpha".to_string()));
    }

    #[test]
    fn pause_freezes_the_frame_while_collection_goes_on() {
        let mut app = App::new(tree());
        let proc = std::path::Path::new("/proc");
        app.on_key(Key::Space, proc);
        let mut next = tree();
        next.root.children[1].instant.cpu = Some(99.0);
        app.update(next);
        assert_eq!(app.view().root.children[1].instant.cpu, Some(2.0));
        app.on_key(Key::Space, proc);
        assert_eq!(app.view().root.children[1].instant.cpu, Some(99.0));
    }

    #[test]
    fn key_program_parses_the_names_tmux_uses() {
        let keys = Key::program("Right Right a Escape /").unwrap();
        assert_eq!(
            keys,
            vec![
                Key::Right,
                Key::Right,
                Key::Char('a'),
                Key::Escape,
                Key::Char('/')
            ]
        );
        assert!(Key::program("Nope").is_err());
    }

    /// The switcher is what makes a palette comparable: two themes side by
    /// side in one session beat two builds a minute apart.
    #[test]
    fn the_theme_key_walks_the_palettes_and_comes_back() {
        let proc = std::path::Path::new("/proc");
        let mut app = App::new(tree());
        let first = app.theme;
        let n = crate::theme::THEMES.len();
        for _ in 0..n {
            app.on_key(Key::Char('t'), proc);
        }
        assert_eq!(app.theme, first, "the switcher did not come back round");
        app.on_key(Key::Char('t'), proc);
        assert_eq!(app.theme, (first + 1) % n);
        assert!(
            app.flash.contains(crate::theme::THEMES[app.theme].name),
            "the switch is silent: {:?}",
            app.flash
        );
    }

    /// The screen can go stale through no fault of this program, so it is
    /// written whole now and then. The rule is the one thing about that worth
    /// testing: due once when the span has passed, and not again until the
    /// next one.
    #[test]
    fn a_full_repaint_falls_due_once_a_span_and_not_oftener() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut r = Repaint::new(Duration::from_secs(3));
        assert!(!r.due(t0), "a repaint fell due before anything was drawn");
        assert!(!r.due(t0 + Duration::from_millis(2999)));
        assert!(
            r.due(t0 + Duration::from_secs(3)),
            "the span passed unnoticed"
        );
        // Rearmed from the moment it fell due, not from the start, so a run
        // that was busy does not answer twice in a row.
        assert!(!r.due(t0 + Duration::from_millis(3001)));
        assert!(r.due(t0 + Duration::from_secs(6)));
    }
}
