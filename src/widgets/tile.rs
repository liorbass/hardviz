//! One reading as a big value over a one-minute sparkline, scaled to its own
//! recent range so small swings stay visible. The `Tile` visual.

use crate::metrics::{MetricId, HISTORY_LEN};
use crate::num::to_f32;
use crate::theme::with_alpha;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use crate::widgets::chart::time_segments;
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};

/// Default card height; the user can resize it.
const DEFAULT_HEIGHT: f32 = 130.0;
const MIN_HEIGHT: f32 = 100.0;
/// Time the sparkline covers.
const SPARK_SPAN_MS: u64 = 60_000;

/// Samples covering [`SPARK_SPAN_MS`] at the current sample rate.
fn spark_samples(interval_ms: u64) -> usize {
    usize::try_from(SPARK_SPAN_MS / interval_ms.max(1)).unwrap_or(HISTORY_LEN).clamp(2, HISTORY_LEN)
}

pub struct TileWidget {
    metric: MetricId,
}

impl TileWidget {
    #[must_use]
    pub fn new(metric: MetricId) -> Self {
        Self { metric }
    }
}

impl Widget for TileWidget {
    fn title(&self) -> &str {
        self.metric.def().label
    }

    fn requires(&self) -> &[MetricId] {
        std::slice::from_ref(&self.metric)
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Small, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter_at(rect);
        let d = self.metric.def();
        let color = theme.metric_color(d.kind);

        let (text, text_color) = match ctx.value(self.metric) {
            Some(v) => (d.unit.format(v), theme.text_primary),
            None => ("N/A".to_string(), theme.text_muted),
        };
        let value_size = (height * 0.32).clamp(15.0, 34.0);
        painter.text(rect.min, Align2::LEFT_TOP, text, theme.font(value_size), text_color);

        let spark = Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + theme.fs(value_size) + 8.0), rect.max);
        let samples = spark_samples(ctx.sample_interval_ms);
        let hist = ctx.history(self.metric);
        let window = &hist[hist.len().saturating_sub(samples)..];
        let finite = || window.iter().copied().filter(|v| !v.is_nan());
        let (lo, hi) = (finite().fold(f64::INFINITY, f64::min), finite().fold(f64::NEG_INFINITY, f64::max));
        if !lo.is_finite() || spark.height() < 6.0 {
            return;
        }
        let span = (hi - lo).max(hi.abs() * 0.05).max(1e-6);
        let y_at = |v: f64| spark.max.y - 2.0 - (spark.height() - 4.0) * to_f32((v - lo) / span);

        painter.add(egui::Shape::line_segment(
            [Pos2::new(spark.min.x, spark.max.y), Pos2::new(spark.max.x, spark.max.y)],
            egui::Stroke::new(1.0_f32, with_alpha(theme.border, 90)),
        ));
        for seg in time_segments(window, samples, spark, y_at) {
            painter.add(egui::Shape::line(seg, egui::Stroke::new(1.5_f32, color)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::FAN_SPEED;

    #[test]
    fn sparkline_covers_one_minute_at_any_rate() {
        assert_eq!(spark_samples(1000), 60);
        assert_eq!(spark_samples(500), 120);
        assert_eq!(spark_samples(250), 240);
        assert_eq!(spark_samples(50), HISTORY_LEN);
        assert_eq!(spark_samples(5000), 12);
        assert_eq!(spark_samples(60_000), 2);
    }

    #[test]
    fn titled_by_its_metric() {
        assert_eq!(TileWidget::new(FAN_SPEED).title(), FAN_SPEED.def().label);
    }
}
