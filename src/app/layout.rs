//! Dashboard geometry: pure functions, no egui state, all unit tested.

use crate::num::{count_f32, floor_usize};
use eframe::egui;

/// Gap between dashboard cards, both directions.
pub(super) const GRID_GAP: f32 = 10.0;

/// Narrowest column at which the dashboard keeps 4 columns.
pub(super) const MIN_COL_4: f32 = 150.0;
/// Narrowest column at which the dashboard keeps 2 columns.
pub(super) const MIN_COL_2: f32 = 220.0;

/// Fit to window grows cards at most this much, so a huge monitor does not
/// turn two rows of gauges into posters.
pub(super) const MAX_FIT_GROW: f32 = 1.6;

/// Geometry the resize grip needs to convert pointer positions into sizes.
#[derive(Clone, Copy)]
pub(super) struct CellGeo {
    pub(super) cols: usize,
    pub(super) col_w: f32,
    /// Fit-to-window factor applied to stored heights.
    pub(super) scale: f32,
    pub(super) min: f32,
}

/// Card size for a resize-grip drag at pointer `p`: (span in quarters,
/// unscaled height).
///
/// Width snaps to whole grid columns and is stored in quarters, so the card
/// keeps its proportion when the column count changes. Height snaps to 10 px
/// and is stored before the Fit-to-window factor, so fitting keeps applying.
pub(super) fn resize_from_pointer(p: egui::Pos2, rect: egui::Rect, geo: CellGeo) -> (u8, f32) {
    let cols = count_f32(geo.cols).max(1.0);
    let n = ((p.x - rect.min.x + GRID_GAP) / (geo.col_w + GRID_GAP)).round().clamp(1.0, cols);
    let span4 = u8::try_from((floor_usize(n) * 4).div_ceil(geo.cols.max(1)).clamp(1, 4)).unwrap_or(4);
    let shown = (p.y - rect.min.y).max(geo.min * geo.scale);
    let height = ((shown / geo.scale.max(0.1)) / 10.0).round() * 10.0;
    (span4, height.max(geo.min))
}

/// Column count: 4, 2 or 1. Never 3 - spans are stored in quarters, which
/// divide evenly into 4, 2 and 1 columns.
pub(super) fn layout_columns(available: f32) -> usize {
    if cell_width(available, GRID_GAP, 4) >= MIN_COL_4 {
        4
    } else if cell_width(available, GRID_GAP, 2) >= MIN_COL_2 {
        2
    } else {
        1
    }
}

/// Row heights for Fit to window, and the factor applied.
///
/// Scales every row by one factor so the total meets `target`: shrinking is
/// floored at each row's minimum, growing is capped at `MAX_FIT_GROW`. With no
/// target (fit off) the natural heights are used as-is.
pub(super) fn fit_rows(natural: &[f32], minimum: &[f32], target: Option<f32>) -> (Vec<f32>, f32) {
    let total: f32 = natural.iter().sum();
    let Some(target) = target.filter(|t| *t > 0.0 && total > 0.0) else {
        return (natural.to_vec(), 1.0);
    };
    let f = (target / total).min(MAX_FIT_GROW);
    if f >= 1.0 {
        return (natural.iter().map(|h| h * f).collect(), f);
    }
    // Shrinking: rows pinned at their minimum free less space than a uniform
    // scale would, so re-solve the factor for the remaining rows a few times.
    let mut f = f;
    for _ in 0..4 {
        let pinned: f32 = natural.iter().zip(minimum).filter(|(n, m)| *n * f < **m).map(|(_, m)| *m).sum();
        let free: f32 = natural.iter().zip(minimum).filter(|(n, m)| *n * f >= **m).map(|(n, _)| *n).sum();
        if free <= 0.0 {
            break;
        }
        f = ((target - pinned) / free).clamp(0.0, 1.0);
    }
    (natural.iter().zip(minimum).map(|(n, m)| (n * f).max(*m)).collect(), f)
}

/// Cell width for `cols` columns filling `available` with `spacing` gutters.
pub(super) fn cell_width(available: f32, spacing: f32, cols: usize) -> f32 {
    let cols = count_f32(cols.max(1));
    ((available - (cols - 1.0) * spacing) / cols).max(40.0)
}

/// One card's place in a packed row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Placed {
    /// Index into the list that was packed.
    pub(super) index: usize,
    /// First grid column.
    pub(super) col: usize,
    /// Width in grid columns.
    pub(super) span: usize,
}

/// Pack cards into rows of `cols` columns, in order. `spans` are in quarters
/// of the full width and are scaled to the column count.
///
/// First-fit: each card goes into the earliest row with room, so a small card
/// listed after a full-width one back-fills the gap beside an earlier card
/// instead of leaving a hole.
pub(super) fn pack_rows(spans: &[u8], cols: usize) -> Vec<Vec<Placed>> {
    let mut rows: Vec<Vec<Placed>> = Vec::new();
    let mut used: Vec<usize> = Vec::new();
    for (index, &quarters) in spans.iter().enumerate() {
        let span = (usize::from(quarters) * cols).div_ceil(4).clamp(1, cols);
        let r = used.iter().position(|u| u + span <= cols).unwrap_or_else(|| {
            rows.push(Vec::new());
            used.push(0);
            rows.len() - 1
        });
        rows[r].push(Placed { index, col: used[r], span });
        used[r] += span;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5)
    }

    #[test]
    fn fit_off_keeps_natural() {
        let (h, f) = fit_rows(&[200.0, 300.0], &[100.0, 100.0], None);
        assert!(close(&h, &[200.0, 300.0]) && f == 1.0);
    }

    #[test]
    fn fit_shrinks_proportionally() {
        let (h, _) = fit_rows(&[200.0, 300.0], &[50.0, 50.0], Some(250.0));
        assert!(close(&h, &[100.0, 150.0]), "{h:?}");
    }

    #[test]
    fn fit_respects_minimums_and_rebalances() {
        // Uniform 0.5 would put row 0 at 100 < its 150 minimum; it pins at 150
        // and row 1 absorbs the rest of the 400 target.
        let (h, _) = fit_rows(&[200.0, 600.0], &[150.0, 100.0], Some(400.0));
        assert!(close(&h, &[150.0, 250.0]), "{h:?}");
    }

    #[test]
    fn fit_falls_back_to_minimums_then_scrolls() {
        let (h, _) = fit_rows(&[200.0, 200.0], &[150.0, 150.0], Some(100.0));
        assert!(close(&h, &[150.0, 150.0]), "{h:?}");
    }

    #[test]
    fn fit_grows_up_to_cap() {
        let (h, f) = fit_rows(&[100.0, 100.0], &[50.0, 50.0], Some(1000.0));
        assert!((f - MAX_FIT_GROW).abs() < 1e-6);
        assert!(close(&h, &[160.0, 160.0]), "{h:?}");
    }

    fn geo(cols: usize, scale: f32) -> CellGeo {
        CellGeo { cols, col_w: 200.0, scale, min: 150.0 }
    }

    #[test]
    fn resize_snaps_width_to_columns_in_quarters() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        // Pointer about two columns across (2 * 200 + one 10 px gap).
        let (span, _) = resize_from_pointer(egui::pos2(405.0, 200.0), rect, geo(4, 1.0));
        assert_eq!(span, 2);
        // With 2 columns, one column is half the dashboard: two quarters.
        let (span, _) = resize_from_pointer(egui::pos2(190.0, 200.0), rect, geo(2, 1.0));
        assert_eq!(span, 2);
        // Never wider than the grid.
        let (span, _) = resize_from_pointer(egui::pos2(5000.0, 200.0), rect, geo(4, 1.0));
        assert_eq!(span, 4);
    }

    #[test]
    fn resize_height_snaps_unscales_and_respects_minimum() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        assert_eq!(resize_from_pointer(egui::pos2(100.0, 263.0), rect, geo(4, 1.0)).1, 260.0);
        // Shown at Fit-to-window scale 0.5: 300 px on screen is 600 stored.
        assert_eq!(resize_from_pointer(egui::pos2(100.0, 300.0), rect, geo(4, 0.5)).1, 600.0);
        assert_eq!(resize_from_pointer(egui::pos2(100.0, 20.0), rect, geo(4, 1.0)).1, 150.0);
    }

    #[test]
    fn first_fit_back_fills_gaps_in_earlier_rows() {
        // quarters: wide(4), small(1), large(2), small(1)
        let rows = pack_rows(&[4, 1, 2, 1], 4);
        assert_eq!(
            rows,
            vec![
                vec![Placed { index: 0, col: 0, span: 4 }],
                vec![
                    Placed { index: 1, col: 0, span: 1 },
                    Placed { index: 2, col: 1, span: 2 },
                    Placed { index: 3, col: 3, span: 1 },
                ]
            ]
        );
    }

    #[test]
    fn spans_scale_with_the_column_count() {
        // Two half-width cards share a row at 4 columns and at 2, but stack at 1.
        assert_eq!(pack_rows(&[2, 2], 4).len(), 1);
        assert_eq!(pack_rows(&[2, 2], 2).len(), 1);
        assert_eq!(pack_rows(&[2, 2], 1).len(), 2);
        // A quarter-width card still takes a whole column when there is one.
        assert_eq!(pack_rows(&[1], 1), vec![vec![Placed { index: 0, col: 0, span: 1 }]]);
    }

    #[test]
    fn cell_width_splits_space_between_gutters() {
        assert_eq!(cell_width(430.0, 10.0, 4), 100.0);
        assert_eq!(cell_width(100.0, 10.0, 4), 40.0, "floored");
    }

    #[test]
    fn columns_step_4_2_1() {
        assert_eq!(layout_columns(1200.0), 4);
        assert_eq!(layout_columns(560.0), 2);
        assert_eq!(layout_columns(300.0), 1);
    }
}
