//! Per-core CPU usage: one small bar per logical core, so a single busy
//! thread hiding behind a low average is visible at a glance.
//!
//! Reads [`crate::widget::WidgetCtx::cores`], which the poller samples only
//! while this card is on screen (like the process list).

use crate::metrics::metric::CPU_USAGE;
use crate::metrics::{MetricId, MetricKind};
use crate::num::{count_f32, floor_usize, to_f32};
use crate::theme::with_alpha;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};

/// Default card height; the user can resize it.
const DEFAULT_HEIGHT: f32 = 120.0;
const MIN_HEIGHT: f32 = 80.0;
/// Narrowest a core cell may get before the grid drops a column.
const MIN_CELL_W: f32 = 90.0;

pub struct PerCoreWidget;

impl PerCoreWidget {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for PerCoreWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// Columns and rows for `cores` cells across `width`: as many columns as fit
/// at [`MIN_CELL_W`], never more than there are cores.
fn core_grid(width: f32, cores: usize) -> (usize, usize) {
    let cols = floor_usize(width / MIN_CELL_W).clamp(1, cores.max(1));
    (cols, cores.div_ceil(cols))
}

impl Widget for PerCoreWidget {
    fn title(&self) -> &'static str {
        "Per-Core CPU"
    }

    fn requires(&self) -> &[MetricId] {
        // The gate is the aggregate reading: no CPU data at all means the
        // per-core list would be empty too.
        std::slice::from_ref(&CPU_USAGE)
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Wide, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn wants_cores(&self) -> bool {
        true
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter_at(rect);

        if ctx.cores.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Collecting per-core usage\u{2026}",
                theme.font(11.0),
                theme.text_muted,
            );
            return;
        }

        let (cols, rows) = core_grid(rect.width(), ctx.cores.len());
        let cell_w = rect.width() / count_f32(cols);
        let cell_h = rect.height() / count_f32(rows);
        let mild = theme.metric_color(MetricKind::Cpu);
        let label_font = theme.font(9.0);
        let value_font = theme.font(10.0);
        let show_labels = cell_h >= theme.fs(9.0) + 14.0;

        for (i, &usage) in ctx.cores.iter().enumerate() {
            let (col, row) = (i % cols, i / cols);
            let cell = Rect::from_min_size(
                Pos2::new(rect.min.x + count_f32(col) * cell_w, rect.min.y + count_f32(row) * cell_h),
                Vec2::new(cell_w, cell_h),
            )
            .shrink(4.0);
            if cell.width() < 20.0 || cell.height() < 8.0 {
                continue;
            }
            let fill = to_f32(f64::from(usage).clamp(0.0, 100.0) / 100.0);
            let color = theme.severity_color(mild, fill);

            let bar_h = 6.0;
            let bar = if show_labels {
                let label_y = cell.min.y;
                painter.text(
                    Pos2::new(cell.min.x, label_y),
                    Align2::LEFT_TOP,
                    format!("C{i}"),
                    label_font.clone(),
                    theme.text_muted,
                );
                painter.text(
                    Pos2::new(cell.max.x, label_y),
                    Align2::RIGHT_TOP,
                    format!("{usage:.0}%"),
                    value_font.clone(),
                    color,
                );
                Rect::from_min_size(Pos2::new(cell.min.x, cell.max.y - bar_h), Vec2::new(cell.width(), bar_h))
            } else {
                cell
            };
            painter.rect_filled(bar, 3.0, with_alpha(theme.gauge_bg, 200));
            let filled = Rect::from_min_size(bar.min, Vec2::new(bar.width() * fill, bar.height()));
            if filled.width() > 0.5 {
                painter.rect_filled(filled, 3.0, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_cores_into_columns_of_a_readable_width() {
        assert_eq!(core_grid(800.0, 8), (8, 1));
        assert_eq!(core_grid(400.0, 8), (4, 2));
        assert_eq!(core_grid(800.0, 16), (8, 2));
        assert_eq!(core_grid(50.0, 4), (1, 4), "never wider than the card");
        assert_eq!(core_grid(800.0, 1), (1, 1));
        assert_eq!(core_grid(800.0, 0), (1, 0), "no cores, no rows");
    }

    #[test]
    fn the_card_asks_for_core_sampling() {
        let widget = PerCoreWidget::new();
        assert!(widget.wants_cores());
        assert!(!widget.wants_processes());
    }
}
