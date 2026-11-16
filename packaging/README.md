# Packaging channels

Manifests that put `hardviz` behind a package manager, so installing it is one
command instead of "download the zip, unblock it, find a folder".

Every channel is pinned to a published GitHub release and to the SHA-256 of its
asset. **`packaging/refresh.sh vX.Y.Z` rewrites all of them after you cut a
release** — do not hand-edit the hashes.

| Channel | File | Who has to act |
|---|---|---|
| winget | `winget/l/LiorBass/HardViz/<version>/` | PR to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) — [opened for 0.1.5](https://github.com/microsoft/winget-pkgs/pull/481402) |
| Scoop | `scoop/hardviz.json` | push to your own `liorbass/scoop-bucket` repo (does not exist yet) |
| AUR | `aur/PKGBUILD`, `aur/hardviz.desktop` | push to `hardviz-bin` on aur.archlinux.org (not submitted) |
| Homebrew | `homebrew-tap/Casks/hardviz.rb` | copy into [liorbass/homebrew-tap](https://github.com/liorbass/homebrew-tap) and push — live for 0.1.5 |
| crates.io | `Cargo.toml` (`exclude`) | `cargo publish`, which needs a registry token (0.1.5 unpublished) |

CI installs none of these. They are the inputs to each submission, and as of
2026-11-15 two of them have been made: the winget PR above, and the tap.

## The asset contract

These manifests assume release assets keep exactly these names inside a
`vX.Y.Z` tag, with a top-level folder of the same name in each archive:

```
hardviz-windows-x64.zip      -> hardviz-windows-x64/{hardviz.exe,hardviz-tui.exe,LibreHardwareMonitor/…}
hardviz-linux-x64.tar.gz     -> hardviz-linux-x64/{hardviz,hardviz-tui,LICENSE-*}
hardviz-macos-arm64.tar.gz   -> hardviz-macos-arm64/{HardViz.app,hardviz-tui,LICENSE-*}
```

Renaming an asset breaks winget, Scoop, AUR and Homebrew at once, and each
channel's CI will reject the update PR with a hash or path mismatch. Hash from
the **published** asset (`refresh.sh` does exactly that): once Windows signing is
active the zip on the release is the one SignPath repacked, whose digest differs
from any local `cargo dist` build.

## winget

Three files, kept in the folder layout winget-pkgs itself merges —
`manifests/<initial>/<Publisher>/<Name>/<version>/`, which here is
`winget/l/LiorBass/HardViz/0.1.5/`. Copying the folder straight across is the
whole submission; nothing has to be renamed. `ManifestVersion` is 1.28.0, the
schema their current pull-request template asks for. Either with
[`wingetcreate`](https://github.com/microsoft/wingetcreate):

```sh
wingetcreate submit packaging/winget/l/LiorBass/HardViz/0.1.5/
```

or by hand: fork, branch off `master`, add the three files under
`manifests/l/LiorBass/HardViz/0.1.5/`, and title the PR
`New package: LiorBass.HardViz version 0.1.5`.

The files submitted are generated, not copied verbatim: every merged manifest
upstream carries exactly one comment — `# yaml-language-server: $schema=…`, with
the file's own kind and `ManifestVersion` in the URL — and none of the prose that
lives here to explain the choices to whoever edits them next. So: drop the
comment lines, prepend that header, and the data is otherwise byte-identical.
The 0.1.5 submission did this by hand; if it happens again it deserves a script.

Three things worth knowing before you do:

- **The ZIP is installed as a nested *portable* package.** `InstallerType: zip`
  is in the schema from 1.4.0 on and must declare `NestedInstallerType` plus one
  `NestedInstallerFiles` entry per executable — `portable` is the only nested
  type that allows more than one, which is how both `hardviz.exe` and
  `hardviz-tui.exe` end up on `PATH`. `ArchiveBinariesDependOnPath: true` keeps
  the whole extracted folder in place (rather than symlinking the exe into
  WinGet's `links` directory) because HardViz looks for `LibreHardwareMonitor/`
  next to its own executable. The key first appears in the 1.9.0 schema, so a
  client older than that reads the manifest and ignores it.
- **Expect review friction, not automatic acceptance.** winget-pkgs is a
  curated community repo: a human reviews each PR, and the checks reject
  anything that cannot install silently. An unauthenticated personal publisher
  ("Lior Bass") on an unsigned binary is the kind of PR that draws questions.
  Answering them is normal; the [SignPath signing](../README.md#code-signing-policy)
  makes the second half of that argument disappear, so submitting *after*
  signing is active is materially easier than before.
- **Two boxes on their checklist cannot honestly be ticked from this machine.**
  The template asks the author to have run `winget validate --manifest` and
  `winget install --manifest`; there is no Windows here, so the PR says that
  plainly and leaves it to their own validation bot. What *was* checked — the
  schemas and the live digest — is in
  [the last section](#what-was-checked-here-and-what-was-not).

## Scoop

Scoop is the least demanding channel and handles a multi-file zip natively, so
this is the one to land first:

```sh
gh repo create liorbass/scoop-bucket
git clone https://github.com/liorbass/scoop-bucket /tmp/scoop-bucket
cp packaging/scoop/hardviz.json /tmp/scoop-bucket/
git -C /tmp/scoop-bucket add -A && git -C /tmp/scoop-bucket commit -m "hardviz 0.1.5" && git -C /tmp/scoop-bucket push
scoop checkup && scoop install https://github.com/liorbass/scoop-bucket/raw/main/hardviz.json
```

`checkver`/`autoupdate` let Scoop bump the version and re-hash itself, so this
channel mostly maintains itself. Anyone can then
` scoop bucket add liorbass https://github.com/liorbass/scoop-bucket`.

## AUR

`hardviz-bin` repackages the release binary; the source build is a different
package and is not what this is. Upload through the AUR web UI or:

```sh
git clone ssh://aur@aur.archlinux.org/hardviz-bin.git && cp packaging/aur/{PKGBUILD,hardviz.desktop} hardviz-bin/
cd hardviz-bin && makepkg --printsrcinfo > .SRCINFO && git add -A && git commit -m "0.1.5" && git push
```

`makepkg --printsrcinfo` output must be committed as `.SRCINFO`; it is not in
this repo because it is generated per release. Verify `package()` on a real
Arch box before the first push — the paths were checked against the released
tarball but not by `makepkg` itself.

## Homebrew

A personal tap, so no upstream review gate:

```sh
gh repo create liorbass/homebrew-tap
mkdir -p Casks && cp packaging/homebrew-tap/Casks/hardviz.rb Casks/
brew tap liorbass/tap && brew install --cask liorbass/tap/hardviz
brew audit --strict --online Casks/hardviz.rb   # run this on macOS
```

`livecheck` points at the latest release, so the cask notices new versions on
its own; only the `sha256` needs a human (or `brew bump` / `brew livecheck
--bump`).

Two macOS realities this tap cannot fix:

- **Homebrew may dodge the Gatekeeper prompt anyway.** brew fetches with curl,
  which does not set the `com.apple.quarantine` xattr that triggers
  Gatekeeper. So `brew install --cask` can work where "download in a browser,
  unzip, double-click" complains. That is an accident of the fetcher, not
  notarisation — it is still worth having, and still worth signing properly.
- **Apple Silicon only.** Releases carry no `x86_64` macOS asset, so the cask
  has one hash and the folder name says `arm64`. Add an Intel asset and a
  `Hardware::CPU` branch before claiming Intel support.

## crates.io

`cargo install hardviz` compiles the application from source. `publish = false`
is gone, and `exclude` keeps the release engineering out of the tarball: measured
with `cargo package --list`, 114 files and 6.8 MB became 96 files with no
`vendor/`, no `packaging/`, no CI, signing or xtask config, and neither README
animation — one GIF alone was 4.07 MB of it.

What a registry build cannot have is LibreHardwareMonitor. `cargo package`
follows `.gitignore`, and the LHM binaries are fetched into `vendor/` by the
release workflow instead of committed, so `bundle::copy_tree` would copy a folder
holding only a license and a config and report that as success. `build.rs` asks
`bundle::has_runtime` first and warns, on Windows only — because at runtime all
the application can say is that a folder is missing.

Publishing is manual, and stays that way: `cargo login` once on a machine that
has the token (not this one), then `cargo publish` immediately after the tag, so
the version in `Cargo.toml` and the release it points at cannot drift. A CI job
would mean putting a registry token in the pipeline that writes releases.

## What was checked here, and what was not

Checked: all YAML parses, the Scoop JSON parses, `ruby -c` accepts the cask,
`bash -n` accepts `PKGBUILD` and `refresh.sh`, every asset hash in these files
matches the file GitHub actually serves for v0.1.5, the paths the manifests
name exist inside those archives, and `refresh.sh` is idempotent (it restored
deliberately corrupted version/hash fields byte-for-byte, and left the
`#{version}` and `$version` templates alone).

The three winget manifests additionally validate against the real 1.28.0 JSON
schemas from [microsoft/winget-cli](https://github.com/microsoft/winget-cli)
(they passed 1.12.0 too, and the folder was then moved to that version), key by
key: required properties present, no key outside the schema's list at either the
top level or inside `Installers`. That check is what caught `Architectures:`
where the schema wants `Installers`, `InstallerScope` where it wants `Scope`,
and `Commands` in the defaultLocale file, which only the installer manifest
allows. Note that the schemas do not set `additionalProperties: false`, so a
plain validator accepts invented keys silently — the key list has to be walked
separately from the shape.

Not checked, by definition: nothing has been validated by `winget validate`,
`scoop checkup`, `makepkg`, `brew audit` or `brew install`. Those tools run on
Windows/Arch/macOS, none of which is this machine, so the tap's first real test
is somebody else's Mac. A schema-legal manifest is also not the same as an
installer that runs: how `InstallModes` and `ArchiveBinariesDependOnPath` behave
for a nested portable ZIP is taken from the documentation, not observed. The
winget PR tells the reviewer exactly that instead of ticking boxes it cannot
honour.

That gap has partly closed on its own since the PR was opened: winget-pkgs runs
its own validation pipeline, and 01 Pull Request Validation, 02 Manifest
Validation, 03 URLs Validation, 04 URL Domain Validation and 06 Catalog Content
Verification all report SUCCESS on 481402, with 05 Manifest Policy Validation
NEUTRAL. The four steps that actually download the installer and install it
(07-10) are still queued behind the Contributor License Agreement, which only
the author can sign. So the schema checking above is no longer the only
evidence — it is the part that was done before a Windows machine was involved.

Two submissions exist as of 2026-11-15: [winget pull request 481402](https://github.com/microsoft/winget-pkgs/pull/481402)
and the [tap](https://github.com/liorbass/homebrew-tap). The Scoop bucket, the
AUR package and the crates.io publish do not — the last only because a registry
token is not on this machine; everything it needs is already on `main`.
