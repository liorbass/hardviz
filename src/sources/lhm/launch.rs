//! Starting the bundled `LibreHardwareMonitor`, and elevation in general.
//!
//! LHM needs administrator rights. `HardViz` starts it at most once per session,
//! preferring the startup task (no prompt) and otherwise asking Windows for
//! elevation (one UAC prompt). Every elevated launch goes through [`Elevated`],
//! which reports a declined prompt as a distinct, user-readable failure.

use super::task;
use std::fmt::{self, Write as _};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::Instant;

/// Hide the console window of helper processes.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Exit code PowerShell scripts below use for a declined UAC prompt
/// (`ERROR_CANCELLED`).
const DECLINED: i32 = 1223;
/// Folder (next to `hardviz.exe`) that `build.rs` copies LHM into.
const BUNDLE_DIR: &str = "LibreHardwareMonitor";
pub const LHM_EXE: &str = "LibreHardwareMonitor.exe";

/// Why an elevated program did not run.
#[derive(Debug)]
pub enum ElevationError {
    /// PowerShell itself could not be started.
    Spawn(std::io::Error),
    /// The user answered "No" to the UAC prompt.
    Declined,
    /// The program ran and failed.
    Exit(i32),
    /// The helper was killed.
    Terminated,
}

impl fmt::Display for ElevationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevationError::Spawn(e) => write!(f, "cannot start PowerShell ({e})"),
            ElevationError::Declined => write!(f, "the administrator prompt was declined"),
            ElevationError::Exit(code) => write!(f, "exit code {code}"),
            ElevationError::Terminated => write!(f, "terminated"),
        }
    }
}

impl std::error::Error for ElevationError {}

/// There is no bundled LHM where `build.rs` puts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleMissing {
    pub expected: PathBuf,
}

impl fmt::Display for BundleMissing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no bundled LibreHardwareMonitor at {}", self.expected.display())
    }
}

/// Path of the LHM that `build.rs` placed next to the executable.
///
/// # Errors
/// There is no LHM there; the error carries the expected path.
pub fn bundled_lhm() -> Result<PathBuf, BundleMissing> {
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    let exe = dir.join(BUNDLE_DIR).join(LHM_EXE);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(BundleMissing { expected: exe })
    }
}

/// Whether any LHM instance is running (bundled or a separate install).
fn lhm_process_running() -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new());
    let found = sys.processes_by_exact_name(std::ffi::OsStr::new(LHM_EXE)).next().is_some();
    found
}

/// A program to run with administrator rights via PowerShell's
/// `Start-Process -Verb RunAs`, which raises the UAC prompt.
///
/// The PowerShell process exits once the prompt is answered (or, with
/// `wait`, once the program exits), with [`DECLINED`] if it was declined. The
/// script is passed as `-EncodedCommand`, so no path or quote needs escaping
/// for the shell.
#[non_exhaustive]
pub struct Elevated<'a> {
    pub program: &'a Path,
    pub args: &'a [&'a str],
    pub working_dir: Option<&'a Path>,
    /// Wait for the program and exit with its exit code.
    pub wait: bool,
    /// Hide the program's window (for console tools).
    pub hidden: bool,
}

impl Elevated<'_> {
    /// The PowerShell script that performs the elevation.
    fn script(&self) -> String {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let mut cmd = format!("Start-Process -FilePath {} -Verb RunAs", quote(&self.program.display().to_string()));
        if !self.args.is_empty() {
            // Each argument double-quoted inside the PowerShell string, so
            // spaces survive Start-Process joining them into one command line.
            let list: Vec<String> = self.args.iter().map(|a| quote(&format!("\"{a}\""))).collect();
            let _ = write!(cmd, " -ArgumentList @({})", list.join(","));
        }
        if let Some(dir) = self.working_dir {
            let _ = write!(cmd, " -WorkingDirectory {}", quote(&dir.display().to_string()));
        }
        if self.hidden {
            cmd += " -WindowStyle Hidden";
        }
        let finish = if self.wait { " -Wait -PassThru; exit $p.ExitCode" } else { "; exit 0" };
        let assign = if self.wait { "$p = " } else { "" };
        format!("try {{ {assign}{cmd}{finish} }} catch {{ exit {DECLINED} }}")
    }

    /// Start PowerShell without waiting for it.
    ///
    /// # Errors
    /// PowerShell could not be started.
    pub fn spawn(&self) -> std::io::Result<Child> {
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand"])
            .arg(base64_utf16le(&self.script()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    }

    /// Run to completion (blocks until the prompt is answered).
    ///
    /// # Errors
    /// PowerShell could not start, the prompt was declined, or the program
    /// failed.
    pub fn run(&self) -> Result<(), ElevationError> {
        let status = self.spawn().and_then(|mut c| c.wait()).map_err(ElevationError::Spawn)?;
        outcome(status)
    }
}

/// What an elevation helper's exit status means.
///
/// # Errors
/// Any status but success: declined, the program's exit code, or killed.
pub fn outcome(status: ExitStatus) -> Result<(), ElevationError> {
    match status.code() {
        Some(0) => Ok(()),
        Some(DECLINED) => Err(ElevationError::Declined),
        Some(c) => Err(ElevationError::Exit(c)),
        None => Err(ElevationError::Terminated),
    }
}

/// The one-per-session attempt to start LHM.
#[derive(Debug)]
pub enum Launch {
    /// An LHM process was already running, so we did not start another one.
    AlreadyRunning,
    NotBundled(BundleMissing),
    /// Started through the startup task: no prompt.
    ViaTask {
        at: Instant,
    },
    /// Asked Windows to elevate the bundled copy. `prompt` is the helper
    /// process until it reports how the UAC prompt was answered.
    Prompted {
        at: Instant,
        prompt: Option<Child>,
    },
    Failed(ElevationError),
}

impl Launch {
    /// Start LHM unless it is running: through the startup task when it is
    /// set up for the bundled copy, otherwise with a UAC prompt.
    #[must_use]
    pub fn start() -> Self {
        if lhm_process_running() {
            return Launch::AlreadyRunning;
        }
        let exe = match bundled_lhm() {
            Ok(exe) => exe,
            Err(missing) => return Launch::NotBundled(missing),
        };
        if task::state(&exe) == task::TaskState::Current {
            match task::run() {
                Ok(()) => {
                    crate::info!("[lhm_http] started LibreHardwareMonitor via startup task");
                    return Launch::ViaTask { at: Instant::now() };
                }
                Err(e) => crate::warn!("[lhm_http] startup task failed to run ({e}); launching directly"),
            }
        }
        let request = Elevated { program: &exe, args: &[], working_dir: exe.parent(), wait: false, hidden: false };
        match request.spawn() {
            Ok(prompt) => {
                crate::info!("[lhm_http] requested elevation for {}", exe.display());
                Launch::Prompted { at: Instant::now(), prompt: Some(prompt) }
            }
            Err(e) => Launch::Failed(ElevationError::Spawn(e)),
        }
    }

    /// Pick up how a pending UAC prompt was answered. Cheap; call every poll.
    pub fn refresh(&mut self) {
        let Launch::Prompted { prompt, .. } = self else { return };
        let Some(child) = prompt else { return };
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => match outcome(status) {
                Ok(()) => *prompt = None,
                Err(e) => *self = Launch::Failed(e),
            },
            Err(e) => *self = Launch::Failed(ElevationError::Spawn(e)),
        }
    }
}

/// PowerShell's `-EncodedCommand` takes base64 of UTF-16LE text.
fn base64_utf16le(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        out.push(char::from(T[(n >> 18) as usize & 63]));
        out.push(char::from(T[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 { char::from(T[(n >> 6) as usize & 63]) } else { '=' });
        out.push(if chunk.len() > 2 { char::from(T[n as usize & 63]) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_powershell() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("exit 0"))
        assert_eq!(base64_utf16le("exit 0"), "ZQB4AGkAdAAgADAA");
    }

    #[test]
    fn script_quotes_paths_and_arguments() {
        let exe = Path::new(r"C:\It's here\schtasks.exe");
        let e = Elevated { program: exe, args: &["/TN", "A B"], working_dir: None, wait: true, hidden: true };
        let s = e.script();
        assert!(s.contains(r"-FilePath 'C:\It''s here\schtasks.exe'"), "{s}");
        assert!(s.contains(r#"-ArgumentList @('"/TN"','"A B"')"#), "{s}");
        assert!(s.contains("-Wait -PassThru; exit $p.ExitCode"), "{s}");
        assert!(s.ends_with("catch { exit 1223 }"), "{s}");
    }

    #[test]
    fn launch_without_wait_exits_once_started() {
        let e = Elevated { program: Path::new("lhm.exe"), args: &[], working_dir: None, wait: false, hidden: false };
        assert!(e.script().contains("-Verb RunAs; exit 0"));
    }
}
