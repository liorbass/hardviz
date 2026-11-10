//! The terminal front end (`hardviz-tui`): the same sources, poller and
//! metric store as the window, drawn with ratatui.
//!
//! It reads the saved config for the theme, refresh interval and disabled
//! sources, but never writes it: the window owns the config. It logs to its
//! own `hardviz-tui.log`, never to stderr, which would scribble over the
//! screen.

mod view;

pub use view::draw;

use crate::config::AppConfig;
use crate::poller::Poller;
use crate::processes::SortKey;
use crate::theme::{Theme, ThemeName};
use crate::{error, info, log, sources};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io;
use std::time::Duration;

/// How often the screen is redrawn without input. Readings arrive at the
/// refresh interval; this only bounds how stale the clock-driven parts get.
const FRAME: Duration = Duration::from_millis(250);
/// Refresh interval limits and step, as in Settings.
const REFRESH_MIN_MS: u64 = 250;
const REFRESH_MAX_MS: u64 = 5000;
const REFRESH_STEP_MS: u64 = 250;

/// What the user can change while the dashboard runs (not saved).
pub struct Ui {
    pub theme_name: ThemeName,
    pub theme: Theme,
    pub sort: SortKey,
    pub refresh_ms: u64,
}

impl Ui {
    #[must_use]
    pub fn new(theme_name: ThemeName, refresh_ms: u64) -> Self {
        Self {
            theme_name,
            theme: theme_name.theme(),
            sort: SortKey::Cpu,
            refresh_ms: refresh_ms.clamp(REFRESH_MIN_MS, REFRESH_MAX_MS),
        }
    }

    fn from_config(config: &AppConfig) -> Self {
        Self::new(config.theme, config.refresh_rate_ms)
    }

    /// Apply one key press. Returns `true` when the key means quit.
    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('c') => self.sort = SortKey::Cpu,
            KeyCode::Char('g') => self.sort = SortKey::Gpu,
            KeyCode::Char('i') => self.sort = SortKey::Io,
            KeyCode::Char('m') => self.sort = SortKey::Memory,
            KeyCode::Char('t') => {
                let all = ThemeName::ALL;
                let next = all.iter().position(|&n| n == self.theme_name).map_or(0, |i| (i + 1) % all.len());
                self.theme_name = all[next];
                self.theme = self.theme_name.theme();
            }
            KeyCode::Char('+' | '=') => self.refresh_ms = (self.refresh_ms + REFRESH_STEP_MS).min(REFRESH_MAX_MS),
            KeyCode::Char('-') => {
                self.refresh_ms = self.refresh_ms.saturating_sub(REFRESH_STEP_MS).max(REFRESH_MIN_MS);
            }
            _ => {}
        }
        false
    }
}

/// Run the terminal dashboard until the user quits.
///
/// # Errors
/// The terminal could not be set up, drawn to, or read from.
pub fn run() -> io::Result<()> {
    // Without a file there is simply no log: stderr is the screen.
    let _ = log::init_file_only("hardviz-tui.log");
    info!("HardViz terminal {} starting", env!("CARGO_PKG_VERSION"));
    let config = crate::startup::load_config();
    let poller = Poller::spawn(sources::default_sources, &config.disabled_sources, config.refresh_rate_ms);
    poller.set_sample_processes(true);
    let mut ui = Ui::from_config(&config);

    // `init` enters raw mode and the alternate screen, and installs a panic
    // hook that restores the terminal before the panic message prints.
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &poller, &mut ui);
    ratatui::restore();
    match &result {
        Ok(()) => info!("terminal UI closed"),
        Err(e) => error!("terminal UI failed: {e}"),
    }
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, poller: &Poller, ui: &mut Ui) -> io::Result<()> {
    let state = poller.state();
    loop {
        poller.set_interval(ui.refresh_ms);
        terminal.draw(|f| {
            if let Some(view) = state.lock() {
                draw(f, &view.store, &view.processes, ui);
            }
        })?;
        if event::poll(FRAME)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && ui.handle_key(key.code, key.modifiers) {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::{CPU_TEMP, CPU_USAGE, RAM_TOTAL, RAM_USAGE, RAM_USED};
    use crate::metrics::MetricStore;
    use crate::processes::ProcRow;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn store() -> MetricStore {
        let mut s = MetricStore::new();
        for i in 0..50 {
            s.push(CPU_USAGE, Some(40.0 + f64::from(i % 10)));
            s.push(CPU_TEMP, Some(61.0));
            s.push(RAM_USAGE, Some(50.0));
            s.push(RAM_USED, Some(16_384.0));
            s.push(RAM_TOTAL, Some(32_768.0));
        }
        s
    }

    fn render(width: u16, height: u16, ui: &Ui) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        let procs =
            vec![ProcRow { name: "blender".into(), cpu: 41.8, gpu: Some(38.0), io: 1.0, mem_mb: 900.0, count: 1 }];
        terminal.draw(|f| draw(f, &store(), &procs, ui)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        buffer.content().iter().map(ratatui::buffer::Cell::symbol).collect()
    }

    #[test]
    fn dashboard_shows_readings_and_processes() {
        let screen = render(120, 40, &Ui::new(ThemeName::MidnightDark, 1000));
        for text in ["HardViz", "CPU", "49%", "61°C", "16.0 / 32.0 GB", "Temperature", "Top processes", "blender"] {
            assert!(screen.contains(text), "missing {text:?}");
        }
        // No GPU readings in this store: say so instead of showing zero.
        assert!(screen.contains("N/A"));
    }

    #[test]
    fn a_standard_80_by_24_terminal_keeps_the_essentials() {
        let screen = render(80, 24, &Ui::new(ThemeName::MidnightDark, 1000));
        for text in ["CPU", "RAM", "GPU", "VRAM", "Temperature", "Top processes", "blender", "quit"] {
            assert!(screen.contains(text), "missing {text:?}");
        }
    }

    #[test]
    fn small_terminals_do_not_panic() {
        let ui = Ui::new(ThemeName::PureLight, 1000);
        for (w, h) in [(20, 5), (60, 20), (80, 24), (1, 1)] {
            render(w, h, &ui);
        }
    }

    #[test]
    fn keys_sort_cycle_themes_change_refresh_and_quit() {
        let mut ui = Ui::new(ThemeName::Solarized, 1000);
        assert!(!ui.handle_key(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(ui.sort, SortKey::Memory);
        ui.handle_key(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(ui.theme_name, ThemeName::ALL[0], "wraps after the last theme");
        ui.handle_key(KeyCode::Char('+'), KeyModifiers::NONE);
        assert_eq!(ui.refresh_ms, 1250);
        for _ in 0..30 {
            ui.handle_key(KeyCode::Char('-'), KeyModifiers::NONE);
        }
        assert_eq!(ui.refresh_ms, REFRESH_MIN_MS);
        assert!(ui.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(ui.handle_key(KeyCode::Char('q'), KeyModifiers::NONE));
    }
}
