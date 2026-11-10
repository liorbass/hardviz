//! A card for one metric whose visual the user picks: gauge, bar, line chart
//! or tile ([`Visual`]). It delegates to the widget for the chosen visual and
//! rebuilds it when the choice changes.

use crate::config::Visual;
use crate::metrics::MetricId;
use crate::widget::{Card, Widget, WidgetCtx};
use crate::widgets::bar::BarWidget;
use crate::widgets::gauge::{Detail, GaugeWidget};
use crate::widgets::line_chart::LineChartWidget;
use crate::widgets::tile::TileWidget;
use egui::Ui;

pub struct MetricWidget {
    metric: MetricId,
    visual: Visual,
    /// Line under a gauge's value.
    detail: Option<Detail>,
    inner: Box<dyn Widget>,
}

impl MetricWidget {
    /// `metric`, shown as `visual` until the user picks another.
    #[must_use]
    pub fn new(metric: MetricId, visual: Visual) -> Self {
        Self { metric, visual, detail: None, inner: build(metric, visual, None) }
    }

    /// The gauge's secondary line (see [`GaugeWidget::detail`]).
    #[must_use]
    pub fn detail(mut self, detail: Detail) -> Self {
        self.detail = Some(detail);
        self.inner = build(self.metric, self.visual, self.detail);
        self
    }
}

fn build(metric: MetricId, visual: Visual, detail: Option<Detail>) -> Box<dyn Widget> {
    match visual {
        Visual::Gauge => {
            let gauge = GaugeWidget::new(metric);
            Box::new(match detail {
                Some(d) => gauge.detail(d),
                None => gauge,
            })
        }
        Visual::Bar => Box::new(BarWidget::new(metric)),
        Visual::Chart => Box::new(LineChartWidget::new(metric.def().label, &[metric])),
        Visual::Tile => Box::new(TileWidget::new(metric)),
    }
}

impl Widget for MetricWidget {
    fn title(&self) -> &str {
        self.inner.title()
    }

    fn requires(&self) -> &[MetricId] {
        std::slice::from_ref(&self.metric)
    }

    fn card(&self) -> Card {
        self.inner.card()
    }

    fn has_data(&self, ctx: &WidgetCtx) -> bool {
        self.inner.has_data(ctx)
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32) {
        self.inner.draw(ui, ctx, height);
    }

    fn visual(&self) -> Option<Visual> {
        Some(self.visual)
    }

    fn set_visual(&mut self, visual: Visual) {
        if visual != self.visual {
            self.visual = visual;
            self.inner = build(self.metric, visual, self.detail);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::metric::{CPU_TEMP, CPU_USAGE};
    use crate::widget::WidgetSize;

    #[test]
    fn switching_visual_changes_the_card() {
        let mut w = MetricWidget::new(CPU_USAGE, Visual::Gauge);
        assert_eq!(w.visual(), Some(Visual::Gauge));
        assert_eq!(w.card().size, WidgetSize::Small);
        w.set_visual(Visual::Chart);
        assert_eq!(w.visual(), Some(Visual::Chart));
        assert_eq!(w.card().size, WidgetSize::Large, "a chart wants room");
        assert_eq!(w.requires(), [CPU_USAGE]);
    }

    #[test]
    fn every_visual_is_titled_by_the_metric() {
        let mut w = MetricWidget::new(CPU_TEMP, Visual::Tile);
        for v in Visual::ALL {
            w.set_visual(v);
            assert_eq!(w.title(), CPU_TEMP.def().label, "{v:?}");
        }
    }
}
