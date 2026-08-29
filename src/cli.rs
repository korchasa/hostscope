//! Command line. Besides the ordinary options it carries the verification
//! hooks of FR-17: substitution of data sources, dumps of the model and of the
//! frame, a key program, a fixed tick and a log to file.

use std::path::PathBuf;

use crate::app::Key;
use crate::enrich::docker::Source;

pub const USAGE: &str = "\
hostscope - an interactive viewer of the current host state

usage: hostscope [options]

  --tick MS               the interval to start at, in milliseconds
                          (default 3000; - and + move it while running)
  --cgroup-root DIR       read a captured snapshot instead of /sys/fs/cgroup
  --proc-root DIR         read a captured snapshot instead of /proc
  --docker-socket PATH    the docker socket, or 'none' to disable enrichment
  --dump-model json       print the tree model as numbers to stdout and exit
  --dump-frame N          render N frames as text to stdout and exit
  --keys \"Right a Esc\"    run a key program and stop
  --size WxH              frame size for --dump-frame (default 100x30)
  --log FILE              write the log to FILE; never to the terminal
  -h, --help              this text
  -V, --version           version

Dumps go to standard output: FR-10 forbids writing outside the settings file,
and a verification hook is no reason to make an exception.

The tree is the process forest of the host: every row is a process and stands
under the process that started it. What runs a process - a container, a
service, a login session - is read from its cgroup and shown on the row itself,
in the OWNER column, where the filter reaches it.

A chain of processes where each one started only the next, and all of them
belong to the same owner, is drawn as one row named for the whole chain: it
said nothing a level at a time and cost a keystroke a level.

A row whose work is inside a container says so in parentheses after its own
name: a runtime shim belongs to the runtime and its whole work is one level
down. Where the row leads into several containers of one pod, it names the pod
instead, by the first group of its identifier.

keys: up and down move, PageUp and PageDown move by a screenful, Enter goes
down and opens the card where there is nothing below, Backspace comes back up,
i opens the card of any row, / filters by name, by command line and by owner,
c m d n sort, v lays the level out as a flat list of its ends, a switches the
measurement window between the average since start and the last interval, space
freezes the screen, - and + move the refresh interval between a pause, 1, 2, 3,
5, 10, 30 and 60 seconds, q quits. The right arrow also descends, and in the
list view it puts a row among its neighbours. Escape undoes the narrowing
nearest at hand: the card, then the filter, then the level.

The bar beside a column belongs to the sorting: it is drawn next to the value
the rows are ordered by, so the longest bar is always the top row.

CPU is written in busy cores and in nothing else.
";

#[derive(Clone, Debug)]
pub struct Options {
    pub tick_ms: u64,
    pub cgroup_root: PathBuf,
    pub proc_root: PathBuf,
    pub docker: Source,
    pub dump_model: bool,
    pub dump_frame: Option<usize>,
    pub keys: Vec<Key>,
    pub size: (u16, u16),
    pub log: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            tick_ms: crate::app::DEFAULT_INTERVAL_MS,
            cgroup_root: PathBuf::from("/sys/fs/cgroup"),
            proc_root: PathBuf::from("/proc"),
            docker: Source::Socket("/var/run/docker.sock".into()),
            dump_model: false,
            dump_frame: None,
            keys: Vec::new(),
            size: (100, 30),
            log: None,
        }
    }
}

pub enum Parsed {
    Run(Box<Options>),
    Help,
    Version,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    let mut o = Options::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| -> Result<String, String> {
            it.next().ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--tick" => {
                let v = value("--tick")?;
                o.tick_ms = v
                    .parse()
                    .map_err(|_| format!("--tick: not a number: {v}"))?;
                if o.tick_ms == 0 {
                    return Err("--tick must be above zero".into());
                }
            }
            "--cgroup-root" => o.cgroup_root = PathBuf::from(value("--cgroup-root")?),
            "--proc-root" => o.proc_root = PathBuf::from(value("--proc-root")?),
            "--docker-socket" => o.docker = Source::parse(&value("--docker-socket")?),
            "--dump-model" => {
                let v = value("--dump-model")?;
                if v != "json" {
                    return Err(format!("--dump-model: only json is supported, got {v}"));
                }
                o.dump_model = true;
            }
            "--dump-frame" => {
                let v = value("--dump-frame")?;
                o.dump_frame = Some(
                    v.parse()
                        .map_err(|_| format!("--dump-frame: not a number: {v}"))?,
                );
            }
            "--keys" => o.keys = Key::program(&value("--keys")?)?,
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v
                    .split_once('x')
                    .ok_or_else(|| format!("--size: expected WxH, got {v}"))?;
                o.size = (
                    w.parse().map_err(|_| format!("--size: bad width: {w}"))?,
                    h.parse().map_err(|_| format!("--size: bad height: {h}"))?,
                );
            }
            "--log" => o.log = Some(PathBuf::from(value("--log")?)),
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(Parsed::Run(Box::new(o)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(args: &[&str]) -> Options {
        match parse(args.iter().map(|s| s.to_string())).unwrap() {
            Parsed::Run(o) => *o,
            _ => panic!("expected a run"),
        }
    }

    #[test]
    fn reads_the_verification_hooks() {
        let o = opts(&[
            "--cgroup-root",
            "/tmp/snap/sys/fs/cgroup",
            "--proc-root",
            "/tmp/snap/proc",
            "--docker-socket",
            "none",
            "--dump-model",
            "json",
            "--tick",
            "250",
            "--keys",
            "Right a Escape",
            "--log",
            "/tmp/app.log",
        ]);
        assert_eq!(o.cgroup_root, PathBuf::from("/tmp/snap/sys/fs/cgroup"));
        assert_eq!(o.docker, Source::Disabled);
        assert!(o.dump_model);
        assert_eq!(o.tick_ms, 250);
        assert_eq!(o.keys.len(), 3);
        assert_eq!(o.log, Some(PathBuf::from("/tmp/app.log")));
    }

    #[test]
    fn rejects_what_it_does_not_understand() {
        assert!(parse(["--nope".to_string()]).is_err());
        assert!(parse(["--tick".to_string()]).is_err());
        assert!(parse(["--dump-model".to_string(), "yaml".to_string()]).is_err());
    }

    #[test]
    fn defaults_match_the_requirements() {
        let o = opts(&[]);
        assert_eq!(o.tick_ms, 3000);
        assert_eq!(o.cgroup_root, PathBuf::from("/sys/fs/cgroup"));
    }
}
