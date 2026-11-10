//! Per-process sampling for the processes widget.
//!
//! Not a metric source: the output is a ranked list, not one number per
//! metric id. Sampled on the poller thread, and only while a widget asks for
//! it, because enumerating every process is the costliest call `HardViz` makes.
//!
//! Network traffic per process is deliberately absent: neither OS exposes it
//! without administrator rights or packet capture. The I/O column is what the
//! OS does report per process: on Windows, all I/O (disk, network and device
//! combined); on Linux, disk I/O only.

use crate::metrics::BYTES_PER_MB;
use crate::num::{count_f64, total_f64};
use crate::nvidia;
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::time::Instant;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Upper bound on rows kept, so a sort by any column still has every
/// realistic candidate without copying thousands of entries per sample.
const MAX_ROWS: usize = 300;

/// One row: every process sharing an executable name, summed.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ProcRow {
    pub name: String,
    /// Share of total CPU capacity, 0-100 (not per-core).
    pub cpu: f64,
    /// GPU (3D/compute) utilisation, 0-100; `None` without an NVIDIA GPU.
    pub gpu: Option<f64>,
    /// Bytes read + written per second, in MB/s.
    pub io: f64,
    pub mem_mb: f64,
    /// Number of processes grouped into this row.
    pub count: usize,
}

/// Columns the widget can sort by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Cpu,
    Gpu,
    Io,
    Memory,
}

impl ProcRow {
    #[must_use]
    pub fn key(&self, k: SortKey) -> f64 {
        match k {
            SortKey::Cpu => self.cpu,
            SortKey::Gpu => self.gpu.unwrap_or(0.0),
            SortKey::Io => self.io,
            SortKey::Memory => self.mem_mb,
        }
    }
}

pub struct ProcessSampler {
    system: System,
    cpus: f64,
    nvml: Option<Nvml>,
    /// NVML returns samples newer than this timestamp (microseconds).
    gpu_seen: u64,
    last: Instant,
}

impl ProcessSampler {
    #[must_use]
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_usage();
        let cpus = count_f64(system.cpus().len().max(1));
        Self { system, cpus, nvml: nvidia::init().ok(), gpu_seen: 0, last: Instant::now() }
    }

    /// Refresh and return process groups ranked by CPU.
    ///
    /// CPU and I/O are deltas between two refreshes, so the first call after
    /// construction reports zero for everything.
    pub fn sample(&mut self) -> Vec<ProcRow> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::new().with_cpu().with_memory().with_disk_usage(),
        );
        let secs = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        let gpu_by_pid = self.gpu_by_pid();

        let mut groups: HashMap<String, ProcRow> = HashMap::new();
        for (pid, p) in self.system.processes() {
            // Windows' idle pseudo-process reports the unused CPU as its own.
            if pid.as_u32() == 0 {
                continue;
            }
            let name = strip_exe_suffix(&p.name().to_string_lossy());
            let row = groups.entry(name.clone()).or_insert(ProcRow {
                name,
                cpu: 0.0,
                gpu: gpu_by_pid.as_ref().map(|_| 0.0),
                io: 0.0,
                mem_mb: 0.0,
                count: 0,
            });
            row.cpu += f64::from(p.cpu_usage()) / self.cpus;
            row.mem_mb += total_f64(p.memory()) / BYTES_PER_MB;
            let du = p.disk_usage();
            row.io += total_f64(du.read_bytes + du.written_bytes) / BYTES_PER_MB / secs;
            if let (Some(g), Some(map)) = (row.gpu.as_mut(), gpu_by_pid.as_ref()) {
                *g += map.get(&pid.as_u32()).copied().unwrap_or(0.0);
            }
            row.count += 1;
        }

        let mut rows: Vec<ProcRow> = groups.into_values().collect();
        rank(&mut rows, SortKey::Cpu);
        rows.truncate(MAX_ROWS);
        rows
    }

    /// Per-process GPU utilisation since the previous call, or `None` when
    /// NVML is unavailable. A process seen in several samples keeps its peak.
    fn gpu_by_pid(&mut self) -> Option<HashMap<u32, f64>> {
        let nvml = self.nvml.as_ref()?;
        let device = nvml.device_by_index(0).ok()?;
        let mut map: HashMap<u32, f64> = HashMap::new();
        // An error here usually means "no samples since then", not failure.
        if let Ok(samples) = device.process_utilization_stats(self.gpu_seen) {
            for s in samples {
                self.gpu_seen = self.gpu_seen.max(s.timestamp);
                let e = map.entry(s.pid).or_insert(0.0);
                *e = e.max(f64::from(s.sm_util));
            }
        }
        Some(map)
    }
}

impl Default for ProcessSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// `chrome.exe` -> `chrome`.
fn strip_exe_suffix(raw: &str) -> String {
    let trimmed = raw.strip_suffix(".exe").or_else(|| raw.strip_suffix(".EXE")).unwrap_or(raw);
    trimmed.to_string()
}

/// Descending by `key`, then by memory, so an idle machine still lists the
/// big memory users rather than an arbitrary order of zeros.
pub fn rank(rows: &mut [ProcRow], key: SortKey) {
    rows.sort_by(|a, b| {
        b.key(key)
            .partial_cmp(&a.key(key))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.mem_mb.partial_cmp(&a.mem_mb).unwrap_or(std::cmp::Ordering::Equal))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, cpu: f64, gpu: f64, io: f64, mem: f64) -> ProcRow {
        ProcRow { name: name.into(), cpu, gpu: Some(gpu), io, mem_mb: mem, count: 1 }
    }

    fn names(rows: &[ProcRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn strips_exe() {
        assert_eq!(strip_exe_suffix("chrome.exe"), "chrome");
        assert_eq!(strip_exe_suffix("firefox"), "firefox");
    }

    #[test]
    fn ranks_by_each_key() {
        let mut rows = vec![
            row("game", 20.0, 90.0, 1.0, 4000.0),
            row("backup", 5.0, 0.0, 80.0, 300.0),
            row("compiler", 60.0, 0.0, 5.0, 900.0),
        ];
        rank(&mut rows, SortKey::Cpu);
        assert_eq!(names(&rows), ["compiler", "game", "backup"]);
        rank(&mut rows, SortKey::Gpu);
        assert_eq!(names(&rows)[0], "game");
        rank(&mut rows, SortKey::Io);
        assert_eq!(names(&rows)[0], "backup");
        rank(&mut rows, SortKey::Memory);
        assert_eq!(names(&rows), ["game", "compiler", "backup"]);
    }

    #[test]
    fn ties_fall_back_to_memory() {
        let mut rows = vec![row("a", 0.0, 0.0, 0.0, 100.0), row("c", 0.0, 0.0, 0.0, 900.0)];
        rank(&mut rows, SortKey::Cpu);
        assert_eq!(names(&rows), ["c", "a"]);
    }
}
