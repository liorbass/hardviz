//! Project tasks, run through the cargo aliases in `.cargo/config.toml`:
//!
//! ```text
//! cargo dist [--name NAME]                        release build, packaged into dist/
//! cargo tag <patch|minor|major|X.Y.Z>            check, bump, commit, tag, push (= release)
//!           [--no-push] [--no-verify]
//! cargo tag --retag [--no-verify]                 retry a failed release from HEAD
//! cargo xtask hooks                               use the repo's git hooks (.githooks/)
//! cargo xtask pre-commit                          what the pre-commit hook runs
//! ```
//!
//! Plain Rust, no shell scripts, so the same commands work on Windows, Linux
//! and macOS, locally and in CI.

use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "\
usage:
  cargo dist [--name NAME]
      Release build of hardviz and hardviz-tui, packaged as
      dist/hardviz-NAME.zip (Windows) or .tar.gz (elsewhere; on macOS the
      window app is packaged as HardViz.app).
      NAME defaults to v<version>-<os>-<arch>.
  cargo tag <patch|minor|major|X.Y.Z> [--no-push] [--no-verify]
      Run the CI checks (fmt, clippy, tests), set the version in Cargo.toml,
      commit it, create the tag vX.Y.Z and push the commit and the tag, which
      starts the workflow that publishes the GitHub release. --no-push stops
      after tagging locally; --no-verify skips the checks.
  cargo tag --retag [--no-verify]
      Retry a release whose CI failed: run the checks, move the tag of the
      version in Cargo.toml (vX.Y.Z) to the current commit, and push the
      commit and the tag. Refuses if GitHub already has a release for it.
  cargo xtask hooks
      Point git at .githooks/ (once per clone), enabling the pre-commit hook.
  cargo xtask pre-commit
      The pre-commit check: fmt and clippy, only when Rust or Cargo files are
      staged. Skip a single commit with `git commit --no-verify`.";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dist") => dist(&args[1..]),
        Some("tag") => tag(&args[1..]),
        Some("hooks") => hooks(),
        Some("pre-commit") => pre_commit(),
        Some("-h" | "--help" | "help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown task `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------- versions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Version(u64, u64, u64);

impl Version {
    fn parse(text: &str) -> Result<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let bad = || format!("`{text}` is not a version like 1.2.3");
        let parts: Vec<&str> = text.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else { return Err(bad()) };
        let number = |part: &str| part.parse::<u64>().map_err(|_| bad());
        Ok(Version(number(major)?, number(minor)?, number(patch)?))
    }

    /// `patch`, `minor`, `major`, or an explicit version.
    fn bumped(self, how: &str) -> Result<Self> {
        let Version(major, minor, patch) = self;
        Ok(match how {
            "patch" => Version(major, minor, patch + 1),
            "minor" => Version(major, minor + 1, 0),
            "major" => Version(major + 1, 0, 0),
            explicit => Version::parse(explicit)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Index of the `version = "..."` line in the `[package]` table.
fn package_version_line(manifest: &str) -> Result<usize> {
    let mut in_package = false;
    for (i, line) in manifest.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
        } else if in_package && trimmed.starts_with("version") && trimmed.contains('=') {
            return Ok(i);
        }
    }
    Err("no `version` in the [package] table of Cargo.toml".into())
}

fn package_version(manifest: &str) -> Result<Version> {
    let line = manifest.lines().nth(package_version_line(manifest)?).unwrap_or_default();
    let value = line.split('=').nth(1).unwrap_or_default().trim().trim_matches('"');
    Version::parse(value)
}

/// `manifest` with the package version set to `v`, everything else untouched.
fn with_package_version(manifest: &str, v: Version) -> Result<String> {
    let at = package_version_line(manifest)?;
    let mut out: Vec<String> = manifest.lines().map(str::to_string).collect();
    out[at] = format!("version = \"{v}\"");
    let mut text = out.join("\n");
    if manifest.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

// ---------------------------------------------------------------- helpers

/// The repository root (this crate lives in `<root>/xtask`).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent directory").to_path_buf()
}

fn cargo() -> Command {
    let mut c = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    c.current_dir(root());
    c
}

fn git() -> Command {
    let mut c = Command::new("git");
    c.current_dir(root());
    c
}

/// Run a command, echoing it, and fail if it does.
fn run(cmd: &mut Command) -> Result<()> {
    let shown = format!(
        "{} {}",
        cmd.get_program().to_string_lossy(),
        cmd.get_args().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
    );
    println!("$ {shown}");
    let status = cmd.status().map_err(|e| format!("cannot run `{shown}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{shown}` failed ({status})"))
    }
}

/// Run a command and return its trimmed stdout.
fn output(cmd: &mut Command) -> Result<String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd.output().map_err(|e| format!("cannot run {program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn read_manifest() -> Result<String> {
    fs::read_to_string(root().join("Cargo.toml")).map_err(|e| format!("reading Cargo.toml: {e}"))
}

// ---------------------------------------------------------------- dist

fn dist(args: &[String]) -> Result<()> {
    let version = package_version(&read_manifest()?)?;
    let name = match args {
        [] => format!("v{version}-{}-{}", env::consts::OS, arch_label()),
        [flag, name] if flag == "--name" => name.clone(),
        _ => return Err(format!("bad arguments for dist\n\n{USAGE}")),
    };

    run(cargo().args(["build", "--release", "--locked", "--package", "hardviz", "--bins"]))?;

    let release = target_dir()?.join("release");
    let out_dir = root().join("dist");
    let folder_name = format!("hardviz-{name}");
    let folder = out_dir.join(&folder_name);
    if folder.exists() {
        fs::remove_dir_all(&folder).map_err(|e| format!("clearing {}: {e}", folder.display()))?;
    }
    fs::create_dir_all(&folder).map_err(|e| format!("creating {}: {e}", folder.display()))?;

    for bin in ["hardviz", "hardviz-tui"] {
        let file = format!("{bin}{}", env::consts::EXE_SUFFIX);
        // On macOS the window app goes into an app bundle, so Finder starts it
        // without a Terminal window; the terminal UI stays a plain binary.
        let to = if cfg!(target_os = "macos") && bin == "hardviz" {
            app_bundle(&folder, version)?.join(&file)
        } else {
            folder.join(&file)
        };
        copy(&release.join(&file), &to)?;
    }
    // build.rs bundles LibreHardwareMonitor next to the Windows exe.
    let lhm = release.join("LibreHardwareMonitor");
    if cfg!(windows) && lhm.is_dir() {
        copy_dir(&lhm, &folder.join("LibreHardwareMonitor"))?;
    }
    for doc in ["README.md", "LICENSE-MIT", "LICENSE-APACHE"] {
        copy(&root().join(doc), &folder.join(doc))?;
    }
    // Seal the bundle with an ad-hoc signature (Apple Silicon refuses to run
    // unsigned code, and the linker's signature covers the binary, not the
    // bundle). Not notarized: a downloaded copy still needs Open from Finder's
    // context menu the first time.
    if cfg!(target_os = "macos") {
        run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(folder.join("HardViz.app")))?;
    }

    // `tar` ships with Windows 10+ (bsdtar, which writes zip with -a), macOS
    // and every Linux, so packaging needs nothing else installed.
    let archive = if cfg!(windows) { format!("{folder_name}.zip") } else { format!("{folder_name}.tar.gz") };
    let _ = fs::remove_file(out_dir.join(&archive));
    let flags = if cfg!(windows) { "-a -c -f" } else { "-c -z -f" };
    let mut tar = Command::new("tar");
    tar.current_dir(&out_dir).args(flags.split(' ')).arg(&archive).arg(&folder_name);
    run(&mut tar)?;
    println!("packaged {}", out_dir.join(&archive).display());
    Ok(())
}

/// `HardViz.app` in `folder`, with its `Info.plist`; returns the directory the
/// executable goes in (`Contents/MacOS`).
fn app_bundle(folder: &Path, version: Version) -> Result<PathBuf> {
    let contents = folder.join("HardViz.app").join("Contents");
    let exe_dir = contents.join("MacOS");
    fs::create_dir_all(&exe_dir).map_err(|e| format!("creating {}: {e}", exe_dir.display()))?;
    fs::write(contents.join("Info.plist"), info_plist(version)).map_err(|e| format!("writing Info.plist: {e}"))?;
    Ok(exe_dir)
}

fn info_plist(version: Version) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>HardViz</string>
    <key>CFBundleDisplayName</key>
    <string>HardViz</string>
    <key>CFBundleIdentifier</key>
    <string>com.hardviz.HardViz</string>
    <key>CFBundleExecutable</key>
    <string>hardviz</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#
    )
}

fn arch_label() -> &'static str {
    match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Cargo's target directory, honouring `CARGO_TARGET_DIR` and config.
fn target_dir() -> Result<PathBuf> {
    let json = output(cargo().args(["metadata", "--format-version", "1", "--no-deps"]))?;
    let meta: serde_json::Value = serde_json::from_str(&json).map_err(|e| format!("cargo metadata: {e}"))?;
    meta["target_directory"].as_str().map(PathBuf::from).ok_or_else(|| "cargo metadata has no target_directory".into())
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).map(drop).map_err(|e| format!("copying {} -> {}: {e}", from.display(), to.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).map_err(|e| format!("creating {}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("reading {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            copy(&entry.path(), &dest)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- tag

fn tag(args: &[String]) -> Result<()> {
    let mut how = None;
    let (mut push, mut verify, mut retag) = (true, true, false);
    for a in args {
        match a.as_str() {
            // Pushing is the default; `--push` is still accepted.
            "--push" => push = true,
            "--no-push" => push = false,
            "--no-verify" => verify = false,
            "--retag" => retag = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
            v if how.is_none() => how = Some(v.to_string()),
            extra => return Err(format!("unexpected argument `{extra}`\n\n{USAGE}")),
        }
    }
    if retag {
        if let Some(how) = how {
            return Err(format!("--retag re-releases the version in Cargo.toml; drop `{how}`\n\n{USAGE}"));
        }
        if !push {
            return Err("--retag always pushes: retrying the release is its whole job".into());
        }
        return retag_current(verify);
    }
    let how = how.ok_or_else(|| format!("which version? patch, minor, major or X.Y.Z\n\n{USAGE}"))?;

    let manifest = read_manifest()?;
    let current = package_version(&manifest)?;
    let next = current.bumped(&how)?;
    let tag = format!("v{next}");

    require_clean_tree()?;
    if local_tag_exists(&tag) {
        return Err(format!("tag {tag} already exists (to retry a failed release of it: cargo tag --retag)"));
    }
    println!("{current} -> {next} (tag {tag})");

    // The same checks as CI, before anything changes.
    if verify {
        checks()?;
    }

    if next != current {
        fs::write(root().join("Cargo.toml"), with_package_version(&manifest, next)?)
            .map_err(|e| format!("writing Cargo.toml: {e}"))?;
        // Record the new version in Cargo.lock (no network needed).
        run(cargo().args(["update", "--workspace", "--offline"]))?;
        run(git().args(["add", "Cargo.toml", "Cargo.lock"]))?;
        run(git().args(["commit", "-m", &format!("Release {tag}")]))?;
    }
    run(git().args(["tag", "-a", &tag, "-m", &format!("HardViz {tag}")]))?;

    if push {
        run(git().args(["push", "origin", "HEAD"]))?;
        run(git().args(["push", "origin", &tag]))?;
        println!("pushed {tag}: the release workflow is starting");
    } else {
        println!("\ntagged {tag} locally. To publish the release:\n  git push origin HEAD\n  git push origin {tag}");
    }
    Ok(())
}

fn require_clean_tree() -> Result<()> {
    if output(git().args(["status", "--porcelain"]))?.is_empty() {
        Ok(())
    } else {
        Err("the working tree has uncommitted changes; commit or stash them first".into())
    }
}

fn local_tag_exists(tag: &str) -> bool {
    git().args(["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]).output().is_ok_and(|o| o.status.success())
}

/// The same checks as CI.
fn checks() -> Result<()> {
    run(cargo().args(["fmt", "--all", "--check"]))?;
    run(cargo().args(["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]))?;
    run(cargo().args(["test", "--workspace", "--locked"]))
}

/// `cargo tag --retag`: release the current version again from HEAD, after
/// its first CI run failed.
fn retag_current(verify: bool) -> Result<()> {
    let manifest = read_manifest()?;
    let tag = format!("v{}", package_version(&manifest)?);
    require_clean_tree()?;
    // A published release may already have been downloaded: moving its tag
    // would silently change what the version means. Only unreleased tags move.
    let repo = manifest_repository(&manifest)?;
    if github_release_exists(&repo, &tag)? {
        return Err(format!(
            "{tag} is already released on GitHub ({repo}/releases/tag/{tag}); \
             release a new version instead: cargo tag patch --push"
        ));
    }
    if verify {
        checks()?;
    }

    let remote = output(git().args(["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")]))?;
    if !remote.trim().is_empty() {
        run(git().args(["push", "origin", &format!(":refs/tags/{tag}")]))?;
    }
    if local_tag_exists(&tag) {
        run(git().args(["tag", "-d", &tag]))?;
    }
    run(git().args(["tag", "-a", &tag, "-m", &format!("HardViz {tag}")]))?;
    run(git().args(["push", "origin", "HEAD"]))?;
    run(git().args(["push", "origin", &tag]))?;
    println!("moved {tag} to the current commit and pushed it: the release workflow is starting");
    Ok(())
}

/// `repository = "https://github.com/owner/repo"` from the [package] section.
fn manifest_repository(manifest: &str) -> Result<String> {
    manifest
        .lines()
        .take_while(|l| !l.starts_with('[') || l.trim() == "[package]")
        .find_map(|l| l.trim().strip_prefix("repository").map(str::trim)?.strip_prefix('=').map(str::trim))
        .map(|v| v.trim_matches('"').trim_end_matches('/').to_string())
        .filter(|v| v.starts_with("https://github.com/"))
        .ok_or_else(|| "Cargo.toml has no `repository = \"https://github.com/...\"`".into())
}

/// Whether GitHub has a published release for `tag` (drafts are not public), asked through
/// the public API with `curl`. An answer other than "found" or "not found"
/// (offline, rate limit) is an error rather than a guess.
fn github_release_exists(repo: &str, tag: &str) -> Result<bool> {
    let path = repo.trim_start_matches("https://github.com/");
    let url = format!("https://api.github.com/repos/{path}/releases/tags/{tag}");
    let code = output(
        Command::new("curl").args(["--silent", "--output", NULL_DEVICE, "--write-out", "%{http_code}"]).arg(&url),
    )?;
    match code.trim() {
        "200" => Ok(true),
        "404" => Ok(false),
        other => Err(format!("could not ask GitHub whether {tag} is released (HTTP {other} from {url})")),
    }
}

const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

// ---------------------------------------------------------------- hooks

/// Use the hooks committed in `.githooks/` for this clone.
fn hooks() -> Result<()> {
    run(git().args(["config", "core.hooksPath", ".githooks"]))?;
    println!("git hooks enabled: .githooks/pre-commit runs before each commit");
    Ok(())
}

/// Whether a staged path can change what fmt or clippy report.
fn affects_build(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs")
        || matches!(
            Path::new(path).file_name().and_then(|n| n.to_str()),
            Some("Cargo.toml" | "Cargo.lock" | "rustfmt.toml")
        )
        || path.starts_with(".cargo/")
}

/// Format and lint check before a commit, skipped when only docs, images or
/// CI files are staged. Checks the working tree, which normally matches what
/// is staged.
fn pre_commit() -> Result<()> {
    let staged = output(git().args(["diff", "--cached", "--name-only", "--diff-filter=ACMRD"]))?;
    if !staged.lines().any(affects_build) {
        println!("pre-commit: no Rust or Cargo changes staged, skipping checks");
        return Ok(());
    }
    run(cargo().args(["fmt", "--all", "--check"]))
        .map_err(|e| format!("{e}\nrun `cargo fmt --all`, stage the result and commit again"))?;
    run(cargo().args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]))?;
    println!("pre-commit: fmt and clippy clean");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[package]\nname = \"hardviz\"\nversion = \"0.4.2\"\nedition = \"2021\"\n\n[dependencies]\nfoo = { version = \"1\" }\n";

    #[test]
    fn versions_parse_and_bump() {
        let v = Version::parse("v0.4.2").unwrap();
        assert_eq!(v, Version(0, 4, 2));
        assert_eq!(v.bumped("patch").unwrap(), Version(0, 4, 3));
        assert_eq!(v.bumped("minor").unwrap(), Version(0, 5, 0));
        assert_eq!(v.bumped("major").unwrap(), Version(1, 0, 0));
        assert_eq!(v.bumped("2.0.1").unwrap(), Version(2, 0, 1));
        assert!(Version::parse("1.2").is_err());
        assert!(v.bumped("next").is_err());
    }

    #[test]
    fn only_the_package_version_changes() {
        assert_eq!(package_version(MANIFEST).unwrap(), Version(0, 4, 2));
        let bumped = with_package_version(MANIFEST, Version(0, 5, 0)).unwrap();
        assert_eq!(bumped, MANIFEST.replace("version = \"0.4.2\"", "version = \"0.5.0\""));
        assert!(bumped.contains("foo = { version = \"1\" }"));
    }

    #[test]
    fn only_code_and_cargo_files_trigger_the_pre_commit_checks() {
        for path in
            ["src/tui/view.rs", "Cargo.toml", "xtask/Cargo.toml", "Cargo.lock", "rustfmt.toml", ".cargo/config.toml"]
        {
            assert!(affects_build(path), "{path}");
        }
        for path in ["README.md", "docs/images/tui.png", ".github/workflows/ci.yml", ".gitignore"] {
            assert!(!affects_build(path), "{path}");
        }
    }

    #[test]
    fn info_plist_names_the_executable_and_version() {
        let plist = info_plist(Version(0, 4, 2));
        assert!(plist.contains("<key>CFBundleExecutable</key>\n    <string>hardviz</string>"));
        assert!(plist.contains("<key>CFBundleShortVersionString</key>\n    <string>0.4.2</string>"));
    }

    #[test]
    fn repository_comes_from_the_package_section() {
        let manifest = "[package]\nname = \"x\"\nrepository = \"https://github.com/o/r/\"\n\n[dependencies]\n";
        assert_eq!(manifest_repository(manifest).unwrap(), "https://github.com/o/r");
        assert!(manifest_repository("[package]\nname = \"x\"\n[x]\nrepository = \"https://github.com/o/r\"").is_err());
        assert!(manifest_repository(&read_manifest().unwrap()).is_ok(), "the real manifest names its repository");
    }

    #[test]
    fn the_real_manifest_has_a_version() {
        package_version(&read_manifest().unwrap()).unwrap();
    }
}
