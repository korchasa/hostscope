//! The log goes to a file only, never to the terminal: a line in the terminal
//! ruins the frame (FR-17). The same log carries the collection time and the
//! render time of every frame, because there is no way to measure them from
//! outside, and without them the 50 ms threshold of section 6 is unverifiable.
//!
//! FR-9 applies here as well: nothing that comes from a process environment is
//! ever written.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

pub struct Log {
    file: Option<Mutex<File>>,
    start: Instant,
}

impl Log {
    pub fn none() -> Log {
        Log {
            file: None,
            start: Instant::now(),
        }
    }

    pub fn open(path: &Path) -> std::io::Result<Log> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Log {
            file: Some(Mutex::new(file)),
            start: Instant::now(),
        })
    }

    pub fn line(&self, message: &str) {
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = writeln!(f, "{:9.3} {}", self.start.elapsed().as_secs_f64(), message);
            }
        }
    }

    /// One line per frame: what the collection cost and what the render cost,
    /// with the collection split by source, because the budget of section 6 is
    /// spent in two very different places - the cgroup files and `/proc`.
    pub fn frame(
        &self,
        tick: u64,
        collect_ms: f64,
        render_ms: f64,
        nodes: usize,
        cost: crate::collect::TickCost,
    ) {
        self.line(&format!(
            "frame tick={tick} collect_ms={collect_ms:.2} render_ms={render_ms:.2} nodes={nodes} \
cgroup_ms={:.2} proc_ms={:.2} processes={}",
            cost.cgroup_ms, cost.proc_ms, cost.processes
        ));
    }
}
