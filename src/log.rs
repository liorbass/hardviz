//! Minimal leveled file logger.
//!
//! Deliberately dependency-free and append-per-call: `HardViz` logs a handful of
//! lines per second at most, so holding a file handle in global state (and the
//! teardown that would require) is not worth it.
//!
//! Output goes to both `<config_dir>/hardviz.log` and stderr, so a crash that
//! takes the window with it still leaves a trail on disk. The terminal UI logs
//! to its own file only ([`init_file_only`]): stderr would scribble over its
//! screen.
//!
//! Call sites use the `info!` / `warn!` / `error!` macros, which are
//! `#[macro_export]`ed to the crate root.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
/// Whether lines are echoed to stderr as well as the file.
static ECHO: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        })
    }
}

/// Start logging, truncating any previous log so each run is self-contained.
///
/// Returns the log file's path, or `None` when there is no usable file (no
/// config directory, or it cannot be created or opened); logging then goes to
/// stderr only, and [`path_label`] says so.
#[must_use = "the path says whether a log file is being written"]
pub fn init() -> Option<PathBuf> {
    open("hardviz.log")
}

/// Like [`init`], but to `file_name` and without the stderr echo. A file
/// that cannot be opened means no logging at all.
#[must_use = "the path says whether a log file is being written"]
pub fn init_file_only(file_name: &str) -> Option<PathBuf> {
    ECHO.store(false, Ordering::Relaxed);
    open(file_name)
}

fn open(file_name: &str) -> Option<PathBuf> {
    let path = crate::config::app_dir()?.join(file_name);
    let opened = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| OpenOptions::new().create(true).write(true).truncate(true).open(&path));
    if let Err(e) = opened {
        if ECHO.load(Ordering::Relaxed) {
            eprintln!("cannot open log file {} ({e}); logging to stderr only", path.display());
        }
        return None;
    }
    // `init` runs once, at startup; a second call keeps the first path.
    let _ = LOG_PATH.set(path.clone());
    Some(path)
}

/// Where the log is being written, for display in the UI.
pub fn path_label() -> String {
    LOG_PATH.get().map_or_else(|| "(stderr only)".into(), |p| p.display().to_string())
}

pub fn log(level: Level, msg: &str) {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("[{stamp}] {level} {msg}");

    if ECHO.load(Ordering::Relaxed) {
        eprintln!("{line}");
    }

    if let Some(path) = LOG_PATH.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Info, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Warn, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Error, &format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_padded_to_one_width() {
        let labels: Vec<String> = [Level::Info, Level::Warn, Level::Error].iter().map(ToString::to_string).collect();
        assert_eq!(labels, ["INFO ", "WARN ", "ERROR"]);
    }
}
