//! macOS sources. None needs root, a helper or a kernel extension.
//!
//! | source | reads | provides |
//! |---|---|---|
//! | [`smc::SmcSource`] | the System Management Controller | CPU and GPU temperature, fan speed |
//! | [`ioreport::IoReportSource`] | `IOReport`, the counters behind `powermetrics` | CPU and GPU clock, GPU (and CPU) power |
//! | [`sensors::MacSensorsSource`] | PMU/SMC sensors through sysinfo | drive temperature, CPU temperature fallback |
//! | [`ioreg::IoregSource`] | the I/O Kit registry, through `/usr/sbin/ioreg` | disk I/O, GPU usage, GPU memory |
//!
//! `smc` and `ioreport` talk to private Apple interfaces (the same ones
//! Stats, iStat Menus and macmon use) whose keys and channels vary by chip and
//! macOS release, so every value is checked for plausibility and anything
//! missing is left unset rather than guessed.

pub mod ioreg;
pub mod ioreport;
pub mod sensors;
pub mod smc;

use std::process::Command;

const IOREG: &str = "/usr/sbin/ioreg";

/// `ioreg` with `args`, its output as text. `ioreg` ships with macOS and
/// answers in about 15 ms.
fn ioreg(args: &[&str]) -> Result<String, String> {
    let out = Command::new(IOREG).args(args).output().map_err(|e| format!("cannot run {IOREG} ({e})"))?;
    if !out.status.success() {
        return Err(format!("{IOREG} {} failed ({})", args.join(" "), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
