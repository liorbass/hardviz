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
| Homebrew | `homebrew-tap/Casks/hardviz.rb` | copy into [liorbass/homebrew-tap](https://github.com/liorbass/homebrew-tap) and push — live for 0.1.5, `brew audit` and `brew install` both green in CI |
| crates.io | `Cargo.toml` (`exclude`) | `cargo publish` right after the tag — live for 0.1.5 |

CI installs none of these for anybody's sake, though since 2026-11-16 it audits
one of them and has run its installer once (see [Homebrew](#homebrew)). These
files are the inputs to each submission, and as of 2026-11-16 three have been
made: the winget PR above, the tap, and the crates.io publish.

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
brew install --cask liorbass/tap/hardviz    # what a user runs; clears the trust gate by itself
brew trust liorbass/tap                     # maintainer-only: audit and info load the file
brew audit --cask --strict --online liorbass/tap/hardviz
```

`brew trust` is not decoration, and it is not for users. Homebrew will not *load* a cask from a tap
that is not official, and a command has to load the file in order to read it:

```
Refusing to load cask liorbass/tap/hardviz from untrusted tap liorbass/tap.
Run `brew trust --cask liorbass/tap/hardviz` or `brew trust liorbass/tap` to trust it.
```

That is the first thing the audit job ever hit here, and it is a real fact about the channel rather
than a CI quirk. Read from the same source, `Trust.explicitly_allowed?` returns true when the
running command is `install`, `reinstall` or `upgrade` and the fully-qualified name is one of the
arguments — which is why the one-liner above is not blocked by the gate while an audit of that very
same cask is. That used to be reasoning about the code. It is now measured, on a cold runner, by the
job itself, before anything is trusted; exit codes are printed rather than raised, because this step
is a measurement and only the audit below is a gate:

```
######## nothing has been trusted yet ########

=== brew outdated --cask ===
exit=0

=== brew upgrade --cask --dry-run ===
exit=0

=== brew info --cask liorbass/tap/hardviz ===
Error: Refusing to load cask liorbass/tap/hardviz from untrusted tap liorbass/tap.
Run `brew trust --cask liorbass/tap/hardviz` or `brew trust liorbass/tap` to trust it.
exit=1

######## the README one-liner ########

=== brew install --cask liorbass/tap/hardviz ===
==> Trusted cask liorbass/tap/hardviz
==> Fetching downloads for: liorbass/tap/hardviz
✔︎ Cask hardviz (0.1.5)
==> Installing Cask hardviz
==> Moving App 'HardViz.app' to '/Applications/HardViz.app'
==> Linking Binary 'hardviz-tui' to '/opt/homebrew/bin/hardviz-tui'
🍺  hardviz was successfully installed!
==> Caveats
==> hardviz
HardViz is not signed or notarised yet. If macOS refuses to open it, right-click HardViz.app and choose Open once.
exit=0
```

Three things came out of that, two of them better than the inference was. Naming the cask in an
install is consent, as expected — but brew does not spend it on that one command, it *records* it
(`==> Trusted cask liorbass/tap/hardviz`, kept in Homebrew's trust file), and the three commands
that had run beforehand all exit 0 once it has happened. So the trust gate is a first-contact gate on
commands that merely read the tap, not a wall around the channel: `brew info` with the full name typed
exactly as documented still refused, because `info` is not an install-shaped command, and the same
`brew info` afterwards printed the whole cask — `Required: macOS >= 11`, both artifacts, the caveats.
And `brew outdated`/`brew upgrade` exiting 0 *before* the install proves nothing either way: with
nothing installed, neither one had any reason to load our file.

Trust is otherwise remembered only by `brew trust` or a Brewfile `trusted:` entry, and
`HOMEBREW_NO_REQUIRE_TAP_TRUST` is already deprecated upstream, so nothing here should lean on it.
The case that has still never been observed is the one nobody has a reason to be in: a cask installed
while its tap is untrusted, which `brew bundle` from a Brewfile is the plausible way to reach.

`.github/workflows/packaging.yml` runs this weekly and on demand on `macos-latest`, in the order the
facts above need: tap, observe the untrusted state, trust, diff `Casks/hardviz.rb` against the copy
in this repository so the two cannot drift, audit. The observation block is deliberately not a gate —
it prints exit codes and carries on, so a red job still means a broken channel rather than an answer
nobody expected. The audit itself has passed since the trust step went in: style cops, the live
download of the real asset, the extraction of its artifacts, no offences.

### The install found what the audit did not

`brew audit --cask --strict --online` reports no offence for the line below, and every brew command
that loads this cask prints it in front of the user anyway:

```
Warning: Calling string comparison format for `depends_on macos:` is deprecated! Use `depends_on macos: :big_sur` instead.
Please report this issue to the liorbass/homebrew-tap tap (not Homebrew/* repositories), or even better, submit a PR to fix it:
  /opt/homebrew/Library/Taps/liorbass/homebrew-tap/Casks/hardviz.rb:16
```

That is `odeprecated` firing inside `MacOSRequirement.parse` on the string `">= :big_sur"` — install,
info, outdated and upgrade each printed it, and the sentence invites people to open an issue against
the tap. The cask now says `depends_on macos: :big_sur`. A bare symbol is not a narrower requirement:
`parse` hands it the comparator its caller already chose, which for `depends_on macos:` is `">="`, so
it is the same check the string expressed and the same one `brew info` renders as
`Required: macOS >= 11`. Nothing about who can install changed; the warning did.

The dated obligation that comes with it: Homebrew's own note in `MacOSVersion::RELEASES` puts the
removal of Big Sur, and of macOS on x86_64, at "September (or later) 2027". When `:big_sur` moves into
`DISABLED_MACOS_VERSIONS`, this line has to be deleted rather than left to warn.

An audit checks the cask; an install checks what people see. The audit had already been green for an
hour while every command a user could run told those users to report this tap for a bug.

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

Two of the audits that `brew audit --online` runs are checkable without a Mac, because each one
compares the cask against the artifact, and the artifact is public:

- `audit_min_os` reads `LSMinimumSystemVersion` out of the app bundle and expects the cask's
  `depends_on macos:` to say the same thing. The released bundle says `11.0` in `Info.plist` and
  the Mach-O agrees (`LC_BUILD_VERSION` minos `11.0.0`); the cask asks for `:big_sur` with the `>=`
  comparator that `depends_on macos:` supplies by default, which is that version — `brew info` says
  `Required: macOS >= 11` — and `:big_sur` is still a live symbol upstream, whose removal Homebrew
  has pencilled in for September 2027.
- `audit_artifact_case` walks every stanza path through the archive with case included, and the
  `sha256` has to be the digest GitHub actually serves. Both stanza paths are in the tarball
  exactly as written, and the hash matches the live asset byte for byte.

`audit_signing` is the one that a still-ad-hoc-signed build might be expected to trip, and it
does not: the method returns early unless the cask comes from an official tap or signing was
explicitly asked for. A personal tap gets no notarisation verdict either way, so the `caveats`
line about right-clicking to open stays the only thing saying it.

What `brew audit` did on that runner covers exactly the two audits above, and it agreed with them:
the download, the checksum, the extraction, the stanza paths and the minimum-macOS comparison all
passed, as did the style cops, and no offence was reported. Reading the source first was still
worth it — it is how the failure that did show up was recognisable as a trust gate rather than a
cask defect in ten seconds instead of ten minutes.

What that install settles is the pair of claims the documentation had been making on faith:
`HardViz.app` lands in `/Applications`, and `hardviz-tui` is linked into the prefix —
`-> /opt/homebrew/Caskroom/hardviz/0.1.5/hardviz-macos-arm64/hardviz-tui` — with `brew list --cask`
reporting `hardviz` afterwards.

What it does not settle is the rest of the machine. That runner was Apple Silicon, a fresh
`/opt/homebrew`, with nothing to upgrade over and Homebrew as it is this week. An upgrade from 0.1.5
to 0.1.6 over an existing Caskroom entry, an Intel Mac with a `/usr/local` prefix, and whether the
`caveats` line is ever needed by someone who fetched the tarball in a browser rather than through
brew: still untested. This machine still has no `brew` at all — not on `PATH`, no `/home/linuxbrew` —
so everything above came off a runner, not from here.

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

Publishing stays manual, and that is deliberate: `cargo publish` immediately after the tag, so
the version in `Cargo.toml` and the release it points at cannot drift. A CI job would mean
putting a registry token in the pipeline that writes releases. The half that needs no token is
the pre-flight — `cargo publish --dry-run --locked` packages, builds the resulting tarball from
scratch in a temporary directory, and stops before the upload. 0.1.5 went through that and is on
the registry as of 2026-11-16.

## What was checked here, and what was not

Checked: all YAML parses, the Scoop JSON parses, `ruby -c` accepts the cask,
`bash -n` accepts `PKGBUILD` and `refresh.sh`, every asset hash in these files
matches the file GitHub actually serves for v0.1.5, the paths the manifests
name exist inside those archives, and `refresh.sh` is idempotent (it restored
deliberately corrupted version/hash fields byte-for-byte, and left the
`#{version}` and `$version` templates alone).

The crates.io channel has been exercised end to end: `cargo install hardviz --locked` against
the published 0.1.5 into a throwaway prefix fetched the crate from the registry, built it, and
left `hardviz` and `hardviz-tui` in its `bin` (1m 29s on a warm dependency cache). For the tap,
its `sha256` is the digest of the asset GitHub actually serves, both stanza paths —
`hardviz-macos-arm64/HardViz.app` and `hardviz-macos-arm64/hardviz-tui` — exist inside that
archive, and `livecheck` resolves to the newest release. That was everything about the cask that
could be checked without a Mac; a Mac in CI has since installed it, and
[Homebrew](#homebrew) is what came out of that.

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

Not checked, by definition: nothing has been validated by `scoop checkup` or `makepkg` — those run on
Arch and Windows. `brew install` and `brew audit` were on that list until 2026-11-16, when
`.github/workflows/packaging.yml` ran each of them on `macos-latest` against the live tap: the audit
passed, and so did the install; see [Homebrew](#homebrew).

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
  client knows it means nothing for a nested *portable* package. Neither the
  schemas nor their pipeline ever complained about it — the local `winget validate`
  run did, and it is the reason the manifest changed
  ([see below](#one-push-for-both)).
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

### One push for both

A community account left a comment calling the top-level `Commands` block a
"cleanup item the validator won't flag": both names are already
`PortableCommandAlias` values, and repeating them "can make tooling that reads
`Commands` as a separate shim set report duplicate commands".

The block is gone, but not because that reasoning holds — the difference matters
the next time something arriving in a comment thread looks like a requirement:

- The account is not on the
  [moderator list](https://github.com/microsoft/winget-pkgs/blob/master/doc/Moderation.md),
  the comment is not a review, and no check on the pull request asked for it. It is
  advice, and advice gets weighed rather than obeyed.
- The mechanism is not what the field is. The schema describes `Commands` as
  *"List of commands or aliases to run the package"* — catalog metadata whose
  `uniqueItems` applies inside its own array, so there is no cross-field uniqueness
  rule it could violate. In the client source it is a plain `std::vector<string_t>`
  on the installer record.
- Measured here with this manifest: `ArchiveBinariesDependOnPath: true` created
  nothing in `WinGet\Links`, so there was never a second shim set to duplicate.
- Steps 08 and 09 — install the package, then compare metadata against what the
  install produced — both passed with `Commands` present.

What settled it is that the key does nothing for us: the two names reach `PATH`
with or without it, so there is no functional argument worth dying on, and `Scope`
has to be pushed regardless because the client actually warns about it. One push,
two changes, one 41-minute revalidation instead of two. The fixed files, with the
`$schema` headers restored the way submission does them, now report:

```
Manifest validation succeeded.
```

Zero warnings, exit 0, client 1.29.380. That is the form 481402 carries now
(`2cb3b6f`), and the push did cost what it was expected to cost: every check went
from *pass* back to queued, because touching a branch there re-runs the whole
thing. That is a price, not a signal that anything was wrong — and it came out
cheap: the re-run finished with 01 through 04, 06 through 10 all **pass** again on
the warning-free manifest, 08 Installation Validation included, `mergeable` back to
`true`. So the removals are proven harmless by their own pipeline, not just by the
local `winget validate`. `Scope` was worth buying; `Commands` shared the restart.

Three channels are live as of 2026-11-16: the crate on
[crates.io](https://crates.io/crates/hardviz), the
[tap](https://github.com/liorbass/homebrew-tap), and [winget pull request 481402](https://github.com/microsoft/winget-pkgs/pull/481402)
— CLA signed, every automated step passed twice, once with the warning and once
without, waiting now on a repository admin (`Policy-Test-2.7`, `Validation-Guide`).
The Scoop bucket and the AUR package are the two that do not exist yet.
