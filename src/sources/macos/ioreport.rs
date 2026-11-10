//! CPU/GPU clocks and power on Apple Silicon, from `IOReport`.
//!
//! `IOReport` (`/usr/lib/libIOReport.dylib`) holds the counters `powermetrics`
//! prints; unlike `powermetrics` it needs no root. Each poll takes a sample of
//! three channel groups and diffs it with the previous one:
//!
//! - **Clocks.** `CPU Stats` → `CPU Core Performance States` (one channel per
//!   core, `ECPU*` / `PCPU*`) and `GPU Stats` → `GPU Performance States`
//!   (`GPUPH`) give the time spent in each performance state. The states map,
//!   in order, onto the chip's DVFS frequency tables, which the `pmgr` device
//!   publishes in the I/O Kit registry (`voltage-states1-sram` E-cores,
//!   `voltage-states5-sram` P-cores, `voltage-states9` GPU). The clock is the
//!   mean frequency over the time the cores were *running*; idle and
//!   power-gated time is left out, so a mostly idle CPU shows the clock it
//!   runs at when it wakes. A GPU that was off the whole interval reads 0 MHz.
//! - **Power.** `Energy Model` → `GPU Energy` / `CPU Energy`, joules over the
//!   measured interval. Recent macOS releases (seen on macOS 27) stop
//!   updating the CPU counters for clients without root, so CPU power is only
//!   reported while its counter is moving: a frozen counter is "unavailable",
//!   never "0 W".

use super::ioreg;
use crate::metrics::{metric, MetricId};
use crate::source::{MetricSource, Readings, SourceStatus};
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation_sys::string::CFStringRef;
use std::ffi::c_void;
use std::time::Instant;

const PROVIDES: &[MetricId] = &[metric::CPU_CLOCK, metric::GPU_CLOCK, metric::CPU_POWER, metric::GPU_POWER];

/// Below this a CPU clock is a misread (the slowest E-core state is ~600 MHz).
const MIN_CPU_MHZ: f64 = 100.0;

/// One channel of a delta sample, copied out of `IOReport`'s dictionaries.
#[derive(Debug, Clone, PartialEq)]
enum Channel {
    /// An `Energy Model` counter: energy spent in the interval, in `unit`.
    Energy { name: String, unit: String, value: i64 },
    /// A performance-state channel: time spent in each state, in order.
    States { name: String, states: Vec<(String, i64)> },
}

/// The DVFS frequency tables, in MHz, lowest state first.
#[derive(Debug, Default, Clone, PartialEq)]
struct Tables {
    ecpu: Vec<f64>,
    pcpu: Vec<f64>,
    gpu: Vec<f64>,
}

impl Tables {
    /// From `ioreg -r -n pmgr -d 1` output; empty tables where a key is missing.
    fn parse(pmgr: &str) -> Self {
        Self {
            ecpu: dvfs_mhz(pmgr, "voltage-states1-sram"),
            pcpu: dvfs_mhz(pmgr, "voltage-states5-sram"),
            gpu: dvfs_mhz(pmgr, "voltage-states9"),
        }
    }
}

/// A DVFS table from ioreg's text, where it is printed as hex data:
/// `"voltage-states5-sram" = <e0391300 16030000 ...>` without the spaces.
/// Each 8-byte entry is a little-endian frequency and voltage. Zero entries
/// (the GPU's "off" state) are dropped. M1-M3 tables are in Hz, M4 CPU
/// tables in kHz; no CPU or GPU state is below 100 MHz or above 100 GHz, so
/// the magnitude tells them apart.
fn dvfs_mhz(text: &str, key: &str) -> Vec<f64> {
    let needle = format!("\"{key}\" = <");
    let Some(start) = text.find(&needle).map(|at| at + needle.len()) else { return Vec::new() };
    let hex = text[start..].split('>').next().unwrap_or_default();
    let bytes: Vec<u8> =
        (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()).collect();
    let (entries, _) = bytes.as_chunks::<8>();
    let freqs: Vec<f64> = entries
        .iter()
        .map(|entry| f64::from(u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]])))
        .filter(|f| *f > 0.0)
        .collect();
    let per_mhz = if freqs.iter().any(|f| *f >= 1e8) { 1e6 } else { 1e3 };
    freqs.iter().map(|f| f / per_mhz).collect()
}

/// (Σ time × MHz, Σ running time) for one channel's performance states.
/// States before the first running one (`IDLE`, `DOWN`, `OFF`) are not
/// running; the rest pair up with `freqs` in order.
fn weigh(states: &[(String, i64)], freqs: &[f64]) -> (f64, f64) {
    let running = states.iter().skip_while(|(name, _)| matches!(name.as_str(), "IDLE" | "DOWN" | "OFF"));
    running.zip(freqs).fold((0.0, 0.0), |(sum, time), ((_, residency), mhz)| {
        // Residencies are tick counts: far below 2^53, exact as f64.
        #[allow(clippy::cast_precision_loss)]
        let t = (*residency).max(0) as f64;
        (sum + t * mhz, time + t)
    })
}

/// Mean CPU clock over the time any core was running, in MHz.
fn cpu_clock(channels: &[Channel], tables: &Tables) -> Option<f64> {
    let (sum, time) = channels
        .iter()
        .filter_map(|c| match c {
            Channel::States { name, states } if name.starts_with("ECPU") => Some(weigh(states, &tables.ecpu)),
            Channel::States { name, states } if name.starts_with("PCPU") => Some(weigh(states, &tables.pcpu)),
            _ => None,
        })
        .fold((0.0, 0.0), |(s, t), (cs, ct)| (s + cs, t + ct));
    (time > 0.0).then(|| sum / time).filter(|mhz| *mhz >= MIN_CPU_MHZ)
}

/// Mean GPU clock while it was running; 0 when it was off throughout.
fn gpu_clock(channels: &[Channel], tables: &Tables) -> Option<f64> {
    if tables.gpu.is_empty() {
        return None;
    }
    channels.iter().find_map(|c| match c {
        Channel::States { name, states } if name == "GPUPH" => {
            let (sum, time) = weigh(states, &tables.gpu);
            Some(if time > 0.0 { sum / time } else { 0.0 })
        }
        _ => None,
    })
}

/// The energy a channel counted this interval, in joules.
fn joules(channels: &[Channel], wanted: &str) -> Option<f64> {
    channels.iter().find_map(|c| match c {
        Channel::Energy { name, unit, value } if name == wanted => {
            let scale = match unit.as_str() {
                "mJ" => 1e-3,
                "uJ" | "µJ" => 1e-6,
                "nJ" => 1e-9,
                _ => return None,
            };
            // Energy deltas are far below 2^53.
            #[allow(clippy::cast_precision_loss)]
            Some((*value).max(0) as f64 * scale)
        }
        _ => None,
    })
}

/// Turns an energy counter into watts, but only while it is live: it must
/// have moved this interval and the one before. A counter macOS has frozen
/// never reports, and one that moves in rare jumps never shows a spike.
///
/// The CPU always draws power, so for it "did not move" means "frozen". The
/// GPU can be power-gated for a whole interval, so once its counter has been
/// seen live, a zero is a real 0 W.
struct PowerMeter {
    zero_is_real: bool,
    moved_last_time: bool,
    seen_live: bool,
}

impl PowerMeter {
    fn cpu() -> Self {
        Self { zero_is_real: false, moved_last_time: false, seen_live: false }
    }

    fn gpu() -> Self {
        Self { zero_is_real: true, ..Self::cpu() }
    }

    fn watts(&mut self, joules: Option<f64>, secs: f64) -> Option<f64> {
        let moved = joules.is_some_and(|j| j > 0.0);
        let live = moved && self.moved_last_time;
        self.moved_last_time = moved;
        self.seen_live |= live;
        let idle = self.zero_is_real && self.seen_live && joules == Some(0.0);
        ((live || idle) && secs > 0.0).then(|| joules.unwrap_or_default() / secs)
    }
}

pub struct IoReportSource {
    report: Option<Subscription>,
    tables: Tables,
    last: Option<(Owned, Instant)>,
    cpu_power: PowerMeter,
    gpu_power: PowerMeter,
    /// Subscribing failed once: don't retry every poll.
    failed: bool,
    status: SourceStatus,
}

impl IoReportSource {
    /// Cheap: the subscription is made on the first poll.
    #[must_use]
    pub fn new() -> Self {
        Self {
            report: None,
            tables: Tables::default(),
            last: None,
            cpu_power: PowerMeter::cpu(),
            gpu_power: PowerMeter::gpu(),
            failed: false,
            status: SourceStatus::not_yet_polled(),
        }
    }
}

impl Default for IoReportSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for IoReportSource {
    fn id(&self) -> &'static str {
        "ioreport"
    }

    fn display_name(&self) -> &'static str {
        "Apple IOReport (CPU/GPU clock, power)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        if self.report.is_none() && !self.failed {
            match Subscription::new() {
                Ok(report) => {
                    self.report = Some(report);
                    match ioreg(&["-r", "-n", "pmgr", "-d", "1"]) {
                        Ok(text) => self.tables = Tables::parse(&text),
                        Err(e) => crate::warn!("[ioreport] no DVFS tables, clocks unavailable ({e})"),
                    }
                }
                Err(reason) => {
                    self.failed = true;
                    self.status = SourceStatus::Unavailable { reason };
                }
            }
        }
        let Some(report) = &self.report else { return };
        let Some(sample) = report.sample() else {
            self.status = SourceStatus::Unavailable { reason: "IOReport returned no sample".into() };
            return;
        };
        let now = Instant::now();

        // The first poll only primes the baseline.
        if let Some((previous, at)) = self.last.take() {
            let channels = delta_channels(&previous, &sample);
            let secs = now.duration_since(at).as_secs_f64();
            out.set(metric::CPU_CLOCK, cpu_clock(&channels, &self.tables));
            out.set(metric::GPU_CLOCK, gpu_clock(&channels, &self.tables));
            out.set(metric::CPU_POWER, self.cpu_power.watts(joules(&channels, "CPU Energy"), secs));
            out.set(metric::GPU_POWER, self.gpu_power.watts(joules(&channels, "GPU Energy"), secs));
        }
        self.last = Some((sample, now));
        self.status = SourceStatus::Online;
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

// ---------------------------------------------------------------- IOReport access

#[link(name = "IOReport", kind = "dylib")]
extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: CFStringRef,
        subgroup: CFStringRef,
        a: u64,
        b: u64,
        c: u64,
    ) -> CFMutableDictionaryRef;
    fn IOReportMergeChannels(into: CFMutableDictionaryRef, from: CFMutableDictionaryRef, unused: CFTypeRef);
    fn IOReportCreateSubscription(
        unused: *const c_void,
        channels: CFMutableDictionaryRef,
        subscribed: *mut CFMutableDictionaryRef,
        channel_id: u64,
        unused2: CFTypeRef,
    ) -> CFTypeRef;
    fn IOReportCreateSamples(
        subscription: CFTypeRef,
        channels: CFMutableDictionaryRef,
        unused: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        previous: CFDictionaryRef,
        current: CFDictionaryRef,
        unused: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportChannelGetGroup(channel: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetChannelName(channel: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(channel: CFDictionaryRef) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(channel: CFDictionaryRef, index: i32) -> i64;
    fn IOReportStateGetCount(channel: CFDictionaryRef) -> i32;
    fn IOReportStateGetNameForIndex(channel: CFDictionaryRef, index: i32) -> CFStringRef;
    fn IOReportStateGetResidency(channel: CFDictionaryRef, index: i32) -> i64;
}

/// A Core Foundation object this code owns (a "Create" or "Copy" result),
/// released on drop.
struct Owned(CFTypeRef);

impl Owned {
    fn new(ptr: CFTypeRef) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: a non-null object this code holds the only reference to.
        unsafe { CFRelease(self.0) }
    }
}

// SAFETY: Core Foundation retain/release is thread-safe, and the source that
// holds these objects is used by one thread at a time (the poller's).
unsafe impl Send for Owned {}

/// The subscription to the channels this source reads.
struct Subscription {
    subscription: Owned,
    channels: Owned,
}

impl Subscription {
    fn new() -> Result<Self, String> {
        const GROUPS: [(&str, Option<&str>); 3] = [
            ("Energy Model", None),
            ("CPU Stats", Some("CPU Core Performance States")),
            ("GPU Stats", Some("GPU Performance States")),
        ];
        // SAFETY: IOReport's documented-by-use C API; every Copy/Create result
        // is owned by an `Owned` and released once.
        unsafe {
            let copy = |group: &str, subgroup: Option<&str>| {
                let group = CFString::new(group);
                let subgroup = subgroup.map(CFString::new);
                let subgroup_ref = subgroup.as_ref().map_or(std::ptr::null(), TCFType::as_concrete_TypeRef);
                Owned::new(IOReportCopyChannelsInGroup(group.as_concrete_TypeRef(), subgroup_ref, 0, 0, 0).cast())
            };
            let mut groups = GROUPS.iter().filter_map(|(g, s)| copy(g, *s));
            let requested = groups.next().ok_or("IOReport has none of the Energy Model, CPU or GPU Stats channels")?;
            for more in groups {
                IOReportMergeChannels(requested.0.cast_mut().cast(), more.0.cast_mut().cast(), std::ptr::null());
            }
            let mut subscribed: CFMutableDictionaryRef = std::ptr::null_mut();
            let subscription = IOReportCreateSubscription(
                std::ptr::null(),
                requested.0.cast_mut().cast(),
                &raw mut subscribed,
                0,
                std::ptr::null(),
            );
            let channels = Owned::new(subscribed.cast_const().cast());
            match (Owned::new(subscription), channels) {
                (Some(subscription), Some(channels)) => Ok(Self { subscription, channels }),
                _ => Err("cannot subscribe to IOReport channels".into()),
            }
        }
    }

    fn sample(&self) -> Option<Owned> {
        // SAFETY: both arguments are live objects owned by `self`.
        Owned::new(
            unsafe { IOReportCreateSamples(self.subscription.0, self.channels.0.cast_mut().cast(), std::ptr::null()) }
                .cast(),
        )
    }
}

/// The channels of the difference between two samples, copied into Rust data.
fn delta_channels(previous: &Owned, current: &Owned) -> Vec<Channel> {
    // SAFETY: the samples are live; the delta is owned and outlives every
    // borrowed (Get-rule) reference read from it below.
    unsafe {
        let Some(delta) =
            Owned::new(IOReportCreateSamplesDelta(previous.0.cast(), current.0.cast(), std::ptr::null()).cast())
        else {
            return Vec::new();
        };
        let key = CFString::from_static_string("IOReportChannels");
        let array: CFArrayRef = CFDictionaryGetValue(delta.0.cast(), key.as_concrete_TypeRef().cast()).cast();
        if array.is_null() {
            return Vec::new();
        }
        (0..CFArrayGetCount(array))
            .map(|i| {
                let channel: CFDictionaryRef = CFArrayGetValueAtIndex(array, i).cast();
                let name = string(IOReportChannelGetChannelName(channel));
                if string(IOReportChannelGetGroup(channel)) == "Energy Model" {
                    let unit = string(IOReportChannelGetUnitLabel(channel));
                    return Channel::Energy { name, unit, value: IOReportSimpleGetIntegerValue(channel, 0) };
                }
                let states = (0..IOReportStateGetCount(channel))
                    .map(|s| (string(IOReportStateGetNameForIndex(channel, s)), IOReportStateGetResidency(channel, s)))
                    .collect();
                Channel::States { name, states }
            })
            .collect()
    }
}

/// A borrowed `CFString` as a Rust string; empty for null.
fn string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: a valid CFString borrowed from a live dictionary.
    unsafe { CFString::wrap_under_get_rule(s) }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The M4 Pro's tables, as `ioreg -r -n pmgr -d 1` prints them (cut).
    const PMGR: &str = r#"
      "voltage-states1-sram" = <60900f0016030000606c15002003000060481b003e030000>
      "voltage-states1" = <fafa00006702000056b600008f020000>
      "voltage-states5-sram" = <e039130016030000401217002a030000>
      "voltage-states9" = <000000007d000000807825145802000080eed5248a020000>
"#;

    fn states(v: &[(&str, i64)]) -> Vec<(String, i64)> {
        v.iter().map(|(n, t)| ((*n).to_string(), *t)).collect()
    }

    #[test]
    fn parses_dvfs_tables_in_khz_and_hz() {
        let t = Tables::parse(PMGR);
        assert_eq!(t.ecpu, [1020.0, 1404.0, 1788.0]); // kHz (M4)
        assert_eq!(t.pcpu, [1260.0, 1512.0]);
        assert_eq!(t.gpu, [338.0, 618.0]); // Hz, "off" entry dropped
        assert!(dvfs_mhz(PMGR, "voltage-states7").is_empty());
    }

    #[test]
    fn clock_is_the_mean_over_running_time() {
        let tables = Tables { ecpu: vec![1000.0, 2000.0], pcpu: vec![3000.0, 4000.0], gpu: vec![500.0, 1000.0] };
        let channels = vec![
            Channel::States {
                name: "ECPU000".into(),
                states: states(&[("DOWN", 0), ("IDLE", 900), ("V0P1", 50), ("V1P0", 50)]),
            },
            Channel::States {
                name: "PCPU000".into(),
                states: states(&[("DOWN", 1000), ("IDLE", 0), ("V0P1", 0), ("V1P0", 0)]),
            },
            Channel::States {
                name: "PCPU100".into(),
                states: states(&[("DOWN", 0), ("IDLE", 800), ("V0P1", 0), ("V1P0", 200)]),
            },
            Channel::States { name: "GPUPH".into(), states: states(&[("OFF", 10), ("P1", 30), ("P2", 10)]) },
        ];
        // (50·1000 + 50·2000 + 200·4000) / 300
        assert_eq!(cpu_clock(&channels, &tables), Some(950_000.0 / 300.0));
        assert_eq!(gpu_clock(&channels, &tables), Some((30.0 * 500.0 + 10.0 * 1000.0) / 40.0));
    }

    #[test]
    fn an_idle_gpu_runs_at_zero_and_no_table_means_no_clock() {
        let tables = Tables { gpu: vec![500.0], ..Tables::default() };
        let off = vec![Channel::States { name: "GPUPH".into(), states: states(&[("OFF", 100), ("P1", 0)]) }];
        assert_eq!(gpu_clock(&off, &tables), Some(0.0));
        assert_eq!(gpu_clock(&off, &Tables::default()), None);
        assert_eq!(cpu_clock(&off, &tables), None);
    }

    #[test]
    fn energy_units() {
        let channels = vec![
            Channel::Energy { name: "GPU Energy".into(), unit: "nJ".into(), value: 2_000_000_000 },
            Channel::Energy { name: "CPU Energy".into(), unit: "mJ".into(), value: 1500 },
        ];
        assert_eq!(joules(&channels, "GPU Energy"), Some(2.0));
        assert_eq!(joules(&channels, "CPU Energy"), Some(1.5));
        assert_eq!(joules(&channels, "ANE"), None);
    }

    #[test]
    fn power_needs_a_live_counter() {
        let mut cpu = PowerMeter::cpu();
        assert_eq!(cpu.watts(Some(3.0), 1.0), None, "first movement only arms it");
        assert_eq!(cpu.watts(Some(3.0), 1.5), Some(2.0));
        assert_eq!(cpu.watts(Some(0.0), 1.0), None, "a frozen counter is not 0 W");
        assert_eq!(cpu.watts(Some(9.0), 1.0), None, "a jump after a freeze is not a reading");
        assert_eq!(cpu.watts(Some(1.0), 1.0), Some(1.0));
    }

    #[test]
    fn an_idle_gpu_draws_zero_once_its_counter_is_known_live() {
        let mut gpu = PowerMeter::gpu();
        assert_eq!(gpu.watts(Some(0.0), 1.0), None, "never moved: maybe frozen");
        assert_eq!(gpu.watts(Some(1.0), 1.0), None);
        assert_eq!(gpu.watts(Some(1.0), 1.0), Some(1.0));
        assert_eq!(gpu.watts(Some(0.0), 1.0), Some(0.0), "power-gated");
        assert_eq!(gpu.watts(None, 1.0), None, "no channel");
    }

    #[test]
    fn samples_on_this_mac() {
        let Ok(report) = Subscription::new() else { return }; // Intel Mac or a VM without IOReport channels
        let (a, b) = (report.sample().unwrap(), report.sample().unwrap());
        assert!(!delta_channels(&a, &b).is_empty());
    }
}
