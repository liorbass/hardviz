//! The Settings panel: the version and update check, theme and sampling,
//! data sources, the metric table, and the widget list.
//!
//! Drawing only reads a snapshot and records what the user did as a
//! [`SettingsAction`]; the action is applied after drawing, so the UI code
//! needs only `&self`.

use super::style::{card_frame, flat_button, section_label};
use super::HardVizApp;
use crate::config::AppConfig;
use crate::info;
use crate::metrics::{MetricId, METRICS};
use crate::num::{to_f32, to_u64, total_f64};
use crate::poller::Diagnoses;
use crate::source::SourceInfo;
use crate::theme::{with_alpha, ThemeName};
use crate::update::{releases_url, UpdateStatus, VERSION};
use eframe::egui;
use std::collections::HashMap;

mod lists;
mod sources;

impl HardVizApp {
    /// The settings panel: snapshot shared state, lay out the sections, then
    /// apply what the user changed.
    pub(super) fn draw_settings(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.settings_snapshot();
        let mut draft = SettingsDraft::from_config(&self.config);
        // Collected during drawing and applied afterwards, so the UI closures
        // only need `&self`.
        let mut action: Option<SettingsAction> = None;

        let card = card_frame(&self.theme, self.theme.bg_secondary, 10.0, egui::Margin::same(14.0));
        // Never wider than the window: an over-wide grid here widens the whole
        // scroll area and cuts the dashboard off on the right.
        let max_w = ui.available_width().min(ui.clip_rect().width()) - 28.0;
        card.show(ui, |ui| {
            ui.set_max_width(max_w);
            self.settings_header(ui, &mut action);
            ui.add_space(6.0);
            self.settings_columns(ui, &snapshot, &mut draft, &mut action);
            ui.add_space(10.0);
            self.settings_widgets(ui, &snapshot.widgets, &mut action);
        });

        draft.apply(&mut self.config);
        if let Some(action) = action {
            self.apply_settings_action(action);
        }
    }

    /// Copy everything the panel shows out of the shared state.
    fn settings_snapshot(&self) -> SettingsSnapshot {
        let sources = self.poller.source_infos();
        let (current, diagnoses): (HashMap<MetricId, Option<f64>>, Diagnoses) = self
            .state
            .lock()
            .map(|v| (METRICS.iter().map(|&id| (id, v.store.current(id))).collect(), v.diagnostics.clone()))
            .unwrap_or_default();
        let is_live = |id: &MetricId| current.get(id).copied().flatten().is_some();

        // Metric -> live provider: the plugin wiring made visible.
        let metrics = METRICS
            .iter()
            .map(|&id| {
                let diag = diagnoses.get(&id);
                let reading = current.get(&id).copied().flatten();
                MetricRow {
                    id,
                    value: reading.map_or_else(|| "--".into(), |v| id.def().unit.format(v)),
                    provider: diag.and_then(|g| g.provider).unwrap_or("none").to_string(),
                    detail: diag.map(super::super::source::MetricDiagnosis::explain).unwrap_or_default(),
                    live: reading.is_some(),
                }
            })
            .collect();

        // Each widget lists its declared metrics with live markers, so its
        // dependencies are inspectable from the UI.
        let widgets = self
            .config
            .widgets
            .iter()
            .map(|w| {
                let deps = self
                    .widgets
                    .get(&w.uid)
                    .map(|inst| {
                        inst.requires()
                            .iter()
                            .map(|id| format!("{} {}", if is_live(id) { "\u{25CF}" } else { "\u{25CB}" }, id))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                WidgetRow {
                    custom_size: w.span.is_some() || w.height.is_some(),
                    uid: w.uid.clone(),
                    type_id: w.type_id.clone(),
                    name: self.widget_registry.name_of(&w.type_id).unwrap_or("Unknown widget").to_string(),
                    visible: w.visible,
                    deps,
                }
            })
            .collect();

        SettingsSnapshot { sources, metrics, widgets }
    }

    fn settings_header(&self, ui: &mut egui::Ui, action: &mut Option<SettingsAction>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Settings").size(self.theme.fs(14.0)).strong().color(self.theme.text_primary));
            ui.label(
                egui::RichText::new(format!("HardViz {VERSION}"))
                    .size(self.theme.fs(10.0))
                    .color(self.theme.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = flat_button("\u{D7}");
                if ui.add(close).on_hover_text("Close settings").clicked() {
                    *action = Some(SettingsAction::CloseSettings);
                }
                ui.label(
                    egui::RichText::new(format!("config: {}", AppConfig::path_label()))
                        .size(self.theme.fs(9.0))
                        .color(self.theme.text_muted),
                );
            });
        });
    }

    /// General | Data sources | Metrics, side by side when there is room;
    /// narrower windows fall back to 2 columns, then 1.
    fn settings_columns(
        &self,
        ui: &mut egui::Ui,
        snapshot: &SettingsSnapshot,
        draft: &mut SettingsDraft,
        action: &mut Option<SettingsAction>,
    ) {
        let section =
            |ui: &mut egui::Ui, k: usize, draft: &mut SettingsDraft, action: &mut Option<SettingsAction>| match k {
                0 => self.settings_general(ui, action, draft),
                1 => self.settings_sources(ui, &snapshot.sources, action),
                _ => self.settings_metrics(ui, &snapshot.metrics),
            };
        let w = ui.available_width();
        let ncols = if w >= 980.0 {
            3
        } else if w >= 640.0 {
            2
        } else {
            1
        };
        if ncols == 1 {
            for k in 0..3 {
                section(ui, k, draft, action);
                ui.add_space(10.0);
            }
            return;
        }
        ui.columns(ncols, |cols| {
            for k in 0..3 {
                // With 2 columns, sources and metrics share the second one.
                let c = if ncols == 3 { k } else { k.min(1) };
                section(&mut cols[c], k, draft, action);
                cols[c].add_space(10.0);
            }
        });
    }

    fn apply_settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::CloseSettings => self.show_settings = false,
            SettingsAction::SetTheme(t) => self.apply_theme(t),
            SettingsAction::ToggleSource(id, on) => self.toggle_source(id, on),
            SettingsAction::SetVisible(uid, vis) => {
                if let Some(w) = self.config.widgets.iter_mut().find(|w| w.uid == uid) {
                    w.visible = vis;
                }
            }
            SettingsAction::Remove(uid) => self.remove_widget(&uid),
            SettingsAction::ResetSize(uid) => {
                if let Some(w) = self.config.widgets.iter_mut().find(|w| w.uid == uid) {
                    w.span = None;
                    w.height = None;
                }
            }
            SettingsAction::Move(uid, delta) => self.move_widget(&uid, delta),
        }
    }

    /// Enable or disable a source and remember the choice. No re-poll needed:
    /// the poller thread picks the change up on its next sample.
    fn toggle_source(&mut self, id: &str, on: bool) {
        if !self.poller.set_source_enabled(id, on) {
            return;
        }
        let id = id.to_string();
        info!("source `{id}` {}", if on { "enabled" } else { "disabled" });
        if on {
            self.config.disabled_sources.retain(|s| *s != id);
        } else if !self.config.disabled_sources.contains(&id) {
            self.config.disabled_sources.push(id);
        }
    }

    fn settings_general(&self, ui: &mut egui::Ui, action: &mut Option<SettingsAction>, draft: &mut SettingsDraft) {
        section_label(ui, &self.theme, "Theme");
        ui.horizontal_wrapped(|ui| {
            for name in ThemeName::ALL {
                let active = self.config.theme == name;
                let btn =
                    egui::Button::new(egui::RichText::new(name.label()).size(self.theme.fs(10.0)).color(if active {
                        self.theme.accent
                    } else {
                        self.theme.text_muted
                    }))
                    .fill(if active { with_alpha(self.theme.accent, 30) } else { self.theme.bg_widget })
                    .stroke(egui::Stroke::new(1.0_f32, if active { self.theme.accent } else { self.theme.border }))
                    .rounding(egui::Rounding::same(6.0))
                    .min_size(egui::vec2(0.0, 24.0));
                if ui.add(btn).clicked() {
                    *action = Some(SettingsAction::SetTheme(name));
                }
            }
        });
        ui.add_space(8.0);
        section_label(ui, &self.theme, "Display & sampling");
        let percent = |v: f64, _: std::ops::RangeInclusive<usize>| format!("{:.0}%", v * 100.0);
        let parse_percent = |s: &str| s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0);
        egui::Grid::new("general_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Text size").on_hover_text("Scales all text in the app.");
            ui.add(
                egui::Slider::new(&mut draft.scale, 0.8..=1.5)
                    .step_by(0.05)
                    .custom_formatter(percent)
                    .custom_parser(parse_percent),
            );
            ui.end_row();

            ui.label("Refresh interval")
                .on_hover_text("Time between samples. Charts always show the last 300 samples.");
            ui.add(egui::Slider::new(&mut draft.rate, 250.0..=5000.0).step_by(250.0).suffix(" ms"));
            ui.end_row();

            ui.label("Overlay opacity").on_hover_text("Background opacity of the Compact view.");
            ui.add(
                egui::Slider::new(&mut draft.opacity, 0.3..=1.0).custom_formatter(percent).custom_parser(parse_percent),
            );
            ui.end_row();
        });
        ui.add_space(8.0);
        self.settings_updates(ui, draft);
    }

    /// The running version, whether a newer one exists, and the opt-out.
    fn settings_updates(&self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
        section_label(ui, &self.theme, "Updates");
        ui.checkbox(&mut draft.check_updates, "Check for updates at start").on_hover_text(
            "Once per start, asks GitHub for the latest HardViz release. \
             Nothing about you or this computer is sent. Takes effect at the next start.",
        );
        let small = |text: String, color| egui::RichText::new(text).size(self.theme.fs(10.0)).color(color);
        let muted = self.theme.text_muted;
        ui.horizontal_wrapped(|ui| {
            match self.update.as_ref().map(crate::update::UpdateCheck::status) {
                None => {
                    ui.label(small(format!("HardViz {VERSION} \u{b7} not checked this session"), muted));
                }
                Some(UpdateStatus::Checking) => {
                    ui.label(small(format!("HardViz {VERSION} \u{b7} checking\u{2026}"), muted));
                }
                Some(UpdateStatus::UpToDate) => {
                    ui.label(small(format!("HardViz {VERSION} is the latest version"), self.theme.success));
                }
                Some(UpdateStatus::Available(release)) => {
                    ui.label(small(format!("HardViz {VERSION} \u{b7}"), muted));
                    ui.hyperlink_to(
                        small(format!("version {} is available", release.version), self.theme.accent),
                        &release.url,
                    )
                    .on_hover_text(format!("Open the release page: {}", release.url));
                }
                Some(UpdateStatus::Failed(reason)) => {
                    ui.label(small(format!("HardViz {VERSION} \u{b7} could not check for updates"), muted))
                        .on_hover_text(reason);
                }
            }
            ui.hyperlink_to(small("All releases".into(), muted), releases_url());
        });
    }
}

/// Everything the settings panel displays, copied out of the shared state.
struct SettingsSnapshot {
    sources: Vec<SourceInfo>,
    metrics: Vec<MetricRow>,
    widgets: Vec<WidgetRow>,
}

/// Slider values being edited this frame, written back to the config after.
struct SettingsDraft {
    rate: f32,
    opacity: f32,
    scale: f32,
    check_updates: bool,
}

impl SettingsDraft {
    fn from_config(c: &AppConfig) -> Self {
        Self {
            rate: to_f32(total_f64(c.refresh_rate_ms)),
            opacity: c.overlay_opacity,
            scale: c.font_scale,
            check_updates: c.check_for_updates,
        }
    }

    fn apply(&self, c: &mut AppConfig) {
        c.refresh_rate_ms = to_u64(self.rate);
        c.overlay_opacity = self.opacity;
        c.font_scale = self.scale;
        c.check_for_updates = self.check_updates;
    }
}

enum SettingsAction {
    CloseSettings,
    SetTheme(ThemeName),
    ToggleSource(&'static str, bool),
    SetVisible(String, bool),
    Remove(String),
    Move(String, isize),
    ResetSize(String),
}

/// One row of the metric -> provider table in settings.
struct MetricRow {
    id: MetricId,
    value: String,
    provider: String,
    detail: String,
    live: bool,
}

/// One row of the widget list in settings.
struct WidgetRow {
    uid: String,
    type_id: String,
    name: String,
    visible: bool,
    /// Declared metric dependencies with live markers, for the hover tooltip.
    deps: String,
    /// The user resized this card.
    custom_size: bool,
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{app, run, wait_for_samples};
    use super::*;

    #[test]
    fn slider_draft_round_trips_the_config() {
        let (_ctx, mut app) = app();
        let mut draft = SettingsDraft::from_config(&app.config);
        draft.rate = 750.0;
        draft.scale = 1.25;
        draft.check_updates = false;
        draft.apply(&mut app.config);
        assert_eq!(app.config.refresh_rate_ms, 750);
        assert_eq!(app.config.font_scale, 1.25);
        assert!(!app.config.check_for_updates);
    }

    #[test]
    fn every_update_state_draws_in_settings_and_the_top_bar() {
        use crate::update::{Release, UpdateCheck};
        let (ctx, mut app) = app();
        assert!(app.update.is_none(), "tests never start a network check");
        app.show_settings = true;
        let release = Release { version: "9.9.9".into(), url: "https://example.invalid/r".into() };
        for status in [
            UpdateStatus::Checking,
            UpdateStatus::UpToDate,
            UpdateStatus::Failed("offline".into()),
            UpdateStatus::Available(release.clone()),
        ] {
            app.update = Some(UpdateCheck::finished(status));
            run(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.update.as_ref().and_then(UpdateCheck::available), Some(release));
    }

    #[test]
    fn actions_change_the_config() {
        let (_ctx, mut app) = app();
        let uid = app.config.widgets[0].uid.clone();
        app.config.widgets[0].span = Some(2);
        app.apply_settings_action(SettingsAction::ResetSize(uid.clone()));
        assert_eq!(app.config.widgets[0].span, None);
        app.apply_settings_action(SettingsAction::SetVisible(uid.clone(), false));
        assert!(!app.config.widgets[0].visible);
        app.apply_settings_action(SettingsAction::SetTheme(ThemeName::OledBlack));
        assert_eq!(app.config.theme, ThemeName::OledBlack);
        app.show_settings = true;
        app.apply_settings_action(SettingsAction::CloseSettings);
        assert!(!app.show_settings);
        app.apply_settings_action(SettingsAction::Remove(uid.clone()));
        assert!(app.config.widgets.iter().all(|w| w.uid != uid));
    }

    #[test]
    fn toggling_a_source_is_remembered() {
        let (_ctx, mut app) = app();
        wait_for_samples(&app);
        app.apply_settings_action(SettingsAction::ToggleSource("steady", false));
        assert_eq!(app.config.disabled_sources, ["steady"]);
        assert!(!app.poller.source_infos()[0].enabled);
        app.apply_settings_action(SettingsAction::ToggleSource("steady", true));
        assert!(app.config.disabled_sources.is_empty());
        app.apply_settings_action(SettingsAction::ToggleSource("unknown", false));
        assert!(app.config.disabled_sources.is_empty(), "unknown ids are not stored");
    }

    #[test]
    fn snapshot_lists_sources_metrics_and_widgets() {
        let (ctx, mut app) = app();
        wait_for_samples(&app);
        app.show_settings = true;
        run(&ctx, &mut app, vec![]);
        let snap = app.settings_snapshot();
        assert_eq!(snap.sources.len(), 1);
        assert_eq!(snap.metrics.len(), METRICS.len());
        let cpu = snap.metrics.iter().find(|m| m.id == crate::metrics::metric::CPU_USAGE).expect("listed");
        assert!(cpu.live && cpu.provider == "steady" && cpu.value == "42%");
        assert_eq!(snap.widgets.len(), app.config.widgets.len());
    }
}
