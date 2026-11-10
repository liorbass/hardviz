//! Project tasks, run through the cargo aliases in `.cargo/config.toml`:
//!
//! ```text
//! cargo dist [--name NAME]                        release build, packaged into dist/
//! cargo tag <patch|minor|major|X.Y.Z> [--push]    check, bump the version, commit, tag
//!           [--no-verify]
//! ```
//!
//! Plain Rust, no shell scripts, so the same commands work on Windows and
//! Linux, locally and in CI.

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
      dist/hardviz-NAME.zip (Windows) or .tar.gz (elsewhere).
      NAME defaults to v<version>-<os>-<arch>.
  cargo tag <patch|minor|major|X.Y.Z> [--push] [--no-verify]
      Run the CI checks (fmt, clippy, tests), set the version in Cargo.toml,
      commit it and create the tag vX.Y.Z. --push pushes the commit and the
      tag (which starts the release workflow). --no-verify skips the checks.";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dist") => dist(&args[1..]),
        Some("tag") => tag(&args[1..]),
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
        copy(&release.join(&file), &folder.join(&file))?;
    }
    // build.rs bundles LibreHardwareMonitor next to the Windows exe.
    let lhm = release.join("LibreHardwareMonitor");
    if cfg!(windows) && lhm.is_dir() {
        copy_dir(&lhm, &folder.join("LibreHardwareMonitor"))?;
    }
    copy(&root().join("README.md"), &folder.join("README.md"))?;

    // `tar` ships with Windows 10+ (bsdtar, which writes zip with -a) and
    // every Linux, so packaging needs nothing else installed.
    let archive = if cfg!(windows) { format!("{folder_name}.zip") } else { format!("{folder_name}.tar.gz") };
    let _ = fs::remove_file(out_dir.join(&archive));
    let flags = if cfg!(windows) { "-a -c -f" } else { "-c -z -f" };
    let mut tar = Command::new("tar");
    tar.current_dir(&out_dir).args(flags.split(' ')).arg(&archive).arg(&folder_name);
    run(&mut tar)?;
    println!("packaged {}", out_dir.join(&archive).display());
    Ok(())
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
    let (mut push, mut verify) = (false, true);
    for a in args {
        match a.as_str() {
            "--push" => push = true,
            "--no-verify" => verify = false,
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
            v if how.is_none() => how = Some(v.to_string()),
            extra => return Err(format!("unexpected argument `{extra}`\n\n{USAGE}")),
        }
    }
    let how = how.ok_or_else(|| format!("which version? patch, minor, major or X.Y.Z\n\n{USAGE}"))?;

    let manifest = read_manifest()?;
    let current = package_version(&manifest)?;
    let next = current.bumped(&how)?;
    let tag = format!("v{next}");

    if !output(git().args(["status", "--porcelain"]))?.is_empty() {
        return Err("the working tree has uncommitted changes; commit or stash them first".into());
    }
    if git()
        .args(["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")])
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return Err(format!("tag {tag} already exists"));
    }
    println!("{current} -> {next} (tag {tag})");

    // The same checks as CI, before anything changes.
    if verify {
        run(cargo().args(["fmt", "--all", "--check"]))?;
        run(cargo().args(["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]))?;
        run(cargo().args(["test", "--workspace", "--locked"]))?;
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
    fn the_real_manifest_has_a_version() {
        package_version(&read_manifest().unwrap()).unwrap();
    }
}
