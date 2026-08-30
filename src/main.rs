//! hostscope - an interactive viewer of the current host state.
//!
//! Read only (FR-10): no external command is ever run, nothing outside the log
//! named on the command line is ever written, and no action changes the state
//! of the host. The build refuses to produce a binary that breaks the first of
//! those (see build.rs).

mod app;
mod cli;
mod collect;
mod dump;
mod enrich;
mod logging;
mod model;
mod render;
mod sample;
mod theme;
mod util;

use std::io::Write;
use std::time::{Duration, Instant};

use app::{App, Key};
use cli::{Options, Parsed};
use collect::Collector;
use enrich::Enricher;
use logging::Log;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(args) {
        Ok(Parsed::Help) => emit(cli::USAGE),
        Ok(Parsed::Version) => emit(&format!("hostscope {}\n", env!("CARGO_PKG_VERSION"))),
        Ok(Parsed::Run(options)) => {
            if let Err(err) = run(*options) {
                eprintln!("hostscope: {err}");
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("hostscope: {err}");
            eprintln!("try: hostscope --help");
            std::process::exit(2);
        }
    }
}

/// Standard output can go away while we are still writing to it: `hostscope
/// --dump-model json | head` closes the pipe after twenty lines. The print
/// macros answer that with a panic, and a panic on a closed pipe is noise on
/// the screen of anyone who pipes the dump anywhere, so the write is done by
/// hand and a closed pipe ends the run quietly.
fn emit(text: &str) {
    let mut out = std::io::stdout();
    if out.write_all(text.as_bytes()).is_err() || out.flush().is_err() {
        std::process::exit(0);
    }
}

fn run(options: Options) -> Result<(), String> {
    let log = match &options.log {
        Some(path) => {
            Log::open(path).map_err(|e| format!("cannot open the log {}: {e}", path.display()))?
        }
        None => Log::none(),
    };
    log.line(&format!(
        "start version={} tick_ms={} cgroup_root={} proc_root={}",
        env!("CARGO_PKG_VERSION"),
        options.tick_ms,
        options.cgroup_root.display(),
        options.proc_root.display()
    ));

    let enricher = Enricher::new(options.docker.clone());
    let mut collector = Collector::new(
        options.cgroup_root.clone(),
        options.proc_root.clone(),
        now_secs(),
        options.etc_passwd,
    );

    // The command line out-votes the environment, and the environment
    // out-votes the first theme. A flag that was not given must not out-vote
    // anything (theme.rs).
    let theme = options.theme.or_else(theme::from_env).unwrap_or(0);

    if options.dump_model {
        // The enrichment runs on this thread: two runs over one snapshot must
        // produce an identical dump, and a background thread would race.
        enricher.refresh();
        let ticks = options.dump_frame.unwrap_or(1).max(1);
        let mut snapshot = None;
        for i in 0..ticks {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(options.tick_ms));
            }
            snapshot =
                Some(collector.tick(now_secs(), &enricher.snapshot(), enricher.docker_enabled()));
        }
        emit(&dump::model_json(&snapshot.unwrap(), options.tick_ms));
        return Ok(());
    }

    if let Some(count) = options.dump_frame {
        enricher.refresh();
        let (w, h) = options.size;
        let mut app =
            App::new(collector.tick(now_secs(), &enricher.snapshot(), enricher.docker_enabled()));
        app.theme = theme;
        app.table_rows = render::table_rows(h);
        // The dumped frame says how often it would be renewed, and a dump runs
        // at the tick it was given.
        app.set_interval_ms(options.tick_ms);
        let frames = count.max(if options.keys.is_empty() {
            1
        } else {
            options.keys.len() + 1
        });
        for i in 0..frames {
            if i > 0 {
                if let Some(key) = options.keys.get(i - 1) {
                    app.on_key(*key, collector.proc_root().to_path_buf().as_path());
                }
                std::thread::sleep(Duration::from_millis(options.tick_ms));
                let snap =
                    collector.tick(now_secs(), &enricher.snapshot(), enricher.docker_enabled());
                app.update(snap);
            }
            let mut text = String::new();
            let lines = render::frame(&app, w, h);
            for line in render::to_text(&lines) {
                text.push_str(&line);
                text.push('\n');
            }
            // The map follows its frame with no blank line between them: one
            // unit of the dump is the frame and the map of the same shape, and
            // whoever reads it splits the unit in half (D-42).
            if options.dump_style {
                for line in render::to_roles(&lines) {
                    text.push_str(&line);
                    text.push('\n');
                }
            }
            text.push('\n');
            emit(&text);
        }
        return Ok(());
    }

    enricher.spawn(Duration::from_millis(3000));
    interactive(options, collector, enricher, log)
}

fn interactive(
    options: Options,
    mut collector: Collector,
    enricher: Enricher,
    log: Log,
) -> Result<(), String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::backend::CrosstermBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    enable_raw_mode().map_err(|e| format!("cannot switch the terminal to raw mode: {e}"))?;
    let mut out = std::io::stdout();
    crossterm::execute!(out, EnterAlternateScreen, crossterm::cursor::Hide)
        .map_err(|e| format!("cannot switch to the alternate screen: {e}"))?;
    let backend = CrosstermBackend::new(out);
    let mut terminal =
        Terminal::new(backend).map_err(|e| format!("cannot start the terminal: {e}"))?;

    let started = Instant::now();
    let mut app =
        App::new(collector.tick(now_secs(), &enricher.snapshot(), enricher.docker_enabled()));
    app.theme = options.theme.or_else(theme::from_env).unwrap_or(0);
    // `--tick` sets the interval the application opens at; from there `-` and
    // `+` move it, so the tick is a starting point rather than a fixed rate.
    app.set_interval_ms(options.tick_ms);
    let mut next_tick = Instant::now() + Duration::from_millis(app.tick_ms());
    let mut program = options.keys.clone().into_iter();
    let mut ticks: u64 = 0;
    let mut result = Ok(());
    // Every three seconds the screen is written whole instead of as a
    // difference against the last frame. What the difference is drawn against
    // is not always what the terminal holds - `sudo` relays the output through
    // a pseudo-terminal of its own - and a lost byte then stays on screen until
    // every cell is written again (D-38).
    let mut repaint = app::Repaint::new(Duration::from_secs(3));

    loop {
        if repaint.due(Instant::now()) {
            // `clear` resets the back buffer as well, so the draw below writes
            // every cell rather than a difference against a buffer that says
            // the screen already holds them.
            if let Err(e) = terminal.clear() {
                result = Err(format!("cannot clear the screen: {e}"));
                break;
            }
        }
        let render_start = Instant::now();
        let draw = terminal.draw(|f| {
            let area = f.area();
            app.table_rows = render::table_rows(area.height);
            let lines = render::frame(&app, area.width, area.height);
            f.render_widget(Paragraph::new(lines), area);
        });
        if let Err(e) = draw {
            result = Err(format!("cannot draw the frame: {e}"));
            break;
        }
        let render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
        log.frame(
            ticks,
            0.0,
            render_ms,
            count_nodes(&app.snapshot.root),
            collector.cost(),
        );
        if ticks == 0 {
            log.line(&format!(
                "first frame after {:.1} ms",
                started.elapsed().as_secs_f64() * 1000.0
            ));
        }

        if app.quit {
            break;
        }

        // The interval is a key away, so the wait is read from the state on
        // every pass rather than fixed at the start. Shortening it moves the
        // next tick nearer at once: the alternative is a minute of waiting
        // after a key that asked for a second.
        let tick = Duration::from_millis(app.tick_ms());
        if next_tick > Instant::now() + tick {
            next_tick = Instant::now() + tick;
        }
        let timeout = next_tick.saturating_duration_since(Instant::now());
        match event::poll(timeout) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c'))
                    {
                        break;
                    }
                    if let Some(k) = translate(key.code) {
                        app.on_key(k, collector.proc_root().to_path_buf().as_path());
                    }
                }
                Ok(Event::Resize(_, _)) => {}
                Ok(_) => {}
                Err(e) => {
                    result = Err(format!("cannot read the keyboard: {e}"));
                    break;
                }
            },
            Ok(false) => {}
            Err(e) => {
                result = Err(format!("cannot wait for input: {e}"));
                break;
            }
        }

        if Instant::now() >= next_tick {
            next_tick += tick;
            if next_tick < Instant::now() {
                next_tick = Instant::now() + tick;
            }
            let collect_start = Instant::now();
            let snap = collector.tick(now_secs(), &enricher.snapshot(), enricher.docker_enabled());
            let collect_ms = collect_start.elapsed().as_secs_f64() * 1000.0;
            let nodes = count_nodes(&snap.root);
            app.update(snap);
            ticks += 1;
            log.frame(ticks, collect_ms, 0.0, nodes, collector.cost());
            // A key program runs one key per tick, so a scenario is a single
            // command and every step is a settled frame (FR-17).
            if !options.keys.is_empty() {
                match program.next() {
                    Some(k) => app.on_key(k, collector.proc_root().to_path_buf().as_path()),
                    None => app.quit = true,
                }
            }
        }
    }

    let _ = disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
    let _ = terminal.show_cursor();
    log.line("stop");
    result
}

fn translate(code: crossterm::event::KeyCode) -> Option<Key> {
    use crossterm::event::KeyCode;
    Some(match code {
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Right => Key::Right,
        KeyCode::Left => Key::Left,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Char(' ') => Key::Space,
        KeyCode::Char(c) => Key::Char(c),
        _ => return None,
    })
}

fn count_nodes(node: &model::Node) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
