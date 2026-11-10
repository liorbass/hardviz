//! Disk read/write throughput from `/proc/diskstats` (Linux).
//!
//! Counts whole physical disks only. Partitions, loop devices, RAM disks and
//! device-mapper volumes repeat I/O already counted on the underlying disk.

use crate::metrics::{metric, MetricId, BYTES_PER_MB};
use crate::num::total_f64;
use crate::source::{MetricSource, Readings, SourceStatus};
use std::time::Instant;

const PROVIDES: &[MetricId] = &[metric::DISK_READ, metric::DISK_WRITE];
/// `/proc/diskstats` counts in 512-byte sectors regardless of the device.
const SECTOR: f64 = 512.0;

pub struct DiskStatsSource {
    /// Total sectors (read, written) at the previous poll.
    last: Option<(u64, u64, Instant)>,
    status: SourceStatus,
}

impl DiskStatsSource {
    #[must_use]
    pub fn new() -> Self {
        Self { last: None, status: SourceStatus::not_yet_polled() }
    }
}

impl Default for DiskStatsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for DiskStatsSource {
    fn id(&self) -> &'static str {
        "diskstats"
    }

    fn display_name(&self) -> &'static str {
        "Linux /proc/diskstats (disk I/O)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        let text = match std::fs::read_to_string("/proc/diskstats") {
            Ok(t) => t,
            Err(e) => {
                self.status = SourceStatus::Unavailable { reason: format!("cannot read /proc/diskstats ({e})") };
                return;
            }
        };
        let (r, w) = total_sectors(&text);
        let now = Instant::now();
        if let Some((lr, lw, at)) = self.last {
            let secs = now.duration_since(at).as_secs_f64();
            if secs > 0.0 {
                // saturating_sub: counters reset if a disk is removed.
                out.set(metric::DISK_READ, total_f64(r.saturating_sub(lr)) * SECTOR / BYTES_PER_MB / secs);
                out.set(metric::DISK_WRITE, total_f64(w.saturating_sub(lw)) * SECTOR / BYTES_PER_MB / secs);
            }
        }
        self.last = Some((r, w, now));
        self.status = SourceStatus::Online;
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

/// Sum sectors read and written over whole physical disks.
///
/// Line format: `major minor name reads_ok reads_merged sectors_read ms
/// writes_ok writes_merged sectors_written ...`.
fn total_sectors(text: &str) -> (u64, u64) {
    let mut read = 0u64;
    let mut written = 0u64;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 || !is_whole_disk(f[2]) {
            continue;
        }
        read += f[5].parse::<u64>().unwrap_or(0);
        written += f[9].parse::<u64>().unwrap_or(0);
    }
    (read, written)
}

fn is_whole_disk(name: &str) -> bool {
    if ["loop", "ram", "dm-", "zram", "md", "sr", "fd"].iter().any(|p| name.starts_with(p)) {
        return false;
    }
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        // nvme0n1 is a disk, nvme0n1p2 a partition; same for mmcblk0 / mmcblk0p1:
        // a partition ends in `p` + digits, right after the disk's own digits.
        let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
        let is_partition = stem.len() < name.len()
            && stem.strip_suffix('p').is_some_and(|disk| disk.ends_with(|c: char| c.is_ascii_digit()));
        return !is_partition;
    }
    // sda / vdb / xvdc are disks; sda1 is a partition.
    !name.ends_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_disks_only() {
        for d in ["sda", "vdb", "nvme0n1", "nvme1n1", "mmcblk0"] {
            assert!(is_whole_disk(d), "{d}");
        }
        for p in ["sda1", "nvme0n1p2", "mmcblk0p1", "loop0", "dm-0", "zram0", "sr0"] {
            assert!(!is_whole_disk(p), "{p}");
        }
    }

    #[test]
    fn sums_sectors() {
        let text = "\
   8       0 sda 100 0 2000 0 50 0 4000 0 0 0 0
   8       1 sda1 90 0 1800 0 40 0 3000 0 0 0 0
 259       0 nvme0n1 10 0 300 0 5 0 600 0 0 0 0
   7       0 loop0 1 0 99 0 1 0 99 0 0 0 0";
        assert_eq!(total_sectors(text), (2300, 4600));
    }
}
