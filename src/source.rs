//! The data-source plugin interface.
//!
//! A source is anything that can produce readings for one or more metrics:
//! sysinfo, NVML, a WMI provider, a network agent, a CSV replayer. Sources are
//! registered by priority per metric — the first one that both *provides* a
//! metric and returns `Some` wins, which gives automatic fallback for free.

use crate::metrics::{MetricId, MetricStore};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// The values one source produced in one poll, keyed by metric id.
///
/// A source writes each reading with [`Readings::set`]; a metric it does not
/// set (or sets to `None`, or to a non-finite value) counts as "not available
/// right now".
#[derive(Debug, Default, Clone)]
pub struct Readings {
    values: HashMap<MetricId, f64>,
}

impl Readings {
    pub fn set(&mut self, id: MetricId, value: impl Into<Option<f64>>) {
        match value.into() {
            Some(v) if v.is_finite() => {
                self.values.insert(id, v);
            }
            _ => {
                self.values.remove(&id);
            }
        }
    }

    #[must_use]
    pub fn get(&self, id: MetricId) -> Option<f64> {
        self.values.get(&id).copied()
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn ids(&self) -> impl Iterator<Item = MetricId> + '_ {
        self.values.keys().copied()
    }
}

/// Whether a source is currently able to report. Surfaced in the UI so an
/// unavailable metric explains itself instead of silently rendering zero.
#[derive(Clone, Debug)]
pub enum SourceStatus {
    Online,
    /// Source cannot function; carries a user-actionable reason.
    Unavailable {
        reason: String,
    },
}

impl SourceStatus {
    /// The status every source starts with.
    #[must_use]
    pub fn not_yet_polled() -> Self {
        SourceStatus::Unavailable { reason: "not yet polled".into() }
    }

    #[must_use]
    pub fn is_online(&self) -> bool {
        matches!(self, SourceStatus::Online)
    }
}

impl fmt::Display for SourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceStatus::Online => write!(f, "online"),
            SourceStatus::Unavailable { reason } => write!(f, "unavailable: {reason}"),
        }
    }
}

/// Implement this to add a new information source.
///
/// Contract:
/// - Construction must be cheap and free of side effects visible to the user
///   (no process launches, no prompts). It runs on the poller thread.
/// - Until its first poll a source reports
///   `SourceStatus::Unavailable { reason: "not yet polled" }` (see
///   [`SourceStatus::not_yet_polled`]).
/// - [`MetricSource::poll`] is called on the poller thread on the refresh
///   interval, never for a disabled source. It must be bounded (no long
///   blocking I/O) and must never panic on missing hardware.
/// - `poll` writes this round's values into `out` (which starts empty). A
///   metric left unset is "not available right now" - never write a fake 0.
/// - Only metrics listed in [`MetricSource::provides`] may be written; debug
///   builds assert this, so a missing declaration fails loudly.
pub trait MetricSource: Send {
    /// Unique, stable identifier (used for config and status display).
    fn id(&self) -> &'static str;

    /// Human-readable name shown in the settings/status panel.
    fn display_name(&self) -> &'static str;

    /// Every metric this source is able to supply.
    fn provides(&self) -> &'static [MetricId];

    /// Take readings for this interval and write them into `out`.
    fn poll(&mut self, out: &mut Readings);

    /// Current health, with a reason when unavailable.
    fn status(&self) -> SourceStatus;
}

/// Snapshot of one source's health, for display.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SourceInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub provides: &'static [MetricId],
    pub status: SourceStatus,
    pub enabled: bool,
}

/// One registered source and what the registry remembers about it.
struct Entry {
    source: Box<dyn MetricSource>,
    /// Values from its latest poll; empty while disabled.
    readings: Readings,
    /// Last logged status, so only transitions are logged.
    last_status: String,
}

/// Ordered collection of sources. Registration order is fallback priority.
///
/// Enabled state is kept by source id, so it can be set before a source is
/// registered (the persisted choice is applied before anything is polled) and
/// survives re-registration.
#[derive(Default)]
pub struct SourceRegistry {
    entries: Vec<Entry>,
    disabled: HashSet<String>,
}

impl SourceRegistry {
    /// A registry in which the sources with these ids start disabled.
    pub fn with_disabled<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self { entries: Vec::new(), disabled: ids.into_iter().map(Into::into).collect() }
    }

    /// Register a source. Earlier registrations take priority for shared
    /// metrics. Nothing is polled here: a source reports "not yet polled"
    /// until the poller's first sample.
    pub fn register(&mut self, source: Box<dyn MetricSource>) {
        self.entries.push(Entry { source, readings: Readings::default(), last_status: String::new() });
    }

    fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id)
    }

    /// Enable or disable a source by id. Returns whether such a source is
    /// registered; the choice is kept either way.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.to_string());
        }
        self.entries.iter().any(|e| e.source.id() == id)
    }

    #[must_use]
    pub fn infos(&self) -> Vec<SourceInfo> {
        self.entries
            .iter()
            .map(|e| SourceInfo {
                id: e.source.id(),
                display_name: e.source.display_name(),
                provides: e.source.provides(),
                status: e.source.status(),
                enabled: self.is_enabled(e.source.id()),
            })
            .collect()
    }

    /// Poll every enabled source into its own [`Readings`]. Disabled sources
    /// are not called at all, so they cause no side effects.
    ///
    /// Source status is logged only on *transition*; polling once a second
    /// would otherwise bury the file in repeats of the same failure.
    pub fn poll_sources(&mut self) {
        let disabled = &self.disabled;
        for e in &mut self.entries {
            e.readings.clear();
            if disabled.contains(e.source.id()) {
                continue;
            }
            e.source.poll(&mut e.readings);
            debug_assert!(
                e.readings.ids().all(|id| e.source.provides().contains(&id)),
                "source `{}` wrote metrics it does not declare in provides(): {:?}",
                e.source.id(),
                e.readings.ids().filter(|id| !e.source.provides().contains(id)).collect::<Vec<_>>()
            );

            let status = e.source.status();
            let status_str = status.to_string();
            if e.last_status != status_str {
                if status.is_online() {
                    crate::info!("[{}] online", e.source.id());
                } else {
                    crate::warn!("[{}] {}", e.source.id(), status_str);
                }
                e.last_status = status_str;
            }
        }
    }

    /// The value for `id` from the highest-priority enabled, online source
    /// that produced one in the latest poll.
    #[must_use]
    pub fn resolve(&self, id: MetricId) -> Option<f64> {
        self.entries
            .iter()
            .filter(|e| self.is_enabled(e.source.id()) && e.source.status().is_online())
            .find_map(|e| e.readings.get(id))
    }

    /// Push the resolved value of every catalogue metric into the store.
    ///
    /// A metric with no live provider is pushed as `None`, which lets the store
    /// mark it stale so the UI shows "N/A" rather than a frozen or zero reading.
    pub fn publish(&self, store: &mut MetricStore, catalogue: &[MetricId]) {
        for &id in catalogue {
            store.push(id, self.resolve(id));
        }
    }

    /// Explain a metric's availability. Returned to widgets so an unavailable
    /// reading renders an actionable message instead of a misleading zero.
    #[must_use]
    pub fn diagnose(&self, id: MetricId) -> MetricDiagnosis {
        let mut provider = None;
        let mut candidates = Vec::new();

        for e in self.entries.iter().filter(|e| e.source.provides().contains(&id)) {
            let source = &e.source;
            if !self.is_enabled(source.id()) {
                candidates
                    .push((source.display_name(), SourceStatus::Unavailable { reason: "disabled by user".into() }));
                continue;
            }
            let status = source.status();
            if provider.is_none() && status.is_online() && e.readings.get(id).is_some() {
                provider = Some(source.id());
            }
            candidates.push((source.display_name(), status));
        }

        MetricDiagnosis { provider, candidates }
    }
}

/// Why a metric is or is not available.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MetricDiagnosis {
    /// Source id currently supplying the metric, if any.
    pub provider: Option<&'static str>,
    /// Every source that claims this metric, with its status.
    pub candidates: Vec<(&'static str, SourceStatus)>,
}

impl MetricDiagnosis {
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.provider.is_some()
    }

    /// Human-readable summary of why nothing is supplying this metric.
    #[must_use]
    pub fn explain(&self) -> String {
        if self.candidates.is_empty() {
            return "no registered source provides this metric".into();
        }
        self.candidates
            .iter()
            .map(|(name, status)| match status {
                SourceStatus::Online => format!("{name}: online but no reading"),
                SourceStatus::Unavailable { reason } => format!("{name}: {reason}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::{CPU_TEMP, CPU_USAGE};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A source that reports fixed values and counts its polls.
    struct Fake {
        id: &'static str,
        online: bool,
        values: Vec<(MetricId, f64)>,
        polls: Arc<AtomicUsize>,
    }

    const PROVIDES: &[MetricId] = &[CPU_TEMP, CPU_USAGE];

    impl Fake {
        fn new(id: &'static str, online: bool, values: &[(MetricId, f64)]) -> (Self, Arc<AtomicUsize>) {
            let polls = Arc::new(AtomicUsize::new(0));
            (Self { id, online, values: values.to_vec(), polls: Arc::clone(&polls) }, polls)
        }
    }

    impl MetricSource for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn display_name(&self) -> &'static str {
            self.id
        }
        fn provides(&self) -> &'static [MetricId] {
            PROVIDES
        }
        fn poll(&mut self, out: &mut Readings) {
            self.polls.fetch_add(1, Ordering::SeqCst);
            for &(id, v) in &self.values {
                out.set(id, v);
            }
        }
        fn status(&self) -> SourceStatus {
            if self.online {
                SourceStatus::Online
            } else {
                SourceStatus::Unavailable { reason: "offline".into() }
            }
        }
    }

    fn registry(sources: Vec<Fake>) -> SourceRegistry {
        let mut reg = SourceRegistry::default();
        for s in sources {
            reg.register(Box::new(s));
        }
        reg
    }

    #[test]
    fn earlier_source_wins_and_later_one_fills_gaps() {
        let (a, _) = Fake::new("a", true, &[(CPU_TEMP, 50.0)]);
        let (b, _) = Fake::new("b", true, &[(CPU_TEMP, 60.0), (CPU_USAGE, 7.0)]);
        let mut reg = registry(vec![a, b]);
        reg.poll_sources();
        assert_eq!(reg.resolve(CPU_TEMP), Some(50.0));
        assert_eq!(reg.resolve(CPU_USAGE), Some(7.0));
        assert_eq!(reg.diagnose(CPU_TEMP).provider, Some("a"));
        assert_eq!(reg.diagnose(CPU_USAGE).provider, Some("b"));
    }

    #[test]
    fn offline_source_falls_through_to_the_next() {
        let (a, _) = Fake::new("a", false, &[(CPU_TEMP, 50.0)]);
        let (b, _) = Fake::new("b", true, &[(CPU_TEMP, 60.0)]);
        let mut reg = registry(vec![a, b]);
        reg.poll_sources();
        assert_eq!(reg.resolve(CPU_TEMP), Some(60.0));
    }

    #[test]
    fn disabled_source_is_never_polled_and_is_explained() {
        let (a, a_polls) = Fake::new("a", true, &[(CPU_TEMP, 50.0)]);
        let (b, _) = Fake::new("b", true, &[(CPU_TEMP, 60.0)]);
        let mut reg = SourceRegistry::with_disabled(["a"]);
        reg.register(Box::new(a));
        reg.register(Box::new(b));
        reg.poll_sources();
        assert_eq!(a_polls.load(Ordering::SeqCst), 0, "a disabled source must not run");
        assert_eq!(reg.resolve(CPU_TEMP), Some(60.0));
        let diag = reg.diagnose(CPU_TEMP);
        assert!(diag.explain().contains("a: disabled by user"), "{}", diag.explain());
        assert!(!reg.infos()[0].enabled);
    }

    #[test]
    fn register_does_not_poll() {
        let (a, polls) = Fake::new("a", true, &[]);
        let _reg = registry(vec![a]);
        assert_eq!(polls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn set_enabled_is_keyed_by_id() {
        let (a, polls) = Fake::new("a", true, &[(CPU_TEMP, 50.0)]);
        let mut reg = registry(vec![a]);
        assert!(reg.set_enabled("a", false));
        assert!(!reg.set_enabled("missing", false));
        reg.poll_sources();
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert_eq!(reg.resolve(CPU_TEMP), None);
        reg.set_enabled("a", true);
        reg.poll_sources();
        assert_eq!(reg.resolve(CPU_TEMP), Some(50.0));
    }

    #[test]
    fn unanswered_metric_is_published_as_missing() {
        let (a, _) = Fake::new("a", true, &[(CPU_USAGE, 5.0)]);
        let mut reg = registry(vec![a]);
        let mut store = MetricStore::new();
        for _ in 0..3 {
            reg.poll_sources();
            reg.publish(&mut store, &[CPU_TEMP, CPU_USAGE]);
        }
        assert_eq!(store.current(CPU_USAGE), Some(5.0));
        assert_eq!(store.current(CPU_TEMP), None);
        let diag = reg.diagnose(CPU_TEMP);
        assert!(!diag.is_available());
        assert_eq!(diag.explain(), "a: online but no reading");
    }

    #[test]
    fn readings_drop_non_finite_values() {
        let mut r = Readings::default();
        r.set(CPU_TEMP, 40.0);
        r.set(CPU_TEMP, f64::NAN);
        assert_eq!(r.get(CPU_TEMP), None);
        r.set(CPU_USAGE, None);
        assert_eq!(r.get(CPU_USAGE), None);
    }

    #[test]
    fn metric_with_no_source_says_so() {
        let reg = SourceRegistry::default();
        assert_eq!(reg.diagnose(CPU_TEMP).explain(), "no registered source provides this metric");
    }
}
