//! Shared chart primitives used by the line-chart widgets.
//!
//! Kept separate from the widgets so a new chart widget is mostly declarative:
//! pick metrics, ranges and colours, then call [`draw_chart`].

use crate::num::{count_f32, floor_usize, to_f32};
use crate::theme::with_alpha;
use crate::theme::Theme;
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};

/// One series to plot.
#[non_exhaustive]
pub struct Series<'a> {
    pub label: &'a str,
    pub color: Color32,
    /// Sampled points in plotting order. `NAN` marks a gap.
    pub points: &'a [f64],
    /// Samples ever recorded for this series, the last of which is the last
    /// of `points`. Anchors the buckets a long window is averaged into, so
    /// they hold still as new samples arrive.
    pub end: u64,
}

#[non_exhaustive]
pub struct ChartSpec<'a> {
    pub series: &'a [Series<'a>],
    pub y_min: f64,
    pub y_max: f64,
    pub y_unit: &'a str,
    pub height: f32,
    /// Shade everything above this value (e.g. a thermal danger zone).
    pub danger_above: Option<f64>,
    pub danger_color: Color32,
    /// Samples spanning the x axis. The newest sample sits on the right edge
    /// and history fills in leftwards, so the axis is a fixed time window
    /// rather than stretching a few seconds of data across the full width.
    pub window: usize,
    /// Human-readable span of `window`, shown at the right of the legend row.
    pub window_label: &'a str,
    /// Formats axis and legend values; `None` prints `{value}{y_unit}`.
    pub format: Option<fn(f64) -> String>,
}

impl ChartSpec<'_> {
    fn fmt(&self, v: f64) -> String {
        match self.format {
            Some(f) => f(v),
            None => format!("{:.0}{}", v, self.y_unit),
        }
    }
}

/// The last `window` samples of `points` as polylines in `plot`, broken at
/// `NaN` gaps so a missing reading never draws a line down to zero.
///
/// The newest sample sits on the right edge and each older one a fixed step
/// to the left (`window` samples span the width), so the x axis is a fixed
/// time window that fills in as data arrives, and series of different lengths
/// stay aligned on "now". Runs of a single sample are dropped (nothing to draw).
pub fn time_segments(points: &[f64], window: usize, plot: Rect, y_at: impl Fn(f64) -> f32) -> Vec<Vec<Pos2>> {
    let window = window.max(2);
    let visible = &points[points.len().saturating_sub(window)..];
    let step = plot.width() / count_f32(window - 1);
    let last = visible.len().saturating_sub(1);
    let mut segments = Vec::new();
    let mut current: Vec<Pos2> = Vec::new();
    for (i, &v) in visible.iter().enumerate() {
        if v.is_nan() {
            if current.len() >= 2 {
                segments.push(std::mem::take(&mut current));
            }
            current.clear();
        } else {
            current.push(Pos2::new(plot.max.x - count_f32(last - i) * step, y_at(v)));
        }
    }
    if current.len() >= 2 {
        segments.push(current);
    }
    segments
}

/// `points` averaged in buckets of `k` consecutive samples; a bucket without
/// a reading is `NAN`. Buckets are fixed to absolute sample positions (`end`
/// is the position just past the last point), so an older bucket keeps its
/// value as new samples arrive and the line does not shimmer; only the newest,
/// still filling bucket changes.
#[must_use]
pub fn bucket_means(points: &[f64], end: u64, k: usize) -> Vec<f64> {
    let k = k.max(1) as u64;
    let start = end.saturating_sub(points.len() as u64);
    let mut out = Vec::with_capacity(points.len().div_ceil(usize::try_from(k).unwrap_or(1)) + 1);
    let (mut bucket, mut sum, mut n) = (start / k, 0.0, 0_u32);
    for (pos, &v) in (start..).zip(points) {
        if pos / k != bucket {
            out.push(if n == 0 { f64::NAN } else { sum / f64::from(n) });
            (bucket, sum, n) = (pos / k, 0.0, 0);
        }
        if !v.is_nan() {
            sum += v;
            n += 1;
        }
    }
    if !points.is_empty() {
        out.push(if n == 0 { f64::NAN } else { sum / f64::from(n) });
    }
    out
}

/// Render a multi-series time chart with gridlines, axis labels and a legend.
///
/// Takes the full available width and `spec.height`. Returns the rect used
/// so callers can attach tooltips.
pub fn draw_chart(ui: &mut Ui, theme: &Theme, spec: &ChartSpec) -> Rect {
    let width = ui.available_width().max(160.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, spec.height), Sense::hover());
    let painter = ui.painter_at(rect);

    let span = (spec.y_max - spec.y_min).max(1e-6);

    // Reserve exactly the width the widest y label needs (`100°` vs `12.5 MB/s`).
    let label_w = (0..=4)
        .map(|i| {
            let text = spec.fmt(spec.y_max - span * f64::from(i) / 4.0);
            ui.fonts(|f| f.layout_no_wrap(text, theme.font(9.0), Color32::WHITE).size().x)
        })
        .fold(0.0_f32, f32::max);
    let left = (label_w + 9.0).max(34.0);

    let plot = Rect::from_min_size(
        Pos2::new(rect.min.x + left, rect.min.y + 4.0),
        Vec2::new((rect.width() - left - 6.0).max(20.0), (rect.height() - 26.0).max(20.0)),
    );

    // Horizontal gridlines + y labels
    for i in 0..=4 {
        let frac = f64::from(i) / 4.0;
        let y = plot.min.y + plot.height() * to_f32(frac);
        painter.add(egui::Shape::line_segment(
            [Pos2::new(plot.min.x, y), Pos2::new(plot.max.x, y)],
            egui::Stroke::new(0.5_f32, with_alpha(theme.border, 50)),
        ));
        let value = spec.y_max - span * frac;
        // Axis values inside the danger zone take the danger colour.
        let in_danger = spec.danger_above.is_some_and(|t| value >= t);
        painter.text(
            Pos2::new(plot.min.x - 5.0, y),
            Align2::RIGHT_CENTER,
            spec.fmt(value),
            theme.font(9.0),
            if in_danger { spec.danger_color } else { theme.text_muted },
        );
    }

    // Danger threshold: a dashed line with its value, plus red axis labels
    // above it. No filled band: a translucent red over the darker or teal card
    // backgrounds blends to grey and read as a rendering artefact.
    if let Some(threshold) = spec.danger_above {
        if threshold < spec.y_max {
            let frac = to_f32(((spec.y_max - threshold) / span).clamp(0.0, 1.0));
            let line_y = plot.min.y + plot.height() * frac;
            painter.extend(egui::Shape::dashed_line(
                &[Pos2::new(plot.min.x, line_y), Pos2::new(plot.max.x, line_y)],
                egui::Stroke::new(1.0_f32, with_alpha(spec.danger_color, 200)),
                5.0,
                4.0,
            ));
            painter.text(
                Pos2::new(plot.max.x - 4.0, line_y - 2.0),
                Align2::RIGHT_BOTTOM,
                format!("{:.0}{}", threshold, spec.y_unit),
                theme.font(9.0),
                spec.danger_color,
            );
        }
    }

    let y_at = |v: f64| plot.max.y - plot.height() * to_f32(((v - spec.y_min) / span).clamp(0.0, 1.0));
    // A long window has more samples than the plot has pixels: average them
    // to about one point per pixel, which reads the same and draws far less.
    let per_point = spec.window.div_ceil(floor_usize(plot.width()).max(2));
    for s in spec.series {
        let (points, window) = if per_point > 1 {
            let visible = &s.points[s.points.len().saturating_sub(spec.window)..];
            (std::borrow::Cow::Owned(bucket_means(visible, s.end, per_point)), spec.window.div_ceil(per_point))
        } else {
            (std::borrow::Cow::Borrowed(s.points), spec.window)
        };
        for segment in time_segments(&points, window, plot, y_at) {
            painter.add(egui::Shape::line(segment, egui::Stroke::new(2.0_f32, s.color)));
        }
        // Leading dot on the newest sample
        if let Some(&last) = s.points.iter().rev().find(|v| !v.is_nan()) {
            painter.add(egui::Shape::circle_filled(Pos2::new(plot.max.x, y_at(last)), 3.0, s.color));
        }
    }

    // Legend
    let legend_y = rect.max.y - 12.0;
    let mut x = plot.min.x;
    for s in spec.series {
        painter.add(egui::Shape::circle_filled(Pos2::new(x + 3.0, legend_y), 3.0, s.color));
        let latest = s.points.iter().rev().find(|v| !v.is_nan());
        let text = match latest {
            Some(v) => format!("{} {}", s.label, spec.fmt(*v)),
            None => format!("{} N/A", s.label),
        };
        let galley = painter.text(
            Pos2::new(x + 9.0, legend_y),
            Align2::LEFT_CENTER,
            text,
            theme.font(10.0),
            match latest {
                Some(_) => theme.text_secondary,
                None => theme.text_muted,
            },
        );
        x += 9.0 + galley.size().x + 14.0;
    }

    // Time window, right-aligned on the legend row.
    if !spec.window_label.is_empty() {
        painter.text(
            Pos2::new(plot.max.x, legend_y),
            Align2::RIGHT_CENTER,
            format!("last {}", spec.window_label),
            theme.font(9.0),
            theme.text_muted,
        );
    }

    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 10.0))
    }

    #[test]
    fn newest_sample_is_on_the_right_edge_and_gaps_split_lines() {
        let segs = time_segments(&[1.0, 2.0, f64::NAN, 3.0, 4.0], 11, plot(), |v| v as f32);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].last().map(|p| p.x), Some(100.0));
        // 11 samples span 100 px: 10 px per step.
        assert_eq!(segs[1][0].x, 90.0);
        assert_eq!(segs[0][0].x, 60.0);
    }

    #[test]
    fn buckets_are_anchored_to_absolute_positions() {
        // Positions 3..8: buckets [3], [4,5], [6,7] for k = 2.
        assert_eq!(bucket_means(&[1.0, 2.0, 4.0, f64::NAN, 6.0], 8, 2), [1.0, 3.0, 6.0]);
        // One more sample: the older buckets keep their values.
        let next = bucket_means(&[1.0, 2.0, 4.0, f64::NAN, 6.0, 8.0], 9, 2);
        assert_eq!(next, [1.0, 3.0, 6.0, 8.0]);
        let gap = bucket_means(&[f64::NAN, f64::NAN, 5.0], 4, 2);
        assert!(gap[0].is_nan() && gap[1] == 5.0 && gap.len() == 2);
        assert!(bucket_means(&[], 0, 4).is_empty());
        assert_eq!(bucket_means(&[1.0, 2.0], 2, 1), [1.0, 2.0]);
    }

    #[test]
    fn isolated_samples_and_overflow_are_dropped() {
        assert!(time_segments(&[1.0, f64::NAN, 2.0], 10, plot(), |v| v as f32).is_empty());
        let segs = time_segments(&[9.0, 9.0, 1.0, 2.0, 3.0], 3, plot(), |v| v as f32);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].iter().map(|p| p.y).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
    }
}
