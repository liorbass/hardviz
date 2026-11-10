//! ACPI thermal-zone CPU temperature via WMI (Windows only).
//!
//! Fallback behind [`LhmHttpSource`](super::lhm::LhmHttpSource); the class is
//! absent on most modern boards.

use crate::metrics::{metric, MetricId};
use crate::source::{MetricSource, Readings, SourceStatus};
use serde::Deserialize;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wmi::{COMLibrary, WMIConnection};

const ACPI_NAMESPACE: &str = "root/wmi";
/// How often the ACPI worker re-queries WMI.
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The class must be named explicitly: `query::<T>()` derives the WMI class
/// name from the Rust type name.
const WQL: &str = "SELECT CurrentTemperature, InstanceName FROM MSAcpi_ThermalZoneTemperature";
const ABSENT_HINT: &str = "This class is absent on most modern motherboards and often needs administrator rights.";

/// One row of `MSAcpi_ThermalZoneTemperature`. Value is in tenths of a Kelvin.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct AcpiThermalZone {
    current_temperature: Option<u32>,
    instance_name: Option<String>,
}

/// The worker's latest result.
#[derive(Debug)]
struct Latest {
    temp: Option<f64>,
    status: SourceStatus,
}

/// Reads CPU temperature from the Windows ACPI thermal-zone WMI class.
///
/// All COM work happens on a **dedicated thread**. The UI thread is already an
/// STA (winit/eframe initialises COM that way for OLE drag-and-drop), and
/// calling `COMLibrary::new()`, which requests MTA, on that same thread fails
/// with `RPC_E_CHANGED_MODE` (0x80010106) before any query can run. COM
/// apartment mode is per-thread, so a worker thread sidesteps the conflict and
/// keeps slow WMI queries out of `poll`.
pub struct AcpiThermalSource {
    latest: Arc<Mutex<Latest>>,
    /// Dropping it stops the worker; `None` until the first poll starts it.
    stop: Option<Sender<()>>,
    /// Snapshot copied out of `latest` on each poll, so `status` never locks.
    status: SourceStatus,
}

impl AcpiThermalSource {
    #[must_use]
    pub fn new() -> Self {
        Self {
            latest: Arc::new(Mutex::new(Latest { temp: None, status: SourceStatus::not_yet_polled() })),
            stop: None,
            status: SourceStatus::not_yet_polled(),
        }
    }

    /// Start the WMI worker on first use, so a disabled source never spawns it.
    fn ensure_worker(&mut self) {
        if self.stop.is_some() {
            return;
        }
        let (stop, stopped) = mpsc::channel();
        let latest = Arc::clone(&self.latest);
        let spawned =
            std::thread::Builder::new().name("hardviz-acpi-wmi".into()).spawn(move || acpi_worker(&latest, &stopped));
        match spawned {
            Ok(_) => publish(&self.latest, None, SourceStatus::Unavailable { reason: "worker starting".into() }),
            Err(e) => publish(
                &self.latest,
                None,
                SourceStatus::Unavailable { reason: format!("cannot start the WMI worker thread ({e})") },
            ),
        }
        // Kept even on failure, so a failed spawn is not retried every poll.
        self.stop = Some(stop);
    }
}

impl Default for AcpiThermalSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Poll loop: initialise COM once, then re-query until `stopped` disconnects.
fn acpi_worker(latest: &Mutex<Latest>, stopped: &mpsc::Receiver<()>) {
    // MTA on *this* thread only, independent of the UI thread's STA.
    let com = match COMLibrary::new() {
        Ok(c) => c,
        Err(e) => {
            let reason = format!("COM init failed on worker thread ({e})");
            return publish(latest, None, SourceStatus::Unavailable { reason });
        }
    };
    let conn = match WMIConnection::with_namespace_path(ACPI_NAMESPACE, com) {
        Ok(c) => c,
        Err(e) => {
            let reason = format!("cannot open `{ACPI_NAMESPACE}` ({e})");
            return publish(latest, None, SourceStatus::Unavailable { reason });
        }
    };

    loop {
        match conn.raw_query::<AcpiThermalZone>(WQL) {
            Ok(zones) => {
                let readings: Vec<(f64, String)> = zones
                    .iter()
                    .filter_map(|z| Some((z.current_temperature?, z.instance_name.clone().unwrap_or_default())))
                    .map(|(tenths_kelvin, name)| (f64::from(tenths_kelvin) / 10.0 - 273.15, name))
                    .collect();
                if let Some(t) = pick_zone_temp(&readings) {
                    publish(latest, Some(t), SourceStatus::Online);
                } else {
                    let reason = format!(
                        "MSAcpi_ThermalZoneTemperature returned {} zone(s), none with a plausible \
                         temperature. {ABSENT_HINT}",
                        zones.len()
                    );
                    publish(latest, None, SourceStatus::Unavailable { reason });
                }
            }
            Err(e) => {
                let reason = format!("MSAcpi_ThermalZoneTemperature query failed ({e}). {ABSENT_HINT}");
                publish(latest, None, SourceStatus::Unavailable { reason });
            }
        }

        // Waits for the interval, but wakes at once when the source is dropped.
        match stopped.recv_timeout(POLL_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Pick the CPU temperature from `(celsius, zone name)` readings: a zone named
/// for the CPU wins; otherwise the hottest plausible zone, which on boards that
/// expose ACPI thermal data is usually the CPU package. Implausible readings
/// (outside 0-150 °C) are ignored.
fn pick_zone_temp(zones: &[(f64, String)]) -> Option<f64> {
    let plausible = || zones.iter().filter(|(c, _)| (0.0..=150.0).contains(c));
    plausible()
        .find(|(_, name)| name.to_lowercase().contains("cpu"))
        .or_else(|| plausible().max_by(|a, b| a.0.total_cmp(&b.0)))
        .map(|(c, _)| *c)
}

fn publish(latest: &Mutex<Latest>, temp: Option<f64>, status: SourceStatus) {
    if let Ok(mut guard) = latest.lock() {
        guard.temp = temp;
        guard.status = status;
    }
}

impl MetricSource for AcpiThermalSource {
    fn id(&self) -> &'static str {
        "acpi_thermal"
    }

    fn display_name(&self) -> &'static str {
        "ACPI Thermal Zone (fallback CPU temp)"
    }

    fn provides(&self) -> &'static [MetricId] {
        &[metric::CPU_TEMP]
    }

    fn poll(&mut self, out: &mut Readings) {
        self.ensure_worker();
        // Cheap: copy the worker thread's latest result.
        if let Ok(guard) = self.latest.lock() {
            out.set(metric::CPU_TEMP, guard.temp);
            self.status = guard.status.clone();
        }
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones(list: &[(f64, &str)]) -> Vec<(f64, String)> {
        list.iter().map(|(c, n)| (*c, n.to_string())).collect()
    }

    #[test]
    fn cpu_named_zone_wins_over_hotter_zones() {
        assert_eq!(pick_zone_temp(&zones(&[(70.0, "TZ00"), (55.0, "\\_TZ.CPUZ")])), Some(55.0));
    }

    #[test]
    fn hottest_plausible_zone_is_the_fallback() {
        assert_eq!(pick_zone_temp(&zones(&[(40.0, "TZ00"), (62.5, "TZ01"), (300.0, "TZ02")])), Some(62.5));
    }

    #[test]
    fn implausible_or_missing_zones_give_nothing() {
        assert_eq!(pick_zone_temp(&zones(&[(-273.15, "CPU"), (200.0, "TZ")])), None);
        assert_eq!(pick_zone_temp(&[]), None);
    }
}
