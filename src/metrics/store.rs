//! The time-series store: current, eased and historical values per metric.

use super::{MetricId, METRICS};
use std::collections::HashMap;

pub const HISTORY_LEN: usize = 300;
/// A metric absent for this many consecutive polls is treated as gone, so a
/// source that stops reporting fades out instead of freezing at its last value.
const STALE_AFTER: u32 = 3;

#[derive(Debug)]
struct Series {
    /// Samples, oldest first, at most `HISTORY_LEN`. A `Vec`, not a
    /// `VecDeque`: `history()` returns one slice, which a wrapped deque cannot
    /// provide.
    history: Vec<f64>,
    current: Option<f64>,
    smoothed: f64,
    smoothed_started: bool,
    missing_streak: u32,
}

impl Series {
    /// Add a sample, dropping the oldest beyond `HISTORY_LEN`.
    fn append(&mut self, v: f64) {
        self.history.push(v);
        if self.history.len() > HISTORY_LEN {
            let excess = self.history.len() - HISTORY_LEN;
            self.history.drain(..excess);
        }
    }

    fn new() -> Self {
        Self {
            history: Vec::with_capacity(HISTORY_LEN + 1),
            current: None,
            smoothed: 0.0,
            smoothed_started: false,
            missing_streak: 0,
        }
    }
}

/// Holds the current value, smoothed value, and rolling history of every metric.
///
/// Smoothing lives here rather than in the widgets so that every consumer gets
/// the same frame-rate-independent easing for free.
pub struct MetricStore {
    series: HashMap<MetricId, Series>,
}

impl MetricStore {
    #[must_use]
    pub fn new() -> Self {
        let mut series = HashMap::new();
        for &id in METRICS {
            series.insert(id, Series::new());
        }
        Self { series }
    }

    /// Record one poll result. `None` means the source could not read it.
    pub fn push(&mut self, id: MetricId, value: Option<f64>) {
        let Some(s) = self.series.get_mut(&id) else { return };

        if let Some(v) = value {
            s.missing_streak = 0;
            s.current = Some(v);
            s.append(v);
        } else {
            s.missing_streak += 1;
            if s.missing_streak >= STALE_AFTER {
                s.current = None;
                // A gap keeps every series on the same time axis.
                s.append(f64::NAN);
            }
        }
    }

    /// Advance easing toward the current value. Call once per frame.
    pub fn advance(&mut self, dt: f32, rate: f32) {
        let k = 1.0 - (-dt.max(0.0) * rate).exp();
        for s in self.series.values_mut() {
            match s.current {
                Some(target) => {
                    if s.smoothed_started {
                        s.smoothed += (target - s.smoothed) * f64::from(k);
                    } else {
                        s.smoothed = target;
                        s.smoothed_started = true;
                    }
                }
                None => {
                    s.smoothed_started = false;
                }
            }
        }
    }

    /// Eased value, or `None` when the metric has never been read successfully.
    /// Widgets must render "N/A" for `None` — never zero.
    #[must_use]
    pub fn smoothed(&self, id: MetricId) -> Option<f64> {
        let s = self.series.get(&id)?;
        if s.current.is_none() || !s.smoothed_started {
            return None;
        }
        Some(s.smoothed)
    }

    /// Raw latest value, or `None` if unavailable.
    #[must_use]
    pub fn current(&self, id: MetricId) -> Option<f64> {
        self.series.get(&id)?.current
    }

    /// Rolling history, oldest first. `NAN` entries mark gaps where the metric
    /// was unavailable.
    #[must_use]
    pub fn history(&self, id: MetricId) -> &[f64] {
        match self.series.get(&id) {
            Some(s) => &s.history,
            None => &[],
        }
    }
}

impl Default for MetricStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::{CPU_TEMP, CPU_USAGE};

    #[test]
    fn history_has_data_after_one_push_and_is_capped() {
        let mut store = MetricStore::new();
        store.push(CPU_USAGE, Some(1.0));
        assert_eq!(store.history(CPU_USAGE), [1.0]);
        for i in 0..HISTORY_LEN + 5 {
            store.push(CPU_USAGE, Some(i as f64));
        }
        let h = store.history(CPU_USAGE);
        assert_eq!(h.len(), HISTORY_LEN);
        assert_eq!(h.last(), Some(&((HISTORY_LEN + 4) as f64)), "newest last");
    }

    #[test]
    fn metric_goes_stale_only_after_repeated_misses() {
        let mut store = MetricStore::new();
        store.push(CPU_TEMP, Some(50.0));
        for _ in 1..STALE_AFTER {
            store.push(CPU_TEMP, None);
            assert_eq!(store.current(CPU_TEMP), Some(50.0), "a brief miss keeps the value");
        }
        store.push(CPU_TEMP, None);
        assert_eq!(store.current(CPU_TEMP), None);
        let h = store.history(CPU_TEMP);
        assert!(h.last().is_some_and(|v| v.is_nan()), "a gap, not a zero");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn smoothing_snaps_first_then_eases() {
        let mut store = MetricStore::new();
        store.push(CPU_USAGE, Some(10.0));
        assert_eq!(store.smoothed(CPU_USAGE), None, "nothing until advanced");
        store.advance(0.016, 9.0);
        assert_eq!(store.smoothed(CPU_USAGE), Some(10.0), "first value is taken as is");
        store.push(CPU_USAGE, Some(90.0));
        store.advance(0.016, 9.0);
        let eased = store.smoothed(CPU_USAGE).expect("value");
        assert!(eased > 10.0 && eased < 90.0, "{eased}");
        store.advance(10.0, 9.0);
        assert!((store.smoothed(CPU_USAGE).expect("value") - 90.0).abs() < 1e-6);
    }
}
