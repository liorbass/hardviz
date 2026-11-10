//! The shell's own controls: keyboard shortcuts, the top bar, and the Add
//! widget panel.

use super::style::{bar_toggle, card_frame, flat_button};
use super::HardVizApp;
use crate::config::ViewMode;
use eframe::egui;

impl HardVizApp {
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (f11, esc, w, c, f, mods) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F11),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::W),
                i.key_pressed(egui::Key::C),
                i.key_pressed(egui::Key::F),
                i.modifiers.ctrl || i.modifiers.alt || i.modifiers.command,
            )
        });

        if f11 {
            let next = match self.config.view_mode {
                ViewMode::Fullscreen => ViewMode::Windowed,
                _ => ViewMode::Fullscreen,
            };
            self.switch_view_mode(next);
        }
        if esc && self.config.view_mode != ViewMode::Windowed {
            self.switch_view_mode(ViewMode::Windowed);
        }
        // Text input must not trigger mode switches.
        let typing = ctx.wants_keyboard_input();
        if !mods && !typing {
            if w {
                self.switch_view_mode(ViewMode::Windowed);
            }
            if c {
                self.switch_view_mode(ViewMode::Compact);
            }
            if f {
                self.switch_view_mode(ViewMode::Fullscreen);
            }
        }
    }

    pub(super) fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("HardViz").size(self.theme.fs(16.0)).strong().color(self.theme.accent));
            ui.separator();

            for (mode, label) in
                [(ViewMode::Windowed, "Windowed"), (ViewMode::Compact, "Compact"), (ViewMode::Fullscreen, "Fullscreen")]
            {
                let theme = self.theme.clone();
                if bar_toggle(ui, &theme, label, self.config.view_mode == mode).clicked() {
                    self.switch_view_mode(mode);
                }
            }

            // Right-aligned toggles, styled like the view-mode buttons so the
            // bar reads as one control set.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let theme = self.theme.clone();
                if bar_toggle(ui, &theme, "Settings", self.show_settings)
                    .on_hover_text("Theme, text size, sampling, data sources, widgets")
                    .clicked()
                {
                    self.show_settings = !self.show_settings;
                    self.show_add_widget = false;
                }
                if bar_toggle(ui, &theme, "Add widget", self.show_add_widget)
                    .on_hover_text("Add a widget to the dashboard")
                    .clicked()
                {
                    self.show_add_widget = !self.show_add_widget;
                    self.show_settings = false;
                }
                if bar_toggle(ui, &theme, "Edit layout", !self.config.widgets_locked)
                    .on_hover_text("Move cards by dragging them; resize from the corner grip")
                    .clicked()
                {
                    self.config.widgets_locked = !self.config.widgets_locked;
                }
                if bar_toggle(ui, &theme, "Fit to window", self.config.fit_to_window)
                    .on_hover_text(
                        "On: card heights scale so the dashboard fills the window \
                         (scrolls only if even the smallest sizes do not fit).\n\
                         Off: cards keep their exact sizes and the page scrolls.",
                    )
                    .clicked()
                {
                    self.config.fit_to_window = !self.config.fit_to_window;
                }
                // Edit-mode hint, only where it fits: in a narrow window it
                // would run into the view-mode buttons on the left.
                if !self.config.widgets_locked {
                    let hint = egui::RichText::new("Drag a card to move it \u{b7} drag its corner to resize")
                        .size(self.theme.fs(10.0))
                        .color(theme.text_muted);
                    let needed = ui.fonts(|f| {
                        f.layout_no_wrap(hint.text().to_string(), self.theme.font(10.0), egui::Color32::WHITE).size().x
                    });
                    if ui.available_width() > needed + 16.0 {
                        ui.label(hint);
                    }
                }
            });
        });

        if self.show_add_widget {
            ui.add_space(4.0);
            self.draw_add_widget_panel(ui);
        }
    }

    fn draw_add_widget_panel(&mut self, ui: &mut egui::Ui) {
        let card = card_frame(&self.theme, self.theme.bg_secondary, 10.0, egui::Margin::same(12.0));

        let factories: Vec<(&'static str, &'static str)> =
            self.widget_registry.factories().iter().map(|f| (f.type_id, f.display_name)).collect();

        let mut to_add: Option<String> = None;

        card.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Add widget").size(self.theme.fs(13.0)).strong().color(self.theme.text_primary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(flat_button("\u{D7}")).clicked() {
                        self.show_add_widget = false;
                    }
                });
            });
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                for (type_id, name) in &factories {
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(*name).size(self.theme.fs(11.0)))
                                .fill(self.theme.bg_widget)
                                .stroke(egui::Stroke::new(1.0_f32, self.theme.border))
                                .rounding(egui::Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        to_add = Some((*type_id).to_string());
                    }
                }
            });
        });

        if let Some(t) = to_add {
            self.add_widget(&t);
            self.show_add_widget = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{app, key, run, run_held};
    use crate::config::ViewMode;
    use eframe::egui;

    #[test]
    fn mode_shortcuts_switch_views_and_escape_returns_to_windowed() {
        let (ctx, mut app) = app();
        run(&ctx, &mut app, vec![key(egui::Key::C)]);
        assert_eq!(app.config.view_mode, ViewMode::Compact);
        run(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert_eq!(app.config.view_mode, ViewMode::Windowed);
        run(&ctx, &mut app, vec![key(egui::Key::F11)]);
        assert_eq!(app.config.view_mode, ViewMode::Fullscreen);
        run(&ctx, &mut app, vec![key(egui::Key::W)]);
        assert_eq!(app.config.view_mode, ViewMode::Windowed);
    }

    #[test]
    fn shortcuts_are_ignored_with_modifiers() {
        let (ctx, mut app) = app();
        let ctrl_c = egui::Event::Key {
            key: egui::Key::C,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        };
        run_held(&ctx, &mut app, vec![ctrl_c], egui::Modifiers::CTRL);
        assert_eq!(app.config.view_mode, ViewMode::Windowed, "Ctrl+C is copy, not compact");
    }

    #[test]
    fn top_bar_and_add_widget_panel_draw() {
        let (ctx, mut app) = app();
        app.show_add_widget = true;
        app.config.widgets_locked = false;
        run(&ctx, &mut app, vec![]);
        assert!(app.show_add_widget);
    }
}
