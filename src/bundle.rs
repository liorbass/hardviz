//! Bundling `LibreHardwareMonitor` next to the executable.
//!
//! Used by `build.rs` (through `#[path]`) and by the launcher, so the two agree
//! on where the bundle is and what starts it. Only `std`: build scripts cannot
//! use the crate's dependencies.

use std::fs;
use std::path::Path;

/// The folder `build.rs` copies next to the executable, and the launcher looks
/// for beside itself.
pub const BUNDLE_DIR: &str = "LibreHardwareMonitor";
/// The entry point of a bundle that can actually be started.
pub const LHM_EXE: &str = "LibreHardwareMonitor.exe";

/// The directory the executable lands in, from a build script's `OUT_DIR`
/// (`<target>/<profile>/build/<crate>-<hash>/out`: three levels up).
pub fn executable_dir(out_dir: &Path) -> Option<&Path> {
    out_dir.parent()?.parent()?.parent()
}

/// Whether `vendor` holds a launchable LibreHardwareMonitor rather than a
/// folder of the same name.
///
/// A crate is source: `cargo package` follows `.gitignore`, which keeps the
/// vendored binaries out, so the published crate carries only this folder's
/// license and config. [`copy_tree`] would report that as a clean copy, which
/// is why the build script asks here before it stays quiet.
pub fn has_runtime(vendor: &Path) -> bool {
    vendor.join(LHM_EXE).is_file()
}

/// Copy `src` into `dst`, recursively, skipping files whose copy is already
/// at least as new. LHM rewrites its config on exit, so a newer copy in
/// `target/` holds the user's settings and must not be clobbered by a rebuild,
/// while an updated `vendor/` file still reaches it.
///
/// # Errors
/// A directory cannot be created or read, or a file cannot be copied.
pub fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else if is_stale(&from, &to) {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Whether `dst` is missing or older than `src`.
fn is_stale(src: &Path, dst: &Path) -> bool {
    let modified = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
    match (modified(src), modified(dst)) {
        (Some(s), Some(d)) => s > d,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("hardviz-bundle-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn set_mtime(path: &Path, t: SystemTime) {
        fs::File::options().write(true).open(path).and_then(|f| f.set_modified(t)).expect("set mtime");
    }

    #[test]
    fn executable_dir_is_three_levels_above_out_dir() {
        let out = Path::new("target/debug/build/hardviz-abc/out");
        assert_eq!(executable_dir(out), Some(Path::new("target/debug")));
        assert_eq!(executable_dir(Path::new("out")), None);
    }

    #[test]
    fn copies_a_tree_and_keeps_newer_copies() {
        let tmp = TempDir::new("copy");
        let (src, dst) = (tmp.0.join("vendor"), tmp.0.join("target"));
        fs::create_dir_all(src.join("sub")).expect("mkdir");
        fs::write(src.join("lhm.config"), "vendored").expect("write");
        fs::write(src.join("sub").join("lib.dll"), "dll").expect("write");

        copy_tree(&src, &dst).expect("first copy");
        assert_eq!(fs::read_to_string(dst.join("sub").join("lib.dll")).expect("copied"), "dll");

        // The app rewrote its config after the build: a rebuild keeps it.
        fs::write(dst.join("lhm.config"), "user settings").expect("write");
        let now = SystemTime::now();
        set_mtime(&src.join("lhm.config"), now - Duration::from_secs(60));
        set_mtime(&dst.join("lhm.config"), now);
        copy_tree(&src, &dst).expect("rebuild");
        assert_eq!(fs::read_to_string(dst.join("lhm.config")).expect("kept"), "user settings");

        // An updated vendored file replaces an older copy.
        set_mtime(&src.join("lhm.config"), now + Duration::from_secs(60));
        copy_tree(&src, &dst).expect("update");
        assert_eq!(fs::read_to_string(dst.join("lhm.config")).expect("updated"), "vendored");
    }

    #[test]
    fn a_folder_without_the_launcher_is_not_a_runtime() {
        let tmp = TempDir::new("runtime");
        // What the published crate actually carries: the folder, its license,
        // its config, and no binaries.
        fs::write(tmp.0.join("LICENSE"), "MPL-2.0").expect("write");
        fs::write(tmp.0.join("LibreHardwareMonitor.config"), "{}").expect("write");
        assert!(!has_runtime(&tmp.0));
        assert!(!has_runtime(&tmp.0.join("nope")));

        fs::write(tmp.0.join(LHM_EXE), "MZ").expect("write");
        assert!(has_runtime(&tmp.0));
    }
}
