//! The dashboard grid: placing cards, drawing them, and the edit-mode drag
//! and resize interactions. The geometry itself is in [`super::layout`].

use super::layout::{cell_width, fit_rows, layout_columns, pack_rows, resize_from_pointer, CellGeo, GRID_GAP};
use super::style::flat_button;
use super::{HardVizApp, SWAP_ANIM_SECS};
use crate::config::{Visual, WidgetInstance};
use crate::num::{count_f32, to_u8};
use crate::poller::StateView;
use crate::theme::with_alpha;
use crate::widget::WidgetCtx;
use eframe::egui;

impl HardVizApp {
    /// Lay out and draw every visible widget.
    ///
    /// One 4-column grid: each card spans 1-4 columns (its saved span, else
    /// its size class's default) and has its own height. Cards are packed into
    /// rows in config order and stretch to their row's height. Narrow windows
    /// drop to 2 or 1 columns, scaling spans proportionally.
    ///
    /// `fit_height`: when set (Fit to window), row heights are scaled together
    /// so the dashboard fills that height - shrinking down to each row's
    /// minimum, or growing up to `MAX_FIT_GROW` - keeping the user's relative
    /// sizes. If even the minimums do not fit, the page scrolls instead.
    pub(super) fn draw_dashboard(&mut self, ui: &mut egui::Ui, fit_height: Option<f32>) {
        struct Entry {
            uid: String,
            span4: u8,
            height: f32,
            min: f32,
            wants_processes: bool,
        }

        self.widget_rects.clear();
        let entries: Vec<Entry> = self
            .config
            .widgets
            .iter()
            .filter(|w| w.visible)
            .filter_map(|w| {
                let inst = self.widgets.get(&w.uid)?;
                let card = inst.card();
                let min = card.min_height;
                Some(Entry {
                    uid: w.uid.clone(),
                    span4: w.span.unwrap_or_else(|| card.size.default_span()).clamp(1, 4),
                    height: w.height.unwrap_or(card.height).max(min),
                    min,
                    wants_processes: inst.wants_processes(),
                })
            })
            .collect();

        // Sample processes only while a widget on screen shows them.
        self.poller.set_sample_processes(entries.iter().any(|e| e.wants_processes));

        // The visible width, never the content width: a vertical scroll area
        // remembers how wide its content was, so sizing from that would let
        // one over-wide frame keep the dashboard too wide.
        let avail_w = ui.available_width().min(ui.clip_rect().width() - 2.0);
        let cols = layout_columns(avail_w);
        let col_w = cell_width(avail_w, GRID_GAP, cols);

        let spans: Vec<u8> = entries.iter().map(|e| e.span4).collect();
        let rows = pack_rows(&spans, cols);

        let row_max = |f: fn(&Entry) -> f32| -> Vec<f32> {
            rows.iter().map(|r| r.iter().map(|p| f(&entries[p.index])).fold(0.0, f32::max)).collect()
        };
        let (natural, minimum) = (row_max(|e| e.height), row_max(|e| e.min));
        let gaps = GRID_GAP * count_f32(rows.len().saturating_sub(1));
        let (heights, scale) = fit_rows(&natural, &minimum, fit_height.map(|h| h - gaps));

        let total_h: f32 = heights.iter().sum::<f32>() + gaps;
        let (area, _) = ui.allocate_exact_size(egui::vec2(avail_w, total_h.max(1.0)), egui::Sense::hover());

        // One lock for the whole dashboard, released before drag resolution.
        let state = self.state.clone();
        let Some(view) = state.lock() else { return };
        let mut y = area.min.y;
        for (row, h) in rows.iter().zip(&heights) {
            for p in row {
                let x = area.min.x + count_f32(p.col) * (col_w + GRID_GAP);
                let w = count_f32(p.span) * col_w + count_f32(p.span - 1) * GRID_GAP;
                let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, *h));
                let entry = &entries[p.index];
                let geo = CellGeo { cols, col_w, scale, min: entry.min };
                self.draw_widget_cell(ui, &view, &entry.uid, rect, geo);
            }
            y += h + GRID_GAP;
        }
        drop(view);

        self.resolve_drag(ui);
    }

    /// Hit-test the pointer against this frame's widget rects and, when a
    /// drag ends over another card, swap the two.
    fn resolve_drag(&mut self, ui: &egui::Ui) {
        let Some(from) = self.dragging.clone() else {
            self.drag_target = None;
            return;
        };
        let pointer = ui.input(|i| i.pointer.interact_pos());
        self.drag_target = pointer.and_then(|p| {
            self.widget_rects.iter().find(|(id, r)| *id != from && r.contains(p)).map(|(id, _)| id.clone())
        });
        if !ui.input(|i| i.pointer.any_released()) {
            return;
        }
        if let Some(to) = self.drag_target.take() {
            let index = |uid: &str| self.config.widgets.iter().position(|w| w.uid == uid);
            if let (Some(a), Some(b)) = (index(&from), index(&to)) {
                self.swap_widgets(a, b);
            }
        }
        self.dragging = None;
    }

    /// One dashboard card: feedback halo, the widget itself, and (in edit
    /// mode) the move and resize interactions.
    fn draw_widget_cell(&mut self, ui: &mut egui::Ui, view: &StateView, uid: &str, rect: egui::Rect, geo: CellGeo) {
        self.paint_cell_feedback(ui, uid, rect);
        self.draw_card(ui, view, uid, rect);
        self.widget_rects.push((uid.to_string(), rect));
        if !self.config.widgets_locked {
            self.handle_cell_editing(ui, uid, rect, geo);
        }
    }

    /// Drag shadow, drop-target outline and post-swap glow, behind the card.
    fn paint_cell_feedback(&self, ui: &egui::Ui, uid: &str, rect: egui::Rect) {
        let accent = self.theme.accent;
        let halo = rect.expand(3.0);
        let rounding = egui::Rounding::same(12.0);

        if self.dragging.as_deref() == Some(uid) {
            let shadow = egui::epaint::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 14.0,
                spread: 0.0,
                color: with_alpha(accent, 90),
            };
            ui.painter().add(shadow.as_shape(rect, rounding));
        }

        let is_target = self.drag_target.as_deref() == Some(uid) && self.dragging.is_some();
        if is_target {
            ui.painter().rect(halo, rounding, with_alpha(accent, 40), egui::Stroke::new(2.0_f32, accent));
            return;
        }
        if !self.recently_swapped.iter().any(|u| u == uid) {
            return;
        }
        // smoothstep so the glow eases out instead of fading linearly
        let t = (self.swap_timer / SWAP_ANIM_SECS).clamp(0.0, 1.0);
        let glow = t * t * (3.0 - 2.0 * t);
        if glow > 0.0 {
            ui.painter().rect(
                halo,
                rounding,
                with_alpha(accent, to_u8(glow * 24.0)),
                egui::Stroke::new(1.0_f32 + glow, with_alpha(accent, to_u8(glow * 160.0))),
            );
        }
    }

    /// The card and its content, filling exactly `rect`.
    fn draw_card(&mut self, ui: &mut egui::Ui, view: &StateView, uid: &str, rect: egui::Rect) {
        // Registered before the widget draws, so the widget's own controls
        // (e.g. the process table's sort headers) get their clicks first.
        let menu = ui.interact(rect, egui::Id::new(("card_menu", uid)), egui::Sense::click());
        let ctx =
            WidgetCtx::new(&view.store, &view.diagnostics, &view.processes, &self.theme, self.config.refresh_rate_ms);
        let widgets = &mut self.widgets;
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                let Some(w) = widgets.get_mut(uid) else { return };
                // The shell owns what every widget shares: the card, its title,
                // and the N/A placeholder (with the reason) when none of the
                // widget's metrics has data.
                let title = w.title().to_string();
                crate::widgets::frame(ui, ctx.theme, &title, |ui, h| {
                    if w.has_data(&ctx) {
                        w.draw(ui, &ctx, h);
                    } else {
                        let reason = w
                            .requires()
                            .iter()
                            .find_map(|&id| ctx.diagnosis(id).filter(|d| !d.is_available()))
                            .map_or_else(|| "no data yet".into(), super::super::source::MetricDiagnosis::explain);
                        crate::widgets::unavailable(ui, ctx.theme, h, &reason);
                    }
                });
            },
        );

        // Single-metric cards: right-click to choose how they are drawn.
        let Some(current) = self.widgets.get(uid).and_then(|w| w.visual()) else { return };
        let mut chosen = None;
        menu.context_menu(|ui| {
            ui.label(egui::RichText::new("Show as").size(self.theme.fs(10.0)).color(self.theme.text_muted));
            for visual in Visual::ALL {
                if ui.radio(current == visual, visual.label()).clicked() {
                    chosen = Some(visual);
                    ui.close_menu();
                }
            }
        });
        if let Some(visual) = chosen {
            self.set_visual(uid, visual);
        }
    }

    /// Edit mode: drag the card to move it, or its corner grip to resize it.
    fn handle_cell_editing(&mut self, ui: &mut egui::Ui, uid: &str, rect: egui::Rect, geo: CellGeo) {
        let response = ui.interact(rect, egui::Id::new(("widget_drag", uid)), egui::Sense::drag());
        if response.drag_started() {
            self.dragging = Some(uid.to_string());
        }

        // Registered after the card so the grip wins the pointer where they overlap.
        let grip = egui::Rect::from_min_max(rect.max - egui::vec2(18.0, 18.0), rect.max);
        let handle = ui
            .interact(grip, egui::Id::new(("widget_resize", uid)), egui::Sense::click_and_drag())
            .on_hover_text("Drag to resize \u{b7} double-click to reset");
        let active = handle.hovered() || handle.dragged();
        if active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
        let grip_color = if active { self.theme.accent } else { self.theme.text_muted };
        for k in 1..=3 {
            let d = count_f32(k) * 4.0;
            ui.painter().line_segment(
                [rect.max - egui::vec2(d + 3.0, 3.0), rect.max - egui::vec2(3.0, d + 3.0)],
                egui::Stroke::new(1.3_f32, grip_color),
            );
        }

        let size = if handle.double_clicked() {
            Some((None, None))
        } else if handle.dragged() {
            ui.ctx().pointer_interact_pos().map(|p| {
                let (span, height) = resize_from_pointer(p, rect, geo);
                (Some(span), Some(height))
            })
        } else {
            None
        };
        if let Some((span, height)) = size {
            if let Some(w) = self.config.widgets.iter_mut().find(|w| w.uid == uid) {
                w.span = span;
                w.height = height;
            }
        }

        // Last, so its buttons sit above the card's drag area.
        self.card_toolbar(ui, uid, rect);
    }

    /// Edit mode: the card's own controls, top right: how a single-metric
    /// card is drawn, and removing the card.
    fn card_toolbar(&mut self, ui: &mut egui::Ui, uid: &str, rect: egui::Rect) {
        let theme = self.theme.clone();
        let top = rect.min.y + 6.0;
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 8.0, top),
            egui::pos2(rect.max.x - 8.0, top + theme.fs(22.0)),
        );
        let current = self.widgets.get(uid).and_then(|w| w.visual());
        let (mut chosen, mut remove) = (None, false);
        ui.allocate_new_ui(
            egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                let close = flat_button(egui::RichText::new("\u{D7}").size(theme.fs(15.0)).color(theme.text_muted));
                if ui.add(close).on_hover_text("Remove this card").clicked() {
                    remove = true;
                }
                let Some(current) = current else { return };
                let label = egui::RichText::new(format!("{} \u{25BE}", current.label()))
                    .size(theme.fs(10.5))
                    .color(theme.accent);
                ui.menu_button(label, |ui| {
                    for visual in Visual::ALL {
                        if ui.radio(current == visual, visual.label()).clicked() {
                            chosen = Some(visual);
                            ui.close_menu();
                        }
                    }
                })
                .response
                .on_hover_text("Show this card as a gauge, bar, line chart or tile");
            },
        );
        if let Some(visual) = chosen {
            self.set_visual(uid, visual);
        }
        if remove {
            self.remove_widget(uid);
        }
    }

    // --- widget instances: the config is the source of truth for the layout ---

    /// Instantiate any config widget that has no live instance yet, and drop
    /// instances no longer present in config.
    pub(super) fn sync_widget_instances(&mut self) {
        for w in &self.config.widgets {
            if !self.widgets.contains_key(&w.uid) {
                if let Some(mut instance) = self.widget_registry.build(&w.type_id) {
                    if let (Some(visual), Some(_)) = (w.visual, instance.visual()) {
                        instance.set_visual(visual);
                    }
                    self.widgets.insert(w.uid.clone(), instance);
                }
            }
        }
        let config = &self.config;
        self.widgets.retain(|uid, _| config.widgets.iter().any(|w| w.uid == *uid));
    }

    pub(super) fn add_widget(&mut self, type_id: &str) {
        let uid = self.config.new_uid(type_id);
        self.config.widgets.push(WidgetInstance::new(uid, type_id));
        self.sync_widget_instances();
    }

    /// Draw a single-metric card as `visual`. Its size goes back to the new
    /// visual's default: a chart wants different proportions than a gauge.
    pub(super) fn set_visual(&mut self, uid: &str, visual: Visual) {
        if let Some(w) = self.config.widgets.iter_mut().find(|w| w.uid == uid) {
            w.visual = Some(visual);
            w.span = None;
            w.height = None;
        }
        if let Some(instance) = self.widgets.get_mut(uid) {
            instance.set_visual(visual);
        }
    }

    pub(super) fn remove_widget(&mut self, uid: &str) {
        self.config.widgets.retain(|w| w.uid != uid);
        self.sync_widget_instances();
    }

    /// Move a widget `delta` places in the layout order.
    pub(super) fn move_widget(&mut self, uid: &str, delta: isize) {
        let Some(from) = self.config.widgets.iter().position(|w| w.uid == uid) else { return };
        if let Some(to) = from.checked_add_signed(delta).filter(|&to| to < self.config.widgets.len()) {
            self.swap_widgets(from, to);
        }
    }

    /// Swap two widgets in the layout order and make both glow briefly.
    pub(super) fn swap_widgets(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        self.recently_swapped = vec![self.config.widgets[a].uid.clone(), self.config.widgets[b].uid.clone()];
        self.swap_timer = SWAP_ANIM_SECS;
        self.config.widgets.swap(a, b);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{app, run, wait_for_samples};

    #[test]
    fn every_visible_card_is_laid_out_and_hidden_ones_are_not() {
        let (ctx, mut app) = app();
        wait_for_samples(&app);
        run(&ctx, &mut app, vec![]);
        assert_eq!(app.widget_rects.len(), app.config.widgets.len());

        app.config.widgets[0].visible = false;
        run(&ctx, &mut app, vec![]);
        assert_eq!(app.widget_rects.len(), app.config.widgets.len() - 1);
        let hidden = &app.config.widgets[0].uid;
        assert!(app.widget_rects.iter().all(|(uid, _)| uid != hidden));
    }

    #[test]
    fn fit_to_window_keeps_cards_within_the_screen_when_it_can() {
        let (ctx, mut app) = app();
        app.config.fit_to_window = true;
        run(&ctx, &mut app, vec![]);
        let bottom = app.widget_rects.iter().map(|(_, r)| r.max.y).fold(0.0, f32::max);
        let unfitted = {
            app.config.fit_to_window = false;
            run(&ctx, &mut app, vec![]);
            app.widget_rects.iter().map(|(_, r)| r.max.y).fold(0.0, f32::max)
        };
        assert!(bottom <= unfitted, "fitting never makes the page taller");
    }

    #[test]
    fn widgets_can_be_added_moved_and_removed() {
        let (_ctx, mut app) = app();
        let n = app.config.widgets.len();
        app.add_widget("gauge.cpu");
        assert_eq!(app.config.widgets.len(), n + 1);
        let added = app.config.widgets[n].uid.clone();
        assert!(app.widgets.contains_key(&added), "an instance is built");
        assert_ne!(added, app.config.widgets[0].uid, "uids stay unique");

        let first = app.config.widgets[0].uid.clone();
        app.move_widget(&first, 1);
        assert_eq!(app.config.widgets[1].uid, first);
        assert_eq!(app.recently_swapped.len(), 2, "the swap glows");
        app.move_widget(&first, -5);
        assert_eq!(app.config.widgets[1].uid, first, "out of range: no move");

        app.remove_widget(&added);
        assert_eq!(app.config.widgets.len(), n);
        assert!(!app.widgets.contains_key(&added));
    }

    #[test]
    fn visual_choice_is_saved_and_survives_a_rebuild() {
        use crate::config::Visual;
        let (ctx, mut app) = app();
        let uid = app.config.widgets.iter().find(|w| w.type_id == "sensor.fan_speed").expect("default").uid.clone();
        app.config.widgets.iter_mut().find(|w| w.uid == uid).expect("present").span = Some(4);
        app.set_visual(&uid, Visual::Gauge);
        let saved = app.config.widgets.iter().find(|w| w.uid == uid).expect("present");
        assert_eq!(saved.visual, Some(Visual::Gauge));
        assert_eq!(saved.span, None, "size resets to the new visual's default");
        assert_eq!(app.widgets[&uid].visual(), Some(Visual::Gauge));

        // A restart rebuilds instances from the config.
        app.widgets.clear();
        app.sync_widget_instances();
        assert_eq!(app.widgets[&uid].visual(), Some(Visual::Gauge));
        run(&ctx, &mut app, vec![]);
    }

    #[test]
    fn edit_mode_close_button_removes_the_card() {
        use eframe::egui;
        let (ctx, mut app) = app();
        app.config.widgets_locked = false;
        run(&ctx, &mut app, vec![]);
        let (uid, rect) = app.widget_rects[0].clone();
        // The close button is the rightmost control in the card's top bar.
        let at = egui::pos2(rect.max.x - 16.0, rect.min.y + 6.0 + app.theme.fs(11.0));
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        run(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
        run(&ctx, &mut app, vec![button(true)]);
        run(&ctx, &mut app, vec![button(false)]);
        assert!(app.config.widgets.iter().all(|w| w.uid != uid), "{uid} was removed");
        assert!(!app.widgets.contains_key(&uid));
    }

    #[test]
    fn locked_layout_has_no_card_controls() {
        let (ctx, mut app) = app();
        app.config.widgets_locked = true;
        let n = app.config.widgets.len();
        run(&ctx, &mut app, vec![]);
        assert_eq!(app.config.widgets.len(), n);
    }

    #[test]
    fn unknown_widget_types_are_not_instantiated() {
        let (_ctx, mut app) = app();
        app.add_widget("no.such.widget");
        let uid = app.config.widgets.last().map(|w| w.uid.clone()).expect("added to config");
        assert!(!app.widgets.contains_key(&uid));
    }
}
