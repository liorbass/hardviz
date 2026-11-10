//! The running version, and a check at start for a newer release on GitHub.
//!
//! One request per start, on its own thread: `GET
//! api.github.com/repos/<owner>/<repo>/releases/latest` (the newest
//! non-draft, non-prerelease release) through the system `curl`, which ships
//! with macOS, Windows 10+ and nearly every Linux desktop, so no TLS stack is
//! compiled in. Nothing is sent but the request itself, with the user agent
//! `HardViz/<version>`. Any failure (offline, no curl, rate limit) is logged
//! and shown in Settings, never as an error dialog. The check is skipped when
//! the user turns it off (Settings → General → Updates).

use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};

/// This build's version, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The project's home, from `Cargo.toml` (`repository`).
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The list of releases, for a "see all releases" link.
#[must_use]
pub fn releases_url() -> String {
    format!("{REPOSITORY}/releases")
}

/// A published release newer than this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The version without the tag's `v`, e.g. `0.2.0`.
    pub version: String,
    /// The release's page on GitHub.
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Checking,
    UpToDate,
    Available(Release),
    /// Could not check; the reason, for a tooltip.
    Failed(String),
}

/// A check started in the background; read its outcome with [`status`](Self::status).
pub struct UpdateCheck {
    status: Arc<Mutex<UpdateStatus>>,
}

impl UpdateCheck {
    /// Start checking on a background thread. Returns at once.
    #[must_use]
    pub fn start() -> Self {
        let status = Arc::new(Mutex::new(UpdateStatus::Checking));
        let shared = Arc::clone(&status);
        let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
            let outcome = match fetch_latest() {
                Ok(json) => compare(&json, VERSION),
                Err(e) => UpdateStatus::Failed(e),
            };
            match &outcome {
                UpdateStatus::Available(r) => crate::info!("update check: {} is available ({})", r.version, r.url),
                UpdateStatus::UpToDate => crate::info!("update check: {VERSION} is the latest release"),
                UpdateStatus::Failed(e) => crate::warn!("update check failed: {e}"),
                UpdateStatus::Checking => {}
            }
            *shared.lock().unwrap_or_else(PoisonError::into_inner) = outcome;
        });
        if let Err(e) = spawned {
            *status.lock().unwrap_or_else(PoisonError::into_inner) =
                UpdateStatus::Failed(format!("cannot start the check ({e})"));
        }
        Self { status }
    }

    #[must_use]
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// A check that has already finished, for tests.
    #[cfg(test)]
    pub(crate) fn finished(status: UpdateStatus) -> Self {
        Self { status: Arc::new(Mutex::new(status)) }
    }

    /// The newer release, once the check has found one.
    #[must_use]
    pub fn available(&self) -> Option<Release> {
        match self.status() {
            UpdateStatus::Available(r) => Some(r),
            _ => None,
        }
    }
}

/// The GitHub API endpoint for the latest release of [`REPOSITORY`].
fn api_url(repository: &str) -> Option<String> {
    let path = repository.strip_prefix("https://github.com/")?.trim_end_matches('/');
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && parts.next().is_none())
        .then(|| format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"))
}

/// The latest release's JSON, via `curl`.
fn fetch_latest() -> Result<String, String> {
    let url = api_url(REPOSITORY).ok_or_else(|| format!("`{REPOSITORY}` is not a GitHub repository"))?;
    let mut curl = Command::new("curl");
    curl.args(["--silent", "--show-error", "--fail", "--location", "--max-time", "10"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(["--user-agent", &format!("HardViz/{VERSION}")])
        .arg(&url);
    #[cfg(windows)]
    {
        // A GUI app spawning a console program would flash a console window.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        curl.creation_flags(CREATE_NO_WINDOW);
    }
    let out = curl.output().map_err(|e| format!("cannot run curl ({e})"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{} ({})", err.trim().trim_start_matches("curl: "), out.status));
    }
    String::from_utf8(out.stdout).map_err(|_| "the response is not text".into())
}

/// Compare the release in GitHub's JSON with `current`.
fn compare(json: &str, current: &str) -> UpdateStatus {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json);
    let Ok(value) = parsed else { return UpdateStatus::Failed("the response is not JSON".into()) };
    let (Some(tag), Some(url)) = (value["tag_name"].as_str(), value["html_url"].as_str()) else {
        return UpdateStatus::Failed("the response has no tag_name / html_url".into());
    };
    let version = tag.strip_prefix('v').unwrap_or(tag);
    match (parse_version(version), parse_version(current)) {
        (Some(latest), Some(ours)) if latest > ours => {
            UpdateStatus::Available(Release { version: version.to_string(), url: url.to_string() })
        }
        (Some(_), Some(_)) => UpdateStatus::UpToDate,
        _ => UpdateStatus::Failed(format!("cannot compare versions `{tag}` and `{current}`")),
    }
}

/// `1.2.3` (or `1.2.3-rc.1`, `1.2.3+build`: the suffix is ignored).
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(tag: &str) -> String {
        format!(r#"{{"tag_name": "{tag}", "html_url": "https://github.com/o/r/releases/tag/{tag}", "draft": false}}"#)
    }

    #[test]
    fn newer_release_is_available() {
        let status = compare(&json("v0.2.0"), "0.1.1");
        let expected = Release { version: "0.2.0".into(), url: "https://github.com/o/r/releases/tag/v0.2.0".into() };
        assert_eq!(status, UpdateStatus::Available(expected));
    }

    #[test]
    fn same_or_older_is_up_to_date() {
        assert_eq!(compare(&json("v0.1.1"), "0.1.1"), UpdateStatus::UpToDate);
        assert_eq!(compare(&json("v0.1.0"), "0.1.1"), UpdateStatus::UpToDate);
        // Numeric, not lexical: 0.10.0 > 0.9.0.
        assert!(matches!(compare(&json("0.10.0"), "0.9.0"), UpdateStatus::Available(_)));
    }

    #[test]
    fn bad_responses_fail_instead_of_guessing() {
        assert!(matches!(compare("<html>", "0.1.1"), UpdateStatus::Failed(_)));
        assert!(matches!(compare(r#"{"message": "Not Found"}"#, "0.1.1"), UpdateStatus::Failed(_)));
        assert!(matches!(compare(&json("nightly"), "0.1.1"), UpdateStatus::Failed(_)));
    }

    #[test]
    fn versions_parse() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(parse_version(VERSION).is_some(), "this build's own version parses");
    }

    #[test]
    fn api_url_from_the_repository() {
        assert_eq!(
            api_url("https://github.com/liorbass/hardviz").as_deref(),
            Some("https://api.github.com/repos/liorbass/hardviz/releases/latest")
        );
        assert_eq!(api_url("https://github.com/liorbass/hardviz/").as_deref(), api_url(REPOSITORY).as_deref());
        assert_eq!(api_url("https://gitlab.com/a/b"), None);
        assert_eq!(api_url("https://github.com/a"), None);
    }
}
