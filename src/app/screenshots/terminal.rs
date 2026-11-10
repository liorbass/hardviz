//! The terminal UI as an image: ratatui draws into a test buffer, and each
//! cell is painted with egui like a terminal emulator would — background,
//! then the glyph in the embedded monospace font. Box-drawing, block and
//! braille characters are drawn as shapes so lines join and bars sit flush.

use super::{demo, render_ui, HISTORY_LEN};
use crate::poller::Poller;
use crate::theme::ThemeName;
use crate::tui::{draw, Ui};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::path::Path;

const COLS: u16 = 140;
const ROWS: u16 = 42;
const FONT_SIZE: f32 = 13.0;
/// Line height as a multiple of the font size.
const LINE: f32 = 1.3;

/// Render the terminal dashboard on demo data and save it to `path`.
pub(super) fn shoot(theme: ThemeName, path: &Path) {
    let poller = Poller::manual(demo::sources());
    for _ in 0..HISTORY_LEN {
        poller.sample_now();
    }
    let ui = Ui::new(theme, 1000);
    let mut term = Terminal::new(TestBackend::new(COLS, ROWS)).expect("test terminal");
    let state = poller.state();
    {
        let view = state.lock().expect("state");
        term.draw(|f| draw(f, &view.store, &demo::processes(), &ui)).expect("draw");
    }
    let buffer = term.backend().buffer().clone();

    let ctx = egui::Context::default();
    // Fonts exist only after a first frame.
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let font = FontId::monospace(FONT_SIZE);
    let cell_w = ctx.fonts(|f| f.glyph_width(&font, 'M'));
    let cell = egui::vec2(cell_w, (FONT_SIZE * LINE).round());
    let size = egui::vec2(cell.x * f32::from(COLS), cell.y * f32::from(ROWS));
    let fg_default = ui.theme.text_primary;
    let bg_default = ui.theme.bg_primary;
    let canvas = render_ui(&ctx, size, 1.0, |ctx| {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg_default)).show(ctx, |ui| {
            paint(ui.painter(), &buffer, cell, &font, fg_default, bg_default);
        });
    });
    canvas.save_png(path, [0, 0, canvas.width, canvas.height]).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    println!("wrote {}", path.display());
}

fn color(c: Color, default: Color32) -> Color32 {
    match c {
        Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        _ => default,
    }
}

fn paint(p: &egui::Painter, buffer: &Buffer, cell: egui::Vec2, font: &FontId, fg0: Color32, bg0: Color32) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let c = &buffer[(x, y)];
            let rect = Rect::from_min_size(Pos2::new(f32::from(x) * cell.x, f32::from(y) * cell.y), cell);
            let bg = color(c.bg, bg0);
            if bg != bg0 {
                // Grow a hair so neighbouring cells leave no hairline seams.
                p.rect_filled(rect.expand(0.3), 0.0, bg);
            }
            let fg = color(c.fg, fg0);
            let sym = c.symbol();
            let Some(ch) = sym.chars().next() else { continue };
            if ch == ' ' || shape(p, ch, rect, fg) {
                continue;
            }
            let pos = rect.left_center();
            p.text(pos, egui::Align2::LEFT_CENTER, sym, font.clone(), fg);
            if c.modifier.contains(Modifier::BOLD) {
                p.text(pos + egui::vec2(0.6, 0.0), egui::Align2::LEFT_CENTER, sym, font.clone(), fg);
            }
        }
    }
}

/// Draw `ch` as shapes if it is a box, block or braille character.
fn shape(p: &egui::Painter, ch: char, r: Rect, fg: Color32) -> bool {
    let stroke = Stroke::new(1.0_f32, fg);
    let c = r.center();
    let (l, rt, t, b) =
        (Pos2::new(r.left(), c.y), Pos2::new(r.right(), c.y), Pos2::new(c.x, r.top()), Pos2::new(c.x, r.bottom()));
    let seg = |a: Pos2, z: Pos2| {
        p.line_segment([a, z], stroke);
    };
    // Rounded corner: from one edge midpoint into the centre region and out
    // to the other, along a quarter circle.
    let arc = |from: Pos2, to: Pos2| {
        let rad = (r.width().min(r.height()) / 2.0).min(4.0);
        let dir_from = (from - c).normalized();
        let dir_to = (to - c).normalized();
        let start = c + dir_from * rad;
        let end = c + dir_to * rad;
        let centre = c + (dir_from + dir_to) * rad;
        let pts: Vec<Pos2> = (0..=6)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / 6.0;
                let (u, v) = (start - centre, end - centre);
                centre + u * a.cos() + v * a.sin()
            })
            .collect();
        seg(from, start);
        p.add(egui::Shape::line(pts, stroke));
        seg(end, to);
    };
    match ch {
        '─' => seg(l, rt),
        '│' => seg(t, b),
        '┌' => {
            seg(c, rt);
            seg(c, b);
        }
        '┐' => {
            seg(l, c);
            seg(c, b);
        }
        '└' => {
            seg(t, c);
            seg(c, rt);
        }
        '┘' => {
            seg(l, c);
            seg(t, c);
        }
        '╭' => arc(rt, b),
        '╮' => arc(l, b),
        '╰' => arc(t, rt),
        '╯' => arc(t, l),
        // Lower blocks ▁..█: bottom n/8 of the cell.
        '\u{2581}'..='\u{2588}' => {
            let n = (ch as u32 - 0x2580) as f32;
            p.rect_filled(
                Rect::from_min_max(Pos2::new(r.left(), r.bottom() - r.height() * n / 8.0), r.max).expand(0.3),
                0.0,
                fg,
            );
        }
        // Left blocks ▉..▏: left n/8 of the cell.
        '\u{2589}'..='\u{258F}' => {
            let n = (0x2590 - ch as u32) as f32;
            p.rect_filled(
                Rect::from_min_max(r.min, Pos2::new(r.left() + r.width() * n / 8.0, r.bottom())).expand(0.3),
                0.0,
                fg,
            );
        }
        '\u{2800}'..='\u{28FF}' => braille(p, ch as u32 - 0x2800, r, fg),
        _ => return false,
    }
    true
}

/// Braille dots: bits 0-2 and 6 are the left column top to bottom, 3-5 and
/// 7 the right.
fn braille(p: &egui::Painter, bits: u32, r: Rect, fg: Color32) {
    const DOTS: [(u32, f32, f32); 8] = [
        (0, 0.0, 0.0),
        (1, 0.0, 1.0),
        (2, 0.0, 2.0),
        (3, 1.0, 0.0),
        (4, 1.0, 1.0),
        (5, 1.0, 2.0),
        (6, 0.0, 3.0),
        (7, 1.0, 3.0),
    ];
    let (dx, dy) = (r.width() / 2.0, r.height() / 4.0);
    let radius = (dx.min(dy) * 0.42).max(0.8);
    for (bit, col, row) in DOTS {
        if bits & (1 << bit) != 0 {
            let centre = Pos2::new(r.left() + dx * (col + 0.5), r.top() + dy * (row + 0.5));
            p.circle_filled(centre, radius, fg);
        }
    }
}
