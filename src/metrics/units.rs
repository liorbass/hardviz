//! Units: value formatting and chart axis labels.

/// Bytes in a megabyte (MiB), the unit every byte count is reported in.
pub const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unit {
    Percent,
    Celsius,
    Megabytes,
    Watts,
    Megahertz,
    Rpm,
    /// Megabytes per second.
    MegabytesPerSec,
    /// A plain number, e.g. a load average.
    Number,
    /// A duration held in seconds, e.g. uptime.
    Duration,
}

/// Formats an axis value when the unit rescales with magnitude.
pub type AxisFormat = fn(f64) -> String;

impl Unit {
    /// Axis labelling for charts: a suffix for plain numbers, or a formatter
    /// when the unit changes scale with magnitude (KB/s -> MB/s, MB -> GB).
    pub fn axis(self) -> (&'static str, Option<AxisFormat>) {
        match self {
            Unit::Percent => ("%", None),
            Unit::Celsius => ("°", None),
            Unit::Watts => (" W", None),
            Unit::Megahertz => ("", Some(|v| Unit::Megahertz.format(v))),
            Unit::Rpm => ("", None),
            Unit::Megabytes => ("", Some(|v| Unit::Megabytes.format(v))),
            Unit::MegabytesPerSec => ("", Some(format_rate)),
            Unit::Number => ("", Some(|v| format!("{v:.1}"))),
            Unit::Duration => ("", Some(format_duration)),
        }
    }

    #[must_use]
    pub fn format(&self, v: f64) -> String {
        match self {
            Unit::Percent => format!("{v:.0}%"),
            Unit::Celsius => format!("{v:.0}°C"),
            Unit::Megabytes => {
                if v >= 1024.0 * 1024.0 {
                    format!("{:.1} TB", v / (1024.0 * 1024.0))
                } else if v >= 1024.0 {
                    format!("{:.1} GB", v / 1024.0)
                } else {
                    format!("{v:.0} MB")
                }
            }
            Unit::Watts => format!("{v:.0} W"),
            Unit::Megahertz => {
                if v >= 1000.0 {
                    format!("{:.2} GHz", v / 1000.0)
                } else {
                    format!("{v:.0} MHz")
                }
            }
            Unit::Rpm => format!("{v:.0} RPM"),
            Unit::MegabytesPerSec => format_rate(v),
            Unit::Number => format!("{v:.2}"),
            Unit::Duration => format_duration(v),
        }
    }
}

/// A duration from seconds, as its two largest units: `3d 4h`, `4h 12m`,
/// `12m 30s`, `45s`.
#[must_use]
pub fn format_duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (d, h, m, sec) = (s / 86_400, s % 86_400 / 3_600, s % 3_600 / 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

/// Human-readable transfer rate from MB/s, e.g. `850 KB/s`, `12.4 MB/s`.
#[must_use]
pub fn format_rate(mb_per_s: f64) -> String {
    if mb_per_s >= 1000.0 {
        format!("{:.2} GB/s", mb_per_s / 1000.0)
    } else if mb_per_s >= 10.0 {
        format!("{mb_per_s:.0} MB/s")
    } else if mb_per_s >= 1.0 {
        format!("{mb_per_s:.1} MB/s")
    } else {
        format!("{:.0} KB/s", mb_per_s * 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_format_at_their_thresholds() {
        assert_eq!(Unit::Megabytes.format(512.0), "512 MB");
        assert_eq!(Unit::Megabytes.format(2048.0), "2.0 GB");
        assert_eq!(Unit::Megabytes.format(3.0 * 1024.0 * 1024.0), "3.0 TB");
        assert_eq!(Unit::Megahertz.format(800.0), "800 MHz");
        assert_eq!(Unit::Megahertz.format(4500.0), "4.50 GHz");
        assert_eq!(format_rate(0.5), "500 KB/s");
        assert_eq!(format_rate(2.5), "2.5 MB/s");
        assert_eq!(format_rate(25.0), "25 MB/s");
        assert_eq!(format_rate(2500.0), "2.50 GB/s");
        assert_eq!(Unit::Number.format(2.345), "2.35");
    }

    #[test]
    fn durations_show_their_two_largest_units() {
        assert_eq!(format_duration(45.0), "45s");
        assert_eq!(format_duration(754.0), "12m 34s");
        assert_eq!(format_duration(4.0 * 3600.0 + 12.0 * 60.0), "4h 12m");
        assert_eq!(format_duration(3.0 * 86_400.0 + 4.0 * 3600.0), "3d 4h");
        assert_eq!(format_duration(-5.0), "0s", "never negative");
    }
}
