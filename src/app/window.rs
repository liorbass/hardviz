//! The OS window across view modes.
//!
//! Mode switches only *queue* viewport commands; [`WindowState::apply`] sends
//! them once per frame in a safe order. The normal window's size is kept in
//! the config while windowed ([`WindowState::track`]), so leaving compact or
//! fullscreen restores it, and it survives a restart.

use crate::config::{AppConfig, ViewMode};
use crate::num::{to_u32, u32_f32};
use eframe::egui::{self, ViewportCommand as Cmd, WindowLevel};
use std::time::{Duration, Instant};

/// Compact overlay window size.
const COMPACT_SIZE: egui::Vec2 = egui::vec2(640.0, 96.0);
/// How long after a mode switch the OS takes to apply the move/resize; window
/// geometry is not recorded until then.
const SETTLE: Duration = Duration::from_millis(800);

pub(super) struct WindowState {
    /// Where the compact overlay's expand button returns to.
    return_mode: ViewMode,
    pending_fullscreen: Option<bool>,
    pending_resize: Option<egui::Vec2>,
    /// Sent after fullscreen and resize.
    pending_cmds: Vec<Cmd>,
    /// Where the normal window was, to put it back after compact mode.
    windowed_pos: Option<egui::Pos2>,
    settle_until: Instant,
}

impl WindowState {
    pub(super) fn new() -> Self {
        Self {
            return_mode: ViewMode::Windowed,
            pending_fullscreen: None,
            pending_resize: None,
            pending_cmds: Vec::new(),
            windowed_pos: None,
            settle_until: Instant::now() + SETTLE,
        }
    }

    /// Apply the window style of the mode the app was restored in.
    pub(super) fn restore(&mut self, config: &AppConfig) {
        if config.view_mode == ViewMode::Compact {
            self.queue_compact_style(config, true);
            self.pending_resize = Some(COMPACT_SIZE);
        }
    }

    /// The mode the compact overlay's expand button returns to.
    pub(super) fn return_mode(&self) -> ViewMode {
        self.return_mode
    }

    /// Queue the window changes for switching `config.view_mode` to `mode`.
    pub(super) fn switch(&mut self, config: &mut AppConfig, mode: ViewMode) {
        let from = config.view_mode;
        if from == mode {
            return;
        }
        self.settle_until = Instant::now() + SETTLE;
        if from == ViewMode::Compact {
            self.queue_compact_style(config, false);
        }
        match mode {
            ViewMode::Compact => {
                self.return_mode = from;
                self.queue_compact_style(config, true);
                // A compact overlay must not be OS-fullscreen.
                self.pending_fullscreen = Some(false);
                self.pending_resize = Some(COMPACT_SIZE);
            }
            ViewMode::Fullscreen => {
                self.pending_fullscreen = Some(true);
                // No explicit resize: the OS sizes a fullscreen viewport.
                self.pending_resize = None;
            }
            ViewMode::Windowed => {
                self.pending_fullscreen = Some(false);
                self.pending_resize = Some(egui::vec2(u32_f32(config.window_width), u32_f32(config.window_height)));
            }
        }
        config.view_mode = mode;
    }

    /// Entering (`true`) or leaving compact mode: borderless, optionally
    /// always on top, at its remembered position.
    fn queue_compact_style(&mut self, config: &AppConfig, compact: bool) {
        if compact {
            self.pending_cmds.push(Cmd::Decorations(false));
            self.set_always_on_top(config.compact_always_on_top);
            if let Some([x, y]) = config.compact_pos {
                self.pending_cmds.push(Cmd::OuterPosition(egui::pos2(x, y)));
            }
        } else {
            self.pending_cmds.push(Cmd::Decorations(true));
            self.set_always_on_top(false);
            if let Some(p) = self.windowed_pos.take() {
                self.pending_cmds.push(Cmd::OuterPosition(p));
            }
        }
    }

    pub(super) fn set_always_on_top(&mut self, on: bool) {
        self.pending_cmds.push(Cmd::WindowLevel(if on { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal }));
    }

    /// Send what was queued. Fullscreen must go before any `InnerSize`, or the
    /// resize fights the OS fullscreen transition.
    pub(super) fn apply(&mut self, ctx: &egui::Context) {
        if let Some(fs) = self.pending_fullscreen.take() {
            ctx.send_viewport_cmd(Cmd::Fullscreen(fs));
        }
        if let Some(size) = self.pending_resize.take() {
            ctx.send_viewport_cmd(Cmd::InnerSize(size));
        }
        for cmd in self.pending_cmds.drain(..) {
            ctx.send_viewport_cmd(cmd);
        }
    }

    /// Remember the window's geometry once a mode switch has settled: the
    /// overlay's position and the normal window's size are persisted; the
    /// normal window's position is restored when leaving compact.
    pub(super) fn track(&mut self, ctx: &egui::Context, config: &mut AppConfig) {
        if Instant::now() < self.settle_until {
            return;
        }
        let (outer, inner, fullscreen, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.fullscreen == Some(true), v.minimized == Some(true))
        });
        if minimized {
            return;
        }
        match config.view_mode {
            ViewMode::Compact => {
                if let Some(o) = outer {
                    config.compact_pos = Some([o.min.x, o.min.y]);
                }
            }
            ViewMode::Windowed if !fullscreen => {
                self.windowed_pos = outer.map(|o| o.min).or(self.windowed_pos);
                if let Some(r) = inner {
                    config.window_width = to_u32(r.width());
                    config.window_height = to_u32(r.height());
                }
            }
            ViewMode::Windowed | ViewMode::Fullscreen => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        let mut c = AppConfig::with_default_widgets(&[]);
        c.window_width = 900;
        c.window_height = 700;
        c
    }

    #[test]
    fn compact_round_trip_restores_the_saved_window_size() {
        let (mut w, mut c) = (WindowState::new(), config());
        w.switch(&mut c, ViewMode::Compact);
        assert_eq!(c.view_mode, ViewMode::Compact);
        assert_eq!(w.pending_resize, Some(COMPACT_SIZE));
        assert_eq!(w.pending_fullscreen, Some(false));
        assert!(w.pending_cmds.contains(&Cmd::Decorations(false)));
        assert_eq!(w.return_mode(), ViewMode::Windowed);

        w.pending_cmds.clear();
        w.switch(&mut c, ViewMode::Windowed);
        assert_eq!(w.pending_resize, Some(egui::vec2(900.0, 700.0)));
        assert!(w.pending_cmds.contains(&Cmd::Decorations(true)));
    }

    #[test]
    fn fullscreen_to_compact_returns_to_fullscreen_and_keeps_the_window_size() {
        let (mut w, mut c) = (WindowState::new(), config());
        w.switch(&mut c, ViewMode::Fullscreen);
        assert_eq!(w.pending_fullscreen, Some(true));
        assert_eq!(w.pending_resize, None);
        w.switch(&mut c, ViewMode::Compact);
        assert_eq!(w.return_mode(), ViewMode::Fullscreen);
        w.switch(&mut c, ViewMode::Windowed);
        // The size to restore is the normal window's, never the fullscreen one.
        assert_eq!(w.pending_resize, Some(egui::vec2(900.0, 700.0)));
    }

    #[test]
    fn restoring_into_compact_applies_its_style() {
        let (mut w, mut c) = (WindowState::new(), config());
        c.view_mode = ViewMode::Compact;
        w.restore(&c);
        assert_eq!(w.pending_resize, Some(COMPACT_SIZE));
        assert!(w.pending_cmds.contains(&Cmd::Decorations(false)));
    }

    #[test]
    fn switching_to_the_current_mode_does_nothing() {
        let (mut w, mut c) = (WindowState::new(), config());
        w.switch(&mut c, ViewMode::Windowed);
        assert!(w.pending_resize.is_none() && w.pending_fullscreen.is_none() && w.pending_cmds.is_empty());
    }

    #[test]
    fn compact_style_honours_always_on_top() {
        let (mut w, mut c) = (WindowState::new(), config());
        c.compact_always_on_top = false;
        c.compact_pos = Some([10.0, 20.0]);
        w.switch(&mut c, ViewMode::Compact);
        assert!(w.pending_cmds.contains(&Cmd::WindowLevel(WindowLevel::Normal)));
        assert!(w.pending_cmds.contains(&Cmd::OuterPosition(egui::pos2(10.0, 20.0))));
    }
}
