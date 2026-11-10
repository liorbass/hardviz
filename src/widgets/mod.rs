//! Widget implementations, the built-in widget set, and the shared widget
//! chrome.
//!
//! Each widget is a self-contained module implementing [`crate::widget::Widget`].
//! Most cards are just one entry in [`default_registry`] using a configurable
//! widget; a new kind of visualisation is a new module here plus an entry.

pub mod bar;
pub mod chart;
pub mod gauge;
pub mod line_chart;
pub mod memory_bar;
pub mod metric;
pub mod processes;
pub mod tile;

use crate::config::Visual;
use crate::metrics::metric::{
    CPU_CLOCK, CPU_POWER, CPU_TEMP, CPU_USAGE, DISK_READ, DISK_WRITE, FAN_SPEED, GPU_CLOCK, GPU_POWER, GPU_TEMP,
    GPU_USAGE, NET_DOWN, NET_UP, RAM_TOTAL, RAM_USAGE, RAM_USED, STORAGE_TEMP, VRAM_TOTAL, VRAM_USAGE, VRAM_USED,
};
use crate::metrics::MetricKind;
use crate::num::total_f64;
use crate::theme::Theme;
use crate::widget::{WidgetFactory, WidgetRegistry};
use egui::{Align2, Pos2, Rect, Ui};
use gauge::GaugeWidget;
use line_chart::LineChartWidget;
use memory_bar::MemoryBarWidget;
use metric::MetricWidget;
use processes::ProcessesWidget;

/// The built-in widget types, in default layout order. Add new cards here.
///
/// `type_id`s are persisted in config.json and must never change (retired ones
/// map to their replacements in [`REPLACED_TYPES`]). The convention is
/// `gauge.*`, `chart.*`, `sensor.*`, `panel.*`; `chart.memory_bar` predates it
/// and keeps its id so saved layouts still find it. The `gauge.*` and
/// `sensor.*` cards are [`MetricWidget`]s: their visual is the user's choice.
#[must_use]
pub fn default_registry() -> WidgetRegistry {
    let mut factories = vec![
        WidgetFactory {
            type_id: "gauge.cpu",
            display_name: "CPU Gauge",
            default: true,
            build: || {
                Box::new(
                    MetricWidget::new(CPU_USAGE, Visual::Gauge).detail(|ctx| GaugeWidget::temperature(ctx, CPU_TEMP)),
                )
            },
        },
        WidgetFactory {
            type_id: "gauge.ram",
            display_name: "RAM Gauge",
            default: true,
            build: || {
                Box::new(
                    MetricWidget::new(RAM_USAGE, Visual::Gauge)
                        .detail(|ctx| GaugeWidget::used_of(ctx, RAM_USED, RAM_TOTAL)),
                )
            },
        },
        WidgetFactory {
            type_id: "gauge.gpu",
            display_name: "GPU Gauge",
            default: true,
            build: || {
                Box::new(
                    MetricWidget::new(GPU_USAGE, Visual::Gauge).detail(|ctx| GaugeWidget::temperature(ctx, GPU_TEMP)),
                )
            },
        },
        WidgetFactory {
            type_id: "gauge.vram",
            display_name: "VRAM Gauge",
            default: true,
            build: || {
                Box::new(
                    MetricWidget::new(VRAM_USAGE, Visual::Gauge)
                        .detail(|ctx| GaugeWidget::used_of(ctx, VRAM_USED, VRAM_TOTAL)),
                )
            },
        },
        WidgetFactory {
            type_id: "chart.memory_bar",
            display_name: "Memory Bars",
            default: true,
            build: || Box::new(MemoryBarWidget::new()),
        },
    ];
    factories.extend(SENSORS.iter().map(|&(type_id, display_name, build)| WidgetFactory {
        type_id,
        display_name,
        default: true,
        build,
    }));
    factories.extend([
        WidgetFactory {
            type_id: "chart.usage",
            display_name: "CPU / GPU Usage",
            default: true,
            build: || Box::new(LineChartWidget::new("CPU / GPU Usage", &[CPU_USAGE, GPU_USAGE])),
        },
        WidgetFactory {
            type_id: "chart.temperature",
            display_name: "Temperature",
            default: true,
            build: || Box::new(LineChartWidget::new("Temperature", &[CPU_TEMP, GPU_TEMP]).threshold(80.0)),
        },
        WidgetFactory {
            type_id: "chart.network",
            display_name: "Network",
            default: true,
            // Both series are MetricKind::Network, so give them distinct colours.
            build: || {
                Box::new(LineChartWidget::new("Network", &[NET_DOWN, NET_UP]).colors(|t| vec![t.accent, t.success]))
            },
        },
        WidgetFactory {
            type_id: "chart.disk",
            display_name: "Disk",
            default: true,
            build: || {
                Box::new(
                    LineChartWidget::new("Disk", &[DISK_READ, DISK_WRITE])
                        .colors(|t| vec![t.metric_color(MetricKind::Storage), t.metric_color(MetricKind::Vram)]),
                )
            },
        },
        WidgetFactory {
            type_id: "panel.processes",
            display_name: "Top Processes",
            default: true,
            build: || Box::new(ProcessesWidget::new()),
        },
    ]);
    WidgetRegistry::new(factories)
}

type Build = fn() -> Box<dyn crate::widget::Widget>;

/// One card per secondary sensor, each a [`MetricWidget`] starting as a tile.
/// They replace the old single "Sensors" strip (`panel.sensors`); see
/// [`REPLACED_TYPES`].
const SENSORS: [(&str, &str, Build); 6] = [
    ("sensor.cpu_power", "CPU Power", || Box::new(MetricWidget::new(CPU_POWER, Visual::Tile))),
    ("sensor.cpu_clock", "CPU Clock", || Box::new(MetricWidget::new(CPU_CLOCK, Visual::Tile))),
    ("sensor.gpu_power", "GPU Power", || Box::new(MetricWidget::new(GPU_POWER, Visual::Tile))),
    ("sensor.gpu_clock", "GPU Clock", || Box::new(MetricWidget::new(GPU_CLOCK, Visual::Tile))),
    ("sensor.storage_temp", "Drive Temperature", || Box::new(MetricWidget::new(STORAGE_TEMP, Visual::Tile))),
    ("sensor.fan_speed", "Fan Speed", || Box::new(MetricWidget::new(FAN_SPEED, Visual::Tile))),
];

/// Widget types that no longer exist, and the types a saved layout gets in
/// their place.
pub const REPLACED_TYPES: &[(&str, &[&str])] = &[(
    "panel.sensors",
    &[
        "sensor.cpu_power",
        "sensor.cpu_clock",
        "sensor.gpu_power",
        "sensor.gpu_clock",
        "sensor.storage_temp",
        "sensor.fan_speed",
    ],
)];

/// Card chrome margins. A widget's drawable width is its cell width minus these.
pub const CARD_MARGIN: f32 = 12.0;
/// Gap between the title row and the widget content.
const TITLE_GAP: f32 = 6.0;

/// Draw the standard card filling the space `ui` offers, and give
/// `add_contents` the area under the title along with its height.
///
/// The card takes its size from the cell, not from its contents: that is what
/// lets the dashboard resize cards and have every widget scale to fit.
pub fn frame(ui: &mut Ui, theme: &Theme, title: &str, add_contents: impl FnOnce(&mut Ui, f32)) {
    let size = ui.available_rect_before_wrap().size();
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect(rect, egui::Rounding::same(10.0), theme.bg_widget, egui::Stroke::new(1.0_f32, theme.border));
    let galley = painter.layout_no_wrap(title.to_string(), theme.font(12.0), theme.text_secondary);
    let title_h = galley.size().y;
    painter.galley(rect.min + egui::vec2(CARD_MARGIN, CARD_MARGIN), galley, theme.text_secondary);

    let content = Rect::from_min_max(
        rect.min + egui::vec2(CARD_MARGIN, CARD_MARGIN + title_h + TITLE_GAP),
        rect.max - egui::vec2(CARD_MARGIN, CARD_MARGIN),
    );
    if content.width() < 10.0 || content.height() < 10.0 {
        return;
    }
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(content.intersect(ui.clip_rect()));
    add_contents(&mut child, content.height());
}

/// Render an explanatory placeholder when a widget has no live data.
///
/// An unavailable metric must never look like a real zero reading.
pub fn unavailable(ui: &mut Ui, theme: &Theme, height: f32, reason: &str) {
    // Measure inside the card so margins are already subtracted.
    let width = ui.available_width().max(80.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.text(
        Pos2::new(rect.center().x, rect.center().y - 10.0),
        Align2::CENTER_CENTER,
        "N/A",
        theme.font(19.0),
        theme.text_muted,
    );

    // Truncate by characters, never bytes: these reasons contain em-dashes and
    // other multi-byte UTF-8, and slicing on a char boundary would panic.
    let short: String = reason.lines().take(2).map(|l| clip_chars(l, 52)).collect::<Vec<_>>().join("\n");

    painter.text(
        Pos2::new(rect.center().x, rect.center().y + 13.0),
        Align2::CENTER_CENTER,
        short,
        theme.font(9.5),
        crate::theme::dim(theme.text_muted),
    );

    // Full reason on hover.
    response.on_hover_text(reason);
}

/// Human-readable span of `samples` taken `interval_ms` apart, e.g. `5 min`.
///
/// Derived from the live refresh rate so a chart's stated window stays true
/// when the user changes it.
#[must_use]
pub fn window_label(samples: usize, interval_ms: u64) -> String {
    let span_ms = samples as u64 * interval_ms;
    let secs = span_ms / 1000;
    if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{} h", secs / 3600)
    } else if secs >= 60 {
        let mins = total_f64(secs) / 60.0;
        if secs.is_multiple_of(60) {
            format!("{mins:.0} min")
        } else {
            format!("{mins:.1} min")
        }
    } else {
        format!("{secs} s")
    }
}

/// Clip to at most `max` characters, appending an ellipsis when truncated.
#[must_use]
pub fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_label_reads_naturally() {
        assert_eq!(window_label(300, 1000), "5 min");
        assert_eq!(window_label(300, 100), "30 s");
        assert_eq!(window_label(300, 250), "1.2 min");
        assert_eq!(window_label(300, 750), "3.8 min");
        assert_eq!(window_label(360, 10_000), "1 h");
    }

    #[test]
    fn clip_chars_counts_characters_not_bytes() {
        assert_eq!(clip_chars("short", 10), "short");
        assert_eq!(clip_chars("abcdef", 4), "abc\u{2026}");
        // Multi-byte characters must not be split.
        assert_eq!(clip_chars("°°°°°°", 3), "°°\u{2026}");
    }

    #[test]
    fn every_registered_type_builds() {
        let registry = default_registry();
        for f in registry.factories() {
            assert!(registry.build(f.type_id).is_some(), "{}", f.type_id);
            assert_eq!(registry.name_of(f.type_id), Some(f.display_name));
        }
        assert!(registry.build("no.such.widget").is_none());
        assert_eq!(registry.name_of("no.such.widget"), None);
    }

    #[test]
    fn default_layout_follows_registration_order() {
        let registry = default_registry();
        let expected: Vec<&str> = registry.factories().iter().filter(|f| f.default).map(|f| f.type_id).collect();
        assert_eq!(registry.default_layout(), expected);
        assert_eq!(registry.default_layout().first(), Some(&"gauge.cpu"));
    }
}
