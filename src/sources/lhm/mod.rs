//! CPU temperature, power, clock, fans and drive sensors from
//! `LibreHardwareMonitor` (Windows only).
//!
//! Windows does not expose CPU temperature through `sysinfo`, so this is the
//! primary source there, with `sources/acpi.rs` as the fallback.
//!
//! ## Why HTTP and not WMI
//!
//! `LibreHardwareMonitor` 0.9.6 was inspected directly (both encodings, since
//! .NET stores string literals as UTF-16 and type/assembly metadata as ASCII):
//!
//! - `root/LibreHardwareMonitor` — **absent**
//! - `WmiProvider` — **absent**
//! - `HttpListener`, `data.json`, `application/json`, `runWebServer`,
//!   `listenerPort`, `8085` — **present**
//!
//! So this build publishes sensors over an HTTP JSON endpoint, not WMI. The
//! `root\Hardware` namespace that does exist is Windows' built-in DMTF/CIM
//! namespace and holds zero `Sensor` instances; it is a red herring.
//!
//! The web server is **off by default** in LHM. The bundled copy under
//! `vendor/LibreHardwareMonitor` ships a config with it switched on (bound to
//! 127.0.0.1, starting minimised to the tray), and when the endpoint cannot be
//! reached [`LhmHttpSource`] launches that bundled copy once per session
//! ([`launch`]): through the startup task when it is set up ([`task`]),
//! otherwise with a UAC prompt. Until data arrives the source reports
//! `Unavailable` with the reason rather than a misleading zero.
//!
//! Modules: [`http`] fetches `data.json`, [`parse`] picks the readings out of
//! it, [`launch`] starts LHM, [`task`] manages the startup task.

pub mod http;
pub mod launch;
pub mod parse;
pub mod task;

use crate::metrics::{metric, MetricId};
use crate::source::{MetricSource, Readings, SourceStatus};
use launch::Launch;
use std::time::Duration;

/// LHM's default web server port (the literal `8085` is present in the binary).
pub const DEFAULT_LHM_PORT: u16 = 8085;

/// Bounds every socket operation, so a poll never blocks the poller for long.
const HTTP_TIMEOUT: Duration = Duration::from_millis(350);
/// How long after launching LHM to report "starting" before assuming the web
/// server is off. LHM loads a kernel driver and probes every sensor at
/// startup, which takes several seconds on its own.
const LAUNCH_GRACE: Duration = Duration::from_secs(45);

const PROVIDES: &[MetricId] = &[
    metric::CPU_TEMP,
    metric::CPU_POWER,
    metric::CPU_CLOCK,
    metric::FAN_SPEED,
    metric::STORAGE_TEMP,
    metric::DISK_READ,
    metric::DISK_WRITE,
];

/// Reads sensors from `LibreHardwareMonitor`'s `data.json` endpoint.
pub struct LhmHttpSource {
    port: u16,
    status: SourceStatus,
    /// `None` until the endpoint is first found unreachable.
    launch: Option<Launch>,
}

impl LhmHttpSource {
    #[must_use]
    pub fn new() -> Self {
        Self::with_port(DEFAULT_LHM_PORT)
    }

    #[must_use]
    pub fn with_port(port: u16) -> Self {
        Self { port, status: SourceStatus::not_yet_polled(), launch: None }
    }

    /// Explain why the endpoint is unreachable, given what was tried.
    fn unreachable_reason(&self, err: &http::HttpError) -> String {
        let url = format!("http://127.0.0.1:{}/data.json", self.port);
        let enable = format!("enable Options -> Remote Web Server (port {})", self.port);
        let Some(launch) = &self.launch else { return format!("cannot reach {url} ({err})") };
        match launch {
            Launch::ViaTask { at } if at.elapsed() < LAUNCH_GRACE => {
                format!("starting LibreHardwareMonitor via its startup task; waiting for {url}")
            }
            Launch::Prompted { prompt: Some(_), .. } => "starting LibreHardwareMonitor: approve the \
                administrator (UAC) prompt. Settings -> Data sources can set up a startup task so \
                this prompt is not needed."
                .into(),
            Launch::Prompted { at, .. } if at.elapsed() < LAUNCH_GRACE => {
                format!("LibreHardwareMonitor is starting; waiting for {url}")
            }
            Launch::ViaTask { .. } | Launch::Prompted { .. } => format!(
                "LibreHardwareMonitor was started but {url} is not answering ({err}). Open LHM \
                 from the tray and {enable}."
            ),
            Launch::AlreadyRunning => format!(
                "LibreHardwareMonitor is running but {url} is not answering ({err}). In LHM, \
                 {enable}."
            ),
            Launch::NotBundled(missing) => format!(
                "cannot reach {url} ({err}) and {missing}. Start LibreHardwareMonitor as \
                 administrator and {enable}."
            ),
            Launch::Failed(e) => format!(
                "cannot reach {url} ({err}); starting the bundled LibreHardwareMonitor failed \
                 ({e}). Restart HardViz to try again, or start it as administrator and {enable}."
            ),
        }
    }
}

impl Default for LhmHttpSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for LhmHttpSource {
    fn id(&self) -> &'static str {
        "lhm_http"
    }

    fn display_name(&self) -> &'static str {
        "LibreHardwareMonitor HTTP (CPU temp, power, clock, fans, drives)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        let body = match http::get(&format!("127.0.0.1:{}", self.port), "/data.json", HTTP_TIMEOUT) {
            Ok(b) => b,
            Err(e) => {
                // At most one launch per session; afterwards only watch it.
                self.launch.get_or_insert_with(Launch::start).refresh();
                self.status = SourceStatus::Unavailable { reason: self.unreachable_reason(&e) };
                return;
            }
        };

        self.status = match parse::parse_readings(&body) {
            Ok(r) if r.any() => {
                out.set(metric::CPU_TEMP, r.cpu_temp);
                out.set(metric::CPU_POWER, r.cpu_power);
                out.set(metric::CPU_CLOCK, r.cpu_clock);
                out.set(metric::FAN_SPEED, r.fan_max);
                out.set(metric::STORAGE_TEMP, r.storage_temp);
                out.set(metric::DISK_READ, r.disk_read);
                out.set(metric::DISK_WRITE, r.disk_write);
                SourceStatus::Online
            }
            Ok(_) => {
                SourceStatus::Unavailable { reason: "LHM data.json reached but contained no usable sensors".into() }
            }
            Err(e) => SourceStatus::Unavailable { reason: format!("could not parse LHM data.json ({e})") },
        };
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serve one HTTP response on a free local port; returns the port.
    fn serve_once(body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        port
    }

    #[test]
    fn readings_from_data_json_reach_the_metrics() {
        let port = serve_once(
            r#"{"Children":[
              {"Text":"CPU Package","Value":"61.0 °C","Type":"Temperature","SensorId":"/intelcpu/0/temperature/8"},
              {"Text":"Package","Value":"36.0 W","Type":"Power","SensorId":"/intelcpu/0/power/0"}]}"#,
        );
        let mut lhm = LhmHttpSource::with_port(port);
        let mut out = Readings::default();
        lhm.poll(&mut out);
        assert!(lhm.status().is_online(), "{}", lhm.status());
        assert_eq!(out.get(metric::CPU_TEMP), Some(61.0));
        assert_eq!(out.get(metric::CPU_POWER), Some(36.0));
        assert_eq!(out.get(metric::FAN_SPEED), None);
    }

    #[test]
    fn a_reachable_endpoint_without_sensors_is_explained() {
        let port = serve_once(r#"{"Children":[]}"#);
        let mut lhm = LhmHttpSource::with_port(port);
        lhm.poll(&mut Readings::default());
        assert!(lhm.status().to_string().contains("no usable sensors"));
    }

    #[test]
    fn unreachable_reasons_say_what_to_do() {
        let err = http::HttpError::Status(503);
        let mut lhm = LhmHttpSource::with_port(1);
        assert!(lhm.unreachable_reason(&err).starts_with("cannot reach http://127.0.0.1:1/data.json"));
        lhm.launch = Some(Launch::AlreadyRunning);
        assert!(lhm.unreachable_reason(&err).contains("Remote Web Server (port 1)"));
        lhm.launch = Some(Launch::Failed(launch::ElevationError::Declined));
        assert!(lhm.unreachable_reason(&err).contains("administrator prompt was declined"));
        lhm.launch = Some(Launch::NotBundled(launch::BundleMissing { expected: "C:\\x\\lhm.exe".into() }));
        assert!(lhm.unreachable_reason(&err).contains("C:\\x\\lhm.exe"));
    }
}
