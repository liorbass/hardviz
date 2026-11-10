//! Copies the bundled `LibreHardwareMonitor` next to the built executable so
//! the release folder is a self-contained portable package.
//!
//! `HardViz` reads CPU temperature from LHM's HTTP JSON endpoint (LHM 0.9.6 has
//! no WMI provider), so shipping the binary alongside the app lets it be
//! launched without a separate install. The copy is skipped when `vendor/` is
//! absent, so the crate still builds for anyone who does not bundle it, and for
//! non-Windows targets. The copy rules live in `src/bundle.rs`, which the
//! library's tests also compile.

#[path = "src/bundle.rs"]
mod bundle;

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let vendor = manifest.join("vendor").join("LibreHardwareMonitor");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/bundle.rs");
    println!("cargo:rerun-if-changed=vendor/LibreHardwareMonitor");

    // LHM is a Windows .NET app; never bundle it into non-Windows builds.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") || !vendor.is_dir() {
        return;
    }
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else { return };
    let Some(exe_dir) = bundle::executable_dir(&out_dir) else { return };
    if let Err(e) = bundle::copy_tree(&vendor, &exe_dir.join("LibreHardwareMonitor")) {
        // Non-fatal: the app still runs, CPU temp just stays unavailable.
        println!("cargo:warning=could not bundle LibreHardwareMonitor: {e}");
    }
}
