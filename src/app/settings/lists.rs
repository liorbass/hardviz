//! Settings -> Metrics (the metric -> provider table) and the widget chips.

use super::super::style::{card_frame, section_label};
use super::{HardVizApp, MetricRow, SettingsAction, WidgetRow};
use crate::num::floor_usize;
use eframe::egui;

/// Gap between widget chips, both directions.
const CHIP_GAP: f32 = 6.0;

impl HardVizApp {
    pub(super) fn settings_metrics(&self, ui: &mut egui::Ui, rows: &[MetricRow]) {
        section_label(ui, &self.theme, "Metrics");
        // Two halves side by side when the column is wide enough, so one long
        // list does not set the height of the whole panel; one list otherwise
        // (a grid cannot shrink, and squeezed halves overlap).
        if ui.available_width() >= 520.0 {
            let half = rows.len().div_ceil(2);
            ui.columns(2, |cols| {
                for (k, chunk) in rows.chunks(half.max(1)).enumerate() {
                    self.metrics_grid(&mut cols[k.min(1)], chunk, k);
                }
            });
        } else {
            self.metrics_grid(ui, rows, 0);
        }
    }

    fn metrics_grid(&self, ui: &mut egui::Ui, rows: &[MetricRow], salt: usize) {
        egui::Grid::new(("metrics_grid", salt)).num_columns(4).spacing([6.0, 2.0]).striped(true).show(ui, |ui| {
            for row in rows {
                ui.colored_label(if row.live { self.theme.success } else { self.theme.danger }, "\u{25CF}");
                ui.label(
                    egui::RichText::new(row.id.key())
                        .size(self.theme.fs(10.0))
                        .monospace()
                        .color(self.theme.text_secondary),
                );
                ui.label(egui::RichText::new(&row.value).size(self.theme.fs(10.0)).color(if row.live {
                    self.theme.text_primary
                } else {
                    self.theme.text_muted
                }));
                ui.label(
                    egui::RichText::new(format!("\u{2190} {}", row.provider))
                        .size(self.theme.fs(10.0))
                        .color(self.theme.text_muted),
                )
                .on_hover_text(row.detail.clone());
                ui.end_row();
            }
        });
    }

    /// Widgets as compact chips that wrap across the panel's full width.
    pub(super) fn settings_widgets(&self, ui: &mut egui::Ui, list: &[WidgetRow], action: &mut Option<SettingsAction>) {
        section_label(ui, &self.theme, "Widgets");
        let chip = card_frame(&self.theme, self.theme.bg_widget, 8.0, egui::Margin::symmetric(6.0, 2.0));
        let icon = |ui: &mut egui::Ui, glyph: &str, tip: &str, enabled: bool| -> bool {
            ui.add_enabled(
                enabled,
                egui::Button::new(egui::RichText::new(glyph).size(self.theme.fs(12.0)))
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(16.0, 16.0)),
            )
            .on_hover_text(tip)
            .clicked()
        };
        // A grid sized from the available width, not a wrapping row: egui
        // places a Frame at a fixed rect, so frames in a wrapping row never
        // wrap and the row widened the whole panel.
        let chip_w = self.theme.fs(250.0);
        let per_row = chips_per_row(ui.available_width(), chip_w);
        egui::Grid::new("widget_chips").spacing([CHIP_GAP, CHIP_GAP]).show(ui, |ui| {
            for (idx, row) in list.iter().enumerate() {
                if idx > 0 && idx % per_row == 0 {
                    ui.end_row();
                }
                chip.show(ui, |ui| {
                    ui.set_width(chip_w - 12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let mut vis = row.visible;
                        if ui.checkbox(&mut vis, "").on_hover_text("Show on the dashboard").changed() {
                            *action = Some(SettingsAction::SetVisible(row.uid.clone(), vis));
                        }
                        let dep_tip = dependency_tip(row);
                        ui.label(egui::RichText::new(&row.name).size(self.theme.fs(11.0)).color(if row.visible {
                            self.theme.text_primary
                        } else {
                            self.theme.text_muted
                        }))
                        .on_hover_text(dep_tip);
                        ui.add_space(4.0);
                        if icon(ui, "\u{2191}", "Move earlier", idx > 0) {
                            *action = Some(SettingsAction::Move(row.uid.clone(), -1));
                        }
                        if icon(ui, "\u{2193}", "Move later", idx + 1 < list.len()) {
                            *action = Some(SettingsAction::Move(row.uid.clone(), 1));
                        }
                        if row.custom_size && icon(ui, "\u{21BA}", "Reset to default size", true) {
                            *action = Some(SettingsAction::ResetSize(row.uid.clone()));
                        }
                        if icon(ui, "\u{D7}", "Remove widget", true) {
                            *action = Some(SettingsAction::Remove(row.uid.clone()));
                        }
                    });
                });
            }
        });
    }
}

/// How many chips of width `chip_w` fit across `available` (at least one).
fn chips_per_row(available: f32, chip_w: f32) -> usize {
    floor_usize((available + CHIP_GAP) / (chip_w + CHIP_GAP)).max(1)
}

/// Hover text for a widget chip: its type and the metrics it requires.
fn dependency_tip(row: &WidgetRow) -> String {
    if row.deps.is_empty() {
        format!("{} (no declared metrics)", row.type_id)
    } else {
        format!("{}\nrequires:\n{}", row.type_id, row.deps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(deps: &str) -> WidgetRow {
        WidgetRow {
            uid: "gauge.cpu#0".into(),
            type_id: "gauge.cpu".into(),
            name: "CPU Gauge".into(),
            visible: true,
            deps: deps.into(),
            custom_size: false,
        }
    }

    #[test]
    fn chips_wrap_to_the_panel_width() {
        assert_eq!(chips_per_row(1024.0, 250.0), 4);
        assert_eq!(chips_per_row(500.0, 250.0), 1);
        assert_eq!(chips_per_row(10.0, 250.0), 1, "always at least one");
    }

    #[test]
    fn dependency_tip_lists_requirements() {
        assert_eq!(dependency_tip(&row("")), "gauge.cpu (no declared metrics)");
        assert_eq!(dependency_tip(&row("● cpu.usage")), "gauge.cpu\nrequires:\n● cpu.usage");
    }
}
