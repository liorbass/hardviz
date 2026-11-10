# Configuration

One JSON file holds your theme, layout, widget list, window size, refresh
interval, disabled sources and whether to check for updates. Everything
HardViz writes goes there, which is why uninstalling is deleting a folder.

| Platform | Path |
|---|---|
| Windows | `C:\Users\<you>\AppData\Roaming\hardviz\HardViz\config\config.json` |
| Linux | `~/.config/hardviz/config.json` (`$XDG_CONFIG_HOME/hardviz/`) |
| macOS | `~/Library/Application Support/com.hardviz.HardViz/config.json` |

The path is resolved by the
[`directories`](https://crates.io/crates/directories) crate from
`ProjectDirs::from("com", "hardviz", "HardViz")`. The authoritative value is
printed at startup in the log (`config path: …`) and shown in the Settings
panel footer — check there rather than guessing.

The file is rewritten every 30 polls and again on exit, so a change you make in
Settings survives a crash in most cases but not all; quit normally if you have
just rearranged the dashboard.

## The file

```jsonc
{
  "theme": "Solarized",              // MidnightDark | PureLight | OledBlack | Cyberpunk | Solarized
  "view_mode": "Windowed",           // Windowed | Compact | Fullscreen
  "widgets": [
    { "uid": "gauge.cpu#0", "type_id": "gauge.cpu", "visible": true },
    { "uid": "chart.usage#5", "type_id": "chart.usage", "visible": true,
      "span": 4, "height": 300 }       // resized: full width, 300 px (omitted = default)
  ],
  "window_width": 1100,
  "window_height": 780,
  "refresh_rate_ms": 1000,           // clamped to a 50 ms floor
  "overlay_opacity": 0.85,           // compact-mode alpha
  "disabled_sources": [],            // source ids turned off in Settings
  "widgets_locked": true,            // drag-to-reorder disabled
  "compact_always_on_top": true,     // compact overlay stays above other windows
  "compact_pos": [1500.0, 40.0],     // last compact overlay position (null = OS default)
  "font_scale": 1.0,                 // Settings -> Text size
  "fit_to_window": true,             // scale card heights to the window
  "check_for_updates": false,        // ask GitHub for a newer release at start (opt-in)
  "chart_span_secs": 300             // line chart time span: 60, 300, 900, 1800 or 3600
}
```

Widgets are stored as **instances referencing a registered `type_id`**, which
is what allows the same type to appear more than once (each instance keeps its
own `uid`, size and visual). `span` and `height` are omitted until you resize a
card.

## Editing it by hand

Safe, with two behaviours worth knowing:

- On load, entries whose `type_id` is no longer registered are dropped, so
  removing a widget type in a future version cannot corrupt a saved layout; if
  that empties the list, the defaults are restored.
- A file that cannot be parsed is renamed to `config.json.bak` and logged, then
  defaults are used — your old file is never silently overwritten. Note that
  `theme` and `view_mode` are stored as enum names, so a misspelled
  `"Solarized"` costs you the whole file, not just the theme.

Delete `config.json` for a factory reset (all default widgets, Midnight theme,
windowed).

## The terminal UI

`hardviz-tui` reads the same file for its theme, refresh interval and disabled
sources, but never writes it — changes you make with `t`, `+` and `-` last for
the session only. Its log goes to `hardviz-tui.log` next to `hardviz.log`.

---

Related: [usage.md](usage.md) for what each setting does in the UI (including
[Updates](usage.md#updates)), [troubleshooting.md](troubleshooting.md) for the
log, [architecture.md](architecture.md#configuration) for the loading and
migration rules the code has to keep.
