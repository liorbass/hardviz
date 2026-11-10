//! Grafana-style gauge: a radial arc plus a sparkline of the metric's own
//! recent history.
//!
//! One implementation parameterised by metric id: the arc and sparkline cover
//! the metric's scale (0-100 % for usage, the busiest recent value for watts
//! or RPM) and the readout uses its unit. What appears
//! under the percentage is chosen at registration too, with
//! [`GaugeWidget::detail`].

use crate::metrics::{MetricId, Scale, HISTORY_LEN};
use crate::num::{count_f32, to_f32, to_u8};
use crate::theme::with_alpha;
use crate::theme::Theme;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use crate::widgets::chart::time_segments;
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};

/// Default card height; the user can resize it.
const DEFAULT_HEIGHT: f32 = 220.0;
const MIN_HEIGHT: f32 = 150.0;
/// Share of the content height given to the sparkline strip, and its bounds.
/// Tall enough that low readings stay visible above the baseline.
const SPARK_SHARE: f32 = 0.26;
const SPARK_MIN: f32 = 30.0;
const SPARK_MAX: f32 = 110.0;
/// Sparkline window in samples: the store's full retained history.
const SPARK_WINDOW: usize = HISTORY_LEN;

/// Text under the percentage, computed each frame.
pub type Detail = fn(&WidgetCtx) -> Option<String>;

pub struct GaugeWidget {
    metric: MetricId,
    detail: Option<Detail>,
}

impl GaugeWidget {
    #[must_use]
    pub fn new(metric: MetricId) -> Self {
        Self { metric, detail: None }
    }

    /// A secondary reading under the percentage. The card title already names
    /// the metric, so this shows what the percentage cannot.
    #[must_use]
    pub fn detail(mut self, detail: Detail) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Detail helper: a temperature, e.g. `64°C`.
    #[must_use]
    pub fn temperature(ctx: &WidgetCtx, id: MetricId) -> Option<String> {
        ctx.raw(id).map(|t| format!("{t:.0}°C"))
    }

    /// Detail helper: two megabyte metrics as `used / total` in GB.
    #[must_use]
    pub fn used_of(ctx: &WidgetCtx, used: MetricId, total: MetricId) -> Option<String> {
        match (ctx.raw(used), ctx.raw(total)) {
            (Some(u), Some(t)) if t > 0.0 => Some(format!("{:.1} / {:.1} GB", u / 1024.0, t / 1024.0)),
            _ => None,
        }
    }
}

impl Widget for GaugeWidget {
    fn title(&self) -> &str {
        self.metric.def().label
    }

    fn requires(&self) -> &[MetricId] {
        std::slice::from_ref(&self.metric)
    }

    /// A gauge shows the current value, so it needs one now (history alone
    /// is not enough).
    fn has_data(&self, ctx: &WidgetCtx) -> bool {
        ctx.value(self.metric).is_some()
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Small, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        let Some(value) = ctx.value(self.metric) else { return };
        let color = theme.metric_color(self.metric.def().kind);

        let width = ui.available_width().max(80.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter_at(rect);
        let (gauge_rect, spark_rect) = split_gauge(rect);

        let def = self.metric.def();
        let history = ctx.history(self.metric);
        let range = def.scale.range(history.iter().copied().chain([value]));
        let detail = self.detail.and_then(|f| f(ctx)).unwrap_or_default();
        let readout = def.unit.format(value);
        let fraction = to_f32(Scale::fraction(value, range));
        let severity = def.high_is_bad();
        Dial { fraction, readout: &readout, color, severity, detail: &detail }.paint(&painter, gauge_rect, theme);

        let window_label = crate::widgets::window_label(SPARK_WINDOW, ctx.sample_interval_ms);
        let labels = (def.unit.format(range.1), def.unit.format(range.0));
        Sparkline { history, color, window_label: &window_label, range, labels: (&labels.0, &labels.1) }
            .paint(&painter, spark_rect, theme);
    }
}

// ---------------------------------------------------------------------------
// Arc
// ---------------------------------------------------------------------------

/// Divide the card between arc and sparkline according to its shape.
///
/// Tall or square cards stack them (arc above, strip below). Wide, short
/// cards - a gauge resized wide, or rows shrunk by Fit to window - put the arc
/// on the left at full height and the strip beside it; stacking there would
/// cap the arc at a fraction of the height and leave the card mostly empty.
fn split_gauge(rect: Rect) -> (Rect, Rect) {
    let (w, h) = (rect.width(), rect.height());
    if w >= h * 2.2 {
        let arc_w = (h * 1.25).min(w * 0.5);
        let gauge = Rect::from_min_size(rect.min, Vec2::new(arc_w, h));
        let spark = Rect::from_min_max(
            Pos2::new(rect.min.x + arc_w + 12.0, rect.min.y + h * 0.18),
            Pos2::new(rect.max.x, rect.max.y - h * 0.08),
        );
        (gauge, spark)
    } else {
        let spark_h = (h * SPARK_SHARE).clamp(SPARK_MIN, SPARK_MAX);
        let gauge_h = (h - spark_h - 2.0).max(40.0);
        let gauge = Rect::from_min_size(rect.min, Vec2::new(w, gauge_h));
        let spark = Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + gauge_h + 2.0), rect.max);
        (gauge, spark)
    }
}

/// The arc, ticks and readout.
struct Dial<'a> {
    /// How full the arc is, 0-1.
    fraction: f32,
    /// The value, formatted in its unit.
    readout: &'a str,
    /// The metric's colour; the colour of the low part of the arc.
    color: Color32,
    /// Fade the arc towards warning colours as it fills ([`Theme::severity_color`]).
    severity: bool,
    detail: &'a str,
}

impl Dial<'_> {
    fn paint(&self, painter: &egui::Painter, gauge_rect: Rect, theme: &Theme) {
        // Size the arc from the space it occupies. The 270° sweep leaves the
        // bottom open: it reaches `r` above the centre but only `r·sin45°` below
        // it, so the vertical extent is about 1.71·r plus the stroke. Centring
        // that extent (not a circle) in the rect lets the arc fill the card.
        const BELOW: f32 = std::f32::consts::FRAC_1_SQRT_2;
        let (fraction, color, detail) = (self.fraction, self.color, self.detail);
        let stroke_frac = 0.19; // line width as a share of the radius
        let radius = (gauge_rect.height() / (1.0 + BELOW + stroke_frac * 1.6)).min(gauge_rect.width() * 0.42).max(12.0);
        let line_width = (radius * stroke_frac).max(4.0);
        let extent = radius * (1.0 + BELOW) + line_width;
        let center = Pos2::new(gauge_rect.center().x, gauge_rect.center().y - extent / 2.0 + radius + line_width / 2.0);

        let start_angle = 135.0_f32.to_radians();
        let end_angle = 405.0_f32.to_radians();
        let sweep = end_angle - start_angle;

        // Track
        painter.add(egui::Shape::line(
            arc_points(center, radius, start_angle, end_angle, 64),
            egui::Stroke::new(line_width, theme.gauge_bg),
        ));

        // Value arc with a soft glow behind it, both in the colour of the
        // current reading.
        // `s` is the position along the filled arc, 0 (start) to 1 (tip).
        let color_at = |s: f32| if self.severity { theme.severity_along(color, fraction, s) } else { color };
        let now = color_at(1.0);
        let value_angle = start_angle + sweep * fraction.clamp(0.0, 1.0);
        let value_points = arc_points(center, radius, start_angle, value_angle, 64);
        for w in (1..3).rev() {
            let alpha = to_u8(60.0_f32 / (count_f32(w) * 2.0));
            painter.add(egui::Shape::line(
                value_points.clone(),
                egui::Stroke::new(line_width * (1.0_f32 + count_f32(w) * 0.5_f32), with_alpha(now, alpha)),
            ));
        }
        // The whole arc takes on the reading's severity, fading along its
        // length to the tip: one short piece per arc step, each overlapping
        // the next by a point so the joins stay seamless.
        let last = value_points.len().saturating_sub(1);
        for start in 0..last {
            let end = (start + 2).min(last);
            let t = count_f32(start + end) / 2.0 / count_f32(last.max(1));
            painter
                .add(egui::Shape::line(value_points[start..=end].to_vec(), egui::Stroke::new(line_width, color_at(t))));
        }

        // Ticks
        for i in 0..=10 {
            let angle = start_angle + sweep * (count_f32(i) / 10.0);
            let major = i % 5 == 0;
            let inner_r = radius - line_width * 0.5 - if major { 7.0 } else { 3.5 };
            let outer_r = radius - line_width * 0.5 - 1.0;
            let tick_color = if major { theme.text_secondary } else { theme.text_muted };
            painter.add(egui::Shape::line_segment(
                [angle_to_pos(center, inner_r, angle), angle_to_pos(center, outer_r, angle)],
                egui::Stroke::new(if major { 1.4_f32 } else { 0.7_f32 }, tick_color),
            ));
        }

        // Readout
        painter.text(
            center + Vec2::new(0.0, -radius * 0.06),
            Align2::CENTER_CENTER,
            self.readout,
            // Longer readouts (`1183 RPM`) shrink to stay inside the arc.
            theme.font(
                (radius * 0.52).min(radius * 2.2 / count_f32(self.readout.chars().count().max(4))).clamp(11.0, 30.0),
            ),
            theme.text_primary,
        );
        if !detail.is_empty() {
            painter.text(
                center + Vec2::new(0.0, radius * 0.38),
                Align2::CENTER_CENTER,
                detail,
                theme.font((radius * 0.20).clamp(8.0, 12.0)),
                theme.text_secondary,
            );
        }

        // Leading dot
        let dot_pos = angle_to_pos(center, radius, value_angle);
        let dot_r = (line_width * 0.30).max(1.5);
        painter.add(egui::Shape::circle_filled(dot_pos, dot_r + 2.5, with_alpha(now, 45)));
        painter.add(egui::Shape::circle_filled(dot_pos, dot_r, now));
    }
}

// ---------------------------------------------------------------------------
// Sparkline
// ---------------------------------------------------------------------------

/// The metric's own recent history as a compact strip.
///
/// The newest sample sits on the right edge and older samples extend leftwards
/// across at most `SPARK_WINDOW` samples, so the strip reads as a fixed time
/// window that fills in as data accumulates rather than stretching to fit.
struct Sparkline<'a> {
    history: &'a [f64],
    color: Color32,
    window_label: &'a str,
    /// The y range, and its top and bottom labels.
    range: (f64, f64),
    labels: (&'a str, &'a str),
}

impl Sparkline<'_> {
    fn paint(&self, painter: &egui::Painter, rect: Rect, theme: &Theme) {
        // Baseline
        painter.add(egui::Shape::line_segment(
            [Pos2::new(rect.min.x, rect.max.y), Pos2::new(rect.max.x, rect.max.y)],
            egui::Stroke::new(1.0_f32, with_alpha(theme.border, 90)),
        ));

        let plot = Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + 2.0), Pos2::new(rect.max.x, rect.max.y - 3.0));
        if plot.width() < 4.0 || plot.height() < 4.0 {
            return;
        }
        let y_at = |v: f64| plot.max.y - plot.height() * to_f32(Scale::fraction(v, self.range));
        let segments = time_segments(self.history, SPARK_WINDOW, plot, y_at);
        let Some(newest) = segments.last() else { return };

        // Faint midline
        let mid_y = y_at(f64::midpoint(self.range.0, self.range.1));
        painter.add(egui::Shape::line_segment(
            [Pos2::new(plot.min.x, mid_y), Pos2::new(plot.max.x, mid_y)],
            egui::Stroke::new(0.5_f32, with_alpha(theme.text_muted, 40)),
        ));

        // Scale legend: y range on the left, time window on the right. Painted
        // before the line so the data draws on top. The strip is too short for
        // a third label: the faint midline marks the middle unlabelled.
        let font = theme.font(8.5);
        let label_color = theme.text_muted;
        let top_y = plot.min.y;
        painter.text(Pos2::new(plot.min.x + 1.0, top_y), Align2::LEFT_TOP, self.labels.0, font.clone(), label_color);
        painter.text(
            Pos2::new(plot.min.x + 1.0, plot.max.y),
            Align2::LEFT_BOTTOM,
            self.labels.1,
            font.clone(),
            label_color,
        );
        painter.text(Pos2::new(plot.max.x - 6.0, top_y), Align2::RIGHT_TOP, self.window_label, font, label_color);

        // Area fill under the newest segment, one quad per step. A single
        // polygon of the whole line is non-convex whenever the line zigzags, and
        // `convex_polygon` then renders stray diagonal slivers across the fill.
        let fill = with_alpha(self.color, 46);
        for pair in newest.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            painter.add(egui::Shape::convex_polygon(
                vec![a, b, Pos2::new(b.x, plot.max.y), Pos2::new(a.x, plot.max.y)],
                fill,
                egui::Stroke::NONE,
            ));
        }
        // Current-value dot at the right edge.
        let dot = newest.last().copied();
        for seg in segments {
            painter.add(egui::Shape::line(seg, egui::Stroke::new(1.7_f32, self.color)));
        }
        if let Some(dot) = dot.filter(|d| d.x >= plot.max.x - 0.5) {
            painter.add(egui::Shape::circle_filled(dot, 2.0, self.color));
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

fn arc_points(center: Pos2, radius: f32, start: f32, end: f32, segments: usize) -> Vec<Pos2> {
    (0..=segments)
        .map(|i| {
            let t = count_f32(i) / count_f32(segments);
            angle_to_pos(center, radius, start + (end - start) * t)
        })
        .collect()
}

fn angle_to_pos(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    Pos2::new(center.x + radius * angle.cos(), center.y + radius * angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tall_cards_stack_and_wide_short_cards_sit_side_by_side() {
        let tall = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 200.0));
        let (g, s) = split_gauge(tall);
        assert!(s.min.y >= g.max.y, "sparkline below the arc");
        assert!((SPARK_MIN..=SPARK_MAX).contains(&s.height()));

        let wide = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 120.0));
        let (g, s) = split_gauge(wide);
        assert!(s.min.x > g.max.x, "sparkline beside the arc");
        assert_eq!(g.height(), 120.0);
    }

    #[test]
    fn arc_points_span_the_requested_angles() {
        let pts = arc_points(Pos2::ZERO, 10.0, 0.0, std::f32::consts::PI, 4);
        assert_eq!(pts.len(), 5);
        assert!((pts[0].x - 10.0).abs() < 1e-4);
        assert!((pts[4].x + 10.0).abs() < 1e-4);
    }
}
