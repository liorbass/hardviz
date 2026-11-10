//! Windows builds only:
//!
//! - Embeds version information (product name, version, copyright) in the
//!   executables. Explorer shows it under Properties → Details, and code
//!   signing (SignPath) requires product name and version to be set.
//! - Copies the bundled `LibreHardwareMonitor` next to the built executable so
//!   the release folder is a self-contained portable package.
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

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    embed_version_info();

    // LHM is a Windows .NET app; never bundle it into non-Windows builds.
    if !vendor.is_dir() {
        return;
    }
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else { return };
    let Some(exe_dir) = bundle::executable_dir(&out_dir) else { return };
    if let Err(e) = bundle::copy_tree(&vendor, &exe_dir.join("LibreHardwareMonitor")) {
        // Non-fatal: the app still runs, CPU temp just stays unavailable.
        println!("cargo:warning=could not bundle LibreHardwareMonitor: {e}");
    }
}

/// The version resource of `hardviz.exe` and `hardviz-tui.exe` (both: the
/// resource is linked into every binary of the package). The version comes
/// from `Cargo.toml`, so it is the same in every build of a release.
fn embed_version_info() {
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "HardViz")
        .set("FileDescription", "HardViz hardware monitor")
        .set("CompanyName", "HardViz contributors")
        .set("LegalCopyright", "Copyright (c) 2026 Lior Bass and the HardViz contributors. MIT OR Apache-2.0.");
    // Fatal on purpose: a release without version information cannot be
    // signed. `rc.exe` ships with the Windows SDK that MSVC linking needs anyway.
    if let Err(e) = res.compile() {
        panic!("could not embed the Windows version resource: {e}");
    }
}
