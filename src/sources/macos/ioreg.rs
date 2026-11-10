//! Disk throughput, GPU utilisation and GPU memory on macOS, from the I/O Kit
//! registry.
//!
//! Reads the same counters Activity Monitor shows, through `/usr/sbin/ioreg`
//! (part of every macOS install; about 15 ms per call, no root needed):
//!
//! - `IOBlockStorageDriver` → `Statistics` → `Bytes (Read)` / `Bytes (Write)`,
//!   cumulative per disk since boot.
//! - `IOAccelerator` → `PerformanceStatistics` → `Device Utilization %`, the
//!   GPU's busy share (Apple Silicon and the AMD GPUs of Intel Macs), and
//!   `In use system memory`, what the GPU has allocated. Apple Silicon has no
//!   VRAM: the GPU shares the system RAM, so "VRAM" here is the GPU's part of
//!   unified memory, out of all of it.
//!
//! Mounted disk images have block drivers of their own, so reading from a
//! `.dmg` is counted both there and on the disk holding it.

use crate::metrics::{metric, MetricId, BYTES_PER_MB};
use crate::num::total_f64;
use crate::source::{MetricSource, Readings, SourceStatus};
use std::time::Instant;
use sysinfo::System;

const PROVIDES: &[MetricId] = &[
    metric::DISK_READ,
    metric::DISK_WRITE,
    metric::GPU_USAGE,
    metric::VRAM_USED,
    metric::VRAM_TOTAL,
    metric::VRAM_USAGE,
];

pub struct IoregSource {
    /// Total bytes (read, written) at the previous poll.
    last_disk: Option<(u64, u64, Instant)>,
    /// Physical memory in bytes, read on the first poll (unified memory: the
    /// GPU's ceiling too).
    ram_total: Option<u64>,
    status: SourceStatus,
}

impl IoregSource {
    #[must_use]
    pub fn new() -> Self {
        Self { last_disk: None, ram_total: None, status: SourceStatus::not_yet_polled() }
    }
}

impl Default for IoregSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for IoregSource {
    fn id(&self) -> &'static str {
        "ioreg"
    }

    fn display_name(&self) -> &'static str {
        "macOS I/O Kit (disk I/O, GPU usage/memory)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        let disks = super::ioreg(&["-r", "-d", "1", "-c", "IOBlockStorageDriver"]);
        let gpus = super::ioreg(&["-r", "-d", "1", "-c", "IOAccelerator"]);

        if let Ok(text) = &disks {
            let (r, w) = (sum_values(text, "Bytes (Read)"), sum_values(text, "Bytes (Write)"));
            let now = Instant::now();
            if let Some((lr, lw, at)) = self.last_disk {
                let secs = now.duration_since(at).as_secs_f64();
                if secs > 0.0 {
                    // saturating_sub: the totals drop when a disk is ejected.
                    out.set(metric::DISK_READ, total_f64(r.saturating_sub(lr)) / BYTES_PER_MB / secs);
                    out.set(metric::DISK_WRITE, total_f64(w.saturating_sub(lw)) / BYTES_PER_MB / secs);
                }
            }
            self.last_disk = Some((r, w, now));
        }
        if let Ok(text) = &gpus {
            // Several GPUs (an Intel Mac's iGPU and dGPU): show the busiest.
            out.set(
                metric::GPU_USAGE,
                values(text, "Device Utilization %").into_iter().max().map(|v| total_f64(v.min(100))),
            );
            let ram_total = *self.ram_total.get_or_insert_with(|| {
                let mut system = System::new();
                system.refresh_memory();
                system.total_memory()
            });
            // Only an integrated GPU shares system RAM; a dGPU's own memory is
            // not reported here, so its "In use" figure would be meaningless
            // against RAM. Apple Silicon has exactly one accelerator.
            if let [used] = values(text, "In use system memory")[..] {
                if ram_total > 0 && cfg!(target_arch = "aarch64") {
                    out.set(metric::VRAM_USED, total_f64(used) / BYTES_PER_MB);
                    out.set(metric::VRAM_TOTAL, total_f64(ram_total) / BYTES_PER_MB);
                    out.set(metric::VRAM_USAGE, total_f64(used) / total_f64(ram_total) * 100.0);
                }
            }
        }

        self.status = match (disks, gpus) {
            (Err(e), Err(_)) => SourceStatus::Unavailable { reason: e },
            _ => SourceStatus::Online,
        };
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

/// Every numeric value of `key` in ioreg's text output, where dictionary
/// entries look like `"Bytes (Read)"=383026995200`.
fn values(text: &str, key: &str) -> Vec<u64> {
    let needle = format!("\"{key}\"=");
    text.match_indices(&needle)
        .filter_map(|(at, m)| {
            let rest = &text[at + m.len()..];
            let digits = rest.find(|c: char| !c.is_ascii_digit()).map_or(rest, |end| &rest[..end]);
            digits.parse().ok()
        })
        .collect()
}

fn sum_values(text: &str, key: &str) -> u64 {
    values(text, key).into_iter().fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISKS: &str = r#"
+-o IOBlockStorageDriver  <class IOBlockStorageDriver, id 0x100000a2c, registered, matched, active, busy 0 (0 ms), retain 7>
    {
      "IOClass" = "IOBlockStorageDriver"
      "Statistics" = {"Operations (Write)"=0,"Bytes (Read)"=0,"Errors (Write)"=0,"Bytes (Write)"=0,"Operations (Read)"=0}
    }
+-o IOBlockStorageDriver  <class IOBlockStorageDriver, id 0x100000b31, registered, matched, active, busy 0 (0 ms), retain 7>
    {
      "Statistics" = {"Operations (Write)"=9360176,"Bytes (Read)"=383026995200,"Total Time (Write)"=590284154848,"Bytes (Write)"=207457230848}
    }
+-o IOBlockStorageDriver  <class IOBlockStorageDriver, id 0x100000c00>
    {
      "Statistics" = {"Bytes (Read)"=1000,"Bytes (Write)"=24}
    }
"#;

    const GPU: &str = r#"
+-o AGXAcceleratorG16X  <class AGXAcceleratorG16X, id 0x1000003d5, registered, matched, active, busy 0 (2 ms), retain 85>
    {
      "model" = "Apple M4 Pro"
      "PerformanceStatistics" = {"In use system memory (driver)"=0,"Tiler Utilization %"=6,"Renderer Utilization %"=24,"Device Utilization %"=24,"In use system memory"=1435254784}
    }
"#;

    #[test]
    fn sums_disk_bytes() {
        assert_eq!(sum_values(DISKS, "Bytes (Read)"), 383_026_996_200);
        assert_eq!(sum_values(DISKS, "Bytes (Write)"), 207_457_230_872);
    }

    #[test]
    fn reads_gpu_utilisation_not_its_sub_units() {
        assert_eq!(values(GPU, "Device Utilization %"), [24]);
        // `"In use system memory (driver)"` is a different key.
        assert_eq!(values(GPU, "In use system memory"), [1_435_254_784]);
        assert!(values(GPU, "Missing").is_empty());
    }

    #[test]
    fn runs_on_this_mac() {
        // ioreg ships with macOS; both classes exist on every supported Mac.
        let mut source = IoregSource::new();
        let mut out = Readings::default();
        source.poll(&mut out);
        assert!(source.status().is_online(), "{}", source.status());
    }
}
