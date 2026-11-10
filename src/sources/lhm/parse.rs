//! Picking `HardViz`'s readings out of LHM's `data.json`.
//!
//! The document is the tree shown in LHM's window: nested nodes with `Text`,
//! `Children` and, on sensors, `Type` (`"Temperature"`, `"Power"`, ...),
//! `SensorId` (`"/amdcpu/0/temperature/2"`) and `Value` formatted for display
//! (`"67.4 °C"`, `"28.5 KB/s"`). It is walked as `serde_json::Value` so extra
//! or renamed fields in other LHM versions do not break parsing.

use crate::metrics::BYTES_PER_MB;
use crate::num::count_f64;

/// Everything `HardViz` reads from one `data.json`.
#[derive(Debug, Default, Clone, PartialEq)]
#[non_exhaustive]
pub struct LhmReadings {
    pub cpu_temp: Option<f64>,
    pub cpu_power: Option<f64>,
    pub cpu_clock: Option<f64>,
    pub fan_max: Option<f64>,
    pub storage_temp: Option<f64>,
    /// MB/s, summed over drives.
    pub disk_read: Option<f64>,
    pub disk_write: Option<f64>,
}

impl LhmReadings {
    #[must_use]
    pub fn any(&self) -> bool {
        *self != Self::default()
    }
}

/// One sensor node, as far as `HardViz` cares.
struct Sensor<'a> {
    kind: &'a str,
    /// Hardware class from the sensor id: `amdcpu`, `nvme`, `lpc`, ...
    hw: &'a str,
    /// Lower-cased display name.
    name: String,
    value: f64,
    /// The raw value text, which carries the unit for throughput.
    text: &'a str,
}

impl<'a> Sensor<'a> {
    fn from_node(node: &'a serde_json::Value) -> Option<Self> {
        let kind = node.get("Type")?.as_str()?;
        let raw = node.get("Value")?;
        let sensor_id = node.get("SensorId").and_then(|s| s.as_str()).unwrap_or("");
        Some(Self {
            kind,
            hw: sensor_id.split('/').nth(1).unwrap_or(""),
            name: node.get("Text").and_then(|t| t.as_str()).unwrap_or("").to_lowercase(),
            value: parse_value(raw)?,
            text: raw.as_str().unwrap_or(""),
        })
    }

    fn is_cpu(&self) -> bool {
        self.hw.ends_with("cpu")
    }

    fn is_storage(&self) -> bool {
        matches!(self.hw, "nvme" | "hdd" | "ssd" | "storage" | "ata")
    }

    fn named(&self, any: &[&str]) -> bool {
        any.iter().any(|k| self.name.contains(k))
    }
}

/// Candidates seen while walking the tree, resolved by [`Collected::finish`].
#[derive(Default)]
struct Collected {
    temp_package: Option<f64>,
    temp_cores: Vec<f64>,
    temp_any: Option<f64>,
    power_package: Option<f64>,
    clock_average: Option<f64>,
    clock_cores: Vec<f64>,
    fan_max: Option<f64>,
    storage_temp: Option<f64>,
    disk_read: Option<f64>,
    disk_write: Option<f64>,
}

impl Collected {
    fn add(&mut self, s: &Sensor) {
        let v = s.value;
        match s.kind {
            "Temperature" if s.is_cpu() => self.add_cpu_temp(s),
            // Drive temperatures; skip the static warning/critical thresholds
            // and the DIMM-style limit/resolution entries.
            "Temperature" if s.is_storage() => {
                if !s.named(&["warning", "critical", "limit", "resolution"]) && (0.0..=120.0).contains(&v) {
                    self.storage_temp = Some(self.storage_temp.map_or(v, |m| m.max(v)));
                }
            }
            "Power" if s.is_cpu() && s.named(&["package"]) => {
                self.power_package.get_or_insert(v);
            }
            "Clock" if s.is_cpu() && !s.named(&["effective", "bus"]) => {
                if s.named(&["average"]) {
                    self.clock_average.get_or_insert(v);
                } else if s.named(&["core"]) {
                    self.clock_cores.push(v);
                }
            }
            // Motherboard (Super I/O) fans only; GPU fans belong to the GPU.
            "Fan" if s.hw == "lpc" => self.fan_max = Some(self.fan_max.map_or(v, |m| m.max(v))),
            "Throughput" if s.is_storage() => {
                let mbps = rate_to_mb_per_s(v, s.text);
                if s.named(&["read"]) {
                    self.disk_read = Some(self.disk_read.unwrap_or(0.0) + mbps);
                } else if s.named(&["write"]) {
                    self.disk_write = Some(self.disk_write.unwrap_or(0.0) + mbps);
                }
            }
            _ => {}
        }
    }

    /// CPU temperature preference: the package/die sensor (Intel `CPU
    /// Package`, AMD `Core (Tctl/Tdie)`), else the mean of per-core/CCD
    /// sensors, else any CPU temperature. Intel's `Distance to TjMax` sensors
    /// are headroom, not a temperature, and are skipped.
    fn add_cpu_temp(&mut self, s: &Sensor) {
        if s.named(&["distance"]) || !(0.0..=150.0).contains(&s.value) {
            return;
        }
        let ccd = s.name.starts_with("ccd");
        if s.named(&["package", "tctl", "tdie", "average"]) && !ccd {
            self.temp_package.get_or_insert(s.value);
        } else if (s.named(&["core"]) || ccd) && !s.named(&["max"]) {
            self.temp_cores.push(s.value);
        } else {
            self.temp_any.get_or_insert(s.value);
        }
    }

    fn finish(self) -> LhmReadings {
        let mean = |xs: &[f64]| (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / count_f64(xs.len()));
        LhmReadings {
            cpu_temp: self.temp_package.or_else(|| mean(&self.temp_cores)).or(self.temp_any),
            cpu_power: self.power_package,
            cpu_clock: self.clock_average.or_else(|| mean(&self.clock_cores)),
            fan_max: self.fan_max,
            storage_temp: self.storage_temp,
            disk_read: self.disk_read,
            disk_write: self.disk_write,
        }
    }
}

/// Walk LHM's `data.json` once and pick out every sensor `HardViz` shows.
///
/// # Errors
/// The body is not JSON.
pub fn parse_readings(body: &str) -> Result<LhmReadings, serde_json::Error> {
    let root: serde_json::Value = serde_json::from_str(body.trim_start_matches('\u{feff}'))?;
    let mut collected = Collected::default();
    let mut stack = vec![&root];
    while let Some(node) = stack.pop() {
        if let Some(children) = node.get("Children").and_then(|c| c.as_array()) {
            stack.extend(children.iter());
        }
        if let Some(sensor) = Sensor::from_node(node) {
            collected.add(&sensor);
        }
    }
    Ok(collected.finish())
}

/// Convert an LHM throughput reading to MB/s using the unit in its display
/// text (`"28.5 KB/s"`, `"1.1 MB/s"`). LHM scales by 1024.
fn rate_to_mb_per_s(v: f64, text: &str) -> f64 {
    let t = text.to_ascii_uppercase();
    if t.contains("GB/S") {
        v * 1024.0
    } else if t.contains("MB/S") {
        v
    } else if t.contains("KB/S") {
        v / 1024.0
    } else {
        // Plain bytes per second.
        v / BYTES_PER_MB
    }
}

/// Read a sensor value: a number, or display text like `"67.4 °C"` (some
/// locales use a decimal comma: `"67,4 °C"`).
fn parse_value(v: &serde_json::Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    let text = v.as_str()?.trim();
    let num: String = text.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    num.replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_temp(body: &str) -> Option<f64> {
        parse_readings(body).expect("valid JSON").cpu_temp
    }

    /// Trimmed from a real LHM 0.9.6 data.json (Ryzen 7 7700X, ASRock B650).
    const AMD_SAMPLE: &str = r#"{"id":0,"Text":"Sensor","Min":"Min","Value":"Value","Max":"Max","ImageURL":"","Children":[
      {"id":1,"Text":"LIORPC","Min":"","Value":"","Max":"","ImageURL":"","Children":[
        {"id":2,"Text":"ASRock B650 PG Lightning WiFi","HardwareId":"/motherboard","Children":[
          {"id":3,"Text":"Nuvoton NCT6799D","Children":[{"id":4,"Text":"Temperatures","Children":[
            {"id":5,"Text":"Temperature #1","Value":"46.0 °C","Type":"Temperature","SensorId":"/lpc/nct6799d/0/temperature/1","Children":[]}]}]}]},
        {"id":42,"Text":"AMD Ryzen 7 7700X","HardwareId":"/amdcpu/0","Children":[
          {"id":43,"Text":"Temperatures","Children":[
            {"id":44,"Text":"Core (Tctl/Tdie)","Value":"67.4 °C","Type":"Temperature","SensorId":"/amdcpu/0/temperature/2","Children":[]},
            {"id":45,"Text":"CCD1 (Tdie)","Value":"54.6 °C","Type":"Temperature","SensorId":"/amdcpu/0/temperature/3","Children":[]}]}]},
        {"id":225,"Text":"NVIDIA GeForce RTX 5070 Ti","HardwareId":"/gpu-nvidia/0","Children":[
          {"id":226,"Text":"Temperatures","Children":[
            {"id":227,"Text":"GPU Core","Value":"37.7 °C","Type":"Temperature","SensorId":"/gpu-nvidia/0/temperature/0","Children":[]}]}]}]}]}"#;

    #[test]
    fn amd_picks_tctl() {
        assert_eq!(cpu_temp(AMD_SAMPLE), Some(67.4));
    }

    #[test]
    fn intel_prefers_package_and_skips_distance() {
        let body = r#"{"Text":"Sensor","Children":[{"Text":"Intel Core i7","Children":[{"Text":"Temperatures","Children":[
          {"Text":"CPU Core #1","Value":"50,0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/0"},
          {"Text":"CPU Core #1 Distance to TjMax","Value":"50.0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/9"},
          {"Text":"CPU Package","Value":"61.0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/8"}]}]}]}"#;
        assert_eq!(cpu_temp(body), Some(61.0));
    }

    #[test]
    fn averages_cores_without_package() {
        let body = r#"{"Children":[
          {"Text":"CPU Core #1","Value":"40.0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/0"},
          {"Text":"CPU Core #2","Value":"50.0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/1"},
          {"Text":"Core Max","Value":"90.0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/5"}]}"#;
        assert_eq!(cpu_temp(body), Some(45.0));
    }

    #[test]
    fn extra_sensors() {
        // Shapes copied from a real data.json (Ryzen 7700X, NCT6799D, two NVMe).
        let body = r#"{"Children":[
          {"Text":"Package","Value":"36.0 W","Type":"Power","SensorId":"/amdcpu/0/power/0"},
          {"Text":"Core #1 (SMU)","Value":"1.1 W","Type":"Power","SensorId":"/amdcpu/0/power/1"},
          {"Text":"Bus Speed","Value":"99.8 MHz","Type":"Clock","SensorId":"/amdcpu/0/clock/0"},
          {"Text":"Cores (Average)","Value":"3151.0 MHz","Type":"Clock","SensorId":"/amdcpu/0/clock/1"},
          {"Text":"Cores (Average Effective)","Value":"127.0 MHz","Type":"Clock","SensorId":"/amdcpu/0/clock/2"},
          {"Text":"Fan #1","Value":"0 RPM","Type":"Fan","SensorId":"/lpc/nct6799d/0/fan/0"},
          {"Text":"Fan #2","Value":"628 RPM","Type":"Fan","SensorId":"/lpc/nct6799d/0/fan/1"},
          {"Text":"GPU Fan 1","Value":"1500 RPM","Type":"Fan","SensorId":"/gpu-nvidia/0/fan/1"},
          {"Text":"Composite Temperature","Value":"44.0 °C","Type":"Temperature","SensorId":"/nvme/2/temperature/0"},
          {"Text":"Critical Temperature","Value":"93.0 °C","Type":"Temperature","SensorId":"/nvme/2/temperature/11"},
          {"Text":"Composite Temperature","Value":"37.0 °C","Type":"Temperature","SensorId":"/nvme/1/temperature/0"},
          {"Text":"Read Rate","Value":"512.0 KB/s","Type":"Throughput","SensorId":"/nvme/2/throughput/54"},
          {"Text":"Write Rate","Value":"2.0 MB/s","Type":"Throughput","SensorId":"/nvme/2/throughput/55"},
          {"Text":"Read Rate","Value":"0.5 MB/s","Type":"Throughput","SensorId":"/nvme/1/throughput/54"},
          {"Text":"GPU PCIe Rx","Value":"8.4 MB/s","Type":"Throughput","SensorId":"/gpu-nvidia/0/throughput/0"}]}"#;
        let r = parse_readings(body).unwrap();
        assert_eq!(r.cpu_power, Some(36.0));
        assert_eq!(r.cpu_clock, Some(3151.0));
        assert_eq!(r.fan_max, Some(628.0));
        assert_eq!(r.storage_temp, Some(44.0));
        assert_eq!(r.disk_read, Some(1.0));
        assert_eq!(r.disk_write, Some(2.0));
        assert_eq!(r.cpu_temp, None);
    }

    #[test]
    fn no_cpu_sensor() {
        let body = r#"{"Children":[{"Text":"GPU Core","Value":"37.7 °C","Type":"Temperature","SensorId":"/gpu-nvidia/0/temperature/0"}]}"#;
        assert_eq!(cpu_temp(body), None);
        assert!(!parse_readings(body).unwrap().any());
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_readings("not json").is_err());
    }

    #[test]
    fn rates_convert_to_megabytes_per_second() {
        assert_eq!(rate_to_mb_per_s(2.0, "2.0 GB/s"), 2048.0);
        assert_eq!(rate_to_mb_per_s(512.0, "512 KB/s"), 0.5);
        assert_eq!(rate_to_mb_per_s(1_048_576.0, "1048576"), 1.0);
    }
}
