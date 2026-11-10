//! One reading as a horizontal bar: the value on top, the bar filled across
//! the metric's scale, and the scale's ends underneath. The `Bar` visual.

use crate::metrics::{MetricId, Scale};
use crate::num::to_f32;
use crate::theme::{with_alpha, Theme};
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use crate::widgets::glow::Glow;
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};

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
        // Clipped to the card, not `rect`, so the glow is not cut off.
        let painter = ui.painter().clone();

        let value_size = (height * 0.30).clamp(14.0, 30.0);
        painter.text(rect.min, Align2::LEFT_TOP, d.unit.format(value), theme.font(value_size), theme.text_primary);

        let bar_h = (height * 0.18).clamp(8.0, 18.0);
        let bar_top = (rect.min.y + theme.fs(value_size) + 8.0).min(rect.max.y - bar_h - theme.fs(12.0));
        let bar = Rect::from_min_size(Pos2::new(rect.min.x, bar_top), Vec2::new(rect.width(), bar_h));
        let rounding = bar_h / 3.0;
        painter.rect_filled(bar, rounding, theme.gauge_bg);
        let fraction = to_f32(Scale::fraction(value, range));
        paint_fill(&painter, theme, bar, rounding, fraction, color, d.high_is_bad());
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

/// A bar's glow: how far it diffuses, in bar heights, and its opacity at the
/// bar's edge.
const GLOW_REACH: f32 = 0.45;
const GLOW_PEAK: f32 = 0.2;

/// Fill `bar` (its track already drawn) from the left to `fraction`, the way
/// a gauge fills its arc: with `severity`, the whole fill shifts towards
/// warning colours as it fills and fades along its length to the tip
/// ([`Theme::severity_along`]); a soft glow in the tip's colour sits behind
/// it. The value is the store's eased one, so length, colour and glow all
/// glide between readings.
pub(crate) fn paint_fill(
    painter: &egui::Painter,
    theme: &Theme,
    bar: Rect,
    rounding: f32,
    fraction: f32,
    color: Color32,
    severity: bool,
) {
    let fraction = fraction.clamp(0.0, 1.0);
    let fill = Rect::from_min_size(bar.min, Vec2::new(bar.width() * fraction, bar.height()));
    if fill.width() < 0.5 {
        return;
    }
    let color_at = |s: f32| if severity { theme.severity_along(color, fraction, s) } else { color };
    let rounding = rounding.min(fill.height() / 2.0).min(fill.width() / 2.0);
    let glow =
        Glow { length: fill.width(), half: fill.height() / 2.0, reach: fill.height() * GLOW_REACH, peak: GLOW_PEAK };
    let cy = fill.center().y;
    painter.add(glow.mesh(|along, across| Pos2::new(fill.min.x + along, cy + across), color_at));
    painter.add(gradient_fill(fill, rounding, color_at));
}

/// A rounded rectangle filled with a left-to-right gradient, `color(s)` at
/// `s` (0 left edge, 1 right edge), as one mesh: no seams or banding, unlike
/// slices of solid rectangles, and its rounded ends match the track's.
///
/// A mesh is not anti-aliased, so its outline gets a one-point feather of its
/// own: the colour fades to transparent across the outer half point of every
/// edge.
fn gradient_fill(rect: Rect, rounding: f32, color: impl Fn(f32) -> Color32) -> egui::Mesh {
    let (w, h, r) = (rect.width(), rect.height() / 2.0, rounding.max(0.0));
    // Columns every point through the rounded ends, sparser in between.
    let mut xs: Vec<f32> = Vec::new();
    let mut d = 0.0;
    while d < w / 2.0 {
        xs.push(d);
        xs.push(w - d);
        d += if d < r + 1.0 { 1.0 } else { 6.0 };
    }
    xs.push(w / 2.0);
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.25);

    let cy = rect.center().y;
    let mut mesh = egui::Mesh::default();
    for (i, &x) in xs.iter().enumerate() {
        let d = x.min(w - x);
        // Half height here: less inside a rounded end.
        let half = if d >= r { h } else { h - r + (r * r - (r - d) * (r - d)).sqrt() };
        // The end columns are transparent: the fill fades in over its first point.
        let c = color(x / w).gamma_multiply(d.min(1.0));
        let px = rect.min.x + x;
        for (y, col) in [
            (cy - half - 0.5, Color32::TRANSPARENT),
            (cy - half + 0.5, c),
            (cy + half - 0.5, c),
            (cy + half + 0.5, Color32::TRANSPARENT),
        ] {
            mesh.colored_vertex(Pos2::new(px, y), col);
        }
        if i > 0 {
            let (a, b) = (u32::try_from(4 * (i - 1)).unwrap_or(0), u32::try_from(4 * i).unwrap_or(0));
            for row in 0..3 {
                mesh.add_triangle(a + row, a + row + 1, b + row);
                mesh.add_triangle(a + row + 1, b + row + 1, b + row);
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_fill_spans_the_rect_with_soft_rounded_ends() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 0.0), Vec2::new(100.0, 12.0));
        let mesh = gradient_fill(rect, 3.0, |s| if s < 0.5 { Color32::RED } else { Color32::BLUE });
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.pos.x).collect();
        assert_eq!(xs.iter().copied().fold(f32::MAX, f32::min), 10.0);
        assert_eq!(xs.iter().copied().fold(f32::MIN, f32::max), 110.0);
        // The ends are lower than the middle (rounded), and fade in.
        let height_at = |x: f32| {
            let ys: Vec<f32> = mesh.vertices.iter().filter(|v| v.pos.x == x).map(|v| v.pos.y).collect();
            ys.iter().copied().fold(f32::MIN, f32::max) - ys.iter().copied().fold(f32::MAX, f32::min)
        };
        assert!(height_at(10.0) < height_at(60.0));
        assert!(mesh.vertices.iter().filter(|v| v.pos.x == 10.0).all(|v| v.color.a() == 0));
        assert!(mesh.vertices.iter().any(|v| v.color == Color32::RED));
        assert!(mesh.vertices.iter().any(|v| v.color == Color32::BLUE));
        assert!(mesh.is_valid());
    }
    use crate::metrics::metric::GPU_POWER;

    #[test]
    fn needs_a_current_value_and_is_titled_by_its_metric() {
        let bar = BarWidget::new(GPU_POWER);
        assert_eq!(bar.title(), GPU_POWER.def().label);
        assert_eq!(bar.requires(), [GPU_POWER]);
    }
}
