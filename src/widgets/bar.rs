//! One reading as a horizontal bar: the value on top, the bar filled across
//! the metric's scale, and the scale's ends underneath. The `Bar` visual.

use crate::metrics::{MetricId, Scale};
use crate::num::{count_f32, to_f32};
use crate::theme::with_alpha;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};

const DEFAULT_HEIGHT: f32 = 110.0;
const MIN_HEIGHT: f32 = 90.0;

pub struct BarWidget {
    metric: MetricId,
}

impl BarWidget {
    #[must_use]
    pub fn new(metric: MetricId) -> Self {
        Self { metric }
    }
}

impl Widget for BarWidget {
    fn title(&self) -> &str {
        self.metric.def().label
    }

    fn requires(&self) -> &[MetricId] {
        std::slice::from_ref(&self.metric)
    }

    /// A bar shows the current value, so it needs one now.
    fn has_data(&self, ctx: &WidgetCtx) -> bool {
        ctx.value(self.metric).is_some()
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Small, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        let Some(value) = ctx.value(self.metric) else { return };
        let d = self.metric.def();
        let color = theme.metric_color(d.kind);
        let range = d.scale.range(ctx.history(self.metric).iter().copied().chain([value]));

        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter_at(rect);

        let value_size = (height * 0.30).clamp(14.0, 30.0);
        painter.text(rect.min, Align2::LEFT_TOP, d.unit.format(value), theme.font(value_size), theme.text_primary);

        let bar_h = (height * 0.18).clamp(8.0, 18.0);
        let bar_top = (rect.min.y + theme.fs(value_size) + 8.0).min(rect.max.y - bar_h - theme.fs(12.0));
        let bar = Rect::from_min_size(Pos2::new(rect.min.x, bar_top), Vec2::new(rect.width(), bar_h));
        let rounding = bar_h / 3.0;
        painter.rect_filled(bar, rounding, theme.gauge_bg);
        let fraction = to_f32(Scale::fraction(value, range));
        let fill_w = bar.width() * fraction;
        if fill_w > 0.5 {
            let fill = Rect::from_min_size(bar.min, Vec2::new(fill_w, bar_h));
            if d.high_is_bad() {
                // Shifts with the reading and fades to the tip, like the gauge.
                painter.rect_filled(fill, rounding, theme.severity_along(color, fraction, 0.0));
                let slices = 24;
                let step = bar.width() / count_f32(slices);
                for k in (0..slices).take_while(|&k| count_f32(k) * step < fill_w) {
                    let x0 = bar.min.x + count_f32(k) * step;
                    let piece = Rect::from_min_max(
                        Pos2::new(x0, bar.min.y),
                        Pos2::new((x0 + step + 0.5).min(fill.max.x), bar.max.y),
                    );
                    let s = ((count_f32(k) + 0.5) * step / fill_w).min(1.0);
                    painter.rect_filled(piece, 0.0, theme.severity_along(color, fraction, s));
                }
            } else {
                painter.rect_filled(fill, rounding, color);
            }
        }
        painter.rect_stroke(bar, rounding, egui::Stroke::new(0.5_f32, with_alpha(color, 70)));

        let font = theme.font(9.0);
        let below = bar.max.y + 3.0;
        painter.text(
            Pos2::new(bar.min.x, below),
            Align2::LEFT_TOP,
            d.unit.format(range.0),
            font.clone(),
            theme.text_muted,
        );
        painter.text(Pos2::new(bar.max.x, below), Align2::RIGHT_TOP, d.unit.format(range.1), font, theme.text_muted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::GPU_POWER;

    #[test]
    fn needs_a_current_value_and_is_titled_by_its_metric() {
        let bar = BarWidget::new(GPU_POWER);
        assert_eq!(bar.title(), GPU_POWER.def().label);
        assert_eq!(bar.requires(), [GPU_POWER]);
    }
}
