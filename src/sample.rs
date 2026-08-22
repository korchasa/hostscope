//! Turning kernel counters into the two windows of FR-13.
//!
//! Cumulative counters (`cpu.stat`, `io.stat`, `/proc/<pid>/io`, interface
//! bytes) are sampled at start and from then on shown only as deltas (FR-15):
//! the instant value is the difference over the last interval, the average is
//! the difference over the window since the application started. Nothing that
//! accumulated before the start ever reaches the output.

use std::collections::HashMap;

/// A value in both windows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rates {
    pub instant: f64,
    pub average: f64,
}

#[derive(Clone, Copy, Debug)]
struct Counter {
    base: f64,
    prev: f64,
    gen: u64,
}

#[derive(Clone, Copy, Debug)]
struct Gauge {
    sum: f64,
    gen: u64,
}

pub struct Sampler {
    counters: HashMap<String, Counter>,
    gauges: HashMap<String, Gauge>,
    gen: u64,
    ticks: u64,
    t0: f64,
    prev_t: f64,
    now: f64,
}

impl Sampler {
    pub fn new(now: f64) -> Sampler {
        Sampler {
            counters: HashMap::new(),
            gauges: HashMap::new(),
            gen: 0,
            ticks: 0,
            t0: now,
            prev_t: now,
            now,
        }
    }

    /// Opens a tick. Everything sampled afterwards belongs to it.
    pub fn begin(&mut self, now: f64) {
        self.gen += 1;
        self.ticks += 1;
        self.prev_t = self.now;
        self.now = now;
    }

    /// Seconds covered by the last interval; never zero, so a rate is always
    /// defined.
    pub fn interval(&self) -> f64 {
        let dt = self.now - self.prev_t;
        if dt > 1e-6 {
            dt
        } else {
            0.0
        }
    }

    /// Seconds since the application started - the window average values are
    /// taken over.
    pub fn window(&self) -> f64 {
        (self.now - self.t0).max(0.0)
    }

    /// How many ticks have been collected since the start.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// A cumulative counter as a rate per second in both windows. The first
    /// sample of a key yields zero in both, which is exactly the FR-15
    /// acceptance.
    pub fn counter(&mut self, key: &str, value: f64) -> Rates {
        let gen = self.gen;
        let dt = self.now - self.prev_t;
        let window = self.window();
        let entry = self.counters.entry(key.to_string()).or_insert(Counter {
            base: value,
            prev: value,
            gen,
        });
        // A counter that went backwards means the node was recreated under the
        // same name: start over rather than print a negative rate.
        if value < entry.prev {
            entry.base = value;
            entry.prev = value;
        }
        let instant = if dt > 1e-6 && entry.gen != gen {
            (value - entry.prev) / dt
        } else {
            0.0
        };
        let average = if window > 1e-6 {
            (value - entry.base) / window
        } else {
            0.0
        };
        entry.prev = value;
        entry.gen = gen;
        Rates {
            instant: instant.max(0.0),
            average: average.max(0.0),
        }
    }

    /// A gauge (`memory.current`, task count) describes the current state, so
    /// it is taken as it is; the average is the mean over the samples of the
    /// window, counting the ticks the node did not exist for as zero.
    pub fn gauge(&mut self, key: &str, value: f64) -> Rates {
        let gen = self.gen;
        let ticks = self.ticks;
        let entry = self
            .gauges
            .entry(key.to_string())
            .or_insert(Gauge { sum: 0.0, gen: 0 });
        if entry.gen != gen {
            entry.sum += value;
            entry.gen = gen;
        }
        let average = if ticks > 0 {
            entry.sum / ticks as f64
        } else {
            value
        };
        Rates {
            instant: value,
            average,
        }
    }

    /// Drops the state of nodes that have not been seen for a while, so a host
    /// that churns through short-lived scopes does not grow the map for ever.
    pub fn sweep(&mut self) {
        let gen = self.gen;
        const KEEP: u64 = 30;
        self.counters
            .retain(|_, c| gen.saturating_sub(c.gen) < KEEP);
        self.gauges.retain(|_, g| gen.saturating_sub(g.gen) < KEEP);
    }

    /// The total a counter accumulated since the application started, used by
    /// the process card ("18 MB / 4 MB since start").
    pub fn total_since_start(&self, key: &str, value: f64) -> f64 {
        match self.counters.get(key) {
            Some(c) => (value - c.base).max(0.0),
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_of_a_counter_is_zero_in_both_windows() {
        let mut s = Sampler::new(0.0);
        s.begin(0.0);
        let r = s.counter("a", 1_000_000.0);
        assert_eq!(r.instant, 0.0);
        assert_eq!(r.average, 0.0);
    }

    #[test]
    fn counter_shows_only_the_delta_after_start() {
        let mut s = Sampler::new(0.0);
        s.begin(0.0);
        s.counter("a", 1_000_000.0);
        s.begin(1.0);
        let r = s.counter("a", 1_000_010.0);
        assert!((r.instant - 10.0).abs() < 1e-9);
        assert!((r.average - 10.0).abs() < 1e-9);
        s.begin(2.0);
        let r = s.counter("a", 1_000_010.0);
        assert_eq!(r.instant, 0.0);
        assert!((r.average - 5.0).abs() < 1e-9);
    }

    #[test]
    fn counter_reset_does_not_produce_a_negative_rate() {
        let mut s = Sampler::new(0.0);
        s.begin(0.0);
        s.counter("a", 500.0);
        s.begin(1.0);
        let r = s.counter("a", 10.0);
        assert_eq!(r.instant, 0.0);
        assert_eq!(r.average, 0.0);
    }

    #[test]
    fn gauge_average_counts_ticks_the_node_was_absent_for() {
        let mut s = Sampler::new(0.0);
        s.begin(0.0);
        s.gauge("m", 0.0);
        s.begin(1.0);
        let r = s.gauge("m", 100.0);
        assert_eq!(r.instant, 100.0);
        assert!((r.average - 50.0).abs() < 1e-9);
    }
}
