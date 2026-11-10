//! Visual style. A theme is plain data: one [`Color32`] per role, the mapping
//! from a metric's [`MetricKind`] to its colour, and the text scale (Settings
//! -> Text size) that every font size goes through.
//!
//! To add a theme: a variant in [`ThemeName`] (with its entry in
//! [`ThemeName::ALL`] and [`ThemeName::label`]) and a preset function below.

use crate::metrics::MetricKind;
use crate::num::{to_f32, to_u8};
use egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Theme {
    pub bg_primary: Color32,
    pub bg_secondary: Color32,
    pub bg_widget: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub border: Color32,
    /// Per-metric-kind colours; widgets reach them through [`Theme::metric_color`].
    cpu: Color32,
    ram: Color32,
    gpu: Color32,
    vram: Color32,
    pub temp_warm: Color32,
    pub temp_hot: Color32,
    pub accent: Color32,
    pub gauge_bg: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// Multiplier for every font size (Settings -> Text size).
    text_scale: f32,
}

impl Theme {
    /// This theme with text scaled by `scale` (clamped to 0.5-3).
    #[must_use]
    pub fn with_text_scale(mut self, scale: f32) -> Self {
        self.text_scale = scale.clamp(0.5, 3.0);
        self
    }

    /// A font size scaled by the text-size setting. Every size in the UI goes
    /// through this (or [`Theme::font`]) so the setting reaches all text.
    #[must_use]
    pub fn fs(&self, size: f32) -> f32 {
        size * self.text_scale
    }

    /// Proportional font at a scaled size.
    #[must_use]
    pub fn font(&self, size: f32) -> egui::FontId {
        egui::FontId::proportional(self.fs(size))
    }

    #[must_use]
    pub fn metric_color(&self, kind: MetricKind) -> Color32 {
        match kind {
            MetricKind::Cpu => self.cpu,
            MetricKind::Ram => self.ram,
            MetricKind::Gpu => self.gpu,
            MetricKind::Vram => self.vram,
            MetricKind::Storage => self.warning,
            MetricKind::Network => self.accent,
            MetricKind::Cooling => self.success,
        }
    }

    /// Colour for a reading `t` (0-1) of the way up a scale where high is
    /// bad: the metric's own `mild` colour up to half way, then fading to
    /// `warning` at three quarters and to `danger` at the top.
    #[must_use]
    pub fn severity_color(&self, mild: Color32, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        if t <= 0.5 {
            mild
        } else if t <= 0.75 {
            lerp_color(mild, self.warning, (t - 0.5) / 0.25)
        } else {
            lerp_color(self.warning, self.danger, (t - 0.75) / 0.25)
        }
    }

    /// Severity colour at position `s` (0-1) along a gauge or bar filled to
    /// `fill` (0-1). The whole fill shifts with the reading: its start has the
    /// colour of a reading [`SEVERITY_SPREAD`] lower, its tip the reading's
    /// own, with a gradient between.
    #[must_use]
    pub fn severity_along(&self, mild: Color32, fill: f32, s: f32) -> Color32 {
        let fill = fill.clamp(0.0, 1.0);
        let start = (fill - SEVERITY_SPREAD).max(0.0);
        self.severity_color(mild, start + (fill - start) * s.clamp(0.0, 1.0))
    }

    /// Warm-to-hot colour for a temperature: warm at 40 °C and below, hot at
    /// 100 °C and above.
    #[must_use]
    pub fn temp_color(&self, celsius: f64) -> Color32 {
        let t = to_f32(((celsius - 40.0) / 60.0).clamp(0.0, 1.0));
        lerp_color(self.temp_warm, self.temp_hot, t)
    }
}

/// How much of the scale a severity gradient spans, from the start of a fill
/// to its tip.
pub const SEVERITY_SPREAD: f32 = 0.25;

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| to_u8(f32::from(x) + (f32::from(y) - f32::from(x)) * t);
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// `c` at `alpha` opacity (premultiplied).
#[must_use]
pub fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    let scale = |v: u8| to_u8(f32::from(v) * f32::from(alpha) / 255.0);
    Color32::from_rgba_premultiplied(scale(c.r()), scale(c.g()), scale(c.b()), alpha)
}

/// `c` at half brightness, slightly translucent: secondary text.
#[must_use]
pub fn dim(c: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(c.r() / 2, c.g() / 2, c.b() / 2, 160)
}

/// The persisted theme choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeName {
    MidnightDark,
    PureLight,
    OledBlack,
    Cyberpunk,
    Solarized,
}

impl ThemeName {
    /// Every theme, in picker order.
    pub const ALL: [ThemeName; 5] = [
        ThemeName::MidnightDark,
        ThemeName::PureLight,
        ThemeName::OledBlack,
        ThemeName::Cyberpunk,
        ThemeName::Solarized,
    ];

    /// Short label for the theme picker.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ThemeName::MidnightDark => "Midnight",
            ThemeName::PureLight => "Light",
            ThemeName::OledBlack => "OLED",
            ThemeName::Cyberpunk => "Cyber",
            ThemeName::Solarized => "Solar",
        }
    }

    #[must_use]
    pub fn theme(self) -> Theme {
        match self {
            ThemeName::MidnightDark => midnight_dark(),
            ThemeName::PureLight => pure_light(),
            ThemeName::OledBlack => oled_black(),
            ThemeName::Cyberpunk => cyberpunk(),
            ThemeName::Solarized => solarized(),
        }
    }
}

fn midnight_dark() -> Theme {
    Theme {
        bg_primary: rgb(15, 17, 23),
        bg_secondary: rgb(22, 25, 35),
        bg_widget: rgb(28, 31, 42),
        text_primary: rgb(235, 237, 245),
        text_secondary: rgb(170, 175, 190),
        text_muted: rgb(133, 136, 148),
        border: rgb(45, 48, 62),
        cpu: rgb(88, 166, 255),
        ram: rgb(63, 185, 80),
        gpu: rgb(255, 163, 71),
        vram: rgb(163, 113, 247),
        temp_warm: rgb(255, 200, 60),
        temp_hot: rgb(255, 80, 80),
        accent: rgb(88, 166, 255),
        gauge_bg: rgb(40, 43, 55),
        success: rgb(63, 185, 80),
        warning: rgb(255, 200, 60),
        danger: rgb(255, 80, 80),
        text_scale: 1.0,
    }
}

fn pure_light() -> Theme {
    Theme {
        bg_primary: rgb(248, 249, 252),
        bg_secondary: rgb(240, 242, 247),
        bg_widget: rgb(255, 255, 255),
        text_primary: rgb(30, 35, 50),
        text_secondary: rgb(90, 95, 110),
        text_muted: rgb(114, 117, 124),
        border: rgb(220, 223, 232),
        cpu: rgb(40, 120, 220),
        ram: rgb(40, 160, 60),
        gpu: rgb(230, 120, 30),
        vram: rgb(130, 80, 210),
        temp_warm: rgb(220, 170, 30),
        temp_hot: rgb(220, 50, 50),
        accent: rgb(40, 120, 220),
        gauge_bg: rgb(230, 233, 240),
        success: rgb(40, 160, 60),
        warning: rgb(220, 170, 30),
        danger: rgb(220, 50, 50),
        text_scale: 1.0,
    }
}

fn oled_black() -> Theme {
    Theme {
        bg_primary: rgb(0, 0, 0),
        bg_secondary: rgb(8, 8, 12),
        bg_widget: rgb(14, 14, 20),
        text_primary: rgb(240, 242, 250),
        text_secondary: rgb(160, 165, 180),
        text_muted: rgb(120, 124, 136),
        border: rgb(30, 30, 40),
        cpu: rgb(80, 160, 255),
        ram: rgb(50, 200, 70),
        gpu: rgb(255, 150, 50),
        vram: rgb(170, 100, 255),
        temp_warm: rgb(255, 200, 40),
        temp_hot: rgb(255, 60, 60),
        accent: rgb(80, 160, 255),
        gauge_bg: rgb(25, 25, 35),
        success: rgb(50, 200, 70),
        warning: rgb(255, 200, 40),
        danger: rgb(255, 60, 60),
        text_scale: 1.0,
    }
}

fn cyberpunk() -> Theme {
    Theme {
        bg_primary: rgb(10, 5, 20),
        bg_secondary: rgb(18, 10, 35),
        bg_widget: rgb(25, 15, 45),
        text_primary: rgb(230, 240, 255),
        text_secondary: rgb(180, 190, 220),
        text_muted: rgb(119, 127, 154),
        border: rgb(50, 30, 80),
        cpu: rgb(0, 255, 240),
        ram: rgb(255, 0, 200),
        gpu: rgb(0, 200, 255),
        vram: rgb(255, 100, 255),
        temp_warm: rgb(255, 255, 0),
        temp_hot: rgb(255, 0, 100),
        accent: rgb(0, 255, 240),
        gauge_bg: rgb(35, 20, 60),
        success: rgb(0, 255, 150),
        warning: rgb(255, 255, 0),
        danger: rgb(255, 0, 100),
        text_scale: 1.0,
    }
}

fn solarized() -> Theme {
    Theme {
        bg_primary: rgb(0, 43, 54),
        bg_secondary: rgb(7, 54, 66),
        bg_widget: rgb(10, 60, 72),
        text_primary: rgb(253, 246, 227),
        text_secondary: rgb(176, 186, 186),
        text_muted: rgb(150, 164, 168),
        border: rgb(35, 75, 85),
        cpu: rgb(38, 139, 210),
        ram: rgb(133, 153, 0),
        gpu: rgb(203, 75, 22),
        vram: rgb(108, 113, 196),
        temp_warm: rgb(181, 137, 0),
        temp_hot: rgb(220, 50, 47),
        accent: rgb(38, 139, 210),
        gauge_bg: rgb(20, 65, 78),
        success: rgb(133, 153, 0),
        warning: rgb(181, 137, 0),
        danger: rgb(220, 50, 47),
        text_scale: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_fades_from_the_metric_colour_to_danger() {
        let t = ThemeName::MidnightDark.theme();
        let mild = t.metric_color(MetricKind::Cpu);
        assert_eq!(t.severity_color(mild, 0.2), mild);
        assert_eq!(t.severity_color(mild, 0.5), mild);
        assert_eq!(t.severity_color(mild, 0.75), t.warning);
        assert_eq!(t.severity_color(mild, 1.0), t.danger);
        assert_ne!(t.severity_color(mild, 0.6), mild, "fading between");
    }

    #[test]
    fn the_whole_fill_shifts_with_the_reading() {
        let t = ThemeName::MidnightDark.theme();
        let mild = t.metric_color(MetricKind::Ram);
        // Low reading: all mild.
        assert_eq!(t.severity_along(mild, 0.3, 0.0), mild);
        assert_eq!(t.severity_along(mild, 0.3, 1.0), mild);
        // Full: the start is already at warning, the tip at danger.
        assert_eq!(t.severity_along(mild, 1.0, 0.0), t.warning);
        assert_eq!(t.severity_along(mild, 1.0, 1.0), t.danger);
        // At 90 % the start has left the mild colour too.
        assert_ne!(t.severity_along(mild, 0.9, 0.0), mild);
    }

    #[test]
    fn temp_color_spans_warm_to_hot() {
        let t = ThemeName::MidnightDark.theme();
        assert_eq!(t.temp_color(20.0), t.temp_warm);
        assert_eq!(t.temp_color(40.0), t.temp_warm);
        assert_eq!(t.temp_color(120.0), t.temp_hot);
        let mid = t.temp_color(70.0);
        assert_ne!(mid, t.temp_warm);
        assert_ne!(mid, t.temp_hot);
    }

    #[test]
    fn text_scale_multiplies_font_sizes() {
        let t = ThemeName::PureLight.theme().with_text_scale(1.5);
        assert_eq!(t.fs(10.0), 15.0);
        assert_eq!(t.font(10.0).size, 15.0);
        assert_eq!(ThemeName::PureLight.theme().with_text_scale(9.0).fs(1.0), 3.0, "clamped");
    }

    #[test]
    fn with_alpha_premultiplies() {
        let c = with_alpha(Color32::from_rgb(200, 100, 0), 128);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (100, 50, 0, 128));
        assert_eq!(with_alpha(Color32::WHITE, 255), Color32::WHITE);
    }

    #[test]
    fn every_theme_has_a_unique_label() {
        let labels: std::collections::HashSet<_> = ThemeName::ALL.iter().map(|n| n.label()).collect();
        assert_eq!(labels.len(), ThemeName::ALL.len());
    }

    #[test]
    fn metric_kinds_map_to_their_theme_colours() {
        let t = ThemeName::Cyberpunk.theme();
        assert_eq!(t.metric_color(MetricKind::Cpu), t.cpu);
        assert_eq!(t.metric_color(MetricKind::Vram), t.vram);
        assert_eq!(t.metric_color(MetricKind::Network), t.accent);
    }
}
