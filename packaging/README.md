# Packaging channels

Manifests that put `hardviz` behind a package manager, so installing it is one
command instead of "download the zip, unblock it, find a folder".

Every channel is pinned to a published GitHub release and to the SHA-256 of its
asset. **`packaging/refresh.sh vX.Y.Z` rewrites all of them after you cut a
release** — do not hand-edit the hashes.

| Channel | File | Who has to act |
|---|---|---|
| winget | `winget/l/LiorBass/HardViz/<version>/` | PR to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) — [481402](https://github.com/microsoft/winget-pkgs/pull/481402) passed their install validation, waiting on an admin's review |
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
- **Both boxes on their checklist get ticked here.** The template asks the author
  to have run `winget validate --manifest` and `winget install --manifest`. This
  repository sits on the Windows filesystem with WSL interop enabled, so
  `winget.exe` is a command away: client 1.29.380 validated the submitted bytes
  and installed the real release ZIP. Quote the tool's verdict, not the schema's —
  they disagreed once, and the tool was right
  ([details](#what-was-checked-here-and-what-was-not)).

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

Not checked, by definition: nothing has been validated by `scoop checkup`,
`makepkg`, `brew audit` or `brew install`. Those run on Arch and macOS, neither of
which is this machine, so the tap's first real test is somebody else's Mac.

The winget half of that list was wrong when it was written, and the correction is
the most useful thing in this section. `winget.exe` is reachable from here —
`/proc/sys/fs/binfmt_misc/WSLInterop` is enabled and the repo is on `C:\` — so the
real client has run against the exact bytes in the pull request. Validate the
submitted copy rather than the folder in this repo (stage it with
`gh api repos/liorbass/winget-pkgs/contents/<path>?ref=hardviz-0.1.5 --jq .content
| base64 -d`), because the copies here keep maintainer prose and drop the
`$schema` header, and the client warns about that header. Client v1.29.380,
`winget validate`:

```
Manifest validation succeeded with warnings.                                                   (exit 0)
Manifest Warning: Scope is not supported for InstallerType portable.
```

`winget install --manifest` then downloaded the published ZIP and installed it:
registered as `ARP\User\X64\LiorBass.HardViz__DefaultSource` at 0.1.5, unpacked to
`%LOCALAPPDATA%\Microsoft\WinGet\Packages\LiorBass.HardViz__DefaultSource\hardviz-windows-x64\`
with `LibreHardwareMonitor\` still beside `hardviz.exe`, and that folder appended to
the user `Path`. Nothing appeared in `WinGet\Links` — which is
`ArchiveBinariesDependOnPath: true` doing the documented thing instead of merely
being documented. So the sentence this section used to carry, that nested portable
ZIP behaviour is "taken from the documentation, not observed", no longer applies.

Two findings from that are worth keeping in mind for the next release:

- **The client knows things the schema does not say.** `Scope` is a legal key, so
  both the Draft-07 validation and the positional key walk passed it; only the
  client knows it means nothing for a nested *portable* package. `Scope: user` is
  still in the open PR pending a decision to amend it — it changes no behaviour,
  but it is the one warning a reviewer will ask about.
- **A manifest install cannot be verified from the shell that ran it.** winget
  updates `Path` in the registry, and a process keeps the environment it started
  with, so `Get-Command hardviz` fails in that window and works in a new one.
  Neither binary takes arguments, so the check is files and `PATH`, never
  `--version`.

Their side of the evidence, and it ended up stronger. The CLA was signed on
2026-11-16 and their pipeline ran to completion on 481402: 01 Pull Request
Validation, 02 Manifest Validation, 03 URLs Validation, 04 URL Domain Validation,
06 Catalog Content Verification, 07 Installers Scan, **08 Installation Validation**
(41 minutes: their machine downloaded the ZIP and installed it), 09 Installer
Metadata Validation and 10 Validation Completed all **pass**. 05 Manifest Policy
Validation reports *skipping* into the label-driven path, with a bot note that the
pull request needs a repository admin's manual review. `mergeable` is `MERGEABLE`
and `mergeStateStatus` is `BLOCKED`: what is left is a person, not a checksum.

The install run above is therefore not the load-bearing evidence — 08 is, on their
hardware and their client. The local run still earned its keep by catching `Scope`,
which neither pipeline step complained about.

### Unsolicited advice about `Commands`

A community account left a comment calling the top-level `Commands` block a
"cleanup item the validator won't flag": both names are already
`PortableCommandAlias` values, and the claim was that repeating them "can make
tooling that reads `Commands` as a separate shim set report duplicate commands".
Not acted on, and worth writing down because it reads like a review and is not
one — the account is not on the
[moderator list](https://github.com/microsoft/winget-pkgs/blob/master/doc/Moderation.md),
the comment is not a review, and no check on the pull request asks for it:

- The schema's own description of the field is *"List of commands or aliases to run
  the package"* — catalog metadata. Its `uniqueItems` applies inside that array, so
  there is no cross-field uniqueness rule for it to violate, and in the client
  source the field is a plain `std::vector<string_t>` on the installer record.
- Measured here with this manifest: `ArchiveBinariesDependOnPath: true` created
  nothing in `WinGet\Links`, so there was no second shim set to duplicate.
- Steps 08 and 09 — the ones that install the package and compare metadata against
  what the install produced — passed with `Commands` present.
- Pushing to the branch restarts that 41-minute pipeline. Spend a restart on the
  one real defect (`Scope`), not on a style preference from a reader with no
  standing over the merge.

Two submissions exist as of 2026-11-16: [winget pull request 481402](https://github.com/microsoft/winget-pkgs/pull/481402)
— CLA signed, every automated step green, an admin's review outstanding — and the
[tap](https://github.com/liorbass/homebrew-tap). The Scoop bucket, the
AUR package and the crates.io publish do not — the last only because a registry
token is not on this machine; everything it needs is already on `main`.
