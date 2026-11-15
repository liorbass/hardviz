//! Synthetic hardware for screenshots: three sources named like the real ones,
//! replaying a scripted 2.5 minutes (a render job starting about two minutes in)
//! so every chart has a full, lively history. Deterministic: no clock, no
//! randomness, no reads from this machine.
//!
//! Past that history the machine switches to a **duty cycle** — one sweep from a
//! deep idle to the redline and back over [`CYCLE`] samples. Only
//! `readme_animation` captures that stretch; `readme_screenshots` stops with the
//! history. The cycle is what lets the GIFs cross the severity colours, which a
//! still frame at one fixed load cannot show.

use crate::metrics::metric::{
    CPU_CLOCK, CPU_LOAD, CPU_POWER, CPU_TEMP, CPU_USAGE, DISK_READ, DISK_WRITE, FAN_SPEED, GPU_CLOCK, GPU_POWER,
    GPU_TEMP, GPU_USAGE, NET_DOWN, NET_UP, RAM_TOTAL, RAM_USAGE, RAM_USED, STORAGE_TEMP, SWAP_USAGE, SWAP_USED, UPTIME,
    VRAM_TOTAL, VRAM_USAGE, VRAM_USED,
};
use crate::metrics::{MetricId, Scale, HISTORY_LEN};
use crate::processes::ProcRow;
use crate::source::{MetricSource, Readings, SourceStatus};

const RAM_TOTAL_MB: f64 = 32_768.0;
const VRAM_TOTAL_MB: f64 = 12_288.0;
const SWAP_TOTAL_MB: f64 = 8_192.0;

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

/// The newest sample the static screenshots contain. `demo_app` and the TUI shot
/// take exactly `HISTORY_LEN` samples from zero, so nothing those stills render
/// can be past this — the duty cycle belongs to the GIFs alone.
const STATIC_LAST: f64 = (HISTORY_LEN - 1) as f64;
/// Samples in one duty cycle, and the samples `readme_animation` captures. Equal
/// so a capture covers exactly one period: its last frame meets its first mid-
/// sweep and the GIF loops without a jump.
const CYCLE: f64 = 72.0;
/// How much the sweep lingers at the ends of its band. Below 1 the phase warp in
/// [`act`] stays monotone; at 0 the cycle is a plain sine, which touches a band's
/// ceiling for a frame or two — not enough for a GIF that has to *show* the
/// change. This dwells there for several, and speeds up through the middle.
const DWELL: f64 = 0.75;
/// How far past its band the sweep reaches before [`act`] clips it. At 1.0 a
/// gauge would touch its ceiling only at the exact crest, which a 72-sample
/// loop may not even step on; overshooting keeps it pinned there instead.
const OVERSHOOT: f64 = 1.15;

/// How far a metric swings during the cycle.
struct Cycle {
    /// Floor and ceiling of the sweep, in the metric's own units, and hard
    /// limits: [`act`] clips to them. Deliberately wide, because
    /// [`Theme::severity_color`](crate::theme::Theme::severity_color) keeps a
    /// metric's own colour until half the scale, fades it to `warning` at three
    /// quarters and to `danger` only at the very top, so a gauge has to cover
    /// most of that range before the colour change reads on screen.
    lo: f64,
    hi: f64,
    /// Ripple added on top, in the metric's own units, so the sweep is not a
    /// perfect sine. Clipped away at the ends of the band, where it would only
    /// disagree with the number the gauge is showing.
    ripple: f64,
}

/// The two usage gauges sweep the whole scale, because they are what the GIF has
/// to show and idle-to-redline is the entire severity colour story. The clip in
/// [`act`] is what makes that safe: the ripple over a 100 % crest would otherwise
/// put "102 %" on a metric whose scale stops at 100. [`OVERSHOOT`] is what parks
/// the gauge on 0 % and 100 % for a dozen frames; this is only the texture that
/// keeps the sweep from reading as a drawn sine.
const CYCLE_CPU: Cycle = Cycle { lo: 0.0, hi: 100.0, ripple: 1.5 };
const CYCLE_GPU: Cycle = Cycle { lo: 0.0, hi: 100.0, ripple: 2.0 };

/// One metric at `t`: the `idle` script throughout the history the stills use,
/// then one [`Cycle`], clipped to that cycle's band.
///
/// The sweep is the sine pair through the point where the script stops, so it
/// leaves the render job at exactly its last value, climbs to `hi` in the first
/// half of the cycle, falls to `lo` in the second, and is back where it started
/// one period later. Nothing `readme_screenshots` renders changes, and no chart
/// grows a cliff at its right edge.
///
/// Metrics need no offset of their own to peak at different times, either: where
/// a metric's scripted value sits inside its band says how far along the sweep it
/// already is. Usage, already high under the render job, crests first; memory and
/// the fan, starting mid-band, a little after; swap, starting near the floor of
/// its band, last.
fn act(t: f64, idle: impl Fn(f64) -> f64, c: Cycle) -> f64 {
    if t <= STATIC_LAST {
        return idle(t);
    }
    let centre = (c.hi + c.lo) / 2.0;
    let amp = ((c.hi - c.lo) / 2.0).max(f64::EPSILON);
    let a = ((idle(STATIC_LAST) - centre) / amp).clamp(-1.0, 1.0);
    let b = (1.0 - a * a).sqrt();
    let phase = (t - STATIC_LAST) / CYCLE * std::f64::consts::TAU;
    // Slow at the ends of the band, quick through the middle, and still exactly
    // `phase` one period later — and zero at the seam, which is what keeps the
    // first captured frame on the value the history ends at.
    let x = phase + DWELL * (2.0 * phase).sin() / 2.0;
    let sweep = OVERSHOOT * (a * x.cos() + b * x.sin());
    (centre + amp * sweep.clamp(-1.0, 1.0) + c.ripple * wander(t, 5.0, 21)).clamp(c.lo, c.hi)
}

/// Overall load, 0..1: one render job, starting about two minutes in.
fn load(t: f64) -> f64 {
    ramp(t, 130.0, 8.0)
}

fn cpu(t: f64) -> f64 {
    (18.0 + 56.0 * load(t) + 7.0 * wander(t, 5.0, 1) + 3.0 * hash(t as i64, 2)).clamp(1.0, 100.0)
}

fn gpu(t: f64) -> f64 {
    (24.0 + 40.0 * ramp(t, 150.0, 6.0) + 9.0 * wander(t, 4.0, 3)).clamp(0.0, 100.0)
}

fn ram_used(t: f64) -> f64 {
    13_900.0 + 5_200.0 * load(t) + 300.0 * wander(t, 30.0, 4)
}

fn vram_used(t: f64) -> f64 {
    3_100.0 + 2_600.0 * ramp(t, 150.0, 6.0) + 120.0 * wander(t, 25.0, 5)
}

/// Values for every metric at `t`.
fn reading(t: f64, id: MetricId) -> f64 {
    // Metrics shown both as a percentage and as an amount are evaluated once, so
    // the gauge and the "12.4 GB / 32 GB" label can never disagree. The amount
    // follows its percentage through the cycle instead of sweeping on its own:
    // two numbers that disagreed mid-sweep would read as a bug, not a busy
    // machine.
    let cpu_now = act(t, cpu, CYCLE_CPU);
    let gpu_now = act(t, gpu, CYCLE_GPU);
    let ram_pct = act(t, |t| ram_used(t) / RAM_TOTAL_MB * 100.0, Cycle { lo: 20.0, hi: 99.0, ripple: 0.8 });
    let vram_pct = act(t, |t| vram_used(t) / VRAM_TOTAL_MB * 100.0, Cycle { lo: 10.0, hi: 98.0, ripple: 0.7 });
    let swap_pct = act(t, |t| 11.0 + 7.0 * load(t) + wander(t, 40.0, 19), Cycle { lo: 3.0, hi: 58.0, ripple: 0.6 });
    let held = |past: f64, from_pct: f64, total: f64| if t <= STATIC_LAST { past } else { total * from_pct / 100.0 };

    let table = [
        (CPU_USAGE, cpu_now),
        (
            CPU_TEMP,
            act(
                t,
                |t| 47.0 + 27.0 * ramp(t, 145.0, 18.0) + 1.5 * wander(t, 6.0, 6),
                Cycle { lo: 40.0, hi: 93.0, ripple: 0.8 },
            ),
        ),
        // Power tracks usage, so its script re-derives usage from the `t` it is
        // handed rather than closing over the frame being drawn: `act` asks for
        // the end of the history to anchor the sweep.
        (
            CPU_POWER,
            act(
                t,
                |t| 28.0 + 1.35 * act(t, cpu, CYCLE_CPU) + 4.0 * wander(t, 3.0, 7),
                Cycle { lo: 30.0, hi: 172.0, ripple: 4.0 },
            ),
        ),
        (
            CPU_CLOCK,
            act(
                t,
                |t| 4_350.0 + 650.0 * load(t) + 220.0 * wander(t, 2.0, 8),
                Cycle { lo: 1_800.0, hi: 5_300.0, ripple: 110.0 },
            ),
        ),
        (RAM_USED, held(ram_used(t), ram_pct, RAM_TOTAL_MB)),
        (RAM_TOTAL, RAM_TOTAL_MB),
        (RAM_USAGE, ram_pct),
        (GPU_USAGE, gpu_now),
        (
            GPU_TEMP,
            act(
                t,
                |t| 41.0 + 22.0 * ramp(t, 160.0, 20.0) + wander(t, 8.0, 9),
                Cycle { lo: 37.0, hi: 89.0, ripple: 1.0 },
            ),
        ),
        (
            GPU_POWER,
            act(
                t,
                |t| 38.0 + 2.6 * act(t, gpu, CYCLE_GPU) + 6.0 * wander(t, 3.0, 10),
                Cycle { lo: 40.0, hi: 300.0, ripple: 8.0 },
            ),
        ),
        (
            GPU_CLOCK,
            act(
                t,
                |t| 1_250.0 + 1_300.0 * ramp(t, 148.0, 5.0) + 90.0 * wander(t, 3.0, 11),
                Cycle { lo: 900.0, hi: 2_700.0, ripple: 60.0 },
            ),
        ),
        (VRAM_USED, held(vram_used(t), vram_pct, VRAM_TOTAL_MB)),
        (VRAM_TOTAL, VRAM_TOTAL_MB),
        (VRAM_USAGE, vram_pct),
        (
            FAN_SPEED,
            act(
                t,
                |t| 780.0 + 820.0 * ramp(t, 155.0, 15.0) + 40.0 * wander(t, 10.0, 12),
                Cycle { lo: 620.0, hi: 2_600.0, ripple: 25.0 },
            ),
        ),
        (
            STORAGE_TEMP,
            act(
                t,
                |t| 39.0 + 5.0 * ramp(t, 140.0, 30.0) + 0.6 * wander(t, 20.0, 13),
                Cycle { lo: 35.0, hi: 54.0, ripple: 0.4 },
            ),
        ),
        (DISK_READ, 1.5 + 160.0 * bursts(t, 14) + 90.0 * (ramp(t, 125.0, 2.0) - ramp(t, 140.0, 2.0))),
        (DISK_WRITE, 0.8 + 45.0 * bursts(t, 15)),
        (NET_DOWN, 0.2 + 18.0 * bursts(t, 16)),
        (NET_UP, 0.05 + 2.5 * bursts(t, 17)),
        // An 8-core machine up for three days, with 8 GB of swap.
        (CPU_LOAD, (cpu_now / 100.0 * 8.0 + 0.4 * wander(t, 7.0, 18)).max(0.0)),
        (SWAP_USAGE, swap_pct),
        (SWAP_USED, held(SWAP_TOTAL_MB * (11.0 + 7.0 * load(t)) / 100.0, swap_pct, SWAP_TOTAL_MB)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;

    /// The samples one GIF captures: a whole duty cycle, beginning with the
    /// first sample past the static history.
    fn frames() -> impl Iterator<Item = i64> {
        HISTORY_LEN as i64..HISTORY_LEN as i64 + CYCLE as i64
    }

    /// The point of the cycle, in the units the widgets actually colour by: the
    /// captured frames have to fill a gauge enough to fade it through `warning`
    /// and nearly to `danger`, and empty enough to show the metric's own colour.
    #[test]
    fn the_animated_sweep_crosses_every_severity_band() {
        let theme = ThemeName::Cyberpunk.theme();
        for id in [CPU_USAGE, GPU_USAGE, RAM_USAGE, VRAM_USAGE] {
            let def = id.def();
            let range = def.scale.range(frames().map(|t| reading(t as f64, id)));
            let fills: Vec<f64> = frames().map(|t| Scale::fraction(reading(t as f64, id), range)).collect();
            let colour = |f: f64| theme.severity_color(theme.metric_color(def.kind), f as f32);
            assert!(fills.iter().any(|&f| f <= 0.5), "{id:?} never idles: fills {fills:?}");
            assert!(fills.iter().any(|&f| f > 0.5 && f <= 0.75), "{id:?} never fades through warning");
            assert!(fills.iter().any(|&f| f > 0.9), "{id:?} never gets near danger: fills {fills:?}");
            let low = colour(*fills.iter().min_by(|a, b| a.total_cmp(b)).unwrap());
            let high = colour(*fills.iter().max_by(|a, b| a.total_cmp(b)).unwrap());
            assert_eq!(low, theme.metric_color(def.kind), "{id:?} idle colour");
            assert_ne!(low, high, "{id:?} colour never changes across the sweep");
        }
    }

    /// Lowest and highest reading a metric reaches across the captured frames.
    fn span(id: MetricId) -> (f64, f64) {
        frames()
            .map(|t| reading(t as f64, id))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
    }

    /// What the README promises, in the units a reader actually sees: the usage
    /// gauges empty to nothing and fill the whole arc, and hold there long enough
    /// to be noticed rather than flashing past.
    #[test]
    fn the_usage_gauges_run_the_full_scale() {
        for id in [CPU_USAGE, GPU_USAGE] {
            let (lo, hi) = span(id);
            assert!(lo <= 0.1, "{id:?} never idles: lowest {lo:.1} %");
            assert!(hi >= 99.9, "{id:?} never reaches the redline: highest {hi:.1} %");
            let bottoms = frames().filter(|&t| reading(t as f64, id) <= 0.5).count();
            let tops = frames().filter(|&t| reading(t as f64, id) >= 99.5).count();
            assert!(bottoms >= 6 && tops >= 6, "{id:?} is at an end of its scale for {tops} / {bottoms} frames");
        }
    }

    /// The other half of the same change: a percentage on a 0..100 scale has to
    /// stay inside it, ripple and clipping notwithstanding.
    #[test]
    fn no_frame_reads_past_the_scale() {
        for id in [CPU_USAGE, GPU_USAGE, RAM_USAGE, VRAM_USAGE, SWAP_USAGE] {
            let (lo, hi) = span(id);
            assert!((0.0..=100.0).contains(&hi), "{id:?} peaks at {hi:.1} %");
            assert!((0.0..=100.0).contains(&lo), "{id:?} bottoms at {lo:.1} %");
        }
    }

    /// No cliff where the history ends and the cycle starts: the step across the
    /// seam is no more abrupt than the loudest step of the sweep itself.
    #[test]
    fn the_cycle_leaves_the_scripted_history_continuously() {
        for id in [CPU_USAGE, GPU_USAGE, RAM_USAGE, VRAM_USAGE, CPU_TEMP, GPU_TEMP, FAN_SPEED, CPU_CLOCK] {
            let seam = (reading(STATIC_LAST + 1.0, id) - reading(STATIC_LAST, id)).abs();
            let loudest =
                frames().map(|t| (reading((t + 1) as f64, id) - reading(t as f64, id)).abs()).fold(0.0, f64::max);
            assert!(
                seam <= loudest * 1.5 + 1e-9,
                "{id:?} steps {seam:.1} at the seam, past the sweep's own {loudest:.1}"
            );
        }
    }

    /// The GIF loops, so the frame just past the capture must repeat its first
    /// one: the capture is exactly one period.
    #[test]
    fn one_cycle_returns_where_it_started() {
        for id in [CPU_USAGE, GPU_USAGE, RAM_USAGE, CPU_TEMP, FAN_SPEED] {
            let a = reading(HISTORY_LEN as f64, id);
            let b = reading(HISTORY_LEN as f64 + CYCLE, id);
            assert!((a - b).abs() < (a.abs() * 0.05).max(1.5), "{id:?}: {a} vs {b}");
        }
    }

    /// The static screenshots are the reason for the split: the half of the
    /// script they read must still produce the numbers they were chosen for.
    #[test]
    fn the_static_history_is_unchanged() {
        // Idle before the job, running the job by the end of the history.
        assert!(reading(60.0, CPU_USAGE) < 30.0);
        assert!((reading(STATIC_LAST, RAM_USAGE) - 58.0).abs() < 2.0);
        assert!((reading(STATIC_LAST, CPU_TEMP) - 74.0).abs() < 2.0);
        assert!((reading(STATIC_LAST, VRAM_USAGE) - 46.0).abs() < 2.0);
        // Used and percentage stay one number, not two that drift apart.
        for t in [10.0, 200.0, STATIC_LAST] {
            let used = reading(t, RAM_USED);
            assert!((reading(t, RAM_USAGE) - used / RAM_TOTAL_MB * 100.0).abs() < 1e-9);
        }
        for t in [310.0, 350.0, 370.0] {
            let used = reading(t, RAM_USED);
            assert!((reading(t, RAM_USAGE) - used / RAM_TOTAL_MB * 100.0).abs() < 1e-9, "cycling side");
        }
    }
}
