//! Enrichment: container data, filled in beside collection.
//!
//! Section 6 requires that a slow socket must not freeze the screen, so this
//! work runs on its own thread and the tree only ever reads the last cached
//! answer. A container whose data has not arrived yet shows its short
//! identifier, exactly as when the socket is unavailable.

pub mod docker;
pub mod json;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::model::ContainerInfo;

#[derive(Clone, Debug, Default)]
pub struct Enrichment {
    pub containers: HashMap<String, ContainerInfo>,
    pub docker_ok: bool,
}

pub struct Enricher {
    data: Arc<Mutex<Enrichment>>,
    stop: Arc<AtomicBool>,
    docker: Arc<docker::Docker>,
}

impl Enricher {
    pub fn new(source: docker::Source) -> Enricher {
        Enricher {
            data: Arc::new(Mutex::new(Enrichment::default())),
            stop: Arc::new(AtomicBool::new(false)),
            docker: Arc::new(docker::Docker::new(source)),
        }
    }

    /// Runs one pass on the calling thread. The dump hooks of FR-17 use this:
    /// two runs over the same snapshot must produce an identical dump, and a
    /// background thread would make that a race.
    pub fn refresh(&self) {
        let next = collect_once(&self.docker, &self.data.lock().unwrap().containers);
        *self.data.lock().unwrap() = next;
    }

    /// Starts the background refresh used by the interactive mode.
    pub fn spawn(&self, period: Duration) {
        let data = Arc::clone(&self.data);
        let stop = Arc::clone(&self.stop);
        let dk = Arc::clone(&self.docker);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let known = data.lock().unwrap().containers.clone();
                let next = collect_once(&dk, &known);
                *data.lock().unwrap() = next;
                let mut slept = Duration::ZERO;
                while slept < period && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    slept += Duration::from_millis(100);
                }
            }
        });
    }

    pub fn snapshot(&self) -> Enrichment {
        self.data.lock().unwrap().clone()
    }

    pub fn docker_enabled(&self) -> bool {
        self.docker.enabled()
    }
}

impl Drop for Enricher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn collect_once(dk: &docker::Docker, known: &HashMap<String, ContainerInfo>) -> Enrichment {
    let mut out = Enrichment::default();
    if let Some(mut list) = dk.list() {
        out.docker_ok = true;
        // The restart count needs a call per container, so it is asked for once
        // and then carried over from the previous pass.
        for (id, info) in list.iter_mut() {
            info.restarts = match known.get(id).and_then(|c| c.restarts) {
                Some(v) => Some(v),
                None => dk.restart_count(id),
            };
        }
        out.containers = list;
    }
    out
}

/// A unix timestamp as `YYYY-MM-DD HH:MM` in UTC. Written here because the only
/// alternative is a dependency, and the binary must stay self-contained.
pub fn stamp(unix_secs: f64) -> String {
    let secs = unix_secs as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Howard Hinnant's days-to-calendar conversion, the standard one.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_timestamp_without_a_calendar_library() {
        assert_eq!(stamp(0.0), "1970-01-01 00:00");
        assert_eq!(stamp(1723635000.0), "2024-08-14 11:30");
    }
}
