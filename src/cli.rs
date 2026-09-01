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
  --dump-style N          the same N frames, each followed by a map of the
                          same shape naming the role of every cell: . plain,
                          c calm, u unusual, a alarm, b bar, s selected,
                          m matched
  --keys \"Right a Esc\"    run a key program and stop
  --size WxH              frame size for --dump-frame and --dump-style
                          (default 100x30)
  --log FILE              write the log to FILE; never to the terminal
  --no-etc-passwd         do not read /etc/passwd; the OWNER column and the
                          card then show the uid instead of the login name
  --theme NAME            the palette to open in: classic, panel, gruvbox,
                          solarized, nord, dracula, tokyo-night, catppuccin
                          (t walks them while running; HOSTSCOPE_THEME sets
                          the one to open in, and --theme out-votes it)
  -h, --help              this text
  -V, --version           version

Dumps go to standard output. FR-10 forbids writing outside the log named on
the command line, and a verification hook is no reason to make an exception.

Besides /proc and /sys/fs/cgroup the application opens one more file for data:
/etc/passwd, once at start, to turn the uid of a login session into the name
the OWNER column shows. It keeps nothing from that file but the number and the
name. --no-etc-passwd leaves it unopened, and the column then shows the number.

The tree is the process forest of the host: every row is a process, and it
stands under the process that started it. What runs a process - a container, a
service, a login session - is read from its cgroup and shown on the row itself,
in the OWNER column. The filter reaches that column as well.

A chain of processes where each one started only the next, all of them with
the same owner, is drawn as one row named for the whole chain. Such a chain
said nothing one level at a time, and cost a keystroke for every level.

A row whose work sits inside a container names that container in parentheses
after its own name: a runtime shim belongs to the container runtime, and the
work is one level below it. A row that leads into several containers of one pod
names the pod instead, by the first group of the pod's identifier.

keys:
  up, down                move
  PageUp, PageDown        move by a screenful
  Enter                   go down; on a row with nothing below, open the card
  Backspace               come back up
  right arrow             also goes down; in the list view it puts a row among
                          its neighbours
  i                       open the card of any row
  /                       filter by name, by command line and by owner
  c m d n                 sort
  v                       lay the level out as a flat list of its ends
  t                       walk the palettes
  a                       switch the measurement window between the average
                          since start and the last interval
  space                   freeze the screen
  - +                     move the refresh interval between a pause, 1, 2, 3,
                          5, 10, 30 and 60 seconds
  Escape                  undo the narrowing nearest at hand: the card, then
                          the filter, then the level
  q                       quit

The bar belongs to the sorted column: it is drawn beside the value the rows
are ordered by, so the longest bar is always on the top row.

CPU is measured in busy cores: 0.5 means half a core is busy, 2.0 means two
cores are.

A figure the machine cannot afford is drawn in another colour, and the row it
sits on is marked in its name column: '!' where something is wrong, '*' where
it is worth a look, and a down arrow where the row is only the way down to the
process that carries it. Every figure is the sum of a subtree, so without that
distinction one busy process would mark every row above it up to the root. The
reading is absolute - a share of what this machine has, or a state the kernel
reports, such as a process left dead or stuck in the kernel. It is never a
comparison against the other rows on screen, so a quiet machine stays quiet.
Disk read and write carry no colour: nothing readable says what the device
underneath can do. The bar keeps comparing the rows of the
level, so a long bar beside a calm figure means large here, not large for this
machine. The card of a marked row says why it is marked: one line per rule that
fired, named after the card row its figure stands on, and naming that figure in
both columns, the whole it was read against and the threshold it crossed.

Eight palettes. 'classic' names the sixteen terminal colours, so the screen
looks the way the reader's own terminal theme draws them. 'panel' fixes the
colours instead - a grey chassis, one orange on the sorted column, and the
selected row as a recessed key. The other six are the terminal schemes their
readers already live in, in their published colours: gruvbox, solarized,
nord, dracula, tokyo-night and catppuccin.
";

#[derive(Clone, Debug)]
pub struct Options {
    pub tick_ms: u64,
    pub cgroup_root: PathBuf,
    pub proc_root: PathBuf,
    pub docker: Source,
    pub dump_model: bool,
    pub dump_frame: Option<usize>,
    /// Print the role of every cell after each frame. Colour is the one thing
    /// drawn text cannot carry, so the reading that decides it needs a channel
    /// of its own before anything can check it (D-42).
    pub dump_style: bool,
    pub keys: Vec<Key>,
    pub size: (u16, u16),
    pub log: Option<PathBuf>,
    /// Whether the uid of a login session is turned into a login name, which
    /// takes reading `/etc/passwd` - the one file for data outside `/proc` and
    /// `/sys`. On by default, because the kernel offers no other source for the
    /// name the `OWNER` column is required to carry (D-26, D-41).
    pub etc_passwd: bool,
    /// The palette to open in, when the command line names one. `None` leaves
    /// the choice to `HOSTSCOPE_THEME` and then to the first theme: a flag
    /// that is not given must not out-vote the environment.
    pub theme: Option<usize>,
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
            dump_style: false,
            keys: Vec::new(),
            size: (100, 30),
            log: None,
            theme: None,
            etc_passwd: true,
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
            "--dump-style" => {
                let v = value("--dump-style")?;
                o.dump_frame = Some(
                    v.parse()
                        .map_err(|_| format!("--dump-style: not a number: {v}"))?,
                );
                o.dump_style = true;
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
            "--no-etc-passwd" => o.etc_passwd = false,
            "--theme" => {
                let v = value("--theme")?;
                o.theme = Some(crate::theme::index_of(&v).ok_or_else(|| {
                    format!(
                        "--theme: no such theme: {v} (have {})",
                        crate::theme::names()
                    )
                })?);
            }
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
            "--theme",
            "panel",
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
    fn the_user_table_is_read_unless_the_command_line_says_otherwise() {
        // The name in the `OWNER` column has no other source, so the file is
        // read by default; the flag is for a host where the one file this tool
        // opens outside `/proc` and `/sys` is one file too many (D-41).
        assert!(opts(&[]).etc_passwd);
        assert!(!opts(&["--no-etc-passwd"]).etc_passwd);
    }

    #[test]
    fn defaults_match_the_requirements() {
        let o = opts(&[]);
        assert_eq!(o.tick_ms, 3000);
        assert_eq!(o.cgroup_root, PathBuf::from("/sys/fs/cgroup"));
    }

    #[test]
    fn the_theme_is_taken_by_name_and_an_unknown_one_is_refused() {
        let ok = parse(["--theme".to_string(), "panel".to_string()]).unwrap();
        match ok {
            Parsed::Run(o) => assert_eq!(o.theme, crate::theme::index_of("panel")),
            _ => panic!("--theme did not produce a run"),
        }
        // Nothing named on the command line leaves the choice to the
        // environment, which the command line has no business reading.
        match parse(Vec::<String>::new()).unwrap() {
            Parsed::Run(o) => assert_eq!(o.theme, None),
            _ => panic!("an empty command line did not produce a run"),
        }
        let bad = parse(["--theme".to_string(), "no-such".to_string()]);
        assert!(bad.is_err(), "an unknown theme was accepted");
    }
}
