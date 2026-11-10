//! Starting the app: logging, the panic hook, the saved config, the window.
//! `main.rs` only calls [`run`].

use crate::app::HardVizApp;
use crate::config::{AppConfig, ConfigError, WidgetTypes};
use crate::num::u32_f32;
use crate::widgets::{default_registry, REPLACED_TYPES};
use crate::{error, info, log, warn};
use eframe::egui;
use std::any::Any;

/// Run the app until its window closes.
///
/// # Errors
/// The window or graphics context could not be created.
pub fn run() -> eframe::Result<()> {
    // Without a file, lines still reach stderr; `path_label` says so.
    let _ = log::init();
    // A panic in a GUI app otherwise vanishes with the window; capture it so
    // there is always a trail on disk.
    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map_or_else(|| "<unknown>".to_string(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        error!("PANIC at {at}: {}", panic_message(info.payload()));
    }));

    info!("HardViz {} starting", env!("CARGO_PKG_VERSION"));
    info!("log file: {}", log::path_label());
    info!("config path: {}", AppConfig::path_label());

    let config = load_config();
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([u32_f32(config.window_width), u32_f32(config.window_height)])
        .with_min_inner_size([420.0, 220.0])
        .with_title("HardViz");
    let options = eframe::NativeOptions { viewport, ..Default::default() };

    let result =
        eframe::run_native("HardViz", options, Box::new(move |cc| Ok(Box::new(HardVizApp::new(&cc.egui_ctx, config)))));
    match &result {
        Ok(()) => info!("window closed cleanly"),
        Err(e) => error!("eframe run_native failed: {e}"),
    }
    result
}

/// The saved config, or defaults (saying why) when there is none or it is
/// unusable. Widget entries whose type is no longer registered are dropped,
/// so removing a widget type cannot corrupt a saved layout.
pub(crate) fn load_config() -> AppConfig {
    let registry = default_registry();
    let known: Vec<&str> = registry.factories().iter().map(|f| f.type_id).collect();
    let defaults = registry.default_layout();
    info!("registered {} widget types: {}", known.len(), known.join(", "));
    let types = WidgetTypes::new(&known, &defaults).with_replacements(REPLACED_TYPES);
    choose_config(AppConfig::load(&types), &defaults)
}

/// Pick the config to start with from a load result, logging the choice.
fn choose_config(loaded: Result<Option<AppConfig>, ConfigError>, defaults: &[&str]) -> AppConfig {
    match loaded {
        Ok(Some(c)) => {
            info!(
                "loaded config: theme={:?} mode={:?} widgets={} refresh={}ms",
                c.theme,
                c.view_mode,
                c.widgets.len(),
                c.refresh_rate_ms
            );
            c
        }
        Ok(None) => {
            info!("no saved config - starting with defaults");
            AppConfig::with_default_widgets(defaults)
        }
        Err(e) => {
            warn!("{e} - starting with defaults");
            AppConfig::with_default_widgets(defaults)
        }
    }
}

/// The text of a panic payload (`panic!` with a literal or a formatted string).
fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;

    #[test]
    fn panic_payloads_become_text() {
        assert_eq!(panic_message(&"literal"), "literal");
        assert_eq!(panic_message(&String::from("formatted 7")), "formatted 7");
        assert_eq!(panic_message(&42_u8), "<non-string panic>");
    }

    #[test]
    fn a_missing_or_broken_config_starts_from_defaults() {
        let defaults = ["gauge.cpu"];
        assert_eq!(choose_config(Ok(None), &defaults).widgets.len(), 1);
        let broken = ConfigError::Read {
            path: "config.json".into(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        assert_eq!(choose_config(Err(broken), &defaults).theme, ThemeName::MidnightDark);
    }

    #[test]
    fn a_loaded_config_is_used_as_is() {
        let mut saved = AppConfig::with_default_widgets(&[]);
        saved.theme = ThemeName::Cyberpunk;
        assert_eq!(choose_config(Ok(Some(saved.clone())), &["gauge.cpu"]), saved);
    }
}
