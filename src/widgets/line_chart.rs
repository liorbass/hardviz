//! A configurable time-series chart: any metrics, one card.
//!
//! Charts are declared, not implemented. Everything specific to a chart is
//! either data on the metrics it shows (label, colour kind, unit, y-axis
//! scale) or one of the optional builder settings below, so a new chart is a
//! single registration in `widgets::default_registry`:
//!
//! ```ignore
//! LineChartWidget::new("Temperature", &[metric::CPU_TEMP, metric::GPU_TEMP]).threshold(80.0)
//! ```
//!
//! Series whose metric has no data are left out and named in a note at the
//! top of the chart, with each one's diagnosis on hover - a missing reading
//! never plots as a flat zero.

use crate::metrics::MetricId;
use crate::theme::Theme;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use crate::widgets::chart::{draw_chart, ChartSpec, Series};
use egui::{Align2, Color32, Pos2, Ui};

const DEFAULT_HEIGHT: f32 = 240.0;
const MIN_HEIGHT: f32 = 150.0;

pub struct LineChartWidget {
    title: &'static str,
    metrics: Vec<MetricId>,
    threshold: Option<f64>,
    colors: Option<fn(&Theme) -> Vec<Color32>>,
}

impl LineChartWidget {
    /// A chart of `metrics`, sharing the first metric's y-axis scale and unit.
    #[must_use]
    pub fn new(title: &'static str, metrics: &[MetricId]) -> Self {
        Self { title, metrics: metrics.to_vec(), threshold: None, colors: None }
    }

    /// Draw a dashed danger line at `value`; axis labels above it turn red.
    #[must_use]
    pub fn threshold(mut self, value: f64) -> Self {
        self.threshold = Some(value);
        self
    }

    /// Series colours, in metric order. By default each series takes its
    /// metric kind's theme colour; set this when several metrics share a kind
    /// (network down/up, disk read/write).
    #[must_use]
    pub fn colors(mut self, colors: fn(&Theme) -> Vec<Color32>) -> Self {
        self.colors = Some(colors);
        self
    }

    fn series_colors(&self, theme: &Theme) -> Vec<Color32> {
        if let Some(f) = self.colors {
            return f(theme);
        }
        self.metrics.iter().map(|id| theme.metric_color(id.def().kind)).collect()
    }
}

impl Widget for LineChartWidget {
    fn title(&self) -> &str {
        self.title
    }

    fn requires(&self) -> &[MetricId] {
        &self.metrics
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Large, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn is_chart(&self) -> bool {
        true
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        let Some(first) = self.metrics.first().map(|id| id.def()) else { return };

        let colors = self.series_colors(theme);

        // Series of different lengths need no padding: the chart anchors every
        // series' newest sample on the right edge.
        let mut series = Vec::new();
        let mut missing = Vec::new();
        for (k, &id) in self.metrics.iter().enumerate() {
            let label = id.def().label;
            if ctx.has_history(id) {
                let color = colors.get(k).copied().unwrap_or(theme.accent);
                series.push(Series { label, color, points: ctx.long_history(id), end: ctx.appended(id) });
            } else {
                missing.push((label, id));
            }
        }

        let (y_min, y_max) = first.scale.range(series.iter().flat_map(|s| s.points.iter().copied()));
        let (y_unit, format) = first.unit.axis();
        let window_label = crate::widgets::span_label(ctx.chart_span_secs * 1000);
        let spec = ChartSpec {
            series: &series,
            y_min,
            y_max,
            y_unit,
            height,
            danger_above: self.threshold,
            danger_color: theme.danger,
            window: ctx.chart_window(),
            window_label: &window_label,
            sample_ms: ctx.sample_interval_ms,
            format,
        };
        let rect = draw_chart(ui, theme, &spec);

        // Name what is missing instead of silently dropping it.
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|(l, _)| *l).collect();
            let note = format!("{} unavailable \u{2014} hover for details", names.join(", "));
            ui.painter_at(rect).text(
                Pos2::new(rect.center().x, rect.min.y + 12.0),
                Align2::CENTER_CENTER,
                note,
                theme.font(10.0),
                theme.text_muted,
            );
            let details: Vec<String> = missing
                .iter()
                .map(|(label, id)| {
                    let why = ctx
                        .diagnosis(*id)
                        .map_or_else(|| "no source".into(), super::super::source::MetricDiagnosis::explain);
                    format!("{label}:\n{why}")
                })
                .collect();
            let strip = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 26.0));
            ui.allocate_rect(strip, egui::Sense::hover()).on_hover_text(details.join("\n\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::{CPU_TEMP, GPU_TEMP, NET_DOWN, NET_UP};

    #[test]
    fn series_take_their_kind_colour_unless_overridden() {
        let theme = crate::theme::ThemeName::MidnightDark.theme();
        let chart = LineChartWidget::new("T", &[CPU_TEMP, GPU_TEMP]);
        assert_eq!(
            chart.series_colors(&theme),
            vec![theme.metric_color(CPU_TEMP.def().kind), theme.metric_color(GPU_TEMP.def().kind),]
        );
        let net = LineChartWidget::new("N", &[NET_DOWN, NET_UP]).colors(|t| vec![t.accent, t.success]);
        assert_eq!(net.series_colors(&theme), vec![theme.accent, theme.success]);
    }
}
