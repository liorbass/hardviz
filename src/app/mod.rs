//! The application shell: state, the per-frame update, and widget instance
//! management. The pieces it draws live in submodules:
//!
//! | module | draws / owns |
//! |---|---|
//! | `chrome` | keyboard shortcuts, top bar, Add widget panel |
//! | `dashboard` | the card grid, drag-to-swap, resize grip |
//! | `layout` | pure grid geometry (packing, fit to window, resize) |
//! | `settings` | the Settings panel |
//! | `compact` | the compact overlay |
//! | `window` | view modes and the OS window |
//! | `style` | shared frames, buttons, egui visuals |
//!
//! Sampling lives on the poller thread (`crate::poller`); the shell only reads
//! what it publishes. It knows nothing about *which* metrics or widgets exist:
//! it iterates the registries, which is what makes both sides pluggable.

mod chrome;
mod compact;
mod dashboard;
mod layout;
mod settings;
mod style;
mod window;

use crate::config::{AppConfig, ViewMode};
use crate::metrics::METRICS;
use crate::poller::{Poller, SharedState, HEALTH_EVERY};
use crate::source::MetricSource;
use crate::sources::default_sources;
use crate::theme::{Theme, ThemeName};
use crate::widget::{Widget, WidgetRegistry};
use crate::widgets::default_registry;
use eframe::egui;
use std::collections::HashMap;
use style::{apply_font_scale, apply_theme_to_context};
use window::WindowState;

/// Glow duration after a widget swap.
const SWAP_ANIM_SECS: f32 = 0.45;
/// Gauge easing rate (1/second).
const GAUGE_SMOOTHING_RATE: f32 = 9.0;
/// Frame budget while animating (30 fps: smooth for easing, half the work of
/// 60); idle frames fall back to the refresh interval.
const ANIM_FRAME_MS: u64 = 33;
/// A value still this far from its target (as a share of its metric's range)
/// keeps repainting at frame rate; closer than that, the step is invisible.
const EASING_EPSILON: f64 = 0.005;

pub struct HardVizApp {
    /// Owns the background sampling thread, so collection continues even when
    /// this window is occluded and egui stops calling `update`.
    poller: Poller,
    /// What the poller publishes: store, diagnoses, processes.
    state: SharedState,
    /// Poll count at the last periodic config save.
    last_saved_poll: u64,

    widget_registry: WidgetRegistry,
    /// Live widget instances keyed by config uid.
    widgets: HashMap<String, Box<dyn Widget>>,
    config: AppConfig,
    /// Derived from `config.theme` and `config.font_scale`.
    theme: Theme,
    /// Text scale currently applied to egui's own styles; differs from the
    /// config value for one frame after the user changes it.
    applied_font_scale: f32,

    window: WindowState,
    show_settings: bool,
    show_add_widget: bool,

    /// Edit layout: the card being dragged and the card under the pointer.
    dragging: Option<String>,
    drag_target: Option<String>,
    /// This frame's card rects, for hit-testing a drop.
    widget_rects: Vec<(String, egui::Rect)>,
    /// Glow on the two cards of the last swap, fading over `SWAP_ANIM_SECS`.
    swap_timer: f32,
    recently_swapped: Vec<String>,

    /// The `LibreHardwareMonitor` startup task, shown in Settings.
    #[cfg(windows)]
    lhm_task: crate::sources::lhm::task::TaskController,
}

impl HardVizApp {
    /// The app with the platform's default sources.
    pub fn new(ctx: &egui::Context, config: AppConfig) -> Self {
        Self::with_sources(ctx, config, default_sources)
    }

    /// The app sampling `sources` (built on the poller thread).
    pub fn with_sources<F>(ctx: &egui::Context, config: AppConfig, sources: F) -> Self
    where
        F: FnOnce() -> Vec<Box<dyn MetricSource>> + Send + 'static,
    {
        // Sources are built and first polled on the poller thread, with the
        // user's disabled set applied before anything runs.
        let poller = Poller::spawn(sources, &config.disabled_sources, config.refresh_rate_ms);
        Self::with_poller(ctx, config, poller)
    }

    /// An app reading from `poller`, which the caller has already started.
    fn with_poller(ctx: &egui::Context, config: AppConfig, poller: Poller) -> Self {
        style::install_font_fallbacks(ctx);
        let mut window = WindowState::new();
        window.restore(&config);

        let mut app = Self {
            state: poller.state(),
            poller,
            last_saved_poll: 0,
            widget_registry: default_registry(),
            widgets: HashMap::new(),
            theme: config.theme.theme().with_text_scale(config.font_scale),
            applied_font_scale: 0.0,
            config,
            window,
            show_settings: false,
            show_add_widget: false,
            dragging: None,
            drag_target: None,
            widget_rects: Vec::new(),
            swap_timer: 0.0,
            recently_swapped: Vec::new(),
            #[cfg(windows)]
            lhm_task: crate::sources::lhm::task::TaskController::new(),
        };
        app.sync_widget_instances();
        app
    }

    fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            crate::warn!("cannot save config to {} ({e})", AppConfig::path_label());
        }
    }

    fn switch_view_mode(&mut self, mode: ViewMode) {
        self.window.switch(&mut self.config, mode);
    }

    fn apply_theme(&mut self, name: ThemeName) {
        self.config.theme = name;
        self.theme = name.theme().with_text_scale(self.config.font_scale);
    }

    /// True while any animation needs display-rate repaints.
    fn is_animating(&self) -> bool {
        if self.swap_timer > 0.0 || self.dragging.is_some() {
            return true;
        }
        // Gauges still easing toward their target?
        let Some(view) = self.state.lock() else { return false };
        METRICS.iter().any(|&id| match (view.store.current(id), view.store.smoothed(id)) {
            (Some(cur), Some(eased)) => still_easing(id, cur, eased),
            _ => false,
        })
    }

    /// Per-frame bookkeeping that does not draw: animation timers, the
    /// poller's interval, theme and text scale, window commands.
    fn tick(&mut self, ctx: &egui::Context) {
        // Clamp dt: egui reports a large delta after window drags or idle
        // periods, which would otherwise snap animations to their end state.
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        self.state.advance(dt, GAUGE_SMOOTHING_RATE);
        self.poller.set_interval(self.config.refresh_rate_ms);

        if self.swap_timer > 0.0 {
            self.swap_timer = (self.swap_timer - dt).max(0.0);
            if self.swap_timer == 0.0 {
                self.recently_swapped.clear();
            }
        }

        apply_theme_to_context(ctx, &self.theme);
        if (self.config.font_scale - self.applied_font_scale).abs() > 1e-3 {
            apply_font_scale(ctx, self.config.font_scale);
            self.theme = self.config.theme.theme().with_text_scale(self.config.font_scale);
            self.applied_font_scale = self.config.font_scale;
        }

        self.window.apply(ctx);
        self.window.track(ctx, &mut self.config);
    }

    /// Top bar, then Settings and the dashboard scrolling together beneath it,
    /// so nothing can end up out of reach.
    fn draw_main(&mut self, ui: &mut egui::Ui) {
        self.draw_top_bar(ui);
        ui.add_space(8.0);
        let view_h = ui.available_height();
        // Measured outside the scroll area: inside it, the width is
        // remembered from the previous frame's content, so one over-wide
        // frame would keep everything wide for good.
        let view_w = ui.available_width();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Leave room for the scrollbar.
            ui.set_max_width(view_w - 14.0);
            if self.show_settings {
                self.draw_settings(ui);
                ui.add_space(8.0);
            }
            // Fit to window targets the full viewport even with Settings
            // open: the panel pushes the dashboard down (scroll) rather than
            // squashing every card each time it is toggled.
            let fit = self.config.fit_to_window.then_some(view_h - 2.0);
            self.draw_dashboard(ui, fit);
        });
    }
}

impl HardVizApp {
    /// One frame: everything `eframe::App::update` does.
    fn frame(&mut self, ctx: &egui::Context) {
        self.tick(ctx);
        self.handle_shortcuts(ctx);

        let panel = egui::Frame::none().fill(self.theme.bg_primary).inner_margin(egui::Margin::same(12.0));
        egui::CentralPanel::default().frame(panel).show(ctx, |ui| match self.config.view_mode {
            ViewMode::Compact => self.draw_compact(ui),
            ViewMode::Windowed | ViewMode::Fullscreen => self.draw_main(ui),
        });

        // Persist once every HEALTH_EVERY polls, keyed off the poller's
        // monotonic counter (history length saturates at its cap).
        let polls = self.poller.poll_count();
        if polls >= self.last_saved_poll + HEALTH_EVERY {
            self.last_saved_poll = polls;
            self.save_config();
        }

        let animating = self.is_animating();
        let next = if animating { ANIM_FRAME_MS } else { self.config.refresh_rate_ms };
        ctx.request_repaint_after(std::time::Duration::from_millis(next));
    }
}

/// Whether an eased value is still visibly short of its target. Relative to
/// the metric's range, so 57 000 MB of RAM and 40 % CPU settle alike.
fn still_easing(id: crate::metrics::MetricId, current: f64, eased: f64) -> bool {
    let (lo, hi) = id.def().scale.range([current].into_iter());
    (current - eased).abs() > EASING_EPSILON * (hi - lo).max(current.abs()).max(1e-9)
}

impl eframe::App for HardVizApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config();
    }
}

#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests {
    use super::test_support::{app, run};
    use super::*;
    use crate::config::WidgetTypes;

    #[test]
    fn theme_and_text_size_follow_the_config() {
        let (ctx, mut app) = app();
        app.apply_theme(ThemeName::PureLight);
        assert_eq!(app.config.theme, ThemeName::PureLight);
        app.config.font_scale = 1.5;
        run(&ctx, &mut app, vec![]);
        assert_eq!(app.theme.fs(10.0), 15.0);
        let body = ctx.style().text_styles[&egui::TextStyle::Body].size;
        assert_eq!(body, egui::Style::default().text_styles[&egui::TextStyle::Body].size * 1.5);
    }

    #[test]
    fn a_saved_layout_round_trips_through_the_registry() {
        let (_ctx, app) = app();
        let registry = default_registry();
        let known: Vec<&str> = registry.factories().iter().map(|f| f.type_id).collect();
        let defaults = registry.default_layout();
        let json = serde_json::to_string(&app.config).expect("serialises");
        let back = AppConfig::from_json(&json, &WidgetTypes::new(&known, &defaults)).expect("parses");
        assert_eq!(back.widgets, app.config.widgets);
    }

    #[test]
    fn easing_is_judged_relative_to_the_metric() {
        use crate::metrics::metric::{CPU_USAGE, RAM_USED};
        assert!(still_easing(CPU_USAGE, 50.0, 40.0));
        assert!(!still_easing(CPU_USAGE, 50.0, 49.8), "0.2 of 100 % is invisible");
        assert!(!still_easing(RAM_USED, 57_000.0, 56_990.0), "10 MB of 57 GB is invisible");
        assert!(still_easing(RAM_USED, 57_000.0, 50_000.0));
    }

    #[test]
    fn gauges_still_easing_keep_repainting() {
        let (_ctx, mut app) = app();
        assert!(!app.is_animating(), "nothing moving yet");
        app.swap_timer = SWAP_ANIM_SECS;
        assert!(app.is_animating());
    }
}
