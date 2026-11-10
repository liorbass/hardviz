//! Background metric collection.
//!
//! Sampling runs on its own thread because `eframe::App::update` only runs
//! when egui repaints, and winit throttles repaints for occluded or unfocused
//! windows (30 one-second polls take over 100 s while the window is covered).
//! Rendering only reads what this thread publishes.
//!
//! # Locks
//!
//! The poller thread takes registry -> store and registry -> diagnostics,
//! never store and diagnostics together, and the registry is touched by the UI
//! only through [`Poller::source_infos`] and [`Poller::set_source_enabled`],
//! which hold nothing else. The UI reads store, diagnostics and processes only
//! through [`SharedState::lock`], which takes them in that fixed order. So no
//! two threads can wait on each other.

use crate::metrics::{MetricId, MetricStore, METRICS};
use crate::processes::{ProcRow, ProcessSampler};
use crate::source::{MetricDiagnosis, MetricSource, SourceInfo, SourceRegistry};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Every metric's latest diagnosis.
pub type Diagnoses = HashMap<MetricId, MetricDiagnosis>;

/// Process list refresh interval. Slower than metrics: it is the costliest
/// sample, and a ranking does not need second-by-second updates.
const PROCESS_INTERVAL: Duration = Duration::from_secs(2);

/// Shortest sampling interval accepted.
const MIN_INTERVAL_MS: u64 = 50;

/// Health line and config save cadence, in polls.
pub const HEALTH_EVERY: u64 = 30;

/// What the poller publishes for rendering. Cloning clones the handles.
#[derive(Clone)]
pub struct SharedState {
    store: Arc<Mutex<MetricStore>>,
    diagnostics: Arc<Mutex<Diagnoses>>,
    processes: Arc<Mutex<Vec<ProcRow>>>,
}

/// The published state, locked for reading. Hold it for one frame at most.
#[non_exhaustive]
pub struct StateView<'a> {
    pub store: MutexGuard<'a, MetricStore>,
    pub diagnostics: MutexGuard<'a, Diagnoses>,
    pub processes: MutexGuard<'a, Vec<ProcRow>>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(MetricStore::new())),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            processes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Lock everything, in the documented order. `None` only if the poller
    /// thread panicked while holding a lock.
    #[must_use]
    pub fn lock(&self) -> Option<StateView<'_>> {
        Some(StateView {
            store: self.store.lock().ok()?,
            diagnostics: self.diagnostics.lock().ok()?,
            processes: self.processes.lock().ok()?,
        })
    }

    /// Ease displayed values toward the latest readings (once per frame).
    pub fn advance(&self, dt: f32, rate: f32) {
        if let Ok(mut store) = self.store.lock() {
            store.advance(dt, rate);
        }
    }
}

enum Command {
    SetInterval(u64),
}

/// Owns the sampling thread. Dropping it stops the thread.
pub struct Poller {
    state: SharedState,
    registry: Arc<Mutex<SourceRegistry>>,
    count: Arc<AtomicU64>,
    /// Set by the UI while a widget that shows processes is on screen.
    want_processes: Arc<AtomicBool>,
    /// Dropping it disconnects the thread's receiver, which stops it.
    commands: Sender<Command>,
    interval_ms: AtomicU64,
}

impl Poller {
    /// Start sampling. `sources` runs on the poller thread, so source
    /// construction never blocks the UI; sources whose ids are in `disabled`
    /// are registered disabled and never polled.
    pub fn spawn<F>(sources: F, disabled: &[String], interval_ms: u64) -> Self
    where
        F: FnOnce() -> Vec<Box<dyn MetricSource>> + Send + 'static,
    {
        let interval_ms = interval_ms.max(MIN_INTERVAL_MS);
        let (commands, inbox) = mpsc::channel();
        let poller = Self {
            state: SharedState::new(),
            registry: Arc::new(Mutex::new(SourceRegistry::with_disabled(disabled.iter().cloned()))),
            count: Arc::new(AtomicU64::new(0)),
            want_processes: Arc::new(AtomicBool::new(false)),
            commands,
            interval_ms: AtomicU64::new(interval_ms),
        };

        let worker = Worker {
            state: poller.state.clone(),
            registry: Arc::clone(&poller.registry),
            count: Arc::clone(&poller.count),
            want_processes: Arc::clone(&poller.want_processes),
            inbox,
            interval: Duration::from_millis(interval_ms),
        };
        let spawned = std::thread::Builder::new().name("hardviz-poller".into()).spawn(move || {
            worker.register(sources());
            worker.run();
        });
        if let Err(e) = spawned {
            // Without the poller nothing is ever sampled; every widget will
            // show N/A, and this line says why.
            crate::error!("cannot start the poller thread: {e}");
        }
        poller
    }

    /// The published store, diagnoses and process list.
    pub fn state(&self) -> SharedState {
        self.state.clone()
    }

    pub fn source_infos(&self) -> Vec<SourceInfo> {
        self.registry.lock().map(|r| r.infos()).unwrap_or_default()
    }

    /// Enable or disable a source by id. Returns whether it is registered.
    /// The poller picks the change up on its next sample.
    pub fn set_source_enabled(&self, id: &str, on: bool) -> bool {
        self.registry.lock().is_ok_and(|mut r| r.set_enabled(id, on))
    }

    /// Turn process sampling on or off; off clears the list.
    pub fn set_sample_processes(&self, on: bool) {
        self.want_processes.store(on, Ordering::Relaxed);
    }

    /// Number of samples taken so far; monotonic.
    pub fn poll_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Change the sampling interval. Cheap to call every frame: the thread is
    /// only woken when the value changes.
    pub fn set_interval(&self, ms: u64) {
        let ms = ms.max(MIN_INTERVAL_MS);
        if self.interval_ms.swap(ms, Ordering::Relaxed) != ms {
            // A send fails only if the thread is gone; nothing to tell it then.
            let _ = self.commands.send(Command::SetInterval(ms));
        }
    }
}

/// The poller thread's side of the shared state.
struct Worker {
    state: SharedState,
    registry: Arc<Mutex<SourceRegistry>>,
    count: Arc<AtomicU64>,
    want_processes: Arc<AtomicBool>,
    inbox: Receiver<Command>,
    interval: Duration,
}

impl Worker {
    fn register(&self, sources: Vec<Box<dyn MetricSource>>) {
        let Ok(mut reg) = self.registry.lock() else { return };
        for s in sources {
            reg.register(s);
        }
        let ids: Vec<String> = reg
            .infos()
            .iter()
            .map(|s| if s.enabled { s.id.to_string() } else { format!("({} disabled)", s.id) })
            .collect();
        crate::info!("sources registered: {}", ids.join(", "));
    }

    fn run(mut self) {
        let catalogue: Vec<MetricId> = METRICS.to_vec();
        let mut processes = ProcessJob { sampler: None, next_due: Instant::now() };
        let mut next_due = Instant::now();

        loop {
            let now = Instant::now();
            if now >= next_due {
                next_due = now + self.interval;
                self.sample_metrics(&catalogue);
                let n = self.count.fetch_add(1, Ordering::Relaxed) + 1;
                if n.is_multiple_of(HEALTH_EVERY) {
                    self.log_health(n);
                }
            }
            processes.tick(now, &self.want_processes, &self.state.processes);

            // Sleep until the next sample or process refresh, waking at once
            // for a command or when the `Poller` is dropped.
            let wake = next_due.min(processes.next_due).max(now);
            match self.inbox.recv_timeout(wake - now) {
                Ok(Command::SetInterval(ms)) => {
                    self.interval = Duration::from_millis(ms);
                    next_due = next_due.min(Instant::now() + self.interval);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        crate::info!("poller stopping after {} samples", self.count.load(Ordering::Relaxed));
    }

    /// Poll every source, then publish values and diagnoses. Sources do their
    /// I/O holding only the registry lock, so a slow source never stalls
    /// rendering; the store is locked just for the publish.
    fn sample_metrics(&self, catalogue: &[MetricId]) {
        let Ok(mut reg) = self.registry.lock() else { return };
        reg.poll_sources();
        if let Ok(mut store) = self.state.store.lock() {
            reg.publish(&mut store, catalogue);
        }
        let fresh: Diagnoses = catalogue.iter().map(|&id| (id, reg.diagnose(id))).collect();
        drop(reg);
        if let Ok(mut diag) = self.state.diagnostics.lock() {
            *diag = fresh;
        }
    }

    /// Periodic health snapshot: confirms samples are accumulating, since
    /// charts render nothing when history is empty.
    fn log_health(&self, n: u64) {
        let Ok(store) = self.state.store.lock() else { return };
        let summary: Vec<String> = METRICS
            .iter()
            .map(|&id| {
                let state = if store.current(id).is_some() { "live" } else { "none" };
                format!("{id}={state}/{}", store.history(id).len())
            })
            .collect();
        crate::info!("poll #{n}: {}", summary.join(", "));
    }
}

/// Process sampling state, created only once a widget asks for processes.
struct ProcessJob {
    sampler: Option<ProcessSampler>,
    next_due: Instant,
}

impl ProcessJob {
    /// Sample every `PROCESS_INTERVAL` while wanted; drop the sampler and clear
    /// the list when no widget shows processes any more.
    fn tick(&mut self, now: Instant, wanted: &AtomicBool, out: &Mutex<Vec<ProcRow>>) {
        if !wanted.load(Ordering::Relaxed) {
            if self.sampler.take().is_some() {
                if let Ok(mut p) = out.lock() {
                    p.clear();
                }
            }
            // Re-check at the normal cadence for the widget appearing.
            self.next_due = now + PROCESS_INTERVAL;
            return;
        }
        if now < self.next_due && self.sampler.is_some() {
            return;
        }
        self.next_due = now + PROCESS_INTERVAL;
        let rows = self.sampler.get_or_insert_with(ProcessSampler::new).sample();
        if let Ok(mut p) = out.lock() {
            *p = rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::CPU_USAGE;
    use crate::source::{Readings, SourceStatus};

    struct Constant(f64);

    impl MetricSource for Constant {
        fn id(&self) -> &'static str {
            "constant"
        }
        fn display_name(&self) -> &'static str {
            "constant"
        }
        fn provides(&self) -> &'static [MetricId] {
            &[CPU_USAGE]
        }
        fn poll(&mut self, out: &mut Readings) {
            out.set(CPU_USAGE, self.0);
        }
        fn status(&self) -> SourceStatus {
            SourceStatus::Online
        }
    }

    fn wait_for(poller: &Poller, polls: u64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while poller.poll_count() < polls {
            assert!(Instant::now() < deadline, "poller did not sample");
            std::thread::yield_now();
        }
    }

    #[test]
    fn samples_sources_into_the_shared_store() {
        let poller = Poller::spawn(|| vec![Box::new(Constant(42.0)) as Box<dyn MetricSource>], &[], 50);
        wait_for(&poller, 1);
        let state = poller.state();
        let view = state.lock().expect("locks");
        assert_eq!(view.store.current(CPU_USAGE), Some(42.0));
        assert_eq!(view.diagnostics.get(&CPU_USAGE).and_then(|d| d.provider), Some("constant"));
    }

    #[test]
    fn disabled_source_is_reported_disabled_and_not_sampled() {
        let poller =
            Poller::spawn(|| vec![Box::new(Constant(42.0)) as Box<dyn MetricSource>], &["constant".into()], 50);
        wait_for(&poller, 1);
        let infos = poller.source_infos();
        assert_eq!(infos.len(), 1);
        assert!(!infos[0].enabled);
        let state = poller.state();
        let view = state.lock().expect("locks");
        assert!(view.store.history(CPU_USAGE).is_empty());
        assert!(poller.set_source_enabled("constant", true));
    }
}
