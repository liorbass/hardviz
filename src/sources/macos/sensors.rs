//! Drive temperature on macOS, and CPU temperature when the SMC source has
//! none, from the sensors sysinfo reads.
//!
//! `sysinfo::Components` reads the Apple Silicon PMU sensors through IOHID
//! (`PMU tdie1`..`PMU tdieN`, `NAND CH0 temp`) and, on Intel Macs, the SMC
//! (`PECI CPU`, `CPU Proximity`). No root access or helper is needed.

use crate::metrics::{metric, MetricId};
use crate::source::{MetricSource, Readings, SourceStatus};
use sysinfo::Components;

const PROVIDES: &[MetricId] = &[metric::CPU_TEMP, metric::STORAGE_TEMP];

pub struct MacSensorsSource {
    components: Option<Components>,
    status: SourceStatus,
}

impl MacSensorsSource {
    /// Cheap: the sensor list is built on the first poll.
    #[must_use]
    pub fn new() -> Self {
        Self { components: None, status: SourceStatus::not_yet_polled() }
    }
}

impl Default for MacSensorsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for MacSensorsSource {
    fn id(&self) -> &'static str {
        "macos_sensors"
    }

    fn display_name(&self) -> &'static str {
        "macOS sensors (CPU/drive temp)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        let components = match &mut self.components {
            Some(c) => {
                // Refresh only the sensors used: an M4 Pro has ~60, and
                // refreshing all of them takes 100-200 ms.
                for c in c.list_mut().iter_mut().filter(|c| is_used(&c.label().to_lowercase())) {
                    c.refresh();
                }
                c
            }
            None => self.components.insert(Components::new_with_refreshed_list()),
        };

        // Unused PMU channels (`PMU tdev*`) read about -9200 °C; drop anything
        // that is not a plausible temperature.
        let readings: Vec<(String, f64)> = components
            .list()
            .iter()
            .map(|c| (c.label().to_lowercase(), f64::from(c.temperature())))
            .filter(|(l, t)| is_used(l) && t.is_finite() && (1.0..=150.0).contains(t))
            .collect();

        let cpu = pick_cpu_temp(&readings);
        let storage = pick_storage_temp(&readings);
        out.set(metric::CPU_TEMP, cpu);
        out.set(metric::STORAGE_TEMP, storage);
        self.status = match cpu.or(storage) {
            Some(_) => SourceStatus::Online,
            None => SourceStatus::Unavailable {
                reason: format!("no CPU or drive sensor among {} reading(s)", readings.len()),
            },
        };
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

/// Labels (lowercase) of the sensors the pickers below read.
fn is_used(label: &str) -> bool {
    ["pmu tdie", "peci cpu", "cpu proximity", "nand"].iter().any(|p| label.starts_with(p))
}

/// Apple Silicon: the hottest PMU die sensor, like a package temperature
/// (the hottest core). Intel: the PECI package reading, else CPU proximity.
fn pick_cpu_temp(readings: &[(String, f64)]) -> Option<f64> {
    let hottest =
        |prefix: &str| readings.iter().filter(|(l, _)| l.starts_with(prefix)).map(|(_, t)| *t).reduce(f64::max);
    hottest("pmu tdie").or_else(|| hottest("peci cpu")).or_else(|| hottest("cpu proximity"))
}

/// The internal SSD's NAND temperature (Apple Silicon).
fn pick_storage_temp(readings: &[(String, f64)]) -> Option<f64> {
    readings.iter().filter(|(l, _)| l.starts_with("nand")).map(|(_, t)| *t).reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: &[(&str, f64)]) -> Vec<(String, f64)> {
        v.iter().map(|(l, t)| (l.to_string(), *t)).collect()
    }

    #[test]
    fn apple_silicon_hottest_die() {
        let v = r(&[("pmu tdie1", 41.6), ("pmu tdie8", 43.1), ("pmu tcal", 51.8), ("gas gauge battery", 28.0)]);
        assert_eq!(pick_cpu_temp(&v), Some(43.1));
    }

    #[test]
    fn intel_prefers_peci() {
        assert_eq!(pick_cpu_temp(&r(&[("cpu proximity", 48.0), ("peci cpu", 61.0)])), Some(61.0));
        assert_eq!(pick_cpu_temp(&r(&[("cpu proximity", 48.0), ("battery", 30.0)])), Some(48.0));
        assert_eq!(pick_cpu_temp(&r(&[("battery", 30.0)])), None);
    }

    #[test]
    fn nand_is_the_drive() {
        assert_eq!(pick_storage_temp(&r(&[("nand ch0 temp", 32.0), ("pmu tdie1", 41.0)])), Some(32.0));
        assert_eq!(pick_storage_temp(&r(&[("pmu tdie1", 41.0)])), None);
    }
}
