//! Drawing one frame. The function is pure: state in, lines out. That is what
//! makes `--dump-frame` show exactly what the terminal shows, and what lets the
//! frame invariants of the testing document run without a terminal at all.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, Row, View};
use crate::model::{Kind, Metrics, Mode, Node, OwnerKind, Sort};
use crate::util::{
    bar, bytes_str, char_width, cores_opt, dur_str, fit, fit_left, mem_str, or_na, pad, pad_left,
    pair_rate_opt, sparkline, str_width, uptime_str,
};

fn frame_style() -> Style {
    Style::default().fg(Color::Gray)
}
fn dim() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM)
}
fn teal() -> Style {
    Style::default().fg(Color::Cyan)
}
fn amber() -> Style {
    Style::default().fg(Color::Yellow)
}
fn red() -> Style {
    Style::default().fg(Color::Red)
}
fn plain() -> Style {
    Style::default()
}
/// The selected row is marked with a background rather than with `REVERSED`.
/// Reversing swaps the colour of the bar with the colour of the line, and the
/// filled blocks then read as a hole instead of a bar: on the one row an
/// engineer is looking at, the bar disappeared.
fn selected() -> Style {
    Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

/// The column plan. The set and the order never change with the level (tag N5);
/// what changes with a narrow terminal is how many of them fit.
///
/// The bar is not a column of its own but a strip drawn beside one of them -
/// the column the table is ordered by. A bar always beside `CORES` compared the
/// rows by a value the order on screen did not follow, so under `m` the longest
/// bar could sit three rows below the largest number (D-27).
#[derive(Clone, Copy, Debug)]
pub struct Cols {
    pub name: usize,
    pub kind: usize,
    pub tasks: usize,
    pub cpu: usize,
    pub bar: usize,
    pub mem: usize,
    /// Drawn only where the host has moved something out of RAM. On a machine
    /// whose swap device is untouched the column is a column of zeros, and the
    /// cells are worth more to the name (D-35).
    pub swap: bool,
    pub disk: bool,
    pub net: bool,
    /// The column the bar is drawn beside, which is the one the rows are
    /// ordered by.
    pub bar_at: Sort,
}

impl Cols {
    /// Thirteen cells for the `OWNER` column: twelve for a short container
    /// identifier and the space after it, because an identifier cut short is
    /// one that cannot be pasted anywhere.
    pub fn plan(usable: usize, sort: Sort, swap: bool) -> Cols {
        let kind = 13usize;
        let mut bar = 10usize;
        let mut swap = swap;
        let mut disk = true;
        let mut net = true;
        // 27 cells of separators and fixed columns: the leading space, the two
        // spaces in front of every value column, `TASKS`, `CORES` and `MEM`.
        // The bar takes its width plus the space that sets it off from the
        // number it belongs to.
        let fixed = |bar: usize, swap: bool, disk: bool, net: bool| {
            27 + kind
                + if bar > 0 { bar + 1 } else { 0 }
                + if swap { 9 } else { 0 }
                + if disk { 12 } else { 0 }
                + if net { 13 } else { 0 }
        };
        let mut name = usable as isize - fixed(bar, swap, disk, net) as isize;
        if name < 20 {
            bar = 6;
            name = usable as isize - fixed(bar, swap, disk, net) as isize;
        }
        // The swap column goes first of all, before the network: the line above
        // the table already says how much swap the machine is using, so what
        // this column adds is which row is in it - and on a terminal this
        // narrow the names are what the reader has left to go on.
        if name < 16 {
            swap = false;
            name = usable as isize - fixed(bar, swap, disk, net) as isize;
        }
        if name < 16 {
            net = false;
            name = usable as isize - fixed(bar, swap, disk, net) as isize;
        }
        if name < 14 {
            disk = false;
            name = usable as isize - fixed(bar, swap, disk, net) as isize;
        }
        if name < 12 {
            bar = 0;
            name = usable as isize - fixed(bar, swap, disk, net) as isize;
        }
        // A bar belongs to a column, so a column the width has already dropped
        // cannot carry one: the cells go back to the name instead, where on a
        // terminal this narrow they are worth more.
        if (sort == Sort::Disk && !disk) || (sort == Sort::Net && !net) {
            bar = 0;
            name = usable as isize - fixed(bar, swap, disk, net) as isize;
        }
        Cols {
            name: name.max(6) as usize,
            kind,
            tasks: 6,
            cpu: 8,
            bar,
            mem: 7,
            swap,
            disk,
            net,
            bar_at: sort,
        }
    }

    /// The cells the bar takes beside `column`, separator included, and nothing
    /// where the bar does not belong.
    fn bar_slot(&self, column: Sort) -> usize {
        if self.bar > 0 && self.bar_at == column {
            self.bar + 1
        } else {
            0
        }
    }
}

/// The whole frame: every line exactly as wide as the frame, in cells. Below
/// 24 by 12 the frame is drawn at 24 by 12 rather than smaller, so a terminal
/// under that size gets a frame wider and taller than it asked for.
pub fn frame(app: &App, width: u16, height: u16) -> Vec<Line<'static>> {
    let w = width.max(24) as usize;
    let h = height.max(12) as usize;
    let u = w - 2;
    let mut out: Vec<Line<'static>> = Vec::with_capacity(h);

    let snap = app.view();
    out.push(rule_title(
        &format!(
            "hostscope {}  {}  Linux {}  up {}",
            env!("CARGO_PKG_VERSION"),
            snap.host.hostname,
            snap.host.kernel,
            uptime_str(snap.host.uptime_secs)
        ),
        u,
    ));
    // The rows of the level are assembled once and read four times: the path
    // line counts them, the table draws them, and the two lines under it speak
    // about the selected one. Assembling them is a walk of the subtree and a
    // sort, and in the list view that is the whole tree.
    let rows = app.rows();
    out.push(summary_cpu(app, u));
    out.push(summary_net(app, u));
    out.push(rule('\u{251c}', '\u{2524}', u));
    out.push(path_line(app, &rows, u));
    out.push(rule('\u{251c}', '\u{2524}', u));

    let content = h - 10;
    if app.card.is_some() {
        card_lines(app, u, content, &mut out);
    } else {
        table_lines(app, &rows, u, content, &mut out);
    }

    out.push(rule('\u{251c}', '\u{2524}', u));
    out.push(hint_line(app, &rows, u));
    out.push(key_line(app, &rows, u));
    out.push(rule('\u{2514}', '\u{2518}', u));
    out
}

/// The number of table rows a frame of this height offers. The application
/// tells its state so scrolling and the frame agree.
pub fn table_rows(height: u16) -> usize {
    (height.max(12) as usize).saturating_sub(11)
}

pub fn to_text(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn clip(spans: Vec<Span<'static>>, u: usize) -> Line<'static> {
    let mut all: Vec<Span<'static>> = vec![Span::styled("\u{2502}", frame_style())];
    let mut used = 0usize;
    for s in spans {
        let len = str_width(&s.content);
        if used + len <= u {
            used += len;
            all.push(s);
        } else if used < u {
            // The span is cut to the cells that are left, never to characters:
            // a wide character that only half fits is dropped whole.
            let room = u - used;
            let text = pad(&s.content, room);
            used = u;
            all.push(Span::styled(text, s.style));
        }
    }
    if used < u {
        all.push(Span::raw(" ".repeat(u - used)));
    }
    all.push(Span::styled("\u{2502}", frame_style()));
    Line::from(all)
}

fn rule(left: char, right: char, u: usize) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("{left}{}{right}", "\u{2500}".repeat(u)),
        frame_style(),
    )])
}

fn rule_title(text: &str, u: usize) -> Line<'static> {
    let mut title = format!(" {} ", text);
    if str_width(&title) > u.saturating_sub(1) {
        title = fit(&title, u.saturating_sub(1));
    }
    let rest = u.saturating_sub(str_width(&title) + 1);
    Line::from(vec![
        Span::styled("\u{250c}\u{2500}", frame_style()),
        Span::styled(title, teal()),
        Span::styled(
            format!("{}\u{2510}", "\u{2500}".repeat(rest)),
            frame_style(),
        ),
    ])
}

fn summary_cpu(app: &App, u: usize) -> Line<'static> {
    let snap = app.view();
    let host = &snap.host;
    let busy = match app.mode {
        Mode::Instant => host.busy_cores,
        Mode::Average => host.busy_cores_avg,
    };
    let mem_used = match app.mode {
        Mode::Instant => host.mem_used,
        Mode::Average => host.mem_used_avg,
    };
    let pct = if host.cores > 0.0 {
        busy / host.cores * 100.0
    } else {
        0.0
    };
    let mem_frac = if host.mem_total > 0.0 {
        mem_used / host.mem_total
    } else {
        0.0
    };
    clip(
        vec![
            Span::styled("  CPU ", dim()),
            Span::styled(
                format!(
                    "{} of {} cores ",
                    pad_left(&format!("{:.2}", busy), 5),
                    host.cores as u64
                ),
                plain(),
            ),
            Span::styled(format!("{:.1}% ", pct), dim()),
            Span::styled(sparkline(&host.history, 12), teal()),
            Span::styled("  MEM ", dim()),
            Span::styled(format!("{} ", mem_str(mem_used)), plain()),
            Span::styled(bar(mem_frac, 8), teal()),
            Span::styled(format!(" {:.0}%", mem_frac * 100.0), dim()),
        ],
        u,
    )
}

fn summary_net(app: &App, u: usize) -> Line<'static> {
    let snap = app.view();
    let host = &snap.host;
    let (rx, tx) = match app.mode {
        Mode::Instant => (host.net_rx, host.net_tx),
        Mode::Average => (host.net_rx_avg, host.net_tx_avg),
    };
    let swap_used = match app.mode {
        Mode::Instant => host.swap_used,
        Mode::Average => host.swap_used_avg,
    };
    let swap_frac = if host.swap_total > 0.0 {
        swap_used / host.swap_total
    } else {
        0.0
    };
    // The window every number on screen is taken over (FR-13). It sits in the
    // right corner so the two modes cannot be confused.
    let window = if app.paused() {
        "PAUSED".to_string()
    } else {
        match app.mode {
            Mode::Instant => format!("INSTANT {:.1}s", snap.interval.max(0.0)),
            Mode::Average => format!("AVG over {}", dur_str(snap.window)),
        }
    };
    let style = if app.paused() || app.mode == Mode::Average {
        amber()
    } else {
        dim()
    };
    let label = format!("{} ", window);
    let reserve = str_width(&label);
    // On a narrow terminal the left half gives way segment by segment, and the
    // window label stays: a number without the window it was taken over cannot
    // be read at all.
    let segments: Vec<Vec<Span<'static>>> = vec![
        vec![
            Span::styled("  NET ", dim()),
            Span::styled(
                format!(
                    "\u{2193} {}/s \u{2191} {}/s   ",
                    bytes_str(rx),
                    bytes_str(tx)
                ),
                plain(),
            ),
        ],
        vec![
            Span::styled("SWAP ", dim()),
            Span::styled(format!("{} ", mem_str(swap_used)), plain()),
            Span::styled(bar(swap_frac, 6), amber()),
            Span::styled(format!(" {:.0}%   ", swap_frac * 100.0), dim()),
        ],
        vec![
            Span::styled("LOAD ", dim()),
            Span::styled(
                format!(
                    "{:.2} {:.2} {:.2}",
                    host.load[0], host.load[1], host.load[2]
                ),
                plain(),
            ),
        ],
    ];
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for segment in segments {
        let len: usize = segment.iter().map(|s| str_width(&s.content)).sum();
        if used + len + reserve <= u {
            used += len;
            spans.extend(segment);
        }
    }
    if used + reserve < u {
        spans.push(Span::raw(" ".repeat(u - used - reserve)));
    }
    spans.push(Span::styled(label, style));
    clip(spans, u)
}

fn path_line(app: &App, rows: &[Row], u: usize) -> Line<'static> {
    // The view belongs next to the level: in the list the rows come from
    // several levels at once, so the sum of the rows no longer equals the
    // parent, and the line has to say which of the two is on screen.
    let right = format!(
        "L{}  view: {}  sort: {} ",
        app.level(),
        app.layout.label(),
        app.sort.label()
    );
    // A filter is state, like the level and the sorting, so it stands with
    // them (D-29): while it is on, every row of the table is there because of it, and
    // a table narrowed by a filter nobody can see reads as a host with almost
    // nothing on it. The count says how much of the level is left.
    let filter = if app.filter.is_empty() {
        String::new()
    } else {
        format!("filter: {} ({})  ", app.filter, rows.len())
    };
    let room = u.saturating_sub(str_width(&right) + str_width(&filter) + 1);
    let crumbs = fit_left(&app.crumbs(), room);
    clip(
        vec![
            Span::styled(format!(" {crumbs}"), teal()),
            Span::styled(filter, amber()),
            Span::styled(right, dim()),
        ],
        u,
    )
}

/// The header, with the column the rows are ordered by named in the colour the
/// key line gives the sorting keys: the bar beside it says which rows are large,
/// and the heading says which value that is.
fn header_line(cols: &Cols, u: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut plain_text = String::from(" ");
    plain_text.push_str(&pad("NAME", cols.name + 1));
    if cols.kind > 0 {
        plain_text.push_str(&pad("OWNER", cols.kind));
    }
    spans.push(Span::styled(plain_text, dim()));
    let column = |title: &str, width: usize, sort: Sort, spans: &mut Vec<Span<'static>>| {
        spans.push(Span::styled("  ".to_string(), dim()));
        spans.push(Span::styled(
            pad_left(title, width),
            if cols.bar_at == sort { amber() } else { dim() },
        ));
        let slot = cols.bar_slot(sort);
        if slot > 0 {
            spans.push(Span::styled(" ".repeat(slot), dim()));
        }
    };
    // `TASKS` is the one value column nothing sorts by, so it never carries a
    // bar and is written before the loop.
    spans.push(Span::styled(pad_left("TASKS", cols.tasks), dim()));
    column("CORES", cols.cpu, Sort::Cpu, &mut spans);
    column("MEM", cols.mem, Sort::Mem, &mut spans);
    if cols.swap {
        // Nothing sorts by it, so like `TASKS` it never carries a bar.
        spans.push(Span::styled("  ".to_string(), dim()));
        spans.push(Span::styled(pad_left("SWAP", 7), dim()));
    }
    if cols.disk {
        column("DISK R/W", 10, Sort::Disk, &mut spans);
    }
    if cols.net {
        column("NET \u{2193}/\u{2191}", 11, Sort::Net, &mut spans);
    }
    clip(spans, u)
}

fn table_lines(app: &App, rows: &[Row], u: usize, content: usize, out: &mut Vec<Line<'static>>) {
    // The host row carries the machine's own swap, so it is also the answer to
    // whether this host has swapped anything at all (D-35).
    let cols = Cols::plan(u, app.sort, app.view().root.instant.swap.is_some());
    out.push(header_line(&cols, u));

    // The bar compares the rows by the value they are ordered by, so the
    // longest bar is always the top row of the level.
    let max = rows
        .iter()
        .map(|r| app.sort.key(r.node.metrics(app.mode)))
        .fold(0.0f64, f64::max);
    let needle = app.filter.to_lowercase();

    let visible = content.saturating_sub(1);
    for i in 0..visible {
        let idx = app.scroll + i;
        match rows.get(idx) {
            None => out.push(clip(vec![], u)),
            Some(r) => {
                let m = r.node.metrics(app.mode);
                let (lead, name) = row_name(r, &cols);
                // The row is drawn in one of three ways: selected, the dimmed
                // `(self)` remainder (FR-14), or an ordinary node. The bar
                // keeps its own colour in all three - on the selected row it
                // only takes the background of the selection.
                let (base, pale, bar_base) = if idx == app.cursor {
                    (
                        selected(),
                        Style::default().fg(Color::Gray).bg(Color::DarkGray),
                        selected(),
                    )
                } else if r.node.kind == Kind::Own {
                    (dim(), dim(), Style::default())
                } else {
                    (plain(), dim(), Style::default())
                };
                let frac = if max > 0.0 {
                    app.sort.key(m) / max
                } else {
                    0.0
                };
                let bar_st = bar_style(app.sort, m, frac).patch(bar_base);
                let mut spans: Vec<Span<'static>> = Vec::new();
                // The filter is marked where it matched, on the three cells of
                // a row that can carry a match: the path the list view puts in
                // front of a name, the name itself, and the owner beside it.
                // The fourth place the filter looks - the command line - is
                // under the table, on the hint line, and is marked there.
                spans.extend(marked(lead, &needle, pale));
                spans.extend(marked(name, &needle, base));
                spans.extend(marked(row_owner(&r.node, &cols), &needle, base));
                spans.push(Span::styled(
                    pad_left(&or_na(m.tasks, |t| format!("{t:.0}")), cols.tasks),
                    base,
                ));
                let value = |text: String, sort: Sort, spans: &mut Vec<Span<'static>>| {
                    spans.push(Span::styled(text, base));
                    if cols.bar_slot(sort) > 0 {
                        spans.push(Span::styled(" ".to_string(), base));
                        spans.push(Span::styled(bar(frac, cols.bar), bar_st));
                    }
                };
                value(
                    format!("  {}", pad_left(&cores_opt(m.cpu), cols.cpu)),
                    Sort::Cpu,
                    &mut spans,
                );
                value(
                    format!("  {}", pad_left(&or_na(m.mem, mem_str), cols.mem)),
                    Sort::Mem,
                    &mut spans,
                );
                if cols.swap {
                    spans.push(Span::styled(
                        format!("  {}", pad_left(&or_na(m.swap, mem_str), 7)),
                        base,
                    ));
                }
                if cols.disk {
                    value(
                        format!("  {}", pad_left(&pair_rate_opt(m.rd, m.wr), 10)),
                        Sort::Disk,
                        &mut spans,
                    );
                }
                if cols.net {
                    value(
                        format!("  {}", pad_left(&pair_rate_opt(m.rx, m.tx), 11)),
                        Sort::Net,
                        &mut spans,
                    );
                }
                // The line is padded to the full width so the selection covers
                // the whole of it rather than stopping at the last column.
                let used: usize = spans.iter().map(|s| str_width(&s.content)).sum();
                spans.push(Span::styled(" ".repeat(u.saturating_sub(used)), base));
                out.push(clip(spans, u));
            }
        }
    }
}

/// The drawn text with every occurrence of the filter marked. A row is on
/// screen because something in it matched, and without this the reader has to
/// find that something by eye - which is the whole of the complaint the mark
/// answers.
///
/// The search walks characters rather than bytes: a case-folded copy would
/// change the length of the text in some scripts, and every offset here is used
/// to cut a string that has already been measured in cells.
fn marked(text: String, needle: &str, base: Style) -> Vec<Span<'static>> {
    if needle.is_empty() {
        return vec![Span::styled(text, base)];
    }
    let want: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    let chars: Vec<char> = text.chars().collect();
    let folded: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    if want.is_empty() || want.len() > folded.len() {
        return vec![Span::styled(text, base)];
    }
    let hit = base.patch(Style::default().fg(Color::Black).bg(Color::Yellow));
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut at = 0usize;
    let mut cut = 0usize;
    while at + want.len() <= folded.len() {
        if folded[at..at + want.len()] == want[..] {
            if at > cut {
                spans.push(Span::styled(
                    chars[cut..at].iter().collect::<String>(),
                    base,
                ));
            }
            spans.push(Span::styled(
                chars[at..at + want.len()].iter().collect::<String>(),
                hit,
            ));
            at += want.len();
            cut = at;
        } else {
            at += 1;
        }
    }
    if spans.is_empty() {
        return vec![Span::styled(text, base)];
    }
    if cut < chars.len() {
        spans.push(Span::styled(chars[cut..].iter().collect::<String>(), base));
    }
    spans
}

/// `n/a` is not a rate, so it never gets a per-second suffix.
fn per_second(value: String) -> String {
    if value == "n/a" {
        value
    } else {
        format!("{value}/s")
    }
}

/// The colour of the bar. Beside `CORES` it is read off the value itself,
/// because a busy core is a fixed amount of host whatever the other rows are
/// doing: above one core the row is holding a core of its own. The other three
/// have no such threshold - a megabyte is large or small only next to the rest
/// of the level - so there the colour follows the share of the largest row.
fn bar_style(sort: Sort, m: &Metrics, frac: f64) -> Style {
    if sort == Sort::Cpu {
        let cores = m.cpu_or_zero();
        if cores > 1.0 {
            red()
        } else if cores > 0.1 {
            amber()
        } else {
            teal()
        }
    } else if frac > 0.66 {
        red()
    } else if frac > 0.33 {
        amber()
    } else {
        teal()
    }
}

/// The name column, split where its colour changes: the mark with the path of
/// the row, then the name itself. In the tree view the path is empty and only
/// the mark is drawn.
///
/// A node with children is marked, a node without children is not: the reader
/// must see where drilling makes sense before pressing a key. When the path
/// does not fit, it is cut from its front and the name is kept whole - the row
/// is read by its name, and the path only says where it came from.
fn row_name(row: &Row, cols: &Cols) -> (String, String) {
    let mark = if row.children > 0 { "> " } else { "  " };
    let room = cols.name.saturating_sub(2);
    // What the row leads into is drawn as part of its name, so it is truncated
    // with the name, the filter marks it like any other match, and the width
    // arithmetic below counts it (D-30).
    let name = match &row.node.detail.leads_into {
        Some(container) => format!("{} ({})", row.node.name, container),
        None => row.node.name.clone(),
    };
    let name_width = str_width(&name);
    let path = if row.prefix.is_empty() {
        String::new()
    } else if name_width + str_width(&row.prefix) <= room {
        row.prefix.clone()
    } else {
        // A path that is there is never cut away to nothing, however narrow
        // the column: two cells still say "there is more above me", and a
        // remainder row cut down to a bare `(self)` would be indistinguishable
        // from the one the level itself carries.
        fit_left(&row.prefix, room.saturating_sub(name_width).clamp(2, room))
    };
    let used = 2 + str_width(&path);
    (format!(" {mark}{path}"), fit(&name, cols.name - used))
}

/// The `OWNER` cell with the space that separates it from the name. The column
/// is blank where there is no owner to name: the `(self)` remainder belongs to
/// the node above it, and repeating that node's owner on it would read as a
/// second process of the same owner. The host and the processes nothing on the
/// host runs are the other blank case.
fn row_owner(r: &Node, cols: &Cols) -> String {
    match &r.detail.owner {
        Some(o) if !o.name.is_empty() => {
            format!(" {}", pad(&fit(&o.name, cols.kind - 1), cols.kind))
        }
        _ => " ".repeat(cols.kind + 1),
    }
}

fn hint_line(app: &App, rows: &[Row], u: usize) -> Line<'static> {
    let snap = app.view();
    let text = if let Some(card) = app.card_node() {
        if card.has_children() {
            format!(
                "Esc closes the card   Enter descends into {} children",
                card.children.len()
            )
        } else {
            "Esc closes the card".to_string()
        }
    } else {
        match rows.get(app.cursor) {
            None => {
                if app.filter.is_empty() {
                    "nothing to show on this level".to_string()
                } else {
                    "no rows match the filter".to_string()
                }
            }
            Some(sel) => hint_for(sel, snap.docker_available),
        }
    };
    // The hint carries the command line of the selected row, which is one of
    // the three places the filter looks (FR-6) and the only one of them that is
    // not a column. A match in it is marked here, or the row is on screen for a
    // reason nothing on the screen shows.
    clip(
        marked(
            format!(" {}", fit(&text, u - 1)),
            &app.filter.to_lowercase(),
            dim(),
        ),
        u,
    )
}

fn hint_for(row: &Row, docker_available: bool) -> String {
    let sel = &row.node;
    match sel.kind {
        Kind::Own => {
            "the process's own usage: what it does itself, not counting the work of its children"
                .into()
        }
        Kind::Process => {
            let line = sel
                .detail
                .cmdline
                .clone()
                .unwrap_or_else(|| format!("pid {}", sel.detail.pid.unwrap_or(0)));
            match &sel.detail.owner {
                Some(o) if o.kind == OwnerKind::Container => {
                    format!("{}   {}", owner_hint(sel, docker_available), line)
                }
                _ => line,
            }
        }
        Kind::Host => descend_hint(row),
    }
}

/// What the container a process belongs to is, in the words the daemon gave.
fn owner_hint(sel: &Node, docker_available: bool) -> String {
    match &sel.detail.container {
        Some(info) => format!("image {}   {}", info.image, {
            if info.status.is_empty() {
                info.state.clone()
            } else {
                info.status.clone()
            }
        }),
        None => {
            if docker_available {
                "container data has not arrived from the socket yet".into()
            } else {
                "container data unavailable - the docker socket is not readable".into()
            }
        }
    }
}

/// What runs this process, on the card. A container gets everything the
/// daemon gave; a service and a user get their name, which is all there is.
fn owner_lines(
    node: &Node,
    kv: &dyn Fn(&str, String, &mut Vec<Line<'static>>),
    note: &dyn Fn(String, &mut Vec<Line<'static>>),
    lines: &mut Vec<Line<'static>>,
) {
    let owner = match &node.detail.owner {
        Some(o) => o,
        None => return,
    };
    match owner.kind {
        OwnerKind::Container => {
            kv(
                "container",
                format!(
                    "{}   id {}",
                    owner.name,
                    node.detail.short_id.clone().unwrap_or_else(|| "n/a".into())
                ),
                lines,
            );
            match &node.detail.container {
                Some(info) => {
                    kv(
                        "image",
                        format!(
                            "{}   {}   restarts {}",
                            info.image,
                            if info.status.is_empty() {
                                info.state.clone()
                            } else {
                                info.status.clone()
                            },
                            info.restarts
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "n/a".into())
                        ),
                        lines,
                    );
                    if !info.created.is_empty() {
                        kv("created", info.created.clone(), lines);
                    }
                    if !info.ports.is_empty() {
                        kv("ports", info.ports.join("  "), lines);
                    }
                    if !info.labels.is_empty() {
                        let labels: Vec<String> = info
                            .labels
                            .iter()
                            .take(4)
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect();
                        kv("labels", labels.join("  "), lines);
                    }
                }
                None => kv(
                    "image",
                    "unavailable - the docker socket is not readable".to_string(),
                    lines,
                ),
            }
        }
        OwnerKind::Service => kv("service", format!("{}.service", owner.name), lines),
        OwnerKind::User => kv(
            "session",
            format!("the login session of {}", owner.name),
            lines,
        ),
        OwnerKind::Kernel => note(
            "a kernel thread: no program on disk, no cgroup of its own".into(),
            lines,
        ),
        OwnerKind::System => {}
    }
    if let Some(path) = &node.detail.cgroup_path {
        // Whole, across as many lines as it takes: a path with its tail cut
        // off names no unit `systemctl` would answer about (D-33).
        kv("cgroup", path.clone(), lines);
    }
}

/// The links of a glued chain, broken into lines no wider than `room`. A link
/// wider than the whole room is cut - a name has to end somewhere - but a link
/// is never lost, because losing one is what makes the card name a chain after
/// a pid that did not do the work (D-25).
///
/// Below about six cells of room the cut takes the pid with it. That is a
/// terminal of twenty-six columns or narrower, where the table has already
/// dropped three of its columns, and there is nothing to be done about it short
/// of not drawing the card at all.
/// A value laid out under its label: as many lines as it needs, broken between
/// words (D-33). Before this a value wider than the card was cut with an
/// ellipsis, which takes the end off a command line - and the end is the part
/// that says which configuration file a process was started with.
///
/// The spacing inside a line is kept as it was given. Several spaces are how
/// this card separates a pid from the command it belongs to, and a wrapper that
/// squeezes them to one merges two columns into one word.
///
/// A word wider than the room is broken where the room ends. A path and an
/// identifier carry no spaces, so a wrapper that only breaks between words
/// would leave them cut after all.
fn wrap(text: &str, room: usize) -> Vec<String> {
    let room = room.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut width = 0usize;
    // Where the word being written started, so a break puts the whole of it on
    // the next line rather than in two halves.
    let mut word_at: Option<usize> = None;
    let mut prev_space = true;
    for c in text.chars() {
        let cw = char_width(c);
        if width + cw > room {
            match word_at {
                Some(at) if at > 0 => {
                    let rest = line.split_off(at);
                    out.push(line.trim_end().to_string());
                    line = rest;
                }
                _ => out.push(std::mem::take(&mut line)),
            }
            width = str_width(&line);
            word_at = if line.is_empty() { None } else { Some(0) };
        }
        if c != ' ' && prev_space {
            word_at = Some(line.len());
        }
        line.push(c);
        width += cw;
        prev_space = c == ' ';
    }
    if !line.trim().is_empty() {
        out.push(line.trim_end().to_string());
    }
    out
}

fn chain_lines(glued: &[(i32, String)], room: usize) -> Vec<String> {
    let arrow = "  \u{2192}  ";
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for (pid, name) in glued {
        let text = format!("{pid} {name}");
        // `fit` pads to the width it is given, which would make every link as
        // wide as the room and put one of them on each line.
        let link = if str_width(&text) > room {
            fit(&text, room).trim_end().to_string()
        } else {
            text
        };
        let joined = if line.is_empty() {
            link.clone()
        } else {
            format!("{line}{arrow}{link}")
        };
        if !line.is_empty() && str_width(&joined) > room {
            out.push(std::mem::replace(&mut line, link));
        } else {
            line = joined;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn descend_hint(row: &Row) -> String {
    if row.children > 0 {
        format!("children: {}   Enter to descend", row.children)
    } else {
        "no children - Enter opens the card instead".to_string()
    }
}

/// How many rows the filter left, in the words the two filter lines use.
fn match_count(rows: &[Row]) -> String {
    match rows.len() {
        0 => "no rows".to_string(),
        1 => "1 row".to_string(),
        n => format!("{n} rows"),
    }
}

fn key_line(app: &App, rows: &[Row], u: usize) -> Line<'static> {
    // While the filter is being typed the line is the filter: what has been
    // typed, how many rows are left of the level, and the two keys that end the
    // typing - one keeping the filter, one dropping it.
    if app.filtering {
        return clip(
            vec![
                Span::styled(" filter: ", dim()),
                Span::styled(format!("{}\u{2588}", app.filter), amber()),
                Span::styled(format!("   {}   ", match_count(rows)), dim()),
                Span::styled("Enter", plain()),
                Span::styled(" keeps it ", dim()),
                Span::styled("Esc", plain()),
                Span::styled(" drops it", dim()),
            ],
            u,
        );
    }
    if !app.flash.is_empty() {
        return clip(vec![Span::styled(format!(" {}", app.flash), amber())], u);
    }
    let avg = app.mode == Mode::Average;
    let mode_style = if avg { amber() } else { plain() };
    let mode_dim = if avg { amber() } else { dim() };
    let pause_style = if app.paused() { amber() } else { plain() };
    let pause_dim = if app.paused() { amber() } else { dim() };
    let list = app.layout == View::List;
    let view_style = if list { amber() } else { plain() };
    let view_dim = if list { amber() } else { dim() };
    // What the filter is stands on the path line, with the level and the
    // sorting; what is left for the keys is the way out of it. While a filter is
    // on, that key takes the place of the one that starts a filter: `/` still
    // works, but the reader who wants the whole level back needs `Esc` and the
    // line is already as long as a hundred columns hold.
    let filtered = !app.filter.is_empty();
    let (find_key, find_word) = if filtered {
        ("Esc", " clears ")
    } else {
        ("/", " find ")
    };
    let find_style = if filtered { amber() } else { plain() };
    let find_dim = if filtered { amber() } else { dim() };
    let mut spans: Vec<Span<'static>> = Vec::new();
    // One space between a key and the next, and one word per key: they have to
    // fit a terminal of a hundred columns.
    spans.extend([
        Span::styled(" \u{2191}\u{2193} ", plain()),
        Span::styled("Enter", plain()),
        Span::styled(" down ", dim()),
        Span::styled("Bksp", plain()),
        Span::styled(" up ", dim()),
        Span::styled("i", plain()),
        Span::styled(" info ", dim()),
        Span::styled(find_key, find_style),
        Span::styled(find_word, find_dim),
        Span::styled("c m d n", plain()),
        Span::styled(" sort ", dim()),
        Span::styled("v", view_style),
        Span::styled(" list ", view_dim),
        Span::styled("a", mode_style),
        Span::styled(" avg ", mode_dim),
        Span::styled("space", pause_style),
        Span::styled(" pause ", pause_dim),
        // The two keys that move the refresh interval, with the interval they
        // move written between them: the value belongs where the keys that
        // change it are, or the reader has to look for it elsewhere.
        Span::styled("- +", plain()),
        Span::styled(" ".to_string(), dim()),
        Span::styled(app.interval_label(), pause_style),
        Span::styled(" ".to_string(), dim()),
        Span::styled("q", plain()),
        Span::styled(" quit", dim()),
    ]);
    clip(spans, u)
}

/// The card of the selected row. It prints only what this node actually has: a
/// cgroup slice has no connections and no open files, and filling the screen
/// with them is not allowed (section 11).
fn card_lines(app: &App, u: usize, content: usize, out: &mut Vec<Line<'static>>) {
    let node = match app.card_node() {
        Some(n) => n,
        None => {
            for _ in 0..content {
                out.push(clip(vec![], u));
            }
            return;
        }
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    let window = dur_str(app.view().window);

    // The label is written once and the value continues under it: a card that
    // cut the tail off a value lost the end of a command line (D-33).
    let kv = |k: &str, v: String, lines: &mut Vec<Line<'static>>| {
        let parts = wrap(&v, u.saturating_sub(20));
        let parts = if parts.is_empty() {
            vec![String::new()]
        } else {
            parts
        };
        for (i, part) in parts.into_iter().enumerate() {
            lines.push(clip(
                vec![
                    Span::styled(format!("  {}", pad(if i == 0 { k } else { "" }, 16)), dim()),
                    Span::styled(part, plain()),
                ],
                u,
            ));
        }
    };
    // An explanation is text, and on a narrow terminal it wraps like any other
    // text on this card: cut against the border it read as "shared pages
    // divid", and the reader had nothing to do with that (D-33).
    let note = |t: String, lines: &mut Vec<Line<'static>>| {
        for part in wrap(&t, u.saturating_sub(4)) {
            lines.push(clip(vec![Span::styled(format!("  {part}"), dim())], u));
        }
    };
    // A figure stands in a column of its own, and the column is the same on
    // every row: the reader runs the eye down it instead of searching each
    // line for the word `avg` (D-32). A value wider than its column pushes the
    // next one rather than being cut - a figure is never shortened in silence.
    let fig = |k: &str, now: String, avg: String, tail: &str, lines: &mut Vec<Line<'static>>| {
        // The three columns take fifty cells; what the terminal leaves after
        // them is what the explanation beside the figures has to live in.
        let room = u.saturating_sub(52);
        let inline = !tail.is_empty() && str_width(tail) <= room;
        lines.push(clip(
            vec![
                Span::styled(format!("  {}", pad(k, 16)), dim()),
                Span::styled(pad(&now, 16), plain()),
                Span::styled(pad(&avg, 16), plain()),
                Span::styled(
                    if inline {
                        tail.to_string()
                    } else {
                        String::new()
                    },
                    dim(),
                ),
            ],
            u,
        ));
        // Too narrow for the explanation to stand beside the figures: it goes
        // under them, aligned with the value column, and wraps there (D-33).
        if !tail.is_empty() && !inline {
            for part in wrap(tail, u.saturating_sub(20)) {
                lines.push(clip(
                    vec![
                        Span::styled(" ".repeat(18), dim()),
                        Span::styled(part, dim()),
                    ],
                    u,
                ));
            }
        }
    };

    match node.kind {
        Kind::Process => {
            // Identity is read by label like everything else on the card: the
            // reader after a pid finds it on the left edge rather than inside
            // the value of another label (D-33).
            kv("process", node.name.clone(), &mut lines);
            kv("pid", node.detail.pid.unwrap_or(0).to_string(), &mut lines);
            kv(
                "parent",
                node.detail.ppid.unwrap_or(0).to_string(),
                &mut lines,
            );
            kv(
                "user",
                node.detail.user.clone().unwrap_or_else(|| "n/a".into()),
                &mut lines,
            );
            kv(
                "threads",
                node.detail.threads.unwrap_or(0).to_string(),
                &mut lines,
            );
            if let Some(started) = &node.detail.started {
                kv("started", started.clone(), &mut lines);
            }
            if !node.detail.glued.is_empty() {
                // The row is one process wide in its figures and several
                // processes wide in its name, so the card names every link of
                // the chain with its own pid before it prints a command line,
                // and the two are never confused (D-25).
                //
                // Every link, not as many as happen to fit: the chain D-25 was
                // decided over is seven links and 113 cells, so a single line
                // cut to the width of a hundred-column terminal drops three of
                // them while the line below still names the last one as the
                // owner of the command. It wraps instead, and the label is
                // written once so the continuation reads as one value.
                for (i, part) in chain_lines(&node.detail.glued, u.saturating_sub(20))
                    .into_iter()
                    .enumerate()
                {
                    kv(if i == 0 { "chain" } else { "" }, part, &mut lines);
                }
            }
            if let Some(cmd) = &node.detail.cmdline {
                // The command wraps like any other value, but only three times.
                // A command of five hundred characters is six lines on a narrow
                // terminal, and the card is cut from its tail: one value would
                // push the figures off the screen (D-33). The cut is marked,
                // like every other cut on the screen (section 11).
                let room = u.saturating_sub(20);
                let text = match node.detail.glued.last() {
                    // The pid goes into the value, not into the label: the label
                    // column is sixteen cells, and a pid put there loses a digit
                    // past five of them and names a different process on a host
                    // with the ordinary `pid_max` of 4194304.
                    Some((pid, _)) => format!("{pid}   {cmd}"),
                    None => cmd.clone(),
                };
                let label = if node.detail.glued.is_empty() {
                    "command"
                } else {
                    "command of"
                };
                let mut parts = wrap(&text, room);
                // A pid is never shown in halves: four digits of it name a
                // different process (D-25). Where the room cannot hold the pid
                // whole, the value goes on one line and the cut is marked, the
                // way it was before this card wrapped anything.
                if let Some((pid, _)) = node.detail.glued.last() {
                    let head = pid.to_string();
                    if !parts.first().is_some_and(|p| p.starts_with(&head)) {
                        parts = vec![fit(&text, room)];
                    }
                }
                if parts.len() > 3 {
                    parts.truncate(3);
                    let last = parts.pop().unwrap_or_default();
                    parts.push(fit(&format!("{last} "), room).trim_end().to_string());
                }
                for (i, part) in parts.into_iter().enumerate() {
                    kv(if i == 0 { label } else { "" }, part, &mut lines);
                }
            }
            owner_lines(&node, &kv, &note, &mut lines);
        }
        Kind::Host => {
            kv(
                "host",
                format!(
                    "{}   {} processes at the root of the forest",
                    app.view().host.hostname,
                    node.children.len()
                ),
                &mut lines,
            );
            note(
                "CPU and memory here are what the machine reports for itself, not a sum".into(),
                &mut lines,
            );
        }
        Kind::Own => {
            // In the list view the remainder can belong to a node several
            // levels down, so the card names that node, not the level in view.
            let parent = match app.card.as_ref() {
                Some(row) => app.own_parent(row),
                None => app.current(),
            };
            kv(
                "(self)",
                format!("own usage of {}", parent.name),
                &mut lines,
            );
            kv(
                "computed as",
                format!(
                    "the counter of {} minus the sum of its {} children",
                    parent.name, node.detail.child_count
                ),
                &mut lines,
            );
            note(
                "page cache charged to the cgroup itself, plus children that started and ended"
                    .into(),
                &mut lines,
            );
            note(
                "while hostscope was running - their work stays in the parent counter".into(),
                &mut lines,
            );
        }
    }

    lines.push(clip(vec![], u));
    // Both modes stand side by side rather than in turn: the gap between them
    // is the diagnosis (section 11). The heading names them once, above the
    // columns they stand in, instead of a word beside every figure (D-32).
    lines.push(clip(
        vec![Span::styled(
            format!("  {}{}avg over {window}", pad("", 16), pad("now", 16)),
            dim(),
        )],
        u,
    ));
    fig(
        "cpu",
        format!("{} cores", cores_opt(node.instant.cpu)),
        format!("{} cores", cores_opt(node.avg.cpu)),
        "",
        &mut lines,
    );
    // RSS is the whole subtree (FR-5), while the virtual size and the PSS come
    // from this process alone. Each is a row with the scope in its label: on
    // one line the reader compared numbers that do not compare (D-32).
    fig(
        if node.kind == Kind::Process {
            "memory RSS"
        } else {
            "memory.current"
        },
        or_na(node.instant.mem, mem_str),
        or_na(node.avg.mem, mem_str),
        // What the number counts, not only whose it is: RSS counts a shared
        // page in full for every process that maps it, which is why the PSS
        // two rows below is smaller and why a column of RSS values sums to
        // more memory than the host has.
        if node.kind == Kind::Process && !node.children.is_empty() {
            "with children; shared pages counted in full"
        } else if node.kind == Kind::Process {
            "resident pages, shared ones counted in full"
        } else {
            ""
        },
        &mut lines,
    );
    if node.kind == Kind::Process {
        // The label carries the scope, so the line beside it does not repeat it:
        // `own` is what the (self) row means everywhere else in the interface.
        fig(
            "own virtual",
            or_na(node.detail.vsz, mem_str),
            String::new(),
            "address space mapped, not memory held",
            &mut lines,
        );
        if let Some(pss) = app.card_extras.as_ref().and_then(|e| e.pss) {
            fig(
                "own PSS",
                mem_str(pss),
                String::new(),
                "shared pages divided among those that map them",
                &mut lines,
            );
        }
        // Always, zero included: the reader opened the card to find out whether
        // this process is in swap, and a row that disappears at zero reads as a
        // figure that could not be read.
        fig(
            "own swap",
            or_na(app.card_extras.as_ref().and_then(|e| e.swap), mem_str),
            String::new(),
            "pages moved out of RAM to the swap device",
            &mut lines,
        );
    }
    fig(
        "disk r/w",
        per_second(pair_rate_opt(node.instant.rd, node.instant.wr)),
        per_second(pair_rate_opt(node.avg.rd, node.avg.wr)),
        &node
            .detail
            .io_total
            .map(|(r, w)| format!("{} / {} since start", bytes_str(r), bytes_str(w)))
            .unwrap_or_default(),
        &mut lines,
    );
    fig(
        "net \u{2193}/\u{2191}",
        per_second(pair_rate_opt(node.instant.rx, node.instant.tx)),
        per_second(pair_rate_opt(node.avg.rx, node.avg.tx)),
        if node.detail.own_netns {
            "own netns"
        } else if app.view().ebpf {
            "host netns, split by eBPF"
        } else if node.kind == Kind::Process {
            "attributed to the namespace, not to the process"
        } else {
            "host netns, not attributable without eBPF"
        },
        &mut lines,
    );

    if node.kind == Kind::Process {
        if let Some(extras) = &app.card_extras {
            lines.push(clip(vec![], u));
            match (extras.files, extras.sockets) {
                (Some(f), Some(s)) => {
                    kv("files", f.to_string(), &mut lines);
                    kv("sockets", s.to_string(), &mut lines);
                }
                _ => kv("files", "n/a without root".to_string(), &mut lines),
            }
            for (name, value) in &extras.limits {
                kv(name, value.clone(), &mut lines);
            }
            kv(
                "connections",
                extras
                    .conns
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "none".to_string()),
                &mut lines,
            );
            for extra in extras.conns.iter().skip(1).take(2) {
                kv("", extra.clone(), &mut lines);
            }
            if !extras.restricted.is_empty() {
                kv(
                    "unavailable",
                    format!("{} - needs root", extras.restricted.join(", ")),
                    &mut lines,
                );
            }
        }
    }
    if !node.detail.restricted.is_empty() {
        kv(
            "unavailable",
            format!("{} - needs root", node.detail.restricted.join(", ")),
            &mut lines,
        );
    }

    // A card that does not fit says how much of it is missing. Cutting in
    // silence is what turned a long chain into a card with no figures on it at
    // all: the reader saw a full screen and no sign that seven lines had gone
    // (D-25). The count goes on the last line, so what is cut is only ever the
    // tail of what was going to be drawn anyway.
    if lines.len() > content {
        let hidden = lines.len() - content + 1;
        lines.truncate(content.saturating_sub(1));
        lines.push(clip(
            vec![Span::styled(
                format!(
                    "  {}",
                    fit(
                        &format!("\u{2026} {hidden} more lines - a taller terminal shows them"),
                        u.saturating_sub(2)
                    )
                ),
                amber(),
            )],
            u,
        ));
    }
    while lines.len() < content {
        lines.push(clip(vec![], u));
    }
    out.extend(lines);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::model::Snapshot;

    #[test]
    fn every_line_is_exactly_the_terminal_width() {
        for (w, h) in [(100u16, 30u16), (60, 20), (200, 60), (80, 24)] {
            let app = App::new(Snapshot::empty());
            let lines = frame(&app, w, h);
            assert_eq!(lines.len(), h as usize, "height at {w}x{h}");
            for (i, line) in to_text(&lines).iter().enumerate() {
                assert_eq!(str_width(line), w as usize, "line {i} at {w}x{h}: {line:?}");
            }
        }
    }

    /// The bar of the selected row is the one thing no linter over drawn text
    /// can judge: the characters were there all along, and the reverse
    /// attribute swapped their colour with the colour of the line (D-20).
    #[test]
    fn the_selected_row_keeps_the_colour_of_its_bar() {
        use crate::model::{Kind, Metrics, Node};
        let mut snap = Snapshot::empty();
        snap.host.cores = 4.0;
        let mut a = Node::new("p:1", "alpha", Kind::Process);
        a.instant = Metrics {
            cpu: Some(2.0),
            mem: Some(100.0),
            ..Metrics::default()
        };
        a.avg = a.instant;
        snap.root.children.push(a);
        let app = App::new(snap);
        let lines = frame(&app, 100, 30);
        let row = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.style.bg == Some(Color::DarkGray)))
            .expect("the selected row is drawn with a background");
        let bar = row
            .spans
            .iter()
            .find(|s| s.content.contains('\u{2588}'))
            .expect("the bar is drawn on the selected row");
        assert_eq!(
            bar.style.bg,
            Some(Color::DarkGray),
            "the bar left the selection"
        );
        assert!(
            matches!(bar.style.fg, Some(Color::Cyan | Color::Yellow | Color::Red)),
            "the bar lost its own colour: {:?}",
            bar.style
        );
    }

    /// The one promise `chain_lines` makes: a link may be cut, never dropped,
    /// and no line it returns is wider than the room it was given. The card
    /// tests exercise it at two widths with short names; these are the edges
    /// those widths never reach.
    #[test]
    fn the_chain_cuts_a_link_but_never_loses_one() {
        let links = |n: usize| -> Vec<(i32, String)> {
            (0..n)
                .map(|i| (201 + i as i32, format!("link{i:02}")))
                .collect()
        };
        assert!(chain_lines(&[], 40).is_empty(), "nothing in, nothing out");
        assert_eq!(chain_lines(&links(1), 40), vec!["201 link00".to_string()]);
        // A link exactly as wide as the room stays whole and stands alone.
        assert_eq!(
            chain_lines(&links(3), 10),
            vec!["201 link00", "202 link01", "203 link02"]
        );
        // A link wider than the room is cut with an ellipsis - and still there.
        let long = vec![(7, "a".repeat(80)), (8, "b".to_string())];
        let out = chain_lines(&long, 10);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(
            out[0].starts_with("7 aaa") && out[0].ends_with('\u{2026}'),
            "{out:?}"
        );
        assert_eq!(out[1], "8 b");
        // Whatever the room, the links come out as many as they went in, each
        // on some line, and no line is wider than the room. Below six cells a
        // link is cut so short that its pid goes with it, which is why the pid
        // itself is only demanded from there up.
        for room in 2..40usize {
            let out = chain_lines(&links(9), room);
            let parts: usize = out.iter().map(|l| l.split("\u{2192}").count()).sum();
            assert_eq!(
                parts, 9,
                "room {room}: {parts} links came out of 9: {out:?}"
            );
            for line in &out {
                assert!(str_width(line) <= room, "room {room}: {line:?} is too wide");
            }
            if room >= 6 {
                for (pid, _) in links(9) {
                    let seen = out.iter().filter(|l| l.contains(&pid.to_string())).count();
                    assert_eq!(
                        seen, 1,
                        "room {room}: pid {pid} appears {seen} times: {out:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_column_plan_always_fits_the_width() {
        for u in 30..250usize {
            for sort in [Sort::Cpu, Sort::Mem, Sort::Disk, Sort::Net] {
                for swap in [false, true] {
                    let c = Cols::plan(u, sort, swap);
                    let total = 27
                        + c.kind
                        + if c.bar > 0 { c.bar + 1 } else { 0 }
                        + c.name
                        + if c.swap { 9 } else { 0 }
                        + if c.disk { 12 } else { 0 }
                        + if c.net { 13 } else { 0 };
                    assert!(
                        total <= u || u < 60,
                        "plan for {u} under {sort:?} takes {total}"
                    );
                    // A column the host cannot fill is never planned for.
                    assert!(swap || !c.swap, "a swap column with no swap at {u}");
                    // A bar is only ever drawn beside a column that is drawn.
                    let column = match sort {
                        Sort::Disk => c.disk,
                        Sort::Net => c.net,
                        _ => true,
                    };
                    assert!(
                        column || c.bar == 0,
                        "the bar for {sort:?} survived the column at {u}"
                    );
                }
            }
        }
    }

    /// The header names the columns and the rows fill them, so the two are one
    /// arithmetic written twice - and the frame is padded to the terminal, which
    /// hides a row that came out short. This is the check that the bar moving
    /// from column to column does not take a cell with it.
    #[test]
    fn every_column_starts_where_the_header_says_under_every_sorting() {
        use crate::model::{Kind, Metrics, Node};
        let mut snap = Snapshot::empty();
        let mut a = Node::new("p:1", "alpha", Kind::Process);
        a.instant = Metrics {
            cpu: Some(2.0),
            mem: Some(100.0),
            rd: Some(10.0),
            wr: Some(20.0),
            rx: Some(30.0),
            tx: Some(40.0),
            swap: Some(50.0),
            tasks: Some(3.0),
        };
        a.avg = a.instant;
        snap.root.children.push(a);
        let mut app = App::new(snap);
        for (key, sort) in [
            ('c', Sort::Cpu),
            ('m', Sort::Mem),
            ('d', Sort::Disk),
            ('n', Sort::Net),
        ] {
            app.on_key(crate::app::Key::Char(key), std::path::Path::new("/proc"));
            assert_eq!(app.sort, sort);
            for w in [60u16, 80, 100, 200] {
                let text = to_text(&frame(&app, w, 30));
                let head = &text[6];
                let row = &text[7];
                // The value of each column ends where its heading ends: both are
                // right-aligned in the same cells.
                let head_chars: Vec<char> = head.chars().collect();
                for column in ["CORES", "MEM"] {
                    // Counted in characters: the frame border is one character
                    // of three bytes, so a byte offset lands two cells early.
                    let want: Vec<char> = column.chars().collect();
                    let start = (0..head_chars.len() - want.len())
                        .find(|&i| head_chars[i..i + want.len()] == want[..])
                        .unwrap();
                    let at = start + want.len();
                    // A space, or the frame itself where the column is the last
                    // one a narrow terminal kept.
                    let after = row.chars().nth(at);
                    assert!(
                        matches!(after, Some(' ') | Some('\u{2502}')),
                        "{sort:?} at {w}: the row runs past {column}: {row:?}\n{head:?}"
                    );
                }
            }
        }
    }

    /// The spacing inside a value is part of what it says: three spaces are how
    /// the card separates a pid from the command it belongs to. A wrapper that
    /// squeezes a run of spaces to one glues the two into a single word.
    #[test]
    fn wrapping_keeps_the_spacing_it_was_given() {
        assert_eq!(
            wrap("999999   node server.js", 50),
            vec!["999999   node server.js"]
        );
        assert_eq!(
            wrap("999999   node server.js", 12),
            vec!["999999", "node", "server.js"]
        );
        // A word with no space in it is broken where the room ends: a path
        // would otherwise be cut after all.
        assert_eq!(wrap("/very/long/path", 5), vec!["/very", "/long", "/path"]);
    }
}
