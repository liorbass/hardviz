//! A Windows Task Scheduler entry that starts `LibreHardwareMonitor` at login
//! with administrator rights, so `HardViz` does not have to show a UAC prompt.
//!
//! Creating a highest-privilege task needs elevation once (one UAC prompt).
//! After that, running the task (`schtasks /Run`) needs no elevation: the
//! task itself carries the rights. `HardViz` uses it both at login and to start
//! LHM on demand when the web server is not reachable.
//!
//! # Trust
//!
//! The task runs whatever executable it names with the user's highest rights,
//! without asking. If that file is in a folder the user can write to (a
//! portable `HardViz` in Downloads, say), any program running as the user can
//! replace it and gain administrator rights at the next login. So:
//!
//! - the task is only run when it names the *current* bundled exe
//!   ([`TaskState::Current`]); a stale task is never run, only offered for
//!   re-setup;
//! - Settings warns, before setting the task up, when the exe is outside the
//!   protected Program Files folders ([`is_protected_location`]).

use super::launch::{BundleMissing, Elevated, ElevationError, CREATE_NO_WINDOW};
use std::fmt;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

pub const TASK_NAME: &str = r"HardViz\LibreHardwareMonitor";

/// Why a task operation failed.
#[derive(Debug)]
pub enum TaskError {
    /// `schtasks` could not be started.
    Io(io::Error),
    /// `schtasks /Run` refused; carries its message.
    Run(String),
    WriteDefinition(io::Error),
    Elevation(ElevationError),
    /// `schtasks` reported success but the task is not as expected.
    NotApplied(&'static str),
    NoBundle(BundleMissing),
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Io(e) => write!(f, "cannot run schtasks ({e})"),
            TaskError::Run(msg) => write!(f, "schtasks: {msg}"),
            TaskError::WriteDefinition(e) => write!(f, "cannot write the task file ({e})"),
            TaskError::Elevation(e) => write!(f, "schtasks: {e}"),
            TaskError::NotApplied(what) => f.write_str(what),
            TaskError::NoBundle(missing) => write!(f, "{missing}"),
        }
    }
}

impl std::error::Error for TaskError {}

fn schtasks(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("schtasks").args(args).creation_flags(CREATE_NO_WINDOW).output()
}

/// Whether the startup task exists and what it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Missing,
    /// Runs the bundled LHM next to this `HardViz`.
    Current,
    /// Runs something else, e.g. a copy from a `HardViz` that has moved.
    Stale {
        command: String,
    },
}

/// Query the task and compare its command with `exe`.
#[must_use]
pub fn state(exe: &Path) -> TaskState {
    let Ok(out) = schtasks(&["/Query", "/TN", TASK_NAME, "/XML"]) else { return TaskState::Missing };
    if !out.status.success() {
        return TaskState::Missing;
    }
    let Some(command) = task_command(&out.stdout) else { return TaskState::Missing };
    if command_matches(&command, &exe.display().to_string()) {
        TaskState::Current
    } else {
        TaskState::Stale { command }
    }
}

/// The `<Command>` of a task's XML, unescaped.
fn task_command(xml: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(xml);
    let start = text.find("<Command>")? + "<Command>".len();
    let end = start + text[start..].find("</Command>")?;
    let raw = text[start..end].trim().trim_matches('"');
    Some(raw.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
}

/// Compare a command read back from `schtasks` with the expected path.
///
/// `schtasks` writes its XML in the console code page, so a character outside
/// it comes back as `?` and one inside it as a byte that is not valid UTF-8
/// (decoded as U+FFFD). Either stands for exactly one non-ASCII character of
/// `expected`; everything else must match, ignoring ASCII case.
fn command_matches(stored: &str, expected: &str) -> bool {
    let mut stored = stored.chars();
    for e in expected.chars() {
        let Some(s) = stored.next() else { return false };
        let lossy = s == '?' || !s.is_ascii();
        let ok = if lossy { !e.is_ascii() || e == s } else { s.eq_ignore_ascii_case(&e) };
        if !ok {
            return false;
        }
    }
    stored.next().is_none()
}

/// Whether `exe` is under a Program Files folder, which only administrators
/// can modify.
pub fn is_protected_location(exe: &Path) -> bool {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .any(|root| starts_with_ignore_case(exe, &root))
}

fn starts_with_ignore_case(path: &Path, root: &Path) -> bool {
    let (p, r) = (path.display().to_string().to_lowercase(), root.display().to_string().to_lowercase());
    p.strip_prefix(r.trim_end_matches('\\')).is_some_and(|rest| rest.starts_with('\\'))
}

/// Start LHM through the task. No UAC prompt.
///
/// # Errors
/// `schtasks` cannot be started or refuses (e.g. no such task).
pub fn run() -> Result<(), TaskError> {
    let out = schtasks(&["/Run", "/TN", TASK_NAME]).map_err(TaskError::Io)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(TaskError::Run(String::from_utf8_lossy(&out.stderr).trim().to_string()))
    }
}

/// Register (or replace) the task for `exe`. Shows one UAC prompt and blocks
/// until it is answered, so call it off the UI thread.
///
/// # Errors
/// The definition cannot be written, the prompt is declined, or the task is
/// not registered afterwards.
pub fn install(exe: &Path) -> Result<(), TaskError> {
    let xml_path = std::env::temp_dir().join("hardviz-lhm-task.xml");
    write_utf16(&xml_path, &task_xml(exe)).map_err(TaskError::WriteDefinition)?;
    let xml_arg = xml_path.display().to_string();
    let result = elevated_schtasks(&["/Create", "/F", "/TN", TASK_NAME, "/XML", &xml_arg]);
    let _ = std::fs::remove_file(&xml_path); // best effort: a temp file
    result?;
    match state(exe) {
        TaskState::Current => Ok(()),
        _ => Err(TaskError::NotApplied("the task was not created")),
    }
}

/// Remove the task. Shows one UAC prompt.
///
/// # Errors
/// The prompt is declined, or the task is still registered afterwards.
pub fn uninstall() -> Result<(), TaskError> {
    elevated_schtasks(&["/Delete", "/F", "/TN", TASK_NAME])?;
    match schtasks(&["/Query", "/TN", TASK_NAME]) {
        Ok(o) if o.status.success() => Err(TaskError::NotApplied("the task is still registered")),
        _ => Ok(()),
    }
}

fn elevated_schtasks(args: &[&str]) -> Result<(), TaskError> {
    Elevated { program: Path::new("schtasks.exe"), args, working_dir: None, wait: true, hidden: true }
        .run()
        .map_err(TaskError::Elevation)
}

/// Task definition: at logon of the current user, run LHM with the highest
/// available rights, no time limit, also on battery.
fn task_xml(exe: &Path) -> String {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts LibreHardwareMonitor at login for HardViz (CPU temperature and sensors).</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{cmd}</Command>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#,
        user = xml_escape(&user),
        cmd = xml_escape(&exe.display().to_string()),
        dir = xml_escape(&dir),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// schtasks reads task XML most reliably as UTF-16LE with a BOM.
fn write_utf16(path: &Path, text: &str) -> std::io::Result<()> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(path, bytes)
}

/// What the Settings row shows about the task.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct TaskStatus {
    /// `None` until the first query finishes.
    pub task: Option<TaskState>,
    /// A query, install or removal is running.
    pub busy: bool,
    /// The last operation's error, if it failed.
    pub message: Option<String>,
    /// The bundled exe, or why there is none.
    pub bundle: Option<Result<PathBuf, BundleMissing>>,
}

/// Runs task operations on worker threads (each spawns processes, and
/// install/remove wait on a UAC prompt) and keeps their latest result.
pub struct TaskController {
    status: Arc<Mutex<TaskStatus>>,
}

impl TaskController {
    /// Start with a background query of the task's state.
    #[must_use]
    pub fn new() -> Self {
        let c = Self { status: Arc::default() };
        c.spawn(|_| Ok(()));
        c
    }

    #[must_use]
    pub fn status(&self) -> TaskStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn install(&self) {
        self.spawn(install);
    }

    pub fn uninstall(&self) {
        self.spawn(|_| uninstall());
    }

    /// Run `job` with the bundled exe on a worker thread, then re-query.
    fn spawn(&self, job: impl FnOnce(&Path) -> Result<(), TaskError> + Send + 'static) {
        let status = Arc::clone(&self.status);
        if let Ok(mut s) = status.lock() {
            s.busy = true;
        }
        let worker = std::thread::Builder::new().name("hardviz-lhm-task".into()).spawn(move || {
            let bundle = super::launch::bundled_lhm();
            let (result, task) = match &bundle {
                Ok(exe) => (job(exe), Some(state(exe))),
                Err(missing) => (Err(TaskError::NoBundle(missing.clone())), None),
            };
            if let Ok(mut s) = status.lock() {
                let message = result.err().map(|e| e.to_string());
                *s = TaskStatus { task, busy: false, message, bundle: Some(bundle) };
            }
        });
        if let Err(e) = worker {
            if let Ok(mut s) = self.status.lock() {
                s.busy = false;
                s.message = Some(format!("cannot start a worker thread ({e})"));
            }
        }
    }
}

impl Default for TaskController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escapes_paths() {
        let xml = task_xml(Path::new(r"C:\A & B\LibreHardwareMonitor.exe"));
        assert!(xml.contains(r"<Command>C:\A &amp; B\LibreHardwareMonitor.exe</Command>"));
        assert!(xml.contains(r"<WorkingDirectory>C:\A &amp; B</WorkingDirectory>"));
    }

    #[test]
    fn command_is_read_back_from_task_xml() {
        let xml = br#"<Exec><Command>"C:\A &amp; B\lhm.exe"</Command></Exec>"#;
        assert_eq!(task_command(xml).as_deref(), Some(r"C:\A & B\lhm.exe"));
        assert_eq!(task_command(b"<Task/>"), None);
    }

    #[test]
    fn command_match_tolerates_code_page_loss_only() {
        let expected = r"C:\Users\Zoë\HardViz\LibreHardwareMonitor\LibreHardwareMonitor.exe";
        assert!(command_matches(expected, expected));
        assert!(command_matches(&expected.to_uppercase(), expected), "case-insensitive");
        assert!(command_matches(&expected.replace('ë', "?"), expected), "unrepresentable char");
        assert!(command_matches(&expected.replace('ë', "\u{fffd}"), expected), "non-UTF-8 byte");
        assert!(!command_matches(&expected.replace("Zoë", "Zoe"), expected), "ASCII must match");
        assert!(!command_matches(r"C:\Other\LibreHardwareMonitor.exe", expected));
        assert!(!command_matches(&format!("{expected}x"), expected), "length must match");
    }

    #[test]
    fn protected_location_is_a_path_prefix_not_a_string_prefix() {
        let root = Path::new(r"C:\Program Files");
        assert!(starts_with_ignore_case(Path::new(r"c:\program files\HardViz\x.exe"), root));
        assert!(!starts_with_ignore_case(Path::new(r"C:\Program Files Evil\x.exe"), root));
        assert!(!starts_with_ignore_case(Path::new(r"C:\Users\me\x.exe"), root));
    }
}
