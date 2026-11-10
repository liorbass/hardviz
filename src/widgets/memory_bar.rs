//! Memory bar widget — RAM and VRAM as used/total capacity bars.
//!
//! Demonstrates a widget that requires several metrics and degrades gracefully:
//! if VRAM is unavailable (no NVIDIA GPU) it still renders the RAM bar.

use crate::metrics::{metric, MetricId, MetricKind, Unit};
use crate::num::to_f32;
use crate::theme::with_alpha;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};

/// Default card height; the user can resize it.
const DEFAULT_HEIGHT: f32 = 100.0;
const MIN_HEIGHT: f32 = 90.0;
const BAR_HEIGHT: f32 = 12.0;
const ROW_SPACING: f32 = 28.0;

pub struct MemoryBarWidget;

impl MemoryBarWidget {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemoryBarWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for MemoryBarWidget {
    fn title(&self) -> &'static str {
        "Memory"
    }

    fn requires(&self) -> &[MetricId] {
        &[
            metric::RAM_USAGE,
            metric::RAM_USED,
            metric::RAM_TOTAL,
            metric::VRAM_USAGE,
            metric::VRAM_USED,
            metric::VRAM_TOTAL,
        ]
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Wide, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        // Measure inside the card so its margins are already subtracted.
        let width = ui.available_width().max(140.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter_at(rect);
        // Centre the two rows when the card is taller than they need.
        let y0 = rect.min.y + ((height - ROW_SPACING - BAR_HEIGHT) / 2.0).max(0.0);
        let row_rect = |y: f32| Rect::from_min_size(Pos2::new(rect.min.x, y), Vec2::new(rect.width(), BAR_HEIGHT));

        let ram = Row {
            label: "RAM",
            color: theme.metric_color(MetricKind::Ram),
            usage: ctx.value(metric::RAM_USAGE),
            amount: amount(ctx.raw(metric::RAM_USED), ctx.raw(metric::RAM_TOTAL)).unwrap_or_else(|| "N/A".into()),
        };
        // No NVIDIA GPU is common enough to say so rather than just "N/A".
        let vram_missing = match ctx.diagnosis(metric::VRAM_USAGE) {
            Some(d) if !d.is_available() => "unavailable",
            _ => "N/A",
        };
        let vram = Row {
            label: "VRAM",
            color: theme.metric_color(MetricKind::Vram),
            usage: ctx.value(metric::VRAM_USAGE),
            amount: amount(ctx.raw(metric::VRAM_USED), ctx.raw(metric::VRAM_TOTAL))
                .unwrap_or_else(|| vram_missing.into()),
        };

        ram.paint(&painter, row_rect(y0), theme);
        vram.paint(&painter, row_rect(y0 + ROW_SPACING), theme);
    }
}

/// `used / total`, or `None` when either is unknown.
fn amount(used: Option<f64>, total: Option<f64>) -> Option<String> {
    Some(format!("{} / {}", Unit::Megabytes.format(used?), Unit::Megabytes.format(total?)))
}

/// Horizontal layout shared by both rows: label | bar | amount.
struct Columns {
    label_x: f32,
    bar_x: f32,
    bar_w: f32,
}

impl Columns {
    const LABEL_W: f32 = 46.0;
    const VALUE_W: f32 = 96.0;

    fn new(rect: Rect) -> Self {
        Self {
            label_x: rect.min.x,
            bar_x: rect.min.x + Self::LABEL_W,
            bar_w: (rect.width() - Self::LABEL_W - Self::VALUE_W).max(20.0),
        }
    }
}

/// One capacity bar.
struct Row {
    label: &'static str,
    color: Color32,
    /// Percentage used; `None` draws an empty bar.
    usage: Option<f64>,
    amount: String,
}

impl Row {
    /// Paint into `rect`, one bar high and the full card wide.
    fn paint(&self, painter: &egui::Painter, rect: Rect, theme: &crate::theme::Theme) {
        let cols = Columns::new(rect);
        let y = rect.min.y;
        let mid_y = y + BAR_HEIGHT / 2.0;
        painter.text(Pos2::new(cols.label_x, mid_y), Align2::LEFT_CENTER, self.label, theme.font(11.0), self.color);

        let bar = Rect::from_min_size(Pos2::new(cols.bar_x, y), Vec2::new(cols.bar_w, BAR_HEIGHT));
        painter.add(egui::Shape::rect_filled(bar, 3.0, theme.gauge_bg));
        if let Some(u) = self.usage {
            let fill_w = cols.bar_w * to_f32((u / 100.0).clamp(0.0, 1.0));
            if fill_w > 0.5 {
                let fill = Rect::from_min_size(bar.min, Vec2::new(fill_w, BAR_HEIGHT));
                painter.add(egui::Shape::rect_filled(fill, 3.0, self.color));
            }
        }
        painter.add(egui::Shape::rect_stroke(bar, 3.0, egui::Stroke::new(0.5_f32, with_alpha(self.color, 70))));

        let text_color = if self.usage.is_some() { theme.text_secondary } else { theme.text_muted };
        painter.text(
            Pos2::new(cols.bar_x + cols.bar_w + 8.0, mid_y),
            Align2::LEFT_CENTER,
            &self.amount,
            theme.font(10.0),
            text_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_needs_both_values() {
        assert_eq!(amount(Some(2048.0), Some(16384.0)).as_deref(), Some("2.0 GB / 16.0 GB"));
        assert_eq!(amount(None, Some(16384.0)), None);
        assert_eq!(amount(Some(1.0), None), None);
    }

    #[test]
    fn columns_leave_room_for_label_and_amount() {
        let c = Columns::new(Rect::from_min_size(Pos2::new(10.0, 0.0), Vec2::new(400.0, 12.0)));
        assert_eq!(c.label_x, 10.0);
        assert_eq!(c.bar_x, 10.0 + Columns::LABEL_W);
        assert_eq!(c.bar_w, 400.0 - Columns::LABEL_W - Columns::VALUE_W);
    }
}
