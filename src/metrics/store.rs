//! The time-series store: current, eased and historical values per metric.

use super::{MetricId, METRICS, RATE_TOTALS};
use std::collections::HashMap;

/// Samples behind gauges, bars, tiles and sparklines: [`MetricStore::history`].
pub const HISTORY_LEN: usize = 300;
/// Most samples [`MetricStore::set_capacity`] keeps per metric: one hour at the
/// fastest refresh interval (250 ms): at most about 230 KB per metric,
/// counting the trimming slack.
pub const MAX_CAPACITY: usize = 14_400;
/// A metric absent for this many consecutive polls is treated as gone, so a
/// source that stops reporting fades out instead of freezing at its last value.
const STALE_AFTER: u32 = 3;

#[derive(Debug)]
struct Series {
    /// Samples, oldest first. A `Vec`, not a `VecDeque`: `history()` returns
    /// one slice, which a wrapped deque cannot provide. Holds up to twice the
    /// store's capacity, so trimming the oldest is an occasional bulk move
    /// rather than shifting thousands of samples on every poll.
    history: Vec<f64>,
    /// Samples ever appended, so a chart can group samples into buckets that
    /// stay put as new samples arrive.
    appended: u64,
    current: Option<f64>,
    smoothed: f64,
    smoothed_started: bool,
    missing_streak: u32,
}

impl Series {
    /// Add a sample, dropping the oldest beyond `capacity` (in bulk, once
    /// the excess reaches `capacity`).
    fn append(&mut self, v: f64, capacity: usize) {
        self.history.push(v);
        self.appended += 1;
        if self.history.len() >= 2 * capacity {
            let excess = self.history.len() - capacity;
            self.history.drain(..excess);
        }
    }

    /// The newest `n` samples (fewer while history fills).
    fn tail(&self, n: usize) -> &[f64] {
        &self.history[self.history.len().saturating_sub(n)..]
    }

    fn new() -> Self {
        Self {
            history: Vec::new(),
            appended: 0,
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
    /// Samples kept per metric, at least `HISTORY_LEN`.
    capacity: usize,
}

impl MetricStore {
    #[must_use]
    pub fn new() -> Self {
        let mut series = HashMap::new();
        for &id in METRICS {
            series.insert(id, Series::new());
        }
        Self { series, capacity: HISTORY_LEN }
    }

    /// Keep `samples` per metric (clamped to `HISTORY_LEN..=MAX_CAPACITY`) for
    /// [`MetricStore::long_history`]. Lowering it hides the excess at once;
    /// raising it keeps what is there and lets history grow into the room.
    pub fn set_capacity(&mut self, samples: usize) {
        self.capacity = samples.clamp(HISTORY_LEN, MAX_CAPACITY);
    }

    /// Samples kept per metric.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Record one poll result. `None` means the source could not read it.
    pub fn push(&mut self, id: MetricId, value: Option<f64>) {
        let Some(s) = self.series.get_mut(&id) else { return };

        if let Some(v) = value {
            s.missing_streak = 0;
            s.current = Some(v);
            s.append(v, self.capacity);
        } else {
            s.missing_streak += 1;
            if s.missing_streak >= STALE_AFTER {
                s.current = None;
                // A gap keeps every series on the same time axis.
                s.append(f64::NAN, self.capacity);
            }
        }
    }

    /// Accumulate the session totals ([`RATE_TOTALS`]) from the current rate
    /// readings over `dt_secs`, the real time since the previous sample.
    /// Called once per published sample.
    ///
    /// The total integrates whatever the store considers current, so a brief
    /// miss (the last reading is held for [`STALE_AFTER`] polls, as gauges
    /// hold it) keeps accumulating at that reading; once the rate goes stale
    /// the total holds its value — it never goes stale or backwards itself.
    pub fn integrate(&mut self, dt_secs: f64) {
        if dt_secs <= 0.0 {
            return;
        }
        for &(rate, total) in &RATE_TOTALS {
            let Some(v) = self.series.get(&rate).and_then(|s| s.current) else { continue };
            let Some(t) = self.series.get_mut(&total) else { continue };
            let sum = t.current.unwrap_or(0.0) + v * dt_secs;
            t.current = Some(sum);
            t.missing_streak = 0;
            t.append(sum, self.capacity);
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

    /// The last `HISTORY_LEN` samples, oldest first. `NAN` entries mark gaps
    /// where the metric was unavailable.
    #[must_use]
    pub fn history(&self, id: MetricId) -> &[f64] {
        self.series.get(&id).map_or(&[], |s| s.tail(HISTORY_LEN))
    }

    /// Everything kept, up to [`MetricStore::capacity`] samples, oldest first:
    /// what line charts draw from. `NAN` marks gaps as in `history`.
    #[must_use]
    pub fn long_history(&self, id: MetricId) -> &[f64] {
        self.series.get(&id).map_or(&[], |s| s.tail(self.capacity))
    }

    /// Samples ever appended to the metric's history. The newest sample's
    /// position in that count is `appended - 1`.
    #[must_use]
    pub fn appended(&self, id: MetricId) -> u64 {
        self.series.get(&id).map_or(0, |s| s.appended)
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
    fn capacity_keeps_more_for_charts_and_history_stays_short() {
        let mut store = MetricStore::new();
        store.set_capacity(1000);
        for i in 0..5000 {
            store.push(CPU_USAGE, Some(f64::from(i)));
        }
        let long = store.long_history(CPU_USAGE);
        assert_eq!(long.len(), 1000);
        assert_eq!(long.first(), Some(&4000.0));
        assert_eq!(long.last(), Some(&4999.0));
        assert_eq!(store.history(CPU_USAGE).len(), HISTORY_LEN);
        assert_eq!(store.history(CPU_USAGE).last(), Some(&4999.0));
        assert_eq!(store.appended(CPU_USAGE), 5000);

        store.set_capacity(10);
        assert_eq!(store.capacity(), HISTORY_LEN, "never below the sparkline window");
        assert_eq!(store.long_history(CPU_USAGE).len(), HISTORY_LEN);
        store.set_capacity(usize::MAX);
        assert_eq!(store.capacity(), MAX_CAPACITY);
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
    fn totals_integrate_rates_and_hold_through_gaps() {
        use crate::metrics::metric::{NET_DOWN, NET_DOWN_TOTAL};
        let mut store = MetricStore::new();
        assert_eq!(store.current(NET_DOWN_TOTAL), None, "nothing moved yet");

        store.push(NET_DOWN, Some(10.0));
        store.integrate(2.0);
        assert_eq!(store.current(NET_DOWN_TOTAL), Some(20.0));
        store.integrate(0.5);
        assert_eq!(store.current(NET_DOWN_TOTAL), Some(25.0));

        // Brief misses hold the last reading (as gauges do), so the total
        // keeps integrating it; the third miss makes the rate stale.
        for _ in 0..2 {
            store.push(NET_DOWN, None);
            store.integrate(1.0);
        }
        assert_eq!(store.current(NET_DOWN_TOTAL), Some(45.0));
        for _ in 0..3 {
            store.push(NET_DOWN, None);
            store.integrate(1.0);
        }
        assert_eq!(store.current(NET_DOWN_TOTAL), Some(45.0), "stale rate: the total holds");

        // And resumes accumulating when the rate returns.
        store.push(NET_DOWN, Some(4.0));
        store.integrate(1.0);
        assert_eq!(store.current(NET_DOWN_TOTAL), Some(49.0));
        assert_eq!(store.history(NET_DOWN_TOTAL).len(), 5, "one sample per accumulation");
        store.integrate(0.0);
        assert_eq!(store.current(NET_DOWN_TOTAL), Some(49.0), "no time, no accumulation");
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
