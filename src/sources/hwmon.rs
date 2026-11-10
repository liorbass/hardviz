//! CPU/drive temperature and fan speed from the Linux kernel's hwmon sensors
//! (non-Windows).
//!
//! `sysinfo::Components` reads `/sys/class/hwmon`, so no extra daemon or root
//! access is needed. Labels look like `"<driver> <sensor label>"`, e.g.
//! `k10temp Tctl`, `coretemp Package id 0`, `zenpower Tdie`, `acpitz temp1`.

use crate::metrics::{metric, MetricId};
use crate::num::count_f64;
use crate::source::{MetricSource, Readings, SourceStatus};
use sysinfo::Components;

const PROVIDES: &[MetricId] = &[metric::CPU_TEMP, metric::STORAGE_TEMP, metric::FAN_SPEED];

/// Drivers that report the CPU die itself, in no particular order.
const CPU_DRIVERS: &[&str] = &["k10temp", "zenpower", "coretemp", "cpu_thermal", "cpu-thermal"];

pub struct HwmonSource {
    components: Components,
    status: SourceStatus,
}

impl HwmonSource {
    #[must_use]
    pub fn new() -> Self {
        Self { components: Components::new_with_refreshed_list(), status: SourceStatus::not_yet_polled() }
    }
}

impl Default for HwmonSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for HwmonSource {
    fn id(&self) -> &'static str {
        "hwmon"
    }

    fn display_name(&self) -> &'static str {
        "Linux hwmon (CPU/drive temp, fans)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        // Sensors can appear late (module load), so rescan when nothing is found.
        if self.components.list().is_empty() {
            self.components.refresh_list();
        } else {
            self.components.refresh();
        }

        let readings: Vec<(String, f64)> = self
            .components
            .list()
            .iter()
            .map(|c| (c.label().to_lowercase(), f64::from(c.temperature())))
            .filter(|(_, t)| t.is_finite() && (0.0..=150.0).contains(t))
            .collect();

        let cpu = pick_cpu_temp(&readings);
        let storage = pick_storage_temp(&readings);
        let fan = read_fan_max();
        out.set(metric::CPU_TEMP, cpu);
        out.set(metric::STORAGE_TEMP, storage);
        out.set(metric::FAN_SPEED, fan);
        self.status = match cpu.or(storage).or(fan) {
            Some(_) => SourceStatus::Online,
            None => SourceStatus::Unavailable {
                reason: format!(
                    "no CPU sensor among {} hwmon reading(s). Load the driver for your CPU \
                     (k10temp for AMD, coretemp for Intel) or run `sensors-detect` from lm-sensors.",
                    readings.len()
                ),
            },
        };
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

/// Preference: a package/die sensor from a CPU driver, else the mean of that
/// driver's per-core sensors, else any CPU driver reading, else an ACPI zone.
fn pick_cpu_temp(readings: &[(String, f64)]) -> Option<f64> {
    let cpu: Vec<&(String, f64)> =
        readings.iter().filter(|(l, _)| CPU_DRIVERS.iter().any(|d| l.starts_with(d))).collect();

    let package = cpu.iter().find(|(l, _)| ["package", "tctl", "tdie", "physical"].iter().any(|k| l.contains(k)));
    if let Some((_, t)) = package {
        return Some(*t);
    }

    let cores: Vec<f64> = cpu.iter().filter(|(l, _)| l.contains("core")).map(|(_, t)| *t).collect();
    if !cores.is_empty() {
        return Some(cores.iter().sum::<f64>() / count_f64(cores.len()));
    }

    cpu.first().map(|(_, t)| *t).or_else(|| readings.iter().find(|(l, _)| l.starts_with("acpitz")).map(|(_, t)| *t))
}

/// Hottest drive: `NVMe` `Composite` (or any `NVMe` sensor) and SATA `drivetemp`.
fn pick_storage_temp(readings: &[(String, f64)]) -> Option<f64> {
    readings
        .iter()
        .filter(|(l, _)| (l.starts_with("nvme") && !l.contains("sensor")) || l.starts_with("drivetemp"))
        .map(|(_, t)| *t)
        .reduce(f64::max)
}

/// Fastest fan across all hwmon devices (`fan*_input`, in RPM). sysinfo only
/// exposes temperatures, so read sysfs directly.
fn read_fan_max() -> Option<f64> {
    let mut max: Option<f64> = None;
    let dirs = std::fs::read_dir("/sys/class/hwmon").ok()?;
    for dev in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dev.path()) else { continue };
        for f in files.flatten() {
            let name = f.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("fan") && name.ends_with("_input")) {
                continue;
            }
            if let Some(rpm) = std::fs::read_to_string(f.path()).ok().and_then(|s| s.trim().parse::<f64>().ok()) {
                max = Some(max.map_or(rpm, |m| m.max(rpm)));
            }
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: &[(&str, f64)]) -> Vec<(String, f64)> {
        v.iter().map(|(l, t)| (l.to_string(), *t)).collect()
    }

    #[test]
    fn prefers_package() {
        let v = r(&[("coretemp core 0", 50.0), ("coretemp package id 0", 60.0)]);
        assert_eq!(pick_cpu_temp(&v), Some(60.0));
    }

    #[test]
    fn amd_tctl() {
        let v = r(&[("nvme composite", 40.0), ("k10temp tctl", 55.0)]);
        assert_eq!(pick_cpu_temp(&v), Some(55.0));
    }

    #[test]
    fn averages_cores() {
        let v = r(&[("coretemp core 0", 40.0), ("coretemp core 1", 50.0)]);
        assert_eq!(pick_cpu_temp(&v), Some(45.0));
    }

    #[test]
    fn storage_hottest_drive() {
        let v = r(&[("nvme composite wd_black", 44.0), ("nvme composite wdc", 37.0), ("k10temp tctl", 60.0)]);
        assert_eq!(pick_storage_temp(&v), Some(44.0));
        assert_eq!(pick_storage_temp(&r(&[("k10temp tctl", 60.0)])), None);
    }

    #[test]
    fn acpi_fallback() {
        let v = r(&[("nvme composite", 40.0), ("acpitz temp1", 48.0)]);
        assert_eq!(pick_cpu_temp(&v), Some(48.0));
    }
}
