//! Shared look of the shell: panels, section labels, toggle buttons, and
//! mapping the theme onto egui's own widgets.

use crate::theme::with_alpha;
use crate::theme::Theme;
use eframe::egui;

/// A rounded panel with the theme's border, e.g. the Settings card.
pub(super) fn card_frame(theme: &Theme, fill: egui::Color32, rounding: f32, margin: egui::Margin) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0_f32, theme.border))
        .rounding(egui::Rounding::same(rounding))
        .inner_margin(margin)
}

/// A borderless icon or text button.
pub(super) fn flat_button(text: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text).fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)
}

pub(super) fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).size(theme.fs(10.0)).strong().color(theme.text_muted));
    ui.add_space(4.0);
}

/// A top-bar toggle: accent text on a tinted fill while active. The view-mode
/// buttons and the right-hand toggles share it, so the bar reads as one set.
pub(super) fn bar_toggle(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(theme.fs(11.0)).color(if active {
            theme.accent
        } else {
            theme.text_muted
        }))
        .fill(if active { with_alpha(theme.accent, 25) } else { egui::Color32::TRANSPARENT })
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::NONE),
    )
}

pub(super) fn apply_theme_to_context(ctx: &egui::Context, theme: &Theme) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(theme.text_primary);
    v.widgets.noninteractive.bg_fill = theme.bg_secondary;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme.text_secondary);
    v.widgets.inactive.bg_fill = theme.bg_widget;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme.text_secondary);
    v.widgets.hovered.bg_fill = with_alpha(theme.accent, 20);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, theme.accent);
    v.widgets.active.bg_fill = with_alpha(theme.accent, 35);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, theme.accent);
    // Buttons and value boxes paint `weak_bg_fill`; left at the dark
    // default, they show as black boxes on the light theme.
    v.widgets.noninteractive.weak_bg_fill = theme.bg_secondary;
    v.widgets.inactive.weak_bg_fill = theme.bg_widget;
    v.widgets.hovered.weak_bg_fill = with_alpha(theme.accent, 20);
    v.widgets.active.weak_bg_fill = with_alpha(theme.accent, 35);
    v.selection.bg_fill = with_alpha(theme.accent, 40);
    v.window_fill = theme.bg_primary;
    v.panel_fill = theme.bg_primary;
    v.window_stroke = egui::Stroke::new(1.0_f32, theme.border);
    v.extreme_bg_color = theme.bg_secondary;
    ctx.set_visuals(v);
}

/// Scale egui's built-in text styles (labels, buttons, sliders) with the
/// text-size setting; custom-drawn text uses `Theme::fs` with the same factor.
pub(super) fn apply_font_scale(ctx: &egui::Context, scale: f32) {
    let base = egui::Style::default().text_styles;
    ctx.style_mut(|style| {
        for (k, v) in &mut style.text_styles {
            if let Some(b) = base.get(k) {
                v.size = b.size * scale;
            }
        }
    });
}

/// Let proportional text fall back to the bundled monospace font.
///
/// egui's proportional chain (Ubuntu Light, Noto Emoji, emoji-icon-font) lacks
/// common UI symbols - `●` `○` `←` `↑` `↓` - which exist only in the bundled
/// Hack font, so they rendered as empty boxes. Appending Hack last fills those
/// gaps without changing any glyph the earlier fonts already provide.
pub(super) fn install_font_fallbacks(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let hack = fonts.families.get(&egui::FontFamily::Monospace).and_then(|m| m.first().cloned());
    if let (Some(hack), Some(prop)) = (hack, fonts.families.get_mut(&egui::FontFamily::Proportional)) {
        if !prop.contains(&hack) {
            prop.push(hack);
        }
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;

    #[test]
    fn card_frame_uses_the_theme_border() {
        let theme = ThemeName::Solarized.theme();
        let frame = card_frame(&theme, theme.bg_widget, 8.0, egui::Margin::same(4.0));
        assert_eq!(frame.fill, theme.bg_widget);
        assert_eq!(frame.stroke.color, theme.border);
        assert_eq!(frame.rounding, egui::Rounding::same(8.0));
    }

    #[test]
    fn egui_visuals_and_text_follow_the_theme() {
        let ctx = egui::Context::default();
        let theme = ThemeName::PureLight.theme();
        apply_theme_to_context(&ctx, &theme);
        assert_eq!(ctx.style().visuals.panel_fill, theme.bg_primary);
        assert_eq!(ctx.style().visuals.widgets.inactive.weak_bg_fill, theme.bg_widget);
        apply_font_scale(&ctx, 2.0);
        let base = egui::Style::default().text_styles[&egui::TextStyle::Button].size;
        assert_eq!(ctx.style().text_styles[&egui::TextStyle::Button].size, base * 2.0);
    }
}
