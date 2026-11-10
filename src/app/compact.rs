//! The compact overlay: a borderless strip of headline readings.
//!
//! Which metrics it shows is the two lists below; labels, colours and units
//! come from the metric catalogue and the theme, like every widget.

use super::style::{card_frame, flat_button};
use super::HardVizApp;
use crate::metrics::metric::{CPU_TEMP, CPU_USAGE, GPU_TEMP, GPU_USAGE, RAM_USAGE, VRAM_USAGE};
use crate::metrics::MetricId;
use crate::num::{to_f32, to_u8};
use crate::theme::with_alpha;
use crate::theme::Theme;
use eframe::egui;

/// Usage bars, left to right (0-100 % metrics).
const USAGE: [MetricId; 4] = [CPU_USAGE, RAM_USAGE, GPU_USAGE, VRAM_USAGE];
/// Temperatures after the bars. The first is always shown (as `--°` with the
/// reason on hover when missing); the others only when they have a reading.
const TEMPS: [MetricId; 2] = [CPU_TEMP, GPU_TEMP];

/// One value to show, read out of the store.
struct Readout {
    label: &'static str,
    value: Option<f64>,
    color: egui::Color32,
    /// Why there is no value.
    why: String,
}

enum Clicked {
    Expand,
    TogglePin,
}

impl HardVizApp {
    pub(super) fn draw_compact(&mut self, ui: &mut egui::Ui) {
        // No processes widget is on screen in compact mode.
        self.poller.set_sample_processes(false);
        let (usage, temps) = self.compact_readouts();
        let theme = &self.theme;
        let alpha = to_u8(self.config.overlay_opacity * 255.0);
        let card = card_frame(theme, with_alpha(theme.bg_widget, alpha), 10.0, egui::Margin::same(10.0));
        let pinned = self.config.compact_always_on_top;
        let mut clicked = None;

        let shown = card.show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, r) in usage.iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    usage_bar(ui, theme, r);
                }
                for (i, t) in temps.iter().enumerate().filter(|(i, t)| *i == 0 || t.value.is_some()) {
                    ui.separator();
                    temperature(ui, theme, t, i == 0);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = buttons(ui, theme, pinned);
                });
            });
        });

        // The overlay is borderless: dragging anywhere on the bar moves the
        // window. Buttons only sense clicks, so they keep working.
        let drag = ui.interact(shown.response.rect, egui::Id::new("compact_drag"), egui::Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        match clicked {
            Some(Clicked::TogglePin) => {
                self.config.compact_always_on_top = !pinned;
                self.window.set_always_on_top(!pinned);
            }
            Some(Clicked::Expand) => self.switch_view_mode(self.window.return_mode()),
            None => {}
        }
    }

    /// Every value the strip shows, under one lock.
    fn compact_readouts(&self) -> (Vec<Readout>, Vec<Readout>) {
        let view = self.state.lock();
        let read = |id: MetricId| Readout {
            label: id.def().label,
            value: view.as_ref().and_then(|v| v.store.smoothed(id)),
            color: self.theme.metric_color(id.def().kind),
            why: view
                .as_ref()
                .and_then(|v| v.diagnostics.get(&id).map(super::super::source::MetricDiagnosis::explain))
                .unwrap_or_else(|| "no data yet".into()),
        };
        (USAGE.into_iter().map(read).collect(), TEMPS.into_iter().map(read).collect())
    }
}

/// Label, percentage and a thin bar.
fn usage_bar(ui: &mut egui::Ui, theme: &Theme, r: &Readout) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(r.label).size(theme.fs(10.0)).strong().color(theme.text_secondary));
            let (text, color) = match r.value {
                Some(v) => (format!("{v:.0}%"), r.color),
                None => ("N/A".into(), theme.text_muted),
            };
            let value = ui.label(egui::RichText::new(text).size(theme.fs(13.0)).strong().color(color));
            if r.value.is_none() {
                value.on_hover_text(&r.why);
            }
        });

        let size = egui::vec2(78.0, 3.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.add(egui::Shape::rect_filled(rect, 1.5, theme.gauge_bg));
        let fill_w = size.x * r.value.map_or(0.0, |v| to_f32((v / 100.0).clamp(0.0, 1.0)));
        if fill_w > 0.5 {
            let fill = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, size.y));
            painter.add(egui::Shape::rect_filled(fill, 1.5, r.color));
        }
    });
}

/// `CPU 64°`, coloured warm to hot; `CPU --°` with the reason when missing.
fn temperature(ui: &mut egui::Ui, theme: &Theme, t: &Readout, explain_missing: bool) {
    let text = match t.value {
        Some(v) => egui::RichText::new(format!("{} {v:.0}°", t.label)).color(theme.temp_color(v)),
        None => egui::RichText::new(format!("{} --°", t.label)).color(theme.text_muted),
    };
    let label = ui.label(text.size(theme.fs(11.0)));
    if t.value.is_none() && explain_missing {
        label.on_hover_text(&t.why);
    }
}

/// Expand and always-on-top, right-aligned.
fn buttons(ui: &mut egui::Ui, theme: &Theme, pinned: bool) -> Option<Clicked> {
    let mut clicked = None;
    if ui.add(flat_button(egui::RichText::new("\u{26F6}").size(theme.fs(13.0)))).on_hover_text("Expand (W)").clicked() {
        clicked = Some(Clicked::Expand);
    }
    let pin_color = if pinned { theme.accent } else { theme.text_muted };
    let pin_tip =
        if pinned { "Always on top: on (click to turn off)" } else { "Always on top: off (click to turn on)" };
    if ui
        .add(flat_button(egui::RichText::new("\u{1F4CC}").size(theme.fs(12.0)).color(pin_color)))
        .on_hover_text(pin_tip)
        .clicked()
    {
        clicked = Some(Clicked::TogglePin);
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{app, run, wait_for_samples};
    use super::*;
    use crate::config::ViewMode;
    use crate::metrics::Unit;

    #[test]
    fn the_strip_shows_percentages_then_temperatures() {
        assert!(USAGE.iter().all(|id| id.def().unit == Unit::Percent));
        assert!(TEMPS.iter().all(|id| id.def().unit == Unit::Celsius));
    }

    #[test]
    fn readouts_come_from_the_catalogue_and_explain_gaps() {
        let (ctx, mut app) = app();
        wait_for_samples(&app);
        run(&ctx, &mut app, vec![]);
        let (usage, temps) = app.compact_readouts();
        assert_eq!(usage[0].label, "CPU");
        assert_eq!(usage[0].value, Some(42.0), "the fake source's reading");
        assert_eq!(usage[0].color, app.theme.metric_color(CPU_USAGE.def().kind));
        assert_eq!(temps[0].value, None);
        assert_eq!(temps[0].why, "no registered source provides this metric");
    }

    #[test]
    fn expand_returns_to_the_previous_mode() {
        let (ctx, mut app) = app();
        app.switch_view_mode(ViewMode::Fullscreen);
        app.switch_view_mode(ViewMode::Compact);
        run(&ctx, &mut app, vec![]);
        app.switch_view_mode(app.window.return_mode());
        assert_eq!(app.config.view_mode, ViewMode::Fullscreen);
    }
}
