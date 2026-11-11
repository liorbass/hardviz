# Data sources

Where each reading comes from, what it needs, and what to do when a sensor is
missing. Nothing here needs configuring in the normal case: HardViz probes
what the machine offers and shows only the sensors it actually reports.

| id | Display name | Platform | Provides | Requirements |
|---|---|---|---|---|
| `lhm_http` | LibreHardwareMonitor HTTP | Windows | `cpu.temp`, `cpu.power`, `cpu.clock`, `fan.speed`, `storage.temp`, `disk.read`, `disk.write` | bundled LHM; started automatically (see below) |
| `acpi_thermal` | ACPI Thermal Zone (fallback CPU temp) | Windows | `cpu.temp` | `MSAcpi_ThermalZoneTemperature` present; often needs administrator rights |
| `hwmon` | Linux hwmon | Linux, other Unix | `cpu.temp`, `storage.temp`, `fan.speed` | a CPU driver: `k10temp`, `zenpower`, `coretemp`, `cpu_thermal` |
| `diskstats` | Linux /proc/diskstats | Linux | `disk.read`, `disk.write` | none |
| `smc` | Apple SMC | macOS | `cpu.temp`, `gpu.temp`, `fan.speed` | none |
| `ioreport` | Apple IOReport | macOS (Apple Silicon) | `cpu.clock`, `gpu.clock`, `gpu.power`, `cpu.power` (see below) | none |
| `macos_sensors` | macOS sensors | macOS | `storage.temp`, `cpu.temp` (fallback) | none |
| `ioreg` | macOS I/O Kit | macOS | `disk.read`, `disk.write`, `gpu.usage`, `vram.*` (unified memory) | none (`/usr/sbin/ioreg` ships with macOS) |
| `nvml` | NVML (NVIDIA GPU) | Windows, Linux | `gpu.usage`, `gpu.temp`, `gpu.power`, `gpu.clock`, `vram.usage`, `vram.used`, `vram.total` | NVIDIA driver |
| `sysinfo` | sysinfo | all | `cpu.usage`, `cpu.clock` (fallback), `ram.*`, `swap.*`, `cpu.load` (not Windows), `net.down`, `net.up`, `system.uptime` | none |

With several NVIDIA GPUs, `nvml` still reports one set of readings:
utilisation, temperature and clock describe the **busiest** device, power and
VRAM the **sum** across devices. The process list's GPU column sums each
process's per-device utilisation the same way (so it can exceed 100 %).

Sources can be turned off in **Settings → Data sources**; the choice is
remembered, and a disabled source is never polled at all — so a disabled
LibreHardwareMonitor is never launched, and never prompts for admin rights.

## Fallback priority

The order of `sources::default_sources()` is fallback priority. For a metric
supplied by several sources, the **earliest registered, enabled, online**
source that returns a value wins, which is what gives CPU temperature its
automatic LHM → ACPI → hwmon fallback chain (and `smc` → `macos_sensors` on
macOS). Settings → Metrics shows which source is currently feeding each metric,
and hovering it lists every source that claims the metric and why it is not
winning.

The resolution rules and their tests are described in
[architecture.md](architecture.md#source-resolution).

## What no source can give

- **CPU power on Linux** — reading RAPL energy counters needs root.
- **CPU power on recent macOS releases** — see *Sensors on macOS* below.
- **Per-process network traffic on any OS** — needs admin rights or packet
  capture.

The process list's **I/O** column is whatever the OS reports per process: on
Windows disk, network and device I/O combined; on Linux and macOS disk only.

## The metric catalogue

Every reading has an id from one catalogue, `src/metrics.rs`: one `metric::*`
constant per metric, listed in display order in `METRICS`. This is what
sources provide and widgets require, and Settings → Metrics lists the whole
catalogue with live values.

| id | Label | Kind | Unit | Scale |
|---|---|---|---|---|
| `cpu.usage` | CPU | Cpu | % | 0–100 |
| `cpu.temp` | CPU | Cpu | °C | 20–100 |
| `ram.usage` | RAM | Ram | % | 0–100 |
| `ram.used` | RAM used | Ram | MB | — |
| `ram.total` | RAM total | Ram | MB | — |
| `swap.usage` | Swap | Ram | % | 0–100 |
| `swap.used` | Swap used | Ram | MB | — |
| `gpu.usage` | GPU | Gpu | % | 0–100 |
| `gpu.temp` | GPU | Gpu | °C | 20–100 |
| `vram.usage` | VRAM | Vram | % | 0–100 |
| `vram.used` | VRAM used | Vram | MB | — |
| `vram.total` | VRAM total | Vram | MB | — |
| `cpu.power` | CPU power | Cpu | W | 0–200 |
| `cpu.clock` | CPU clock | Cpu | MHz | 0–6000 |
| `cpu.load` | Load (1 min) | Cpu | number | auto |
| `gpu.power` | GPU power | Gpu | W | 0–450 |
| `gpu.clock` | GPU clock | Gpu | MHz | 0–3200 |
| `fan.speed` | Fan (fastest) | Cooling | RPM | 0–3000 |
| `storage.temp` | Drive (hottest) | Storage | °C | 20–90 |
| `disk.read` / `disk.write` | Read / Write | Storage | MB/s | auto |
| `disk.read.total` / `disk.write.total` | Read / Write total | Storage | MB | auto |
| `net.down` / `net.up` | Down / Up | Network | MB/s | auto |
| `net.down.total` / `net.up.total` | Down / Up total | Network | MB | auto |
| `system.uptime` | Uptime | System | duration | auto |

Each entry's `scale` (`Scale::Fixed(lo, hi)` or `Scale::Auto(floor)`) sets a
chart's y axis, and its `Unit` the value and axis formatting (MB as GB/TB,
MHz as GHz, rates as KB/s / MB/s / GB/s, durations as `3d 4h`) — so charts and
tiles need no per-metric code.

Notes on the newer readings:

- **Load average** (`cpu.load`) is the one-minute load: runnable plus waiting
  tasks, not a percentage — on an 8-core machine, 8.0 means fully busy.
  Windows does not report one, so it stays `N/A` there.
- **Swap** is only reported when swap is configured; a machine without swap
  shows `N/A`, never a fake zero.
- **Session totals** (`*.total`) are *derived*: no source provides them. The
  store integrates the matching rate over the real time between samples, so
  they count what moved **since this run started** and reset on restart. A
  rate that goes missing holds the total at its last value; the total itself
  never goes stale. Settings → Metrics lists their provider as `hardviz`.
- **Per-core CPU** is not a catalogue metric (the catalogue is one id per
  reading, and cores are a list). The **Per-Core CPU** card reads a dedicated
  per-core channel that the poller samples only while the card is on screen,
  the same arrangement as the process list.

---

## CPU temperature on Windows

Windows exposes no CPU temperature through `sysinfo`, and LibreHardwareMonitor
0.9.6 ships **no WMI provider** — the `root\LibreHardwareMonitor` namespace
simply does not exist in that build. It does publish an HTTP JSON endpoint, so
HardViz reads `http://127.0.0.1:8085/data.json`. The bundled LHM config already
has the web server on (bound to `127.0.0.1`) and starts minimised to the tray.

When the endpoint is unreachable at startup, HardViz starts the bundled LHM
itself, once per session:

- **With the startup task set up** (Settings → Data sources → Startup task →
  *Set up*, one UAC prompt): LHM starts at login with admin rights, and HardViz
  can also start it on demand through the task — no prompts ever again.
- **Without it:** HardViz launches LHM directly, and Windows shows a UAC prompt
  each time (LHM requires administrator rights).

LHM keeps running in the tray after HardViz closes. `acpi_thermal` takes over if
the board exposes ACPI thermal zones; most modern boards do not.

**Windows Security notifications** ("Protected memory access blocked …
LibreHardwareMonitor.exe"): Controlled folder access blocks LHM's raw-disk
reads, which it uses for drive health. CPU temperature is unaffected. To allow
it, add the exe under Windows Security → Ransomware protection → Allow an app.
Allowance is per path, so `target\debug\…` and `target\release\…` are separate
entries.

## CPU temperature on Linux

`sysinfo::Components` reads `/sys/class/hwmon`, so no daemon or root access is
needed — but the CPU driver must be loaded:

```sh
sensors            # is anything reported?
sudo sensors-detect
sudo modprobe k10temp    # AMD
sudo modprobe coretemp   # Intel
```

Sensor preference: a package/die sensor (`Package id 0`, `Tctl`, `Tdie`), else
the mean of per-core sensors, else any CPU-driver reading, else an `acpitz`
zone. Fans and other `*_input` values are read straight from
`/sys/class/hwmon`. In WSL there are no hardware sensors at all, so
temperatures stay `N/A` — that is expected, not a bug.

## Sensors on macOS

No setup, no root, no helper. Four sources, all in `src/sources/macos/`:

- **`smc`** opens the System Management Controller read-only, the way Stats
  and iStat Menus do. CPU temperature is the mean of the core sensors (`Tp*`
  P-cores, `Te*` E-cores on Apple Silicon; `TC*` on Intel), GPU temperature
  the mean of `Tg*` (`TG*`), fan speed the fastest `F<n>Ac`. Placeholder keys
  that read nonsense on a given chip (-4 °C) are dropped by range. Fans at
  `0 RPM` are stopped, not broken: MacBook Pro fans stop at idle; Macs
  without fans show `N/A`.
- **`ioreport`** reads `IOReport`, the counters `powermetrics` prints, without
  `powermetrics`' root requirement. CPU and GPU clocks are the time in each
  performance state weighted by the chip's DVFS table (from the `pmgr` device
  in the I/O Kit registry), averaged over the time the cores were running.
  GPU power comes from the `GPU Energy` counter. **CPU power** comes from
  `CPU Energy`, but recent macOS releases (macOS 27 at least) freeze that
  counter for non-root readers, so HardViz shows CPU power only while the
  counter is moving and `N/A` otherwise, never a fake `0 W`. Apple Silicon
  only; an Intel Mac has none of these channels.
- **`macos_sensors`** reads `sysinfo::Components`: the SSD's `NAND`
  temperature, and CPU temperature (hottest `PMU tdie*`; `PECI CPU` on Intel)
  as a fallback when `smc` has none.
- **`ioreg`** runs `/usr/sbin/ioreg` for disk throughput (each
  `IOBlockStorageDriver`'s byte counters) and GPU usage and memory
  (`IOAccelerator`'s `Device Utilization %` and `In use system memory`), the
  numbers Activity Monitor shows. Apple Silicon has no VRAM: the VRAM gauge
  shows the GPU's allocation out of the whole unified memory. Reading from a
  mounted `.dmg` counts on both the image and the disk holding it.

The SMC keys and IOReport channels are private Apple interfaces that vary by
chip and macOS release; they were checked on an M4 Pro under macOS 27. NVML is
not registered: macOS has no NVIDIA driver.

---

## Adding a source

A new backend is one `MetricSource` impl plus one line in `default_sources()`;
a new reading from an existing backend is one metric constant and one
`out.set(...)`. [adding-sensors.md](adding-sensors.md) walks through both,
including the platform traps that have already cost someone a day (Windows COM
threading, macOS private interfaces lying quietly, sysinfo's CPU-refresh
aliasing).
