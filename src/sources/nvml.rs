//! NVIDIA GPU readings via NVML.
//!
//! Works on Windows and Linux with NVIDIA drivers installed; no extra setup.
//! Reports GPU utilisation, VRAM capacity/usage, temperature, power and clock.
//!
//! With several GPUs, one reading still describes "the GPU": utilisation,
//! temperature and clock report the busiest device, power and VRAM the sum
//! across devices. A query one board does not support is left unset, never
//! reported as zero.

use crate::metrics::{metric, MetricId, BYTES_PER_MB};
use crate::num::total_f64;
use crate::nvidia;
use crate::source::{MetricSource, Readings, SourceStatus};
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;

const PROVIDES: &[MetricId] = &[
    metric::GPU_USAGE,
    metric::GPU_TEMP,
    metric::VRAM_USAGE,
    metric::VRAM_USED,
    metric::VRAM_TOTAL,
    metric::GPU_POWER,
    metric::GPU_CLOCK,
];

/// The NVML library, loaded on the first poll so construction stays cheap.
enum Library {
    NotLoaded,
    Loaded(Box<Nvml>),
    /// Loading failed; not retried (a driver does not appear mid-session).
    Failed,
}

pub struct NvmlSource {
    nvml: Library,
    status: SourceStatus,
}

impl NvmlSource {
    #[must_use]
    pub fn new() -> Self {
        Self { nvml: Library::NotLoaded, status: SourceStatus::not_yet_polled() }
    }

    fn library(&mut self) -> Option<&Nvml> {
        if matches!(self.nvml, Library::NotLoaded) {
            self.nvml = match nvidia::init() {
                Ok(n) => Library::Loaded(Box::new(n)),
                Err(e) => {
                    crate::warn!("[nvml] init failed: {e}");
                    self.status = SourceStatus::Unavailable {
                        reason: format!("NVML not available, no NVIDIA driver detected ({e})"),
                    };
                    Library::Failed
                }
            };
        }
        match &self.nvml {
            Library::Loaded(n) => Some(n),
            _ => None,
        }
    }
}

impl Default for NvmlSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for NvmlSource {
    fn id(&self) -> &'static str {
        "nvml"
    }

    fn display_name(&self) -> &'static str {
        "NVML (NVIDIA GPU)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    /// Each query is independent: one the board does not support (power on
    /// some laptops, for example) is simply left unset rather than reported
    /// as zero. Across devices, maxima describe the busiest GPU and sums the
    /// whole machine's VRAM and draw.
    fn poll(&mut self, out: &mut Readings) {
        let Some(nvml) = self.library() else { return };
        let count = match nvml.device_count() {
            Ok(n) if n > 0 => n,
            Ok(_) => {
                self.status = SourceStatus::Unavailable { reason: "no NVIDIA device enumerable".into() };
                return;
            }
            Err(e) => {
                self.status = SourceStatus::Unavailable { reason: format!("no NVIDIA device enumerable ({e})") };
                return;
            }
        };

        let (mut usage, mut temp, mut clock): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
        let (mut vram_used, mut vram_total, mut power) = (0.0, 0.0, 0.0);
        let (mut any, mut have_mem, mut have_power) = (false, false, false);
        for i in 0..count {
            let Ok(device) = nvml.device_by_index(i) else { continue };
            any = true;
            if let Ok(u) = device.utilization_rates() {
                usage = Some(usage.unwrap_or(0.0).max(f64::from(u.gpu)));
            }
            if let Ok(info) = device.memory_info() {
                have_mem = true;
                vram_used += total_f64(info.used) / BYTES_PER_MB;
                vram_total += total_f64(info.total) / BYTES_PER_MB;
            }
            // 0 is not a plausible GPU temperature; treat it as "no reading".
            if let Some(t) = device.temperature(TemperatureSensor::Gpu).ok().filter(|t| *t > 0) {
                temp = Some(temp.unwrap_or(0.0).max(f64::from(t)));
            }
            // NVML reports milliwatts.
            if let Ok(mw) = device.power_usage() {
                have_power = true;
                power += f64::from(mw) / 1000.0;
            }
            if let Ok(c) = device.clock_info(Clock::Graphics) {
                clock = Some(clock.unwrap_or(0.0).max(f64::from(c)));
            }
        }
        if !any {
            self.status = SourceStatus::Unavailable { reason: "no NVIDIA device responded".into() };
            return;
        }

        out.set(metric::GPU_USAGE, usage);
        if have_mem {
            out.set(metric::VRAM_USED, vram_used);
            out.set(metric::VRAM_TOTAL, vram_total);
            if vram_total > 0.0 {
                out.set(metric::VRAM_USAGE, vram_used / vram_total * 100.0);
            }
        }
        out.set(metric::GPU_TEMP, temp);
        out.set(metric::GPU_POWER, have_power.then_some(power));
        out.set(metric::GPU_CLOCK, clock);

        self.status = SourceStatus::Online;
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_unpolled_and_settles_after_one_poll() {
        let mut nvml = NvmlSource::new();
        assert!(!nvml.status().is_online());
        let mut out = Readings::default();
        nvml.poll(&mut out);
        // With an NVIDIA driver it is online; without one it says why. Either
        // way it only writes what it declares.
        match nvml.status() {
            SourceStatus::Online => {}
            SourceStatus::Unavailable { reason } => assert!(reason.contains("NV"), "{reason}"),
        }
        assert!(out.ids().all(|id| PROVIDES.contains(&id)));
    }

    #[test]
    fn a_failed_load_is_not_retried() {
        let mut nvml = NvmlSource { nvml: Library::Failed, status: SourceStatus::not_yet_polled() };
        assert!(nvml.library().is_none());
        assert!(matches!(nvml.nvml, Library::Failed));
    }
}
