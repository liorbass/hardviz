//! Concrete [`crate::source::MetricSource`] implementations.
//!
//! To add a new information source:
//! 1. Create a module here implementing `MetricSource`.
//! 2. Add it to `pub mod` below.
//! 3. Add it to `default_sources()`; position determines fallback priority.
//!
//! Nothing else needs to change: widgets resolve metrics by id through the
//! store, so a new source becomes visible to every widget that requires the
//! metrics it provides.

#[cfg(windows)]
pub mod acpi;
#[cfg(target_os = "linux")]
pub mod diskstats;
#[cfg(not(windows))]
pub mod hwmon;
#[cfg(windows)]
pub mod lhm;
pub mod nvml;
pub mod sysinfo_source;

use crate::source::MetricSource;

/// The default source set, in fallback-priority order.
///
/// For a metric supplied by several sources, the earliest *online* source
/// that returns a value wins. CPU temperature is tried as LHM-over-HTTP first
/// (most accurate), ACPI thermal zone second (rarely present on modern boards)
/// on Windows; elsewhere hwmon via sysinfo.
///
/// Constructors are cheap and have no side effects; each source does its
/// setup on its first poll, on the poller thread, and only if enabled.
#[must_use]
pub fn default_sources() -> Vec<Box<dyn MetricSource>> {
    let mut sources = platform_sources();
    // GPU family: NVML is the only provider.
    sources.push(Box::new(nvml::NvmlSource::new()));
    // CPU/RAM usage: sysinfo. Registered last so it never shadows a more
    // specific provider for metrics it also happens to expose.
    sources.push(Box::new(sysinfo_source::SysinfoSource::new()));
    sources
}

/// CPU temperature on Windows: LHM's HTTP JSON endpoint first, ACPI WMI as
/// fallback. LHM 0.9.6 has no WMI provider; see sources/lhm.rs for the evidence.
#[cfg(windows)]
fn platform_sources() -> Vec<Box<dyn MetricSource>> {
    vec![Box::new(lhm::LhmHttpSource::new()), Box::new(acpi::AcpiThermalSource::new())]
}

/// Linux: kernel hwmon sensors, and disk throughput from /proc/diskstats.
#[cfg(target_os = "linux")]
fn platform_sources() -> Vec<Box<dyn MetricSource>> {
    vec![Box::new(hwmon::HwmonSource::new()), Box::new(diskstats::DiskStatsSource::new())]
}

/// Other Unix: kernel hwmon sensors through sysinfo.
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_sources() -> Vec<Box<dyn MetricSource>> {
    vec![Box::new(hwmon::HwmonSource::new())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn source_ids_are_unique() {
        let sources = default_sources();
        let ids: HashSet<_> = sources.iter().map(|s| s.id()).collect();
        assert_eq!(ids.len(), sources.len(), "duplicate source id");
    }

    #[test]
    fn sources_start_not_yet_polled() {
        for s in default_sources() {
            assert!(!s.status().is_online(), "{} claims to be online before its first poll", s.id());
        }
    }
}
