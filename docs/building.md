# Building and releasing

For contributors: what you need, how to run the checks CI runs, how a release
is cut, and where everything lives. Looking to just *use* HardViz? Take
[install.md](install.md) instead.

## Requirements

| | |
|---|---|
| Rust | 2021 edition toolchain (`rustup` stable) |
| Windows | MSVC toolchain; CPU temperature additionally needs LibreHardwareMonitor (bundled) |
| Linux | a C toolchain plus egui's windowing/GL libraries (see below); CPU temperature needs `k10temp`, `coretemp` or `zenpower` loaded (`lm-sensors`) |
| macOS | Xcode Command Line Tools (`xcode-select --install`); nothing else |
| NVIDIA (optional) | A current driver — GPU/VRAM metrics come from NVML |

## Build and run

```sh
cargo run --release                     # the window
cargo run --release --bin hardviz-tui   # the terminal UI
```

On Debian/Ubuntu, install the build dependencies first:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev
```

Release builds land in `target/release/`:

```
target/release/
├── hardviz.exe
├── hardviz-tui.exe           # terminal UI
└── LibreHardwareMonitor/     # copied from vendor/ by build.rs (Windows only)
```

The `vendor/` → `target/<profile>/` copy is skipped when `vendor/` is absent
and on non-Windows targets. Files are re-copied only when the vendored one is
newer, so LHM settings changed in the running app (it rewrites its config on
exit) survive rebuilds.

The release profile is tuned for size and speed: `opt-level = "z"`, `lto`,
`codegen-units = 1`, `strip`.

## Tests

```sh
cargo test
```

Pure parsing and selection logic is unit tested: LHM `data.json` parsing and
the startup-task helpers (Windows), hwmon sensor picking and `/proc/diskstats`
(Linux), SMC key decoding, IOReport clock/power maths and `ioreg` parsing
(macOS), network adapter filtering and process ranking (all). The app itself
is exercised headlessly (frames run without a window), and
`tests/pipeline.rs` runs sources → poller → store → widgets end to end. Tests
never read or write your real config file.

The documentation screenshots are rendered the same way, from scripted demo
data rather than your machine, and rasterised on the CPU, so no window opens:

```sh
cargo test --lib readme_screenshots -- --ignored   # rewrites docs/images/*.png
```

## CI

`.github/workflows/ci.yml` does not run on ordinary commits. It runs when a
version tag is pushed (see [Packaging and releasing](#packaging-and-releasing)),
or when started by hand from the **Actions** tab (**CI** → **Run workflow**) to
check a commit and get test builds without publishing anything.

| Job | What it does |
|---|---|
| Format | `cargo fmt --all --check` (style in `rustfmt.toml`) |
| Clippy + test | `cargo clippy --workspace -- -D warnings` and `cargo test --workspace`, on Windows, Ubuntu and macOS (each compiles platform-only sources the others cannot) |
| Package | `cargo dist` on each platform (macOS: Apple Silicon); the Windows job first downloads LibreHardwareMonitor 0.9.6 (checksum-pinned). Packages are downloadable from the run's **Artifacts** |
| GitHub release | on a `v*` tag only: publishes every package as a [release](https://github.com/liorbass/hardviz/releases), which the download buttons in the README always point at |

Run the same checks locally before pushing:

```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

**Pre-commit hook** — enable once per clone:

```sh
cargo xtask hooks
```

Before each commit it runs `cargo fmt --all --check` and `cargo clippy`, but
only when Rust or Cargo files are staged; commits touching just docs, images
or CI skip it instantly. Fix formatting with `cargo fmt --all`; skip the hook
for one commit with `git commit --no-verify`.

## Packaging and releasing

Two cargo commands (aliases in `.cargo/config.toml`, implemented in `xtask/`):

```sh
cargo dist                  # release build, packaged into dist/ (zip on Windows, tar.gz elsewhere)
cargo tag patch             # release: checks, bump 0.1.0 -> 0.1.1, commit, tag v0.1.1, push
cargo tag --retag           # the release's CI failed: fix, commit, then re-release the same version
```

`cargo tag` takes `patch`, `minor`, `major` or an exact version
(`cargo tag 0.1.0` tags the current version without a bump). It needs a clean
working tree, runs the same fmt / clippy / test checks as CI first
(`--no-verify` skips them), then pushes the commit and the tag. The tag push
starts CI, which builds every platform and publishes the GitHub release a few
minutes later. `--no-push` stops after tagging locally.

If that CI run fails, nothing is published. Fix the problem, commit it, and run
`cargo tag --retag`: it runs the checks, moves the version's tag (local and on
GitHub) to the current commit and pushes again, which starts a new release
run. It refuses when GitHub already has a release for that version, since
people may have downloaded it; release a new version instead.

`cargo dist` on Windows bundles whatever is in `vendor/LibreHardwareMonitor/`;
for a package with LHM, extract the
[0.9.6 release](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/tag/v0.9.6)
(`LibreHardwareMonitor.zip`) there first, as CI does. The licensing side of
that arrangement is in the
[README](../README.md#bundled-third-party-binary).

## Repository layout

```
hardviz/
├── .cargo/config.toml    # cargo dist / cargo tag aliases
├── .githooks/pre-commit  # fmt + clippy before commits (enable: cargo xtask hooks)
├── xtask/                # their implementation: packaging, version bump + tag, hooks
├── build.rs              # bundles vendor/LibreHardwareMonitor next to the exe (Windows)
├── vendor/               # LibreHardwareMonitor (config + license tracked; binaries added locally)
├── docs/                 # the documentation, and the rendered images it references
├── tests/pipeline.rs     # end to end: sources -> poller -> store -> widgets, config <-> registry
└── src/
    ├── main.rs           # the window: calls startup::run
    ├── bin/hardviz-tui.rs # the terminal UI: calls tui::run
    ├── lib.rs            # crate root (everything else is a library, so it can be tested)
    ├── startup.rs        # window startup: logging, panic hook, config load, viewport
    ├── update.rs         # the version, and the release check at start
    ├── tui/              # terminal UI (ratatui): event loop, keys, and view.rs drawing
    ├── app/              # the UI shell
    │   ├── mod.rs        #   state, per-frame update, widget instances
    │   ├── dashboard.rs  #   card grid, drag-to-swap, resize grip
    │   ├── layout.rs     #   pure grid geometry (packing, fit to window, resize)
    │   ├── settings/     #   Settings panel (general, sources, metric and widget lists)
    │   ├── compact.rs    #   compact overlay
    │   ├── window.rs     #   view modes and the OS window
    │   ├── chrome.rs     #   top bar, shortcuts, Add widget panel
    │   ├── style.rs      #   shared frames and buttons, egui visuals, fonts
    │   ├── screenshots/  #   the headless demo renders behind docs/images
    │   └── test_support.rs # building the app in tests without a window
    ├── config.rs         # persisted AppConfig (serde JSON), the app directory
    ├── metrics/          # metric catalogue, units, scales, MetricStore (history + smoothing)
    ├── source.rs         # MetricSource trait, SourceRegistry, diagnostics
    ├── widget.rs         # Widget trait, WidgetCtx, WidgetRegistry
    ├── poller.rs         # background sampling thread and the state it publishes
    ├── processes.rs      # per-process sampling (CPU, GPU via NVML, I/O, memory)
    ├── nvidia.rs         # loading NVML (shared by the NVML source and processes)
    ├── bundle.rs         # the LHM copy rules, shared by build.rs and the tests
    ├── theme.rs          # colour tokens, text scale, five presets
    ├── num.rs            # the one place float/int casts are allowed
    ├── log.rs            # leveled file+stderr logger and info!/warn!/error! macros
    ├── sources/          # concrete MetricSource implementations
    │   ├── mod.rs        #   default_sources(): the priority order
    │   ├── sysinfo_source.rs
    │   ├── nvml.rs
    │   ├── lhm/          #   Windows: LibreHardwareMonitor
    │   │   ├── mod.rs    #     the source
    │   │   ├── http.rs   #     hand-rolled localhost HTTP GET
    │   │   ├── parse.rs  #     picking readings out of data.json
    │   │   ├── launch.rs #     starting LHM, UAC elevation
    │   │   └── task.rs   #     the login startup task
    │   ├── acpi.rs       #   Windows: WMI thermal zone on its own COM thread
    │   ├── hwmon.rs      #   Linux: temps via sysinfo::Components, fans via sysfs
    │   ├── diskstats.rs  #   Linux: disk throughput from /proc/diskstats
    │   └── macos/        #   macOS
    │       ├── smc.rs    #     CPU/GPU temp, fans from the SMC
    │       ├── ioreport.rs #   CPU/GPU clock and power from IOReport
    │       ├── sensors.rs #    drive temp, CPU temp fallback via sysinfo::Components
    │       └── ioreg.rs  #     disk I/O, GPU usage/memory from the I/O Kit registry
    └── widgets/          # widget implementations
        ├── mod.rs        #   default_registry(), card frame, N/A placeholder
        ├── chart.rs      #   multi-series chart primitive, NaN-gap segments
        ├── line_chart.rs #   configurable chart: any metrics, one registry entry
        ├── gauge.rs      #   arc + history for any 0-100 % metric
        ├── metric.rs     #   one metric, user-chosen visual (gauge/bar/chart/tile)
        ├── bar.rs        #   bar visual
        ├── tile.rs       #   tile visual: value + 1-minute sparkline
        ├── processes.rs  #   sortable top-processes table
        └── memory_bar.rs
```

What each `src/` module is responsible for, and the invariants it has to keep,
is in [architecture.md](architecture.md#module-map).

## Dependencies

`eframe` / `egui` 0.29 (UI), `ratatui` 0.29 with crossterm (terminal UI),
`sysinfo` 0.32 (CPU/RAM, hwmon), `nvml-wrapper` 0.10 (NVIDIA, incl.
per-process GPU), `wmi` 0.13 (Windows ACPI fallback), `serde` + `serde_json`
(config, LHM parsing), `directories` 5 (config path), `chrono` 0.4 (log
timestamps). On macOS, `core-foundation` for the SMC and IOReport sources —
both already in the tree through winit and sysinfo.
