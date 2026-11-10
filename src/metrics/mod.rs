//! Metric identity, definitions, and the time-series store.
//!
//! Everything in `HardViz` flows through a [`MetricId`]. Sources *provide*
//! metrics, widgets *require* them, and neither knows about the other. That
//! indirection is what makes both sides pluggable.

mod store;
mod units;

pub use store::{MetricStore, HISTORY_LEN};
pub use units::{format_rate, AxisFormat, Unit, BYTES_PER_MB};

use std::fmt;

/// A metric from the catalogue ([`metric`]). It can only be one of those
/// constants, so its definition is always there: [`MetricId::def`]. Compared
/// and hashed by its stable string key, e.g. `"cpu.usage"`.
#[derive(Clone, Copy)]
pub struct MetricId(&'static MetricDef);

impl MetricId {
    #[must_use]
    pub const fn def(self) -> &'static MetricDef {
        self.0
    }

    /// The stable key, for display and logs.
    #[must_use]
    pub const fn key(self) -> &'static str {
        self.0.key
    }
}

impl PartialEq for MetricId {
    fn eq(&self, other: &Self) -> bool {
        self.0.key == other.0.key
    }
}

impl Eq for MetricId {}

impl std::hash::Hash for MetricId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.key.hash(state);
    }
}

impl fmt::Debug for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.key)
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.key)
    }
}

/// The metric catalogue: every metric `HardViz` knows. A metric exists only as
/// one of these constants, so every [`MetricId`] has a definition.
pub mod metric {
    use super::{MetricDef, MetricId, MetricKind, Scale, Unit};

    pub const CPU_USAGE: MetricId = MetricId(&MetricDef {
        key: "cpu.usage",
        label: "CPU",
        kind: MetricKind::Cpu,
        unit: Unit::Percent,
        scale: Scale::Fixed(0.0, 100.0),
    });
    pub const CPU_TEMP: MetricId = MetricId(&MetricDef {
        key: "cpu.temp",
        label: "CPU",
        kind: MetricKind::Cpu,
        unit: Unit::Celsius,
        scale: Scale::Fixed(20.0, 100.0),
    });
    pub const RAM_USAGE: MetricId = MetricId(&MetricDef {
        key: "ram.usage",
        label: "RAM",
        kind: MetricKind::Ram,
        unit: Unit::Percent,
        scale: Scale::Fixed(0.0, 100.0),
    });
    pub const RAM_USED: MetricId = MetricId(&MetricDef {
        key: "ram.used",
        label: "RAM used",
        kind: MetricKind::Ram,
        unit: Unit::Megabytes,
        scale: Scale::Auto(1024.0),
    });
    pub const RAM_TOTAL: MetricId = MetricId(&MetricDef {
        key: "ram.total",
        label: "RAM total",
        kind: MetricKind::Ram,
        unit: Unit::Megabytes,
        scale: Scale::Auto(1024.0),
    });
    pub const GPU_USAGE: MetricId = MetricId(&MetricDef {
        key: "gpu.usage",
        label: "GPU",
        kind: MetricKind::Gpu,
        unit: Unit::Percent,
        scale: Scale::Fixed(0.0, 100.0),
    });
    pub const GPU_TEMP: MetricId = MetricId(&MetricDef {
        key: "gpu.temp",
        label: "GPU",
        kind: MetricKind::Gpu,
        unit: Unit::Celsius,
        scale: Scale::Fixed(20.0, 100.0),
    });
    pub const VRAM_USAGE: MetricId = MetricId(&MetricDef {
        key: "vram.usage",
        label: "VRAM",
        kind: MetricKind::Vram,
        unit: Unit::Percent,
        scale: Scale::Fixed(0.0, 100.0),
    });
    pub const VRAM_USED: MetricId = MetricId(&MetricDef {
        key: "vram.used",
        label: "VRAM used",
        kind: MetricKind::Vram,
        unit: Unit::Megabytes,
        scale: Scale::Auto(1024.0),
    });
    pub const VRAM_TOTAL: MetricId = MetricId(&MetricDef {
        key: "vram.total",
        label: "VRAM total",
        kind: MetricKind::Vram,
        unit: Unit::Megabytes,
        scale: Scale::Auto(1024.0),
    });
    pub const CPU_POWER: MetricId = MetricId(&MetricDef {
        key: "cpu.power",
        label: "CPU power",
        kind: MetricKind::Cpu,
        unit: Unit::Watts,
        scale: Scale::Fixed(0.0, 200.0),
    });
    pub const CPU_CLOCK: MetricId = MetricId(&MetricDef {
        key: "cpu.clock",
        label: "CPU clock",
        kind: MetricKind::Cpu,
        unit: Unit::Megahertz,
        scale: Scale::Fixed(0.0, 6000.0),
    });
    pub const GPU_POWER: MetricId = MetricId(&MetricDef {
        key: "gpu.power",
        label: "GPU power",
        kind: MetricKind::Gpu,
        unit: Unit::Watts,
        scale: Scale::Fixed(0.0, 450.0),
    });
    pub const GPU_CLOCK: MetricId = MetricId(&MetricDef {
        key: "gpu.clock",
        label: "GPU clock",
        kind: MetricKind::Gpu,
        unit: Unit::Megahertz,
        scale: Scale::Fixed(0.0, 3200.0),
    });
    /// Fastest motherboard fan. Fan headers are usually unnamed (`Fan #1`..),
    /// so the busiest one is the meaningful single reading.
    pub const FAN_SPEED: MetricId = MetricId(&MetricDef {
        key: "fan.speed",
        label: "Fan",
        kind: MetricKind::Cooling,
        unit: Unit::Rpm,
        scale: Scale::Fixed(0.0, 3000.0),
    });
    /// Hottest drive.
    pub const STORAGE_TEMP: MetricId = MetricId(&MetricDef {
        key: "storage.temp",
        label: "Drive",
        kind: MetricKind::Storage,
        unit: Unit::Celsius,
        scale: Scale::Fixed(20.0, 90.0),
    });
    pub const DISK_READ: MetricId = MetricId(&MetricDef {
        key: "disk.read",
        label: "Read",
        kind: MetricKind::Storage,
        unit: Unit::MegabytesPerSec,
        scale: Scale::Auto(1.0),
    });
    pub const DISK_WRITE: MetricId = MetricId(&MetricDef {
        key: "disk.write",
        label: "Write",
        kind: MetricKind::Storage,
        unit: Unit::MegabytesPerSec,
        scale: Scale::Auto(1.0),
    });
    pub const NET_DOWN: MetricId = MetricId(&MetricDef {
        key: "net.down",
        label: "Down",
        kind: MetricKind::Network,
        unit: Unit::MegabytesPerSec,
        scale: Scale::Auto(1.0),
    });
    pub const NET_UP: MetricId = MetricId(&MetricDef {
        key: "net.up",
        label: "Up",
        kind: MetricKind::Network,
        unit: Unit::MegabytesPerSec,
        scale: Scale::Auto(1.0),
    });
}

/// Drives colouring. Widgets ask the theme for a colour by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    Cpu,
    Ram,
    Gpu,
    Vram,
    Storage,
    Network,
    Cooling,
}

/// Static description of one metric.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct MetricDef {
    /// Stable key, e.g. `"cpu.usage"`. Never rename one.
    pub key: &'static str,
    /// Short label: gauge title, chart legend, sensor tile.
    pub label: &'static str,
    pub kind: MetricKind,
    pub unit: Unit,
    /// How a chart's y axis covers this metric.
    pub scale: Scale,
}

impl MetricDef {
    /// Whether a high reading is bad (usage, memory, temperature), so its
    /// gauge and bar fade towards the warning colours as it rises. Fan speed,
    /// clocks and power are not: high is just busy.
    #[must_use]
    pub fn high_is_bad(&self) -> bool {
        matches!(self.unit, Unit::Percent | Unit::Celsius)
    }
}

/// Y-axis range for a metric.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// Always this range (percentages, temperatures).
    Fixed(f64, f64),
    /// From 0 to the busiest sample shown, rounded up to a 1-2-5 step, but
    /// never below the floor, so idle noise is not magnified to full height.
    Auto(f64),
}

impl Scale {
    /// The range to show, given the samples on screen (`NaN` = gap): the
    /// fixed range, or 0 to the busiest sample rounded up to a 1-2-5 step.
    pub fn range(self, samples: impl Iterator<Item = f64>) -> (f64, f64) {
        match self {
            Scale::Fixed(lo, hi) => (lo, hi),
            Scale::Auto(floor) => (0.0, nice_ceiling(samples.filter(|v| !v.is_nan()).fold(0.0, f64::max), floor)),
        }
    }

    /// Where `v` sits in `range`, from 0 to 1.
    #[must_use]
    pub fn fraction(v: f64, (lo, hi): (f64, f64)) -> f64 {
        ((v - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0)
    }
}

/// Round `max` up to a 1-2-5 step so auto-scaled axes get readable gridlines.
#[must_use]
pub fn nice_ceiling(max: f64, floor: f64) -> f64 {
    let m = max.max(floor);
    let mag = 10f64.powf(m.log10().floor());
    for step in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if m <= step * mag {
            return step * mag;
        }
    }
    10.0 * mag
}

/// Every metric, in display order (Settings -> Metrics).
pub const METRICS: &[MetricId] = &[
    metric::CPU_USAGE,
    metric::CPU_TEMP,
    metric::RAM_USAGE,
    metric::RAM_USED,
    metric::RAM_TOTAL,
    metric::GPU_USAGE,
    metric::GPU_TEMP,
    metric::VRAM_USAGE,
    metric::VRAM_USED,
    metric::VRAM_TOTAL,
    metric::CPU_POWER,
    metric::CPU_CLOCK,
    metric::GPU_POWER,
    metric::GPU_CLOCK,
    metric::FAN_SPEED,
    metric::STORAGE_TEMP,
    metric::DISK_READ,
    metric::DISK_WRITE,
    metric::NET_DOWN,
    metric::NET_UP,
];

#[cfg(test)]
mod tests {
    use super::metric::{CPU_TEMP, CPU_USAGE};
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn nice_ceiling_rounds_to_readable_steps() {
        assert_eq!(nice_ceiling(0.3, 1.0), 1.0);
        assert_eq!(nice_ceiling(3.0, 1.0), 5.0);
        assert_eq!(nice_ceiling(21.0, 1.0), 25.0);
        assert_eq!(nice_ceiling(260.0, 1.0), 500.0);
    }

    #[test]
    fn scale_ranges_are_fixed_or_follow_the_data() {
        assert_eq!(Scale::Fixed(20.0, 100.0).range([500.0].into_iter()), (20.0, 100.0));
        assert_eq!(Scale::Auto(1.0).range([0.2, f64::NAN, 3.4].into_iter()), (0.0, 5.0));
        assert_eq!(Scale::Auto(10.0).range([f64::NAN].into_iter()), (0.0, 10.0));
        assert_eq!(Scale::fraction(60.0, (20.0, 100.0)), 0.5);
        assert_eq!(Scale::fraction(500.0, (0.0, 100.0)), 1.0);
    }

    #[test]
    fn usage_and_temperatures_are_high_is_bad_but_fans_are_not() {
        use metric::{CPU_TEMP, FAN_SPEED, GPU_CLOCK, RAM_USAGE};
        assert!(RAM_USAGE.def().high_is_bad() && CPU_TEMP.def().high_is_bad());
        assert!(!FAN_SPEED.def().high_is_bad() && !GPU_CLOCK.def().high_is_bad());
    }

    #[test]
    fn catalogue_keys_are_unique() {
        let keys: HashSet<_> = METRICS.iter().map(|id| id.key()).collect();
        assert_eq!(keys.len(), METRICS.len());
    }

    #[test]
    fn ids_compare_by_key() {
        assert_eq!(CPU_USAGE, METRICS[0]);
        assert_ne!(CPU_USAGE, CPU_TEMP);
        assert_eq!(CPU_USAGE.to_string(), "cpu.usage");
        assert_eq!(CPU_USAGE.def().label, "CPU");
    }
}
