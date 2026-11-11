//! Synthetic hardware for screenshots: three sources named like the real
//! ones, replaying a scripted 2.5 minutes (a render job starting about a
//! minute in) so every chart has a full, lively history. Deterministic: no
//! clock, no randomness, no reads from this machine.

use crate::metrics::metric::{
    CPU_CLOCK, CPU_LOAD, CPU_POWER, CPU_TEMP, CPU_USAGE, DISK_READ, DISK_WRITE, FAN_SPEED, GPU_CLOCK, GPU_POWER,
    GPU_TEMP, GPU_USAGE, NET_DOWN, NET_UP, RAM_TOTAL, RAM_USAGE, RAM_USED, STORAGE_TEMP, SWAP_USAGE, SWAP_USED, UPTIME,
    VRAM_TOTAL, VRAM_USAGE, VRAM_USED,
};
use crate::metrics::MetricId;
use crate::processes::ProcRow;
use crate::source::{MetricSource, Readings, SourceStatus};

const RAM_TOTAL_MB: f64 = 32_768.0;
const VRAM_TOTAL_MB: f64 = 12_288.0;

/// Deterministic noise in -1..1 for integer `n`.
fn hash(n: i64, seed: u64) -> f64 {
    let mut x = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x % 20_001) as f64 / 10_000.0 - 1.0
}

/// Smooth noise in -1..1: `hash` values every `period` samples, eased between.
fn wander(t: f64, period: f64, seed: u64) -> f64 {
    let x = t / period;
    let (i, f) = (x.floor() as i64, x.fract());
    let f = f * f * (3.0 - 2.0 * f);
    hash(i, seed) * (1.0 - f) + hash(i + 1, seed) * f
}

/// 0 before `at`, rising to 1 over about `width` samples.
fn ramp(t: f64, at: f64, width: f64) -> f64 {
    1.0 / (1.0 + (-(t - at) / width).exp())
}

/// Occasional bursts: mostly 0, sometimes up to 1.
fn bursts(t: f64, seed: u64) -> f64 {
    let n = t as i64;
    let h = f64::midpoint(hash(n / 3, seed), 1.0);
    if h > 0.82 {
        ((h - 0.82) / 0.18) * (0.6 + 0.4 * hash(n, seed + 1).abs())
    } else {
        0.0
    }
}

/// The scripted machine at sample `t` (0..300 covers the visible history).
struct Machine(f64);

impl Machine {
    fn load(&self) -> f64 {
        ramp(self.0, 130.0, 8.0)
    }
    fn cpu(&self) -> f64 {
        (18.0 + 56.0 * self.load() + 7.0 * wander(self.0, 5.0, 1) + 3.0 * hash(self.0 as i64, 2)).clamp(1.0, 100.0)
    }
    fn gpu(&self) -> f64 {
        (24.0 + 40.0 * ramp(self.0, 150.0, 6.0) + 9.0 * wander(self.0, 4.0, 3)).clamp(0.0, 100.0)
    }
    fn ram_used(&self) -> f64 {
        13_900.0 + 5_200.0 * self.load() + 300.0 * wander(self.0, 30.0, 4)
    }
    fn vram_used(&self) -> f64 {
        3_100.0 + 2_600.0 * ramp(self.0, 150.0, 6.0) + 120.0 * wander(self.0, 25.0, 5)
    }
}

/// Values for every metric at `t`.
fn reading(t: f64, id: MetricId) -> f64 {
    let m = Machine(t);
    let table = [
        (CPU_USAGE, m.cpu()),
        (CPU_TEMP, 47.0 + 27.0 * ramp(t, 145.0, 18.0) + 1.5 * wander(t, 6.0, 6)),
        (CPU_POWER, 28.0 + 1.35 * m.cpu() + 4.0 * wander(t, 3.0, 7)),
        (CPU_CLOCK, 4_350.0 + 650.0 * m.load() + 220.0 * wander(t, 2.0, 8)),
        (RAM_USED, m.ram_used()),
        (RAM_TOTAL, RAM_TOTAL_MB),
        (RAM_USAGE, m.ram_used() / RAM_TOTAL_MB * 100.0),
        (GPU_USAGE, m.gpu()),
        (GPU_TEMP, 41.0 + 22.0 * ramp(t, 160.0, 20.0) + wander(t, 8.0, 9)),
        (GPU_POWER, 38.0 + 2.6 * m.gpu() + 6.0 * wander(t, 3.0, 10)),
        (GPU_CLOCK, 1_250.0 + 1_300.0 * ramp(t, 148.0, 5.0) + 90.0 * wander(t, 3.0, 11)),
        (VRAM_USED, m.vram_used()),
        (VRAM_TOTAL, VRAM_TOTAL_MB),
        (VRAM_USAGE, m.vram_used() / VRAM_TOTAL_MB * 100.0),
        (FAN_SPEED, 780.0 + 820.0 * ramp(t, 155.0, 15.0) + 40.0 * wander(t, 10.0, 12)),
        (STORAGE_TEMP, 39.0 + 5.0 * ramp(t, 140.0, 30.0) + 0.6 * wander(t, 20.0, 13)),
        (DISK_READ, 1.5 + 160.0 * bursts(t, 14) + 90.0 * (ramp(t, 125.0, 2.0) - ramp(t, 140.0, 2.0))),
        (DISK_WRITE, 0.8 + 45.0 * bursts(t, 15)),
        (NET_DOWN, 0.2 + 18.0 * bursts(t, 16)),
        (NET_UP, 0.05 + 2.5 * bursts(t, 17)),
        // An 8-core machine up for three days, with 8 GB of swap.
        (CPU_LOAD, (m.cpu() / 100.0 * 8.0 + 0.4 * wander(t, 7.0, 18)).max(0.0)),
        (SWAP_USAGE, 11.0 + 7.0 * m.load() + wander(t, 40.0, 19)),
        (SWAP_USED, 8_192.0 * (11.0 + 7.0 * m.load()) / 100.0),
        (UPTIME, 3.0 * 86_400.0 + t),
    ];
    table.iter().find(|(m, _)| *m == id).map_or(0.0, |&(_, v)| v)
}

/// One demo source: a fixed set of metrics, advancing one sample per poll.
struct Demo {
    id: &'static str,
    name: &'static str,
    metrics: &'static [MetricId],
    t: f64,
}

impl MetricSource for Demo {
    fn id(&self) -> &'static str {
        self.id
    }
    fn display_name(&self) -> &'static str {
        self.name
    }
    fn provides(&self) -> &'static [MetricId] {
        self.metrics
    }
    fn poll(&mut self, out: &mut Readings) {
        for &id in self.metrics {
            out.set(id, reading(self.t, id));
        }
        self.t += 1.0;
    }
    fn status(&self) -> SourceStatus {
        SourceStatus::Online
    }
}

/// The demo machine's sources, split the way the real Windows ones are.
pub(super) fn sources() -> Vec<Box<dyn MetricSource>> {
    let demo = |id, name, metrics| Box::new(Demo { id, name, metrics, t: 0.0 }) as Box<dyn MetricSource>;
    vec![
        demo(
            "lhm_http",
            "LibreHardwareMonitor HTTP (CPU temp, power, clock, fans, drives)",
            &[CPU_TEMP, CPU_POWER, CPU_CLOCK, FAN_SPEED, STORAGE_TEMP, DISK_READ, DISK_WRITE],
        ),
        demo(
            "nvml",
            "NVML (NVIDIA GPU)",
            &[GPU_USAGE, GPU_TEMP, VRAM_USAGE, VRAM_USED, VRAM_TOTAL, GPU_POWER, GPU_CLOCK],
        ),
        demo(
            "sysinfo",
            "sysinfo (CPU/RAM)",
            &[CPU_USAGE, RAM_USAGE, RAM_USED, RAM_TOTAL, SWAP_USAGE, SWAP_USED, CPU_LOAD, NET_DOWN, NET_UP, UPTIME],
        ),
    ]
}

/// A plausible process table for the same moment.
pub(super) fn processes() -> Vec<ProcRow> {
    let row =
        |name: &str, cpu, gpu, io, mem_mb, count| ProcRow { name: name.into(), cpu, gpu: Some(gpu), io, mem_mb, count };
    vec![
        row("blender", 41.8, 38.0, 12.4, 6_820.0, 1),
        row("chrome", 7.6, 2.0, 1.3, 3_140.0, 23),
        row("code", 3.9, 0.0, 0.4, 1_210.0, 9),
        row("obs64", 2.7, 11.0, 2.2, 640.0, 1),
        row("steam", 1.1, 0.0, 0.1, 410.0, 4),
        row("hardviz", 0.7, 1.0, 0.0, 118.0, 1),
        row("spotify", 0.6, 0.0, 0.2, 380.0, 5),
    ]
}
