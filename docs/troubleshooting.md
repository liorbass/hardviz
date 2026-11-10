# Troubleshooting

The log is the first place to look. It is written to `<config_dir>/hardviz.log`
(where that is depends on your OS — see
[configuration.md](configuration.md)) **and** to stderr, truncated on each
start so one run is one file, and a panic hook records crashes that take the
window with them. The terminal UI logs to `hardviz-tui.log` beside it, never to
the screen.

Most `N/A`s are not bugs: they are the truth about your hardware. When a
sensor cannot be read, the card says so and hovering it explains why — and
Settings → Metrics shows every source that was asked. The per-platform sections
of [data-sources.md](data-sources.md) cover what each sensor needs.

| Symptom | Cause / fix |
|---|---|
| CPU temp shows `N/A` with "cannot reach http://127.0.0.1:8085/data.json" | LHM is not running or its Remote Web Server is off. See [CPU temperature on Windows](data-sources.md#cpu-temperature-on-windows). |
| `N/A` with "MSAcpi_ThermalZoneTemperature returned 0 zone(s)" | ACPI fallback found no thermal zones — normal on modern boards. Use LHM. |
| `N/A` with "no CPU sensor among N hwmon reading(s)" (Linux) | CPU driver not loaded. `sudo modprobe k10temp` / `coretemp`, or run `sensors-detect`. In WSL this is expected: there are no hardware sensors. |
| A UAC prompt every time HardViz starts (Windows) | LHM is being launched directly. Set up the startup task in Settings → Data sources. |
| All GPU/VRAM metrics `N/A`, log says "NVML not available" | No NVIDIA driver, or an AMD/Intel GPU. On Windows and Linux NVML is the only GPU provider today; on macOS the `smc`, `ioreport` and `ioreg` sources cover the built-in GPU. |
| CPU power `N/A` on macOS | The `CPU Energy` counter is frozen for non-root readers on this macOS release; see [Sensors on macOS](data-sources.md#sensors-on-macos). |
| "HardViz.app is damaged" / "cannot be opened" (macOS) | Gatekeeper quarantine on a downloaded, un-notarized build. System Settings → Privacy & Security → **Open Anyway**, or `xattr -dr com.apple.quarantine /Applications/HardViz.app`. See [install.md](install.md#macos-apple-silicon-macos-11). |
| The update notice never appears, or the check fails | The check is a single request to GitHub through the system `curl`, on a background thread; offline or missing `curl` is noted in the log and in Settings → Updates and changes nothing else. See [Updates](usage.md#updates). |
| A metric reads `N/A` but the source shows online | The source is up but returned `None` for that metric — hover the row in Settings → Metrics for the per-source diagnosis. |
| History looks gapped | A source dropped out for ≥3 consecutive polls; gaps are stored as `NaN` and drawn as breaks rather than lines to zero. |
| Charts are empty for the first seconds | History fills one sample per poll; 300 samples at 1 s is a 5-minute window. |
| Window is slow to update while unfocused | Sampling is on its own thread, so it is not; repaints are throttled by design when nothing is animating. |

`hardviz.log` also emits a health snapshot every 30 polls:

```
poll #30: cpu.usage=live/30, cpu.temp=none/0, gpu.usage=live/30, …
```

`live`/`none` is whether a value is currently held; the number after `/` is how
many history samples have accumulated.

---

Still stuck? Open an
[issue](https://github.com/liorbass/hardviz/issues) with the log attached —
it usually already says what happened.
