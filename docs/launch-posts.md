# Launch posts and submissions

Ready-to-send text for the one-time backlinks. Nothing here has been posted:
these are drafts for you to put your own name on, because on every one of these
platforms the account and the persona are yours, not mine.

**Numbers used below, measured from the v0.1.5 assets on 2026-11-15** — quote
these, not adjectives:

| | Windows | Linux | macOS (arm64) |
|---|---|---|---|
| GUI binary | 4.3 MiB | 7.3 MiB | 3.5 MB (`.app`) |
| TUI binary | 1.5 MiB | 1.4 MiB | 1.1 MiB |
| Whole package | 9.5 MiB zip (incl. LibreHardwareMonitor) | — | — |

Do **not** claim a memory or CPU footprint, "lighter than X", or any ranking —
none of that has been measured. No benchmark, no number.

---

## 1. Hacker News — Show HN

Post the **repository URL** as the story. Title (≤ 80 chars, no marketing voice):

```
Show HN: HardViz – a hardware monitor where you arrange the cards yourself
```

Alternates, if you want to lead with a different angle:

```
Show HN: HardViz – hardware monitoring in Rust, same sources in a GUI and a TUI
Show HN: HardViz – a 640x96 always-on-top sensor strip that isn't Electron
```

Then immediately post this as your own **first comment** (this is where the
thread actually happens; the story itself is just the link):

> I wrote HardViz because I wanted a hardware monitor whose layout I could
> actually set up the way I wanted — which cards, how big, where — instead of
> picking from four presets. It's Rust, with egui for the window and ratatui for
> the terminal UI, and both read the same sources, so the TUI over SSH shows the
> same numbers as the desktop.
>
> Two things I was picky about:
>
> **It never guesses.** If a sensor can't be read, the card says `N/A` and
> hovering it tells you why. I got tired of tools showing a confident 0% and
> leaving me to work out whether that was real. In practice this meant building
> a fallback chain per metric (CPU temp on Windows is LibreHardwareMonitor →
> ACPI → hwmon) and being honest at the end of it.
>
> **It keeps watching when hidden.** Sampling happens on its own thread, so
> history keeps filling while the window is minimized. That sounds small and it
> is, but it's why the charts are actually useful.
>
> The readings come from LibreHardwareMonitor's HTTP endpoint on Windows,
> hwmon/`/proc/diskstats` on Linux, SMC/IOReport/I-O Kit on macOS, NVML for
> NVIDIA, and `sysinfo` everywhere. Adding a sensor is one constant and one
> `out.set(...)`; adding a card is one registry line. That is the part I'd most
> like critique — the plugin-ish split between sources and widgets.
>
> No installer, no service, no account. It sends nothing over the network on its
> own; the one request it can make is an update check to GitHub which is off
> unless you turn it on.
>
> Fair warnings: it's six days old, the Windows binaries aren't signed yet (I've
> applied to SignPath, SmartScreen will complain until then), and macOS is Apple
> Silicon only. The README's download buttons go straight to the release.
>
> Happy to answer anything about the egui/Rust side in particular — I'm not a GUI
> person by training.

Notes for when you post:

- Weekday morning Pacific (roughly 14:00–16:00 UTC Tue–Thu) is when an OSS
  tool like this gets seen.
- **Do not** ask friends to upvote; HN detects it and kills the account.
- The self-disclosure ("I wrote this") is required, not polite.
- If the Windows signing lands first, mention it once in a reply rather than in
  the top comment.
- The two most likely critical questions are "why not btop/HWiNFO/Mission
  Center" and "isn't this just another egui demo app". Answer the first with the
  layout story, not a feature matrix.

## 2. r/rust

Title:

```
Show this month: HardViz – hardware monitor in Rust; egui GUI and ratatui TUI over one set of sources
```

Body:

> Six days ago I got tired of hardware monitors where I can't choose which cards
> exist or how big they are, so I wrote one.
>
> - `eframe`/`egui` for the window, `ratatui` for the terminal UI, and both sit
>   on the same source layer — `sysinfo`, NVML, hwmon, `/proc/diskstats`, SMC /
>   IOReport on macOS, LibreHardwareMonitor's HTTP endpoint on Windows. A metric
>   is an id like `cpu.temp`; several sources can provide it and the first
>   healthy enabled one wins, which is what gives CPU temp its automatic
>   fallback chain.
> - Sampling runs on its own thread, so history keeps filling while the window
>   is hidden.
> - Widgets ask the store for ids and draw whatever is there. Adding a card is
>   one registry entry; adding a reading is one constant plus one `out.set`.
> - Single binary per platform (4–7 MB), no installer, no telemetry. The only
>   network request it can make is an opt-in update check to GitHub.
> - `cargo run --release` works on all three OSes; releases have Windows x64,
>   Linux x64 and macOS arm64.
>
> Repo: <https://github.com/liorbass/hardviz>
>
> Rust-specific things I'd like feedback on: whether the `Readings`/`MetricStore`
> split is over-abstracted for a one-person project, and whether keeping three
> different `windows` crate versions in the tree (eframe, wmi, sysinfo) is
> something I should fight or just document.

## 3. egui Discussions / Matrix

Short, because that audience already knows the tool:

> **HardViz — a full hardware monitor built on egui + ratatui**
>
> Modular dashboard: drag-to-reorder grid, resizable cards, an always-on-top
> 640×96 compact overlay, five themes. Also a TUI binary that reuses the same
> sources and theme colours.
>
> What egui specifically bought me: the whole app is immediate-mode, so the
> layout editor (drag, resize handles, per-card menus, lock toggle) was
> straightforward, and I could render README screenshots and GIFs **headlessly**
> — `Context::run_ui` plus a CPU rasteriser, no window, no GPU, deterministic
> demo data. Repo: <https://github.com/liorbass/hardviz>

## 4. This Week in Rust

Submit via a PR to <https://github.com/rust-lang/this-week-in-rust> — check its
current contributing note for which section accepts project links; there is a
"Rust Projects" section for one-or-two-line entries. Entry text:

> **[HardViz](https://github.com/liorbass/hardviz)** — a hardware monitoring
> dashboard in Rust where the layout is yours: an egui window with a
> drag-and-drop card grid, an always-on-top compact overlay, and a `ratatui`
> front-end that reads the same sources over SSH.

A separate, higher-probability route: post it in the weekly **"Rust in
Production"/"Project showcase"** thread and the `r/rust` "What's everyone
working on" thread, which are low-effort and recurring.

## 5. Awesome-list PRs

Each is one line in one table. Match each list's existing format exactly and
keep the description to the feature, not the pitch.

`awesome-egui` (probably the highest-acceptance-odds one — small list, on-topic):

```markdown
* [HardViz](https://github.com/liorbass/hardviz) - Hardware monitoring dashboard with a drag-and-drop card layout, an always-on-top compact overlay, and a companion ratatui TUI.
```

`awesome-rust` → *Utilities* / *System monitoring* area:

```markdown
* [HardViz](https://github.com/liorbass/hardviz) - Cross-platform hardware monitor: gauges, streaming charts, sensor tiles, per-core CPU, `N/A` instead of guessed values.
```

`awesome-linux-apps` / an `awesome-sysadmin` monitoring section:

```markdown
* [HardViz](https://github.com/liorbass/hardviz) - Hardware and resource monitor with a GUI dashboard and a terminal UI over the same hwmon/NVML/sysinfo sources.
```

Before each PR: read that repo's contribution rules (most require the link to be
in alphabetical order, one line, no exclamation marks, and no self-promotion of
a repo with fewer than N stars — `awesome-rust` in particular is selective, so
expect `awesome-egui` to land first).

## 6. AlternativeTo

Register HardViz as an open-source alternative to each of **HWiNFO, HWMonitor,
iStat Menus, Rainmeter, conky, btop, Mission Center** and **Glances**. Those
comparison pages rank on Google for "X alternative" and keep sending traffic for
years. Fields:

- One-liner: `Modular hardware monitoring dashboard with a drag-and-drop card layout, an always-on-top overlay and a terminal UI. Rust, cross-platform.`
- Tags: Hardware Monitoring, System Information, Cross-platform, Lightweight, Terminal/CLI, Customizable, Open Source
- Platforms: Windows, Linux, macOS
- License: MIT / Apache-2.0

## 7. Niche posts that are actually on-target

These are the highest-intent audiences; they are also the ones that will tell you
what is missing.

- **r/DataHoarder** — angle: the hottest-drive card, per-drive temperature
  history, and CSV export of retained history. Ask whether people want alerting.
- **r/buildapost** / a custom-loop-PC sub — angle: fan and power tiles, and
  "it doesn't need to be installed to check a build".
- **r/OBS** and streaming-adjacent subs — angle: the 640×96 overlay as a
  hardware HUD for a second monitor or a scene. This is the wedge: the ask is
  small and specific ("a thin strip with CPU/GPU/fan that stays on top"), and
  the incumbents do it with Electron or a paid app.
- **TouchDesigner / vj / homelab Discords and subs** — only if someone asks for a
  monitoring HUD; do not cold-post.

## 8. Order and gates

| Step | Ready? | Gate |
|---|---|---|
| README GIFs, theme gallery, tagline | done in-repo | needs committing + pushing |
| GitHub description + 10 topics | **live on the repo already** | — |
| Scoop bucket | manifest ready | needs a `liorbass/scoop-bucket` repo |
| AUR `hardviz-bin` | manifest ready | needs AUR account + a real `makepkg` run |
| Homebrew cask | manifest ready | needs `liorbass/homebrew-tap` repo; verify on a Mac |
| winget | manifest ready | needs a PR to winget-pkgs; **easier after signing** |
| Show HN / r/rust | text ready | post only after at least one package manager works, so the top comment can say `scoop install hardviz` |
| TWiR + awesome lists + AlternativeTo | text ready | independent, can go any time |

The one real dependency: the launch posts are worth much more once a reader can
install with one command. Ship Scoop first (it needs no third-party review), then
post.
