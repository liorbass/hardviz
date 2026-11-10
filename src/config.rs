//! Persisted application configuration.
//!
//! Widgets are stored as *instances* referencing a registered `type_id`, which
//! is what lets the same widget type appear more than once and lets new widget
//! types be added without a config migration. This module does not know which
//! widget types exist: callers pass them in as [`WidgetTypes`].

use crate::theme::ThemeName;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

/// `HardViz`'s per-user directory (config and log), if the OS provides one.
#[must_use]
pub fn app_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "hardviz", "HardViz").map(|dirs| dirs.config_dir().to_path_buf())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    Windowed,
    Compact,
    Fullscreen,
}

/// How a single-metric card shows its metric. Chosen per card (Edit layout's
/// card menu, or right-click) and saved with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visual {
    /// Arc with the value, plus a sparkline.
    Gauge,
    /// Horizontal bar with the value.
    Bar,
    /// Line chart of the recent history.
    Chart,
    /// Big value with a one-minute sparkline.
    Tile,
}

impl Visual {
    /// Every visual, in menu order.
    pub const ALL: [Visual; 4] = [Visual::Gauge, Visual::Bar, Visual::Chart, Visual::Tile];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Visual::Gauge => "Gauge",
            Visual::Bar => "Bar",
            Visual::Chart => "Line chart",
            Visual::Tile => "Tile",
        }
    }
}

/// One placed widget. `uid` is stable across restarts so per-instance settings
/// can be attached later without breaking saved layouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WidgetInstance {
    pub uid: String,
    pub type_id: String,
    pub visible: bool,
    /// Width in quarters of the dashboard (1-4); `None` = the widget's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<u8>,
    /// Card height in points; `None` = the widget's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    /// How a single-metric card draws; `None` = the widget's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual: Option<Visual>,
}

impl WidgetInstance {
    /// A visible instance at the widget's default size and visual.
    #[must_use]
    pub fn new(uid: String, type_id: &str) -> Self {
        Self { uid, type_id: type_id.to_string(), visible: true, span: None, height: None, visual: None }
    }
}

/// The widget types the running build knows, supplied by the widget registry.
#[non_exhaustive]
pub struct WidgetTypes<'a> {
    /// Every registered type id; saved instances of any other type are dropped.
    pub known: &'a [&'a str],
    /// The default layout, used for a fresh config or when nothing known is left.
    pub defaults: &'a [&'a str],
    /// Retired type ids and what replaces each in a saved layout.
    pub replaced: &'a [(&'a str, &'a [&'a str])],
}

impl<'a> WidgetTypes<'a> {
    #[must_use]
    pub fn new(known: &'a [&'a str], defaults: &'a [&'a str]) -> Self {
        Self { known, defaults, replaced: &[] }
    }

    /// Also migrate retired type ids to their replacements.
    #[must_use]
    pub fn with_replacements(mut self, replaced: &'a [(&'a str, &'a [&'a str])]) -> Self {
        self.replaced = replaced;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AppConfig {
    pub theme: ThemeName,
    pub view_mode: ViewMode,
    pub widgets: Vec<WidgetInstance>,
    /// Inner size of the normal window, kept up to date while windowed.
    pub window_width: u32,
    pub window_height: u32,
    pub refresh_rate_ms: u64,
    pub overlay_opacity: f32,
    /// Source ids the user has turned off in settings.
    #[serde(default)]
    pub disabled_sources: Vec<String>,
    /// Start with widgets locked so drags cannot happen by accident.
    #[serde(default = "default_true")]
    pub widgets_locked: bool,
    /// Keep the compact overlay above other windows.
    #[serde(default = "default_true")]
    pub compact_always_on_top: bool,
    /// Last screen position of the compact overlay (outer top-left, points).
    #[serde(default)]
    pub compact_pos: Option<[f32; 2]>,
    /// Text size multiplier (Settings -> Text size).
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    /// Scale card heights so the dashboard fits the window (scrolls only when
    /// even the minimum sizes do not fit).
    #[serde(default = "default_true")]
    pub fit_to_window: bool,
}

fn default_font_scale() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

/// Why the saved config could not be used.
#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    /// The file exists but is not a valid config. It was renamed to `backup`
    /// (when that succeeded) so the next save cannot destroy it.
    Parse {
        path: PathBuf,
        backup: Option<PathBuf>,
        source: serde_json::Error,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read { path, source } => write!(f, "cannot read {} ({source})", path.display()),
            ConfigError::Parse { path, backup, source } => {
                write!(f, "{} is not a valid config ({source})", path.display())?;
                match backup {
                    Some(b) => write!(f, "; kept it as {}", b.display()),
                    None => write!(f, "; it could not be backed up and will be overwritten"),
                }
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl AppConfig {
    /// A fresh config with the given default layout.
    #[must_use]
    pub fn with_default_widgets(defaults: &[&str]) -> Self {
        Self {
            theme: ThemeName::MidnightDark,
            view_mode: ViewMode::Windowed,
            widgets: default_instances(defaults),
            window_width: 1100,
            window_height: 780,
            refresh_rate_ms: 1000,
            overlay_opacity: 0.85,
            disabled_sources: Vec::new(),
            widgets_locked: true,
            compact_always_on_top: true,
            compact_pos: None,
            font_scale: default_font_scale(),
            fit_to_window: true,
        }
    }

    fn path() -> Option<PathBuf> {
        app_dir().map(|d| d.join("config.json"))
    }

    /// Where the config lives, for display.
    #[must_use]
    pub fn path_label() -> String {
        Self::path().map_or_else(|| "(unavailable)".into(), |p| p.display().to_string())
    }

    /// Parse a saved config. Instances of a retired type are replaced, in
    /// place, by its successors; instances of any other unknown type are
    /// dropped (and the default layout is used if none is left).
    ///
    /// # Errors
    /// The JSON is not a valid config (syntax, or an unknown theme or mode).
    pub fn from_json(data: &str, types: &WidgetTypes) -> Result<Self, serde_json::Error> {
        let mut cfg: AppConfig = serde_json::from_str(data)?;
        cfg.replace_retired(types.replaced);
        cfg.widgets.retain(|w| types.known.contains(&w.type_id.as_str()));
        if cfg.widgets.is_empty() {
            cfg.widgets = default_instances(types.defaults);
        }
        Ok(cfg)
    }

    /// Load the saved config. `Ok(None)` means there is none yet (or the OS
    /// has no config directory); a file that exists but cannot be used is an
    /// error, so the caller can say so instead of silently resetting.
    ///
    /// # Errors
    /// [`ConfigError::Read`] when the file exists but cannot be read;
    /// [`ConfigError::Parse`] when it is not a valid config (it is then kept
    /// as `config.json.bak`).
    pub fn load(types: &WidgetTypes) -> Result<Option<Self>, ConfigError> {
        let Some(path) = Self::path() else { return Ok(None) };
        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(ConfigError::Read { path, source }),
        };
        Self::from_json(&data, types).map(Some).map_err(|source| {
            let backup = path.with_extension("json.bak");
            let backup = fs::rename(&path, &backup).ok().map(|()| backup);
            ConfigError::Parse { path, backup, source }
        })
    }

    /// Write the config, creating its directory if needed.
    ///
    /// # Errors
    /// There is no config directory, or creating or writing the file failed.
    pub fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config directory"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&path, json)
    }

    fn replace_retired(&mut self, replaced: &[(&str, &[&str])]) {
        let mut migrated = Vec::with_capacity(self.widgets.len());
        for w in std::mem::take(&mut self.widgets) {
            match replaced.iter().find(|(old, _)| *old == w.type_id) {
                Some((_, successors)) => {
                    for t in *successors {
                        let probe = AppConfig { widgets: migrated.clone(), ..self.clone() };
                        let mut instance = WidgetInstance::new(probe.new_uid(t), t);
                        instance.visible = w.visible;
                        migrated.push(instance);
                    }
                }
                None => migrated.push(w),
            }
        }
        self.widgets = migrated;
    }

    /// Next unique instance id for a widget type.
    #[must_use]
    pub fn new_uid(&self, type_id: &str) -> String {
        let existing: Vec<&str> =
            self.widgets.iter().filter(|w| w.type_id == type_id).map(|w| w.uid.as_str()).collect();
        // `existing.len() + 1` candidates and at most `existing.len()` taken,
        // so one is always free; the fallback only satisfies the type checker.
        let start = existing.len();
        (start..=start * 2)
            .map(|n| format!("{type_id}#{n}"))
            .find(|candidate| !existing.contains(&candidate.as_str()))
            .unwrap_or_else(|| format!("{type_id}#{}", start * 2 + 1))
    }
}

fn default_instances(types: &[&str]) -> Vec<WidgetInstance> {
    types.iter().enumerate().map(|(i, type_id)| WidgetInstance::new(format!("{type_id}#{i}"), type_id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: WidgetTypes =
        WidgetTypes { known: &["gauge.cpu", "chart.usage"], defaults: &["gauge.cpu"], replaced: &[] };

    /// A config from before the optional fields existed.
    const LEGACY: &str = r#"{
        "theme": "Solarized", "view_mode": "Windowed",
        "widgets": [
            {"uid": "gauge.cpu#0", "type_id": "gauge.cpu", "visible": true},
            {"uid": "gone#0", "type_id": "removed.widget", "visible": true}
        ],
        "window_width": 900, "window_height": 700, "refresh_rate_ms": 500, "overlay_opacity": 0.5
    }"#;

    #[test]
    fn legacy_config_loads_with_defaults_for_new_fields() {
        let cfg = AppConfig::from_json(LEGACY, &TYPES).expect("parses");
        assert_eq!(cfg.theme, ThemeName::Solarized);
        assert_eq!(cfg.refresh_rate_ms, 500);
        assert!(cfg.widgets_locked && cfg.fit_to_window && cfg.compact_always_on_top);
        assert_eq!(cfg.font_scale, 1.0);
        assert!(cfg.disabled_sources.is_empty());
    }

    #[test]
    fn unknown_widget_types_are_dropped() {
        let cfg = AppConfig::from_json(LEGACY, &TYPES).expect("parses");
        let types: Vec<&str> = cfg.widgets.iter().map(|w| w.type_id.as_str()).collect();
        assert_eq!(types, ["gauge.cpu"]);
    }

    #[test]
    fn all_unknown_widgets_fall_back_to_the_default_layout() {
        let types = WidgetTypes::new(&["other"], &["other"]);
        let cfg = AppConfig::from_json(LEGACY, &types).expect("parses");
        assert_eq!(cfg.widgets.len(), 1);
        assert_eq!(cfg.widgets[0].uid, "other#0");
    }

    #[test]
    fn retired_types_are_replaced_in_place() {
        let json = r#"{
            "theme": "Solarized", "view_mode": "Windowed",
            "widgets": [
                {"uid": "gauge.cpu#0", "type_id": "gauge.cpu", "visible": true},
                {"uid": "old#0", "type_id": "panel.old", "visible": false},
                {"uid": "chart.usage#0", "type_id": "chart.usage", "visible": true}
            ],
            "window_width": 900, "window_height": 700, "refresh_rate_ms": 500, "overlay_opacity": 0.5
        }"#;
        let known = ["gauge.cpu", "chart.usage", "new.a", "new.b"];
        let replaced: &[(&str, &[&str])] = &[("panel.old", &["new.a", "new.b"])];
        let types = WidgetTypes::new(&known, &["gauge.cpu"]).with_replacements(replaced);
        let cfg = AppConfig::from_json(json, &types).expect("parses");
        let ids: Vec<&str> = cfg.widgets.iter().map(|w| w.type_id.as_str()).collect();
        assert_eq!(ids, ["gauge.cpu", "new.a", "new.b", "chart.usage"]);
        assert!(!cfg.widgets[1].visible && !cfg.widgets[2].visible, "visibility carries over");
        assert_eq!(cfg.widgets[1].uid, "new.a#0");
    }

    #[test]
    fn visual_choice_round_trips() {
        let mut cfg = AppConfig::with_default_widgets(&["gauge.cpu"]);
        cfg.widgets[0].visual = Some(Visual::Bar);
        let json = serde_json::to_string(&cfg).expect("serialises");
        assert!(json.contains(r#""visual":"bar""#));
        assert_eq!(AppConfig::from_json(&json, &TYPES).expect("parses").widgets[0].visual, Some(Visual::Bar));
    }

    #[test]
    fn invalid_json_is_an_error_not_a_default() {
        assert!(AppConfig::from_json("{ not json", &TYPES).is_err());
        assert!(AppConfig::from_json(r#"{"theme": "NoSuchTheme"}"#, &TYPES).is_err());
    }

    #[test]
    fn config_round_trips_through_json() {
        let mut cfg = AppConfig::with_default_widgets(TYPES.defaults);
        cfg.widgets[0].span = Some(2);
        cfg.disabled_sources.push("nvml".into());
        let json = serde_json::to_string(&cfg).expect("serialises");
        assert_eq!(AppConfig::from_json(&json, &TYPES).expect("parses"), cfg);
    }

    #[test]
    fn new_uid_skips_ids_in_use() {
        let mut cfg = AppConfig::with_default_widgets(&["gauge.cpu", "gauge.cpu"]);
        // Defaults are numbered by position: gauge.cpu#0, gauge.cpu#1.
        assert_eq!(cfg.new_uid("gauge.cpu"), "gauge.cpu#2");
        cfg.widgets.remove(0);
        // One left (#1), so counting starts at 1, which is taken.
        assert_eq!(cfg.new_uid("gauge.cpu"), "gauge.cpu#2");
        assert_eq!(cfg.new_uid("chart.usage"), "chart.usage#0");
    }
}
