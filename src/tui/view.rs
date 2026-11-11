//! Drawing the terminal dashboard: a pure function of the metric store, the
//! process list and the UI state, so it renders the same into a real terminal
//! or a test buffer.
//!
//! ```text
//!  HardViz 0.1.1  terminal                          Midnight · 1000 ms · 5 min charts
//! ╭ CPU ──────────╮╭ RAM ──────────╮╭ GPU ──────────╮╭ VRAM ─────────╮
//! │ 68%  73°C     ││ 59%  18.8/32  ││ 69%  63°C     ││ 47%  5.7/12.0 │   usage, bar,
//! │ ██████▋░░░░░░ ││ █████▉░░░░░░░ ││ ██████▉░░░░░░ ││ ████▋░░░░░░░░ │   history
//! │ ▁▂▂▃▅▆▇▆▇▇▆▇▇ ││ ▅▅▅▅▆▆▆▆▆▆▆▆▆ ││ ▂▃▂▄▅▆▇▆▇▆▇▆▇ ││ ▃▃▃▃▅▅▅▅▅▅▅▅▅ │
//! ╰───────────────╯╰───────────────╯╰───────────────╯╰───────────────╯
//! ╭ Sensors ───────────────────────────────────────────────────────────╮
//! │ CPU power 121 W   CPU clock 4.90 GHz   GPU power 218 W   Fan …     │
//! ╰────────────────────────────────────────────────────────────────────╯
//! ╭ Temperature ──────────────────╮╭ Network ──────────────────────────╮
//! ╰───────────────────────────────╯╰───────────────────────────────────╯
//! ╭ Disk ─────────────────────────╮╭ Top processes ────────────────────╮
//! ╰───────────────────────────────╯╰───────────────────────────────────╯
//!  q quit  c/g/i/m sort  t theme  w chart span  +/- refresh  e export
//! ```

use super::Ui;
use crate::metrics::metric::{
    CPU_CLOCK, CPU_POWER, CPU_TEMP, CPU_USAGE, DISK_READ, DISK_WRITE, FAN_SPEED, GPU_CLOCK, GPU_POWER, GPU_TEMP,
    GPU_USAGE, NET_DOWN, NET_UP, RAM_TOTAL, RAM_USAGE, RAM_USED, STORAGE_TEMP, VRAM_TOTAL, VRAM_USAGE, VRAM_USED,
};
use crate::metrics::{MetricId, MetricKind, MetricStore, Scale, Unit};
use crate::num::{count_f64, to_f32, to_u64};
use crate::processes::{rank, ProcRow, SortKey};
use crate::theme::Theme;
use crate::widget::chart_samples;
use crate::widgets::span_label;
use egui::Color32;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, BorderType, Cell, Chart, Dataset, GraphType, LegendPosition, Paragraph, Row, Sparkline, Table,
};
use ratatui::Frame;

/// Below this width the four usage panels go two by two.
const WIDE: u16 = 100;
/// Height of one row of usage panels: value, bar, three rows of history.
const GAUGE_ROW: u16 = 7;
/// The same without history, for short terminals.
const GAUGE_ROW_SHORT: u16 = 4;
/// Narrow, two-by-two panels keep their history only from this body height.
const TALL: u16 = 34;
const SENSOR_ROW: u16 = 5;
/// Width one sensor needs, spacing included.
const SENSOR_MIN_WIDTH: u16 = 21;
/// The sensor metrics, shown when they have a reading.
const SENSORS: [MetricId; 6] = [CPU_POWER, CPU_CLOCK, GPU_POWER, GPU_CLOCK, FAN_SPEED, STORAGE_TEMP];

/// A theme colour as a terminal colour (24-bit).
fn tc(c: Color32) -> Color {
    let [r, g, b, _] = c.to_array();
    Color::Rgb(r, g, b)
}

/// Draw the whole dashboard into `f`.
pub fn draw(f: &mut Frame, store: &MetricStore, processes: &[ProcRow], ui: &Ui) {
    let t = &ui.theme;
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(tc(t.bg_primary)).fg(tc(t.text_primary))), area);
    let [title, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    draw_title(f, title, ui);
    draw_footer(f, footer, ui);

    let wide = body.width >= WIDE;
    let gauges_h = match (wide, body.height >= TALL) {
        (true, _) => GAUGE_ROW,
        (false, true) => GAUGE_ROW * 2,
        (false, false) => GAUGE_ROW_SHORT * 2,
    };
    let sensors: Vec<MetricId> = SENSORS.into_iter().filter(|&id| store.current(id).is_some()).collect();
    let sensors_h = if sensors.is_empty() || body.height < gauges_h + SENSOR_ROW + 8 { 0 } else { SENSOR_ROW };
    let [gauges, sensor_row, rest] =
        Layout::vertical([Constraint::Length(gauges_h), Constraint::Length(sensors_h), Constraint::Fill(1)])
            .areas(body);

    draw_gauges(f, gauges, store, ui, wide);
    if sensors_h > 0 {
        draw_sensors(f, sensor_row, store, &sensors, t);
    }

    let halves = |r: Rect| Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas::<2>(r);
    if rest.height >= 16 {
        let [top, bottom] = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(rest);
        let [temp, net] = halves(top);
        let [disk, procs] = halves(bottom);
        draw_chart(f, temp, "Temperature", &[CPU_TEMP, GPU_TEMP], store, ui);
        draw_chart(f, net, "Network", &[NET_DOWN, NET_UP], store, ui);
        draw_chart(f, disk, "Disk", &[DISK_READ, DISK_WRITE], store, ui);
        draw_processes(f, procs, processes, ui);
    } else if rest.height >= 6 {
        let [temp, procs] = halves(rest);
        draw_chart(f, temp, "Temperature", &[CPU_TEMP, GPU_TEMP], store, ui);
        draw_processes(f, procs, processes, ui);
    }
}

fn draw_title(f: &mut Frame, area: Rect, ui: &Ui) {
    let t = &ui.theme;
    let left = Line::from(vec![
        Span::styled(" HardViz", Style::new().fg(tc(t.accent)).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {}  terminal", crate::update::VERSION), Style::new().fg(tc(t.text_muted))),
    ]);
    let mut right = Vec::new();
    if let Some(release) = &ui.update {
        // The release page when the line has room for it, else just the news.
        let long = format!("\u{2191} {} available: {}   ", release.version, release.url);
        let room = usize::from(area.width).saturating_sub(left.width() + 22);
        let notice =
            if long.chars().count() <= room { long } else { format!("\u{2191} {} available   ", release.version) };
        right.push(Span::styled(notice, Style::new().fg(tc(t.accent))));
    }
    right.push(Span::styled(
        format!("{} · {} ms · {} charts ", ui.theme_name.label(), ui.refresh_ms, span_label(ui.chart_span_secs * 1000)),
        Style::new().fg(tc(t.text_secondary)),
    ));
    let right = Line::from(right).right_aligned();
    f.render_widget(Paragraph::new(left), area);
    f.render_widget(Paragraph::new(right), area);
}

fn draw_footer(f: &mut Frame, area: Rect, ui: &Ui) {
    let t = &ui.theme;
    // The export outcome replaces the key help until the next key press.
    if let Some(note) = &ui.export_note {
        let line = Line::from(Span::styled(format!(" {note}"), Style::new().fg(tc(t.accent))));
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    let key = |k: &'static str| Span::styled(k, Style::new().fg(tc(t.accent)).add_modifier(Modifier::BOLD));
    let what = |w: &'static str| Span::styled(w, Style::new().fg(tc(t.text_muted)));
    let line = Line::from(vec![
        Span::raw(" "),
        key("q"),
        what(" quit   "),
        key("c g i m"),
        what(" sort by CPU/GPU/IO/mem   "),
        key("t"),
        what(" theme   "),
        key("w"),
        what(" chart span   "),
        key("+ -"),
        what(" refresh   "),
        key("e"),
        what(" export CSV"),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// A rounded panel with a title, in the theme's card colours.
fn panel<'a>(title: &'a str, t: &Theme) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(tc(t.border)))
        .title(Span::styled(format!(" {title} "), Style::new().fg(tc(t.text_secondary))))
        .style(Style::new().bg(tc(t.bg_widget)).fg(tc(t.text_primary)))
}

fn draw_gauges(f: &mut Frame, area: Rect, store: &MetricStore, ui: &Ui, wide: bool) {
    let quarter = Constraint::Ratio(1, 4);
    let half = Constraint::Ratio(1, 2);
    let cells: Vec<Rect> = if wide {
        Layout::horizontal([quarter; 4]).split(area).to_vec()
    } else {
        let [top, bottom] = Layout::vertical([half; 2]).areas(area);
        let mut v = Layout::horizontal([half; 2]).split(top).to_vec();
        v.extend(Layout::horizontal([half; 2]).split(bottom).iter());
        v
    };
    let t = &ui.theme;
    let temp =
        |id| store.current(id).map(|c| Span::styled(Unit::Celsius.format(c), Style::new().fg(tc(t.temp_color(c)))));
    let memory = |used, total| match (store.current(used), store.current(total)) {
        (Some(u), Some(tot)) => Some(Span::styled(
            format!("{} / {}", Unit::Megabytes.format(u).replace(" GB", ""), Unit::Megabytes.format(tot)),
            Style::new().fg(tc(t.text_secondary)),
        )),
        _ => None,
    };
    let panels = [
        ("CPU", CPU_USAGE, temp(CPU_TEMP)),
        ("RAM", RAM_USAGE, memory(RAM_USED, RAM_TOTAL)),
        ("GPU", GPU_USAGE, temp(GPU_TEMP)),
        ("VRAM", VRAM_USAGE, memory(VRAM_USED, VRAM_TOTAL)),
    ];
    for ((title, id, detail), cell) in panels.into_iter().zip(cells) {
        draw_gauge(f, cell, title, id, detail, store, t);
    }
}

/// One usage panel: the reading (and a detail such as temperature), a bar
/// coloured like the GUI gauge, and the recent history.
fn draw_gauge(
    f: &mut Frame,
    area: Rect,
    title: &str,
    id: MetricId,
    detail: Option<Span<'_>>,
    store: &MetricStore,
    t: &Theme,
) {
    let block = panel(title, t);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width < 4 {
        return;
    }
    let inner = inner.inner(ratatui::layout::Margin::new(1, 0));
    let [value_row, bar_row, history] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    let def = id.def();
    let mild = t.metric_color(def.kind);
    let range = def.scale.range(std::iter::empty());
    let Some(v) = store.current(id) else {
        f.render_widget(Paragraph::new(Span::styled("N/A", Style::new().fg(tc(t.text_muted)))), value_row);
        return;
    };
    let fill = to_f32(Scale::fraction(v, range));
    let tip = if def.high_is_bad() { t.severity_color(mild, fill) } else { mild };
    let mut spans = vec![Span::styled(def.unit.format(v), Style::new().fg(tc(tip)).add_modifier(Modifier::BOLD))];
    if let Some(d) = detail {
        spans.push(Span::raw("  "));
        spans.push(d);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), value_row);
    f.render_widget(Paragraph::new(bar(bar_row.width, fill, mild, def.high_is_bad(), t)), bar_row);
    draw_history(f, history, store.history(id), Some(range), mild);
}

/// A horizontal bar `width` cells long, filled to `fill` (0-1) with eighth-
/// cell precision. High-is-bad bars shade along their length like the GUI's.
fn bar(width: u16, fill: f32, mild: Color32, graded: bool, t: &Theme) -> Line<'static> {
    const PARTS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
    let fill = fill.clamp(0.0, 1.0);
    let eighths = to_u64(fill * f32::from(width) * 8.0);
    let track = tc(t.gauge_bg);
    let spans = (0..width)
        .map(|i| {
            let e = usize::try_from(eighths.saturating_sub(u64::from(i) * 8).min(8)).unwrap_or(8);
            let along = (f32::from(i) + 0.5) / (f32::from(width) * fill).max(1e-3);
            let c = if graded { t.severity_along(mild, fill, along) } else { mild };
            Span::styled(PARTS[e], Style::new().fg(tc(c)).bg(track))
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// The newest samples that fit, as a sparkline. With `range` the bars are on
/// that scale; without, they span the samples' own min to max, so small
/// changes in a clock or power reading stay visible.
fn draw_history(f: &mut Frame, area: Rect, history: &[f64], range: Option<(f64, f64)>, color: Color32) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let shown: Vec<f64> = history.iter().copied().filter(|v| !v.is_nan()).collect();
    let shown = &shown[shown.len().saturating_sub(usize::from(area.width))..];
    let (lo, hi) = range.unwrap_or_else(|| {
        let lo = shown.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = shown.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    });
    // 1..=100: a sample at the bottom of the range still shows a sliver.
    let data: Vec<u64> = shown.iter().map(|&v| 1 + to_u64(to_f32(Scale::fraction(v, (lo, hi)) * 99.0))).collect();
    f.render_widget(Sparkline::default().data(&data).max(100).style(Style::new().fg(tc(color))), area);
}

fn draw_sensors(f: &mut Frame, area: Rect, store: &MetricStore, ids: &[MetricId], t: &Theme) {
    let block = panel("Sensors", t);
    let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
    f.render_widget(block, area);
    // As many as fit with label and value unclipped ("CPU clock 4.90 GHz").
    let ids = &ids[..ids.len().min(usize::from(inner.width / SENSOR_MIN_WIDTH).max(1))];
    let n = u32::try_from(ids.len()).unwrap_or(1);
    let cols = Layout::horizontal(vec![Constraint::Ratio(1, n); ids.len()]).spacing(2).split(inner);
    for (&id, &col) in ids.iter().zip(cols.iter()) {
        let def = id.def();
        let [head, history] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(col);
        let value = store.current(id).map_or_else(|| "N/A".to_string(), |v| def.unit.format(v));
        let line = Line::from(vec![
            Span::styled(format!("{} ", def.label), Style::new().fg(tc(t.text_muted))),
            Span::styled(value, Style::new().add_modifier(Modifier::BOLD)),
        ]);
        f.render_widget(Paragraph::new(line), head);
        draw_history(f, history, store.history(id), None, t.metric_color(def.kind));
    }
}

/// Series colours as the GUI's charts use them.
fn series_colors(ids: &[MetricId], t: &Theme) -> Vec<Color32> {
    if ids == [NET_DOWN, NET_UP] {
        vec![t.accent, t.success]
    } else if ids == [DISK_READ, DISK_WRITE] {
        vec![t.metric_color(MetricKind::Storage), t.metric_color(MetricKind::Vram)]
    } else {
        ids.iter().map(|id| t.metric_color(id.def().kind)).collect()
    }
}

/// A line chart of `ids` over the chart span, legend showing current values.
fn draw_chart(f: &mut Frame, area: Rect, title: &str, ids: &[MetricId], store: &MetricStore, ui: &Ui) {
    let t = &ui.theme;
    let window = chart_samples(ui.chart_span_secs, ui.refresh_ms);
    let series: Vec<Vec<(f64, f64)>> = ids
        .iter()
        .map(|&id| {
            let h = store.long_history(id);
            let h = &h[h.len().saturating_sub(window)..];
            let start = window - h.len();
            h.iter().enumerate().filter(|(_, v)| !v.is_nan()).map(|(i, &v)| (count_f64(start + i), v)).collect()
        })
        .collect();
    let def = ids[0].def();
    let (lo, hi) = def.scale.range(series.iter().flatten().map(|p| p.1));
    let colors = series_colors(ids, t);
    let datasets: Vec<Dataset> = ids
        .iter()
        .zip(&series)
        .zip(&colors)
        .map(|((&id, data), &c)| {
            let now = store.current(id).map_or_else(|| "N/A".into(), |v| id.def().unit.format(v));
            Dataset::default()
                .name(format!("{} {now}", id.def().label))
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(tc(c)))
                .data(data)
        })
        .collect();
    let muted = Style::new().fg(tc(t.text_muted));
    let x = Axis::default()
        .bounds([0.0, count_f64(window - 1)])
        .labels([format!("-{}", span_label(ui.chart_span_secs * 1000)), "now".to_string()])
        .style(muted);
    let y = Axis::default()
        .bounds([lo, hi])
        .labels([def.unit.format(lo), def.unit.format(f64::midpoint(lo, hi)), def.unit.format(hi)])
        .style(muted);
    let chart = Chart::new(datasets)
        .block(panel(title, t))
        // The plot area and legend otherwise reset to the terminal's colours.
        .style(Style::new().bg(tc(t.bg_widget)).fg(tc(t.text_primary)))
        .x_axis(x)
        .y_axis(y)
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 1), Constraint::Ratio(1, 1)));
    f.render_widget(chart, area);
}

/// One process-table column.
struct Column {
    title: &'static str,
    key: Option<SortKey>,
    width: u16,
    cell: fn(&ProcRow) -> String,
}

const COLUMNS: [Column; 4] = [
    Column { title: "CPU", key: Some(SortKey::Cpu), width: 7, cell: |r| format!("{:.1}%", r.cpu) },
    Column {
        title: "GPU",
        key: Some(SortKey::Gpu),
        width: 5,
        cell: |r| r.gpu.map_or_else(|| "–".into(), |g| format!("{g:.0}%")),
    },
    Column { title: "I/O", key: Some(SortKey::Io), width: 10, cell: |r| crate::metrics::format_rate(r.io) },
    Column { title: "MEMORY", key: Some(SortKey::Memory), width: 9, cell: |r| Unit::Megabytes.format(r.mem_mb) },
];

/// Room the process name needs before value columns are dropped.
const NAME_MIN: u16 = 14;

fn draw_processes(f: &mut Frame, area: Rect, processes: &[ProcRow], ui: &Ui) {
    let t = &ui.theme;
    let mut rows = processes.to_vec();
    rank(&mut rows, ui.sort);
    // Drop GPU, then I/O, while the name would be squeezed; never the
    // column being sorted by.
    let inner = area.width.saturating_sub(2);
    let fits = |cols: &[&Column]| NAME_MIN + cols.iter().map(|c| c.width + 1).sum::<u16>() <= inner;
    let mut cols: Vec<&Column> = COLUMNS.iter().collect();
    for drop in ["GPU", "I/O"] {
        if !fits(&cols) {
            cols.retain(|c| c.title != drop || c.key == Some(ui.sort));
        }
    }
    let head = |c: &Column| {
        let sorted = c.key == Some(ui.sort);
        let text = if sorted { format!("{} ↓", c.title) } else { c.title.to_string() };
        let style = if sorted { Style::new().fg(tc(t.accent)) } else { Style::new().fg(tc(t.text_muted)) };
        Cell::from(Line::from(text).right_aligned()).style(style)
    };
    let mut header = vec![Cell::from("PROCESS").style(Style::new().fg(tc(t.text_muted)))];
    header.extend(cols.iter().map(|c| head(c)));
    let body = rows.iter().map(|r| {
        let name = if r.count > 1 { format!("{} ×{}", r.name, r.count) } else { r.name.clone() };
        let mut cells = vec![Cell::from(name)];
        cells.extend(cols.iter().map(|c| Cell::from(Line::from((c.cell)(r)).right_aligned())));
        Row::new(cells)
    });
    let mut widths = vec![Constraint::Fill(1)];
    widths.extend(cols.iter().map(|c| Constraint::Length(c.width)));
    let table = Table::new(body, widths).header(Row::new(header)).column_spacing(1).block(panel("Top processes", t));
    f.render_widget(table, area);
}
