# Packaging channels

Manifests that put `hardviz` behind a package manager, so installing it is one
command instead of "download the zip, unblock it, find a folder".

Every channel is pinned to a published GitHub release and to the SHA-256 of its
asset. **`packaging/refresh.sh vX.Y.Z` rewrites all of them after you cut a
release** — do not hand-edit the hashes.

| Channel | File | Who has to act |
|---|---|---|
| winget | `winget/LiorBass.HardViz/<version>/` | PR to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) |
| Scoop | `scoop/hardviz.json` | push to your own `liorbass/scoop-bucket` repo |
| AUR | `aur/PKGBUILD`, `aur/hardviz.desktop` | push to `hardviz-bin` on aur.archlinux.org |
| Homebrew | `homebrew-tap/Casks/hardviz.rb` | create the tap repo, then `brew tap` |

Nothing here is installed or published by CI. These are inputs to the four
submissions.

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

Three files, the singleton-plus-split layout winget-pkgs asks for. Submit with
the [`wingetcreate`](https://github.com/microsoft/wingetcreate) tool or by
opening the PR by hand:

```sh
wingetcreate submit packaging/winget/LiorBass.HardViz/0.1.5/
```

Two things worth knowing before you do:

- **The ZIP is installed as a nested *portable* package.** `InstallerType: zip`
  needs winget ≥ 1.5 and must declare `NestedInstallerType` plus one
  `NestedInstallerFiles` entry per executable — `portable` is the only nested
  type that allows more than one, which is how both `hardviz.exe` and
  `hardviz-tui.exe` end up on `PATH`. `ArchiveBinariesDependOnPath: true` keeps
  the whole extracted folder in place (rather than symlinking the exe into
  WinGet's `links` directory) because HardViz looks for `LibreHardwareMonitor/`
  next to its own executable.
- **Expect review friction, not automatic acceptance.** winget-pkgs is a
  curated community repo: a human reviews each PR, and the checks reject
  anything that cannot install silently. An unauthenticated personal publisher
  ("Lior Bass") on an unsigned binary is the kind of PR that draws questions.
  Answering them is normal; the [SignPath signing](../README.md#code-signing-policy)
  makes the second half of that argument disappear, so submitting *after*
  signing is active is materially easier than before.

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

## What was checked here, and what was not

Checked: all YAML parses, the Scoop JSON parses, `ruby -c` accepts the cask,
`bash -n` accepts `PKGBUILD` and `refresh.sh`, every asset hash in these files
matches the file GitHub actually serves for v0.1.5, the paths the manifests
name exist inside those archives, and `refresh.sh` is idempotent (it restored
deliberately corrupted version/hash fields byte-for-byte, and left the
`#{version}` and `$version` templates alone).

Not checked, by definition: nothing has been validated by `winget validate`,
`scoop checkup`, `makepkg`, `brew audit` or `brew install`, and no submission
has been opened. Those tools run on Windows/Arch/macOS, none of which is this
machine. Treat each manifest as a well-formed draft until its own tooling
accepts it.
