# Adding or tweaking a theme

Themes are pure data: a `Theme` is a flat struct of colour tokens, and five
preset constructors in `src/theme.rs` return one. The active theme is rebuilt
from `ThemeName` whenever it changes and applied to `egui::Visuals` once per
frame. Everything about a theme lives in `src/theme.rs`.

---

## The token set

```rust
pub struct Theme {
    pub bg_primary: Color32,   pub bg_secondary: Color32,   pub bg_widget: Color32,
    pub text_primary: Color32, pub text_secondary: Color32, pub text_muted: Color32,
    pub border: Color32,
    cpu: Color32, ram: Color32, gpu: Color32, vram: Color32,   // private: use metric_color
    pub temp_warm: Color32,    pub temp_hot: Color32,
    pub accent: Color32,       pub gauge_bg: Color32,
    pub success: Color32,      pub warning: Color32,        pub danger: Color32,
}
```

Tokens are read as fields (`theme.text_muted`). `Theme` is never written to
disk (only the `ThemeName` is), so it needs no serde support.

| Token | Used for |
|---|---|
| `bg_primary` | window/panel background (`CentralPanel` fill, `window_fill`, `panel_fill`) |
| `bg_secondary` | Settings and Add-widget cards, `extreme_bg_color`, noninteractive widget fill |
| `bg_widget` | every widget card, button fills, the compact overlay (with `overlay_opacity` alpha) |
| `text_primary` | main labels, gauge readouts, live values |
| `text_secondary` | card titles, axis labels, chart legends |
| `text_muted` | `N/A` placeholders, disabled rows, gridline text, footer path |
| `border` | card strokes, chart gridlines, sparkline baseline |
| `cpu` / `ram` / `gpu` / `vram` | private; resolved through `metric_color(kind)` — gauge arcs and sparklines, chart series, compact bars, Settings row labels |
| `warning` / `accent` / `success` | also the colours of the Storage, Network and Cooling metric kinds (`metric_color`) |
| `temp_warm` / `temp_hot` | endpoints of the `temp_color(°C)` ramp |
| `accent` | active view-mode tab, drag target outline, swap glow, hovered/active egui widgets, selection |
| `gauge_bg` | arc track, capacity bar background |
| `success` / `warning` / `danger` | source and metric status dots, the chart threshold line and its axis labels |

Two derived helpers, both worth reusing instead of hardcoding colours:

```rust
theme.metric_color(MetricKind::Gpu)  // the only way to get a metric's colour
theme.temp_color(72.0)               // °C; lerp(warm, hot, (t - 40) / 60 clamped)
```

`temp_color` maps 40 °C → `temp_warm` and 100 °C → `temp_hot`, clamped at both
ends. Chart thresholds (`LineChartWidget::threshold`, e.g. 80 °C on the
Temperature chart) use `danger` separately.

Widget code should **only** take colours from the theme. `widgets::dim()`
and `widgets::with_alpha()` exist for the translucent fills and glows; a
hardcoded `Color32` in a widget is a theme bug waiting to happen.

---

## Tweaking an existing preset

Edit the `rgb(r, g, b)` values in the corresponding constructor —
`midnight_dark()`, `pure_light()`, `oled_black()`, `cyberpunk()`,
`solarized()`. Nothing else needs to change; the theme is rebuilt on selection.

`oled_black()` uses `rgb(0, 0, 0)` for `bg_primary` deliberately. If you touch it,
keep `bg_widget` distinguishable from pure black or cards lose their edges
(they still have a `border` stroke, but a 30/255 border on black is faint).

For a light theme, watch three specific contrast pairs that dark presets get for
free:

- `text_muted` on `bg_widget` — this carries every `N/A` reason and the config
  path footer, so it must stay legible, not just decorative;
- `gauge_bg` (arc track) against `bg_widget` — too close and gauges look broken;
- `border` against `bg_primary` — too close and cards float with no definition.

---

## Adding a new theme

Three edits, all in `src/theme.rs`. Each is checked by the compiler or a
test, so a half-added theme cannot build.

### 1. Write the constructor (`src/theme.rs`)

```rust
fn my_theme() -> Theme {
    Theme {
        bg_primary: rgb(12, 14, 20),
        bg_secondary: rgb(20, 23, 32),
        bg_widget: rgb(26, 30, 40),
        text_primary: rgb(235, 238, 245),
        text_secondary: rgb(168, 175, 190),
        text_muted: rgb(98, 105, 120),
        border: rgb(44, 48, 62),
        cpu: rgb(88, 166, 255),
        ram: rgb(63, 185, 80),
        gpu: rgb(255, 163, 71),
        vram: rgb(163, 113, 247),
        temp_warm: rgb(255, 200, 60),
        temp_hot: rgb(255, 80, 80),
        accent: rgb(88, 166, 255),
        gauge_bg: rgb(38, 42, 54),
        success: rgb(63, 185, 80),
        warning: rgb(255, 200, 60),
        danger: rgb(255, 80, 80),
    }
}
```

Every field is required — the struct has no `Default`, so the compiler enforces
completeness.

### 2. Add the `ThemeName` variant

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeName {
    MidnightDark,
    PureLight,
    OledBlack,
    Cyberpunk,
    Solarized,
    MyTheme,          // <- add
}
```

### 3. List it in `ThemeName`'s three tables

```rust
impl ThemeName {
    pub const ALL: [ThemeName; 6] = [/* ... */, ThemeName::MyTheme];   // picker order

    pub fn label(self) -> &'static str {
        match self {
            // ...
            ThemeName::MyTheme => "Mine",   // short: the picker row wraps
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            // ...
            ThemeName::MyTheme => my_theme(),
        }
    }
}
```

`label` and `theme` are exhaustive matches, so a missing arm is a compile
error. `ALL` has a fixed length, and the Settings picker iterates it, so the
theme is reachable from the UI as soon as it is listed there.

`apply_theme_to_context` maps tokens onto `egui::Visuals` for native egui
widgets (buttons, checkboxes, sliders, scrollbars). It reads tokens generically,
so a new preset needs no change there — but a new **token** does.

---

## ⚠ `ThemeName` is persisted by variant name

`config.json` stores `"theme": "Solarized"` — the serde-derived variant name as
a string. `AppConfig::load` does:

```rust
let mut cfg: AppConfig = serde_json::from_str(&data).ok()?;
```

so a variant name that no longer exists makes the **entire config fail to
parse**, and `main.rs` then falls back to `AppConfig::default()` with only a
`no usable config found - starting with defaults` log line. The user silently
loses their theme, layout, widget list, window size, refresh rate and disabled
sources.

Therefore:

- **Adding** a variant is safe.
- **Renaming or removing** one breaks every existing install. If you must, add
  a `#[serde(alias = "OldName")]` on the new variant so old files still parse.
- The same applies to `ViewMode`.

Only `ThemeName` is persisted; `Theme` itself is never written to disk (it is
rebuilt from the name), so changing RGB values needs no migration and users pick
it up on their next launch.

---

## Adding a new token

Occasionally a new metric family wants its own colour (see
[adding-sensors.md](adding-sensors.md), Part A). That is a wider change:

1. add the `Color32` field to `Theme`;
2. add a value in **every** preset constructor — the compiler will list them all;
3. if it represents a metric family, add the `MetricKind` variant and a
   `metric_color()` arm;
4. if native egui widgets should see it, wire it in `apply_theme_to_context`.

Text contrast: keep `text_muted` at 4.5:1 or better against `bg_widget` (the
usual minimum for small text); all five presets were tuned to that.

---

## Verifying a theme

```sh
cargo run --release
```

Check the new theme against every surface, since tokens interact differently in
each:

1. **Windowed, default layout** — all seven widgets: card edges, title text,
   gauge arc vs track, sparkline, chart gridlines and legend.
2. **`N/A` state** — disable a source in Settings and confirm the placeholder
   *and its two-line reason* are readable in `text_muted`. This is the most
   common contrast failure.
3. **Danger band** — the temperature chart's 80 °C shading must be visible but
   not swamp the series lines.
4. **Compact overlay** at 100 % and at the 30 % floor of the opacity slider:
   `bg_widget` is alpha-composited there, so a theme that looks fine opaque can
   turn to mud over a busy desktop.
5. **Fullscreen**, and a window dragged narrow enough to drop the gauge grid
   from four columns to two.
6. **Native widgets** — sliders, checkboxes, the scroll area and hover states in
   Settings, which come from `apply_theme_to_context` rather than your tokens
   directly.
7. **Drag feedback** — turn on **Edit layout** and drag: the target outline, glow and
   shadow all use `accent` at various alphas.
8. Restart to confirm the selection persisted in `config.json`.
