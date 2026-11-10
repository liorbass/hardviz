//! Shared helpers for the app's tests: a fake source and a headless frame
//! runner. Nothing here touches hardware or the user's config file.

use super::HardVizApp;
use crate::config::AppConfig;
use crate::metrics::metric::CPU_USAGE;
use crate::metrics::MetricId;
use crate::source::{MetricSource, Readings, SourceStatus};
use crate::widgets::default_registry;
use eframe::egui;
use std::time::{Duration, Instant};

/// Reports CPU usage 42 %, always online.
pub(super) struct Steady;

impl MetricSource for Steady {
    fn id(&self) -> &'static str {
        "steady"
    }
    fn display_name(&self) -> &'static str {
        "Steady"
    }
    fn provides(&self) -> &'static [MetricId] {
        &[CPU_USAGE]
    }
    fn poll(&mut self, out: &mut Readings) {
        out.set(CPU_USAGE, 42.0);
    }
    fn status(&self) -> SourceStatus {
        SourceStatus::Online
    }
}

/// An app sampling only [`Steady`], with the default layout.
pub(super) fn app() -> (egui::Context, HardVizApp) {
    let registry = default_registry();
    let mut config = AppConfig::with_default_widgets(&registry.default_layout());
    config.refresh_rate_ms = 50;
    let ctx = egui::Context::default();
    let app = HardVizApp::with_sources(&ctx, config, || vec![Box::new(Steady) as Box<dyn MetricSource>]);
    (ctx, app)
}

/// Run one frame with `events` on a 1200 x 800 screen.
pub(super) fn run(ctx: &egui::Context, app: &mut HardVizApp, events: Vec<egui::Event>) {
    run_held(ctx, app, events, egui::Modifiers::NONE);
}

/// Like [`run`], with `held` modifier keys down (as winit reports them).
pub(super) fn run_held(ctx: &egui::Context, app: &mut HardVizApp, events: Vec<egui::Event>, held: egui::Modifiers) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
        events,
        modifiers: held,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.frame(ctx));
}

pub(super) fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

/// Wait until the poller has taken a couple of samples.
pub(super) fn wait_for_samples(app: &HardVizApp) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.poller.poll_count() < 2 && Instant::now() < deadline {
        std::thread::yield_now();
    }
}
