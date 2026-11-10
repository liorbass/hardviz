# Install

HardViz is portable. There is no installer, no service to register and nothing
to clean up afterwards beyond one folder and one per-user config directory.
Download the archive for your platform, put it somewhere permanent, and run it.

| Package | Contents |
|---|---|
| `hardviz-windows-x64.zip` | `hardviz.exe` (window), `hardviz-tui.exe` (terminal), bundled LibreHardwareMonitor for CPU temperature |
| `hardviz-linux-x64.tar.gz` | `hardviz`, `hardviz-tui` (needs the windowing/GL libraries your desktop session already has) |
| `hardviz-macos-arm64.tar.gz` | `HardViz.app` (window), `hardviz-tui` (terminal); Apple Silicon, macOS 11+ |

Downloads: [Windows x64](https://github.com/liorbass/hardviz/releases/latest/download/hardviz-windows-x64.zip) ·
[Linux x64](https://github.com/liorbass/hardviz/releases/latest/download/hardviz-linux-x64.tar.gz) ·
[macOS Apple Silicon](https://github.com/liorbass/hardviz/releases/latest/download/hardviz-macos-arm64.tar.gz) ·
[all releases](https://github.com/liorbass/hardviz/releases)

Prefer to build it yourself? See [Build and run](building.md#build-and-run).

---

## Windows (x64)

1. Download
   [`hardviz-windows-x64.zip`](https://github.com/liorbass/hardviz/releases/latest/download/hardviz-windows-x64.zip)
   and extract it somewhere permanent, for example
   `%LOCALAPPDATA%\Programs\HardViz`. Keep the `LibreHardwareMonitor` folder
   next to `hardviz.exe`: it supplies CPU temperature, power and fans.
2. Run `hardviz.exe`. The build is not code-signed, so SmartScreen may say
   *Windows protected your PC*: click **More info** → **Run anyway** (once).
3. The first start launches LibreHardwareMonitor, which asks for admin rights
   (UAC). To stop the prompt, open **Settings → Data sources → Startup task →
   Set up**: one last UAC prompt, and LHM then starts at login on its own. See
   [CPU temperature on Windows](data-sources.md#cpu-temperature-on-windows).
4. Optional:
   - **Start at login:** press <kbd>Win</kbd>+<kbd>R</kbd>, run `shell:startup`
     and put a shortcut to `hardviz.exe` in the folder that opens.
   - **Terminal UI:** run `hardviz-tui.exe` from that folder, or add the folder
     to your `PATH`.

**Uninstall:** in **Settings → Data sources**, **Remove** the startup task if
you set it up; quit HardViz and LibreHardwareMonitor (tray icon); delete the
folder from step 1, and `%APPDATA%\hardviz` (settings and log).

---

## Linux (x64)

1. Download
   [`hardviz-linux-x64.tar.gz`](https://github.com/liorbass/hardviz/releases/latest/download/hardviz-linux-x64.tar.gz)
   and install both binaries for your user:

   ```sh
   tar -xzf hardviz-linux-x64.tar.gz
   install -Dm755 hardviz-linux-x64/hardviz hardviz-linux-x64/hardviz-tui -t ~/.local/bin
   ```

   (`~/.local/bin` is on the `PATH` of most distributions; any `PATH`
   directory works.)
2. Run `hardviz` (window) or `hardviz-tui` (terminal). A desktop session
   already has the X11/Wayland and OpenGL libraries the window needs.
3. For CPU temperature, the CPU's sensor driver must be loaded (`k10temp` on
   AMD, `coretemp` on Intel). See
   [CPU temperature on Linux](data-sources.md#cpu-temperature-on-linux).
4. Optional, an app-menu entry (copy it to `~/.config/autostart/` as well to
   start HardViz at login):

   ```sh
   cat > ~/.local/share/applications/hardviz.desktop <<EOF
   [Desktop Entry]
   Type=Application
   Name=HardViz
   Comment=Hardware monitor
   Exec=$HOME/.local/bin/hardviz
   Categories=System;Monitor;
   EOF
   ```

**Uninstall:**

```sh
rm ~/.local/bin/hardviz ~/.local/bin/hardviz-tui
rm -f ~/.local/share/applications/hardviz.desktop ~/.config/autostart/hardviz.desktop
rm -rf ~/.config/hardviz        # settings and log
```

---

## macOS (Apple Silicon, macOS 11+)

1. Download
   [`hardviz-macos-arm64.tar.gz`](https://github.com/liorbass/hardviz/releases/latest/download/hardviz-macos-arm64.tar.gz)
   and double-click it in Finder to extract.
2. Drag `HardViz.app` into **Applications**.
3. Open it. The build is signed ad hoc but not notarized, so the first launch
   is blocked ("Apple could not verify…"). Click **Done**, then open
   **System Settings → Privacy & Security**, scroll to *HardViz was blocked*
   and click **Open Anyway** (once). Or clear the quarantine flag instead:

   ```sh
   xattr -dr com.apple.quarantine /Applications/HardViz.app
   ```

   No permissions or root are needed; every sensor is read as your user. See
   [Sensors on macOS](data-sources.md#sensors-on-macos).
4. Optional:
   - **Start at login:** **System Settings → General → Login Items → +** and
     pick HardViz.
   - **Terminal UI:** put `hardviz-tui` on your `PATH`:

     ```sh
     mkdir -p ~/.local/bin && cp hardviz-macos-arm64/hardviz-tui ~/.local/bin/
     xattr -d com.apple.quarantine ~/.local/bin/hardviz-tui 2>/dev/null
     ```

     (zsh does not include `~/.local/bin` by default; add
     `export PATH="$HOME/.local/bin:$PATH"` to `~/.zshrc`, or copy it to
     `/usr/local/bin` with `sudo`.)

**Uninstall:** quit HardViz, remove it from Login Items, then:

```sh
rm -rf /Applications/HardViz.app ~/.local/bin/hardviz-tui
rm -rf ~/Library/Application\ Support/com.hardviz.HardViz   # settings and log
```

---

## After installing

- Something reads `N/A`? The hover text says why, and
  [Troubleshooting](troubleshooting.md) lists the common cases.
- Your settings, layout and theme live in one JSON file — see
  [configuration.md](configuration.md) for where it is and how to reset it.
- The window and the terminal UI share those settings; the terminal UI never
  writes them. See [usage.md](usage.md#terminal-ui).
