//! Top processes, sortable by CPU, GPU, I/O or memory.
//!
//! Answers the obvious follow-up to a busy gauge: what is using it. Processes
//! sharing an executable name are grouped (`chrome ×38`). Click a column
//! header to sort by it; a faint bar behind each row shows its share of the
//! sorted column relative to the top row.

use crate::metrics::{format_rate, MetricId, MetricKind, Unit};
use crate::num::{count_f32, floor_usize, to_f32};
use crate::processes::{rank, ProcRow, SortKey};
use crate::theme::with_alpha;
use crate::theme::Theme;
use crate::widget::{Card, Widget, WidgetCtx, WidgetSize};
use crate::widgets::clip_chars;
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};

/// Default card height; the user can resize it.
const DEFAULT_HEIGHT: f32 = 240.0;
const MIN_HEIGHT: f32 = 140.0;
const ROW_H: f32 = 19.0;
const HEADER_H: f32 = 20.0;

/// Windows counts every byte a process moves; Linux only its disk traffic.
#[cfg(windows)]
const IO_TIP: &str = "Disk + network + device I/O per process (Windows reports them combined). \
                      Per-process network traffic alone is not available without admin rights.";
#[cfg(not(windows))]
const IO_TIP: &str = "Disk I/O per process. Per-process network traffic is not available \
                      without root or packet capture.";

pub struct ProcessesWidget {
    sort: SortKey,
}

impl ProcessesWidget {
    #[must_use]
    pub fn new() -> Self {
        Self { sort: SortKey::Cpu }
    }
}

impl Default for ProcessesWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// One column: header text, sort key, width, and how to render a value.
struct Col {
    title: &'static str,
    key: SortKey,
    width: f32,
    tip: &'static str,
    /// Right edge (values are right-aligned).
    right: f32,
}

impl Widget for ProcessesWidget {
    fn title(&self) -> &'static str {
        "Top Processes"
    }

    fn requires(&self) -> &[MetricId] {
        &[]
    }

    fn card(&self) -> Card {
        Card::new(WidgetSize::Large, DEFAULT_HEIGHT, MIN_HEIGHT)
    }

    fn wants_processes(&self) -> bool {
        true
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        let theme = ctx.theme;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let painter = ui.painter_at(rect);

        if ctx.processes.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Collecting process list\u{2026}",
                theme.font(11.0),
                theme.text_muted,
            );
            return;
        }

        let has_gpu = ctx.processes.iter().any(|p| p.gpu.is_some());
        if !has_gpu && self.sort == SortKey::Gpu {
            self.sort = SortKey::Cpu;
        }
        let cols = columns(rect, has_gpu);
        self.draw_header(ui, &painter, rect, &cols, theme);

        let mut rows: Vec<ProcRow> = ctx.processes.to_vec();
        rank(&mut rows, self.sort);
        let visible = floor_usize((rect.height() - HEADER_H) / ROW_H);
        let name_w = cols.first().map_or(rect.max.x, |c| c.right - c.width) - rect.min.x;
        let table = Table {
            rect,
            cols: &cols,
            top: rows.first().map_or(0.0, |r| r.key(self.sort)).max(1e-9),
            max_chars: floor_usize(((name_w - 10.0) / 6.0).max(8.0)),
            theme,
        };
        for (i, p) in rows.iter().take(visible).enumerate() {
            self.draw_row(&painter, &table, i, p);
        }
    }
}

impl ProcessesWidget {
    /// "PROCESS" plus right-aligned column titles that sort when clicked.
    fn draw_header(&mut self, ui: &Ui, painter: &egui::Painter, rect: Rect, cols: &[Col], theme: &Theme) {
        let head_font = theme.font(9.5);
        painter.text(rect.min, Align2::LEFT_TOP, "PROCESS", head_font.clone(), theme.text_muted);
        for c in cols {
            let hit = Rect::from_min_max(
                Pos2::new(c.right - c.width, rect.min.y - 2.0),
                Pos2::new(c.right, rect.min.y + HEADER_H - 4.0),
            );
            let resp = ui
                .interact(hit, ui.id().with(("proc_sort", c.title)), Sense::click())
                .on_hover_text(format!("{}\nClick to sort", c.tip));
            if resp.clicked() {
                self.sort = c.key;
            }
            let active = self.sort == c.key;
            let color = match (active, resp.hovered()) {
                (true, _) => theme.accent,
                (false, true) => theme.text_secondary,
                _ => theme.text_muted,
            };
            let label = if active { format!("{} \u{2193}", c.title) } else { c.title.to_string() };
            painter.text(Pos2::new(c.right, rect.min.y), Align2::RIGHT_TOP, label, head_font.clone(), color);
        }
    }

    /// One process: zebra stripe, share-of-top bar, name, then the columns.
    fn draw_row(&self, painter: &egui::Painter, t: &Table, index: usize, p: &ProcRow) {
        let (rect, theme, max_chars) = (t.rect, t.theme, t.max_chars);
        let y = rect.min.y + HEADER_H + count_f32(index) * ROW_H;
        let mid = y + ROW_H / 2.0;
        let row_rect = Rect::from_min_max(Pos2::new(rect.min.x - 4.0, y), Pos2::new(rect.max.x + 4.0, y + ROW_H));

        // Background first so text draws over it.
        if index % 2 == 1 {
            painter.add(egui::Shape::rect_filled(row_rect, 3.0, with_alpha(theme.border, 28)));
        }
        let share = to_f32((p.key(self.sort) / t.top).clamp(0.0, 1.0));
        if share > 0.005 {
            let bar = Rect::from_min_size(row_rect.min, Vec2::new(row_rect.width() * share, ROW_H));
            painter.add(egui::Shape::rect_filled(bar, 3.0, with_alpha(sort_color(self.sort, theme), 38)));
        }

        let body = theme.font(11.0);
        let name = if p.count > 1 {
            format!("{} \u{D7}{}", clip_chars(&p.name, max_chars.saturating_sub(4)), p.count)
        } else {
            clip_chars(&p.name, max_chars)
        };
        painter.text(Pos2::new(rect.min.x, mid), Align2::LEFT_CENTER, name, body.clone(), theme.text_primary);

        for c in t.cols {
            let (text, negligible) = cell(p, c.key);
            let color = if c.key == self.sort {
                theme.text_primary
            } else if negligible {
                theme.text_muted
            } else {
                theme.text_secondary
            };
            painter.text(Pos2::new(c.right, mid), Align2::RIGHT_CENTER, text, body.clone(), color);
        }
    }
}

/// Per-frame layout shared by every row.
struct Table<'a> {
    rect: Rect,
    cols: &'a [Col],
    /// The sorted column's value in the top row; bars are relative to it.
    top: f64,
    /// Name column width in characters.
    max_chars: usize,
    theme: &'a Theme,
}

/// Value columns, right-aligned against the card edge. GPU only with NVML.
fn columns(rect: Rect, has_gpu: bool) -> Vec<Col> {
    let mut cols = vec![Col { title: "CPU", key: SortKey::Cpu, width: 56.0, tip: "Share of total CPU", right: 0.0 }];
    if has_gpu {
        cols.push(Col {
            title: "GPU",
            key: SortKey::Gpu,
            width: 50.0,
            tip: "GPU 3D/compute utilisation (NVML)",
            right: 0.0,
        });
    }
    cols.push(Col { title: "I/O", key: SortKey::Io, width: 72.0, tip: IO_TIP, right: 0.0 });
    cols.push(Col { title: "MEMORY", key: SortKey::Memory, width: 72.0, tip: "Resident memory", right: 0.0 });
    let mut right = rect.max.x;
    for c in cols.iter_mut().rev() {
        c.right = right;
        right -= c.width;
    }
    cols
}

/// A column's text for one process, and whether the value is negligible
/// (drawn muted).
fn cell(p: &ProcRow, key: SortKey) -> (String, bool) {
    match key {
        SortKey::Cpu => (format!("{:.1}%", p.cpu), p.cpu < 0.05),
        SortKey::Gpu => {
            let g = p.gpu.unwrap_or(0.0);
            (format!("{g:.0}%"), g < 0.5)
        }
        SortKey::Io => (format_rate(p.io), p.io < 0.001),
        SortKey::Memory => (Unit::Megabytes.format(p.mem_mb), false),
    }
}

fn sort_color(key: SortKey, theme: &Theme) -> Color32 {
    match key {
        SortKey::Cpu => theme.metric_color(MetricKind::Cpu),
        SortKey::Gpu => theme.metric_color(MetricKind::Gpu),
        SortKey::Io => theme.metric_color(MetricKind::Storage),
        SortKey::Memory => theme.metric_color(MetricKind::Ram),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(cpu: f64, gpu: Option<f64>, io: f64) -> ProcRow {
        ProcRow { name: "p".into(), cpu, gpu, io, mem_mb: 512.0, count: 1 }
    }

    #[test]
    fn columns_right_align_and_skip_gpu_without_nvml() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(500.0, 100.0));
        let with_gpu = columns(rect, true);
        assert_eq!(with_gpu.iter().map(|c| c.title).collect::<Vec<_>>(), ["CPU", "GPU", "I/O", "MEMORY"]);
        assert_eq!(with_gpu.last().unwrap().right, 500.0);
        assert_eq!(with_gpu[2].right, 500.0 - 72.0);
        assert!(columns(rect, false).iter().all(|c| c.key != SortKey::Gpu));
    }

    #[test]
    fn cells_format_and_mute_negligible_values() {
        let p = proc(12.34, Some(0.2), 0.0);
        assert_eq!(cell(&p, SortKey::Cpu), ("12.3%".into(), false));
        assert_eq!(cell(&p, SortKey::Gpu), ("0%".into(), true));
        assert!(cell(&p, SortKey::Io).1);
        assert_eq!(cell(&p, SortKey::Memory), ("512 MB".into(), false));
    }
}
