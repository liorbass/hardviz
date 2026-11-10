//! Loading the NVIDIA Management Library, shared by the NVML metric source
//! and per-process GPU sampling.

use nvml_wrapper::error::NvmlError;
use nvml_wrapper::Nvml;

/// Load NVML. Driver packages on Linux usually ship only the versioned
/// soname; the unversioned `libnvidia-ml.so` that `init()` looks for comes from
/// dev packages, so fall back to `.so.1`.
///
/// # Errors
/// No NVIDIA driver is installed (the library cannot be loaded), or it fails
/// to initialise.
pub fn init() -> Result<Nvml, NvmlError> {
    let init = Nvml::init();
    #[cfg(target_os = "linux")]
    let init = init.or_else(|_| Nvml::builder().lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1")).init());
    init
}

#[cfg(test)]
mod tests {
    #[test]
    fn loading_reports_rather_than_panics() {
        // Machines without an NVIDIA driver get an error that can be shown.
        if let Err(e) = super::init() {
            assert!(!e.to_string().is_empty());
        }
    }
}
