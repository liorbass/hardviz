//! CSV export of the retained history: Settings -> Data -> Export, or `e`
//! in the terminal UI.
//!
//! A local file next to the config, nothing more: HardViz still sends nothing
//! over the network. The export covers what the store keeps (the chart time
//! span at the current refresh rate), one column per metric, one row per
//! sample, oldest first. Gaps are empty cells, never zeros.

use crate::metrics::{MetricStore, METRICS};
use std::path::PathBuf;

/// The history as CSV. `now` is the newest sample's time; earlier rows are
/// `interval_ms` apart. Series shorter than the longest (a session total
/// that started accumulating later, say) are aligned to the newest row and
/// left empty before that.
#[must_use]
pub fn history_csv(store: &MetricStore, interval_ms: u64, now: chrono::DateTime<chrono::Local>) -> String {
    let histories: Vec<&[f64]> = METRICS.iter().map(|&id| store.long_history(id)).collect();
    let rows = histories.iter().map(|h| h.len()).max().unwrap_or(0);

    let mut out = String::from("time");
    for id in METRICS {
        out.push(',');
        out.push_str(id.key());
    }
    out.push('\n');

    let interval = chrono::Duration::milliseconds(i64::try_from(interval_ms.max(1)).unwrap_or(i64::MAX));
    for i in 0..rows {
        let ago = i32::try_from(rows - 1 - i).unwrap_or(i32::MAX);
        out.push_str(&(now - interval * ago).format("%Y-%m-%d %H:%M:%S%.3f").to_string());
        for h in &histories {
            out.push(',');
            // Row i is `rows - i` places from the newest; a shorter series
            // has no sample that far back.
            let v = h.len().checked_sub(rows - i).and_then(|j| h.get(j)).filter(|v| !v.is_nan());
            if let Some(v) = v {
                out.push_str(&format!("{v:.3}"));
            }
        }
        out.push('\n');
    }
    out
}

/// Where an export taken at `now` is written: next to the config.
#[must_use]
pub fn export_path(now: chrono::DateTime<chrono::Local>) -> Option<PathBuf> {
    Some(crate::config::app_dir()?.join(format!("hardviz-export-{}.csv", now.format("%Y%m%d-%H%M%S"))))
}

/// Write the store's retained history to a timestamped CSV next to the
/// config, returning the path written.
///
/// # Errors
/// There is no config directory, or the file cannot be written.
pub fn write_export(store: &MetricStore, interval_ms: u64) -> Result<PathBuf, String> {
    let now = chrono::Local::now();
    let path = export_path(now).ok_or_else(|| "no config directory for the export".to_string())?;
    let csv = history_csv(store, interval_ms, now);
    std::fs::write(&path, &csv).map_err(|e| format!("cannot write {} ({e})", path.display()))?;
    crate::info!("exported {} samples of history to {}", csv.lines().count().saturating_sub(1), path.display());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::{CPU_USAGE, NET_DOWN, NET_DOWN_TOTAL};
    use chrono::TimeZone;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    #[test]
    fn rows_are_oldest_first_with_a_column_per_metric() {
        let mut store = MetricStore::new();
        for i in 0..3 {
            store.push(CPU_USAGE, Some(f64::from(i)));
        }
        let csv = history_csv(&store, 1000, now());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 4, "header + three samples");
        assert!(lines[0].starts_with("time,cpu.usage,"));
        assert!(lines[0].contains("net.down.total"));
        assert!(lines[1].starts_with("2026-01-02 03:04:03.000,0.000,"));
        assert!(lines[3].starts_with("2026-01-02 03:04:05.000,2.000,"));
    }

    #[test]
    fn gaps_are_empty_and_short_series_align_to_the_newest() {
        let mut store = MetricStore::new();
        store.push(CPU_USAGE, Some(1.0));
        for _ in 0..3 {
            store.push(CPU_USAGE, None); // the third miss appends one NaN gap
        }
        store.push(CPU_USAGE, Some(5.0)); // history: [1, NaN, 5]
                                          // A total that only exists for the last two samples.
        store.push(NET_DOWN, Some(10.0));
        store.integrate(1.0);
        store.push(NET_DOWN, Some(10.0));
        store.integrate(1.0);

        let csv = history_csv(&store, 1000, now());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 4, "header + three samples");
        let cpu_col = crate::metrics::METRICS.iter().position(|&id| id == CPU_USAGE).expect("listed") + 1;
        let total_col = crate::metrics::METRICS.iter().position(|&id| id == NET_DOWN_TOTAL).expect("listed") + 1;
        let cell = |line: &str, col: usize| line.split(',').nth(col).expect("column").to_string();
        assert_eq!(cell(lines[1], cpu_col), "1.000");
        assert_eq!(cell(lines[2], cpu_col), "", "a gap is empty, not zero");
        assert_eq!(cell(lines[3], cpu_col), "5.000");
        assert_eq!(cell(lines[1], total_col), "", "no total that far back");
        assert_eq!(cell(lines[2], total_col), "10.000");
        assert_eq!(cell(lines[3], total_col), "20.000");
    }

    #[test]
    fn an_empty_store_exports_just_the_header() {
        let csv = history_csv(&MetricStore::new(), 1000, now());
        assert_eq!(csv.lines().count(), 1);
    }

    #[test]
    fn export_files_are_timestamped_next_to_the_config() {
        let path = export_path(now()).expect("a config dir exists on this machine");
        assert_eq!(path.file_name().expect("name"), "hardviz-export-20260102-030405.csv");
        assert_eq!(path.parent(), crate::config::app_dir().as_deref());
    }
}
