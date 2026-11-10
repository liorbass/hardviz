//! The widget plugin interface.
//!
//! A widget declares which metrics it needs and renders them from the store. It
//! never touches a data source directly, so any widget works with any source
//! that supplies its metrics.
//!
//! Most widgets need no code: a chart, gauge or sensor strip for any metrics
//! is one entry in [`crate::widgets::default_registry`] using the configurable
//! widgets (`LineChartWidget`, `MetricWidget`). Write a module
//! under `widgets/` only for a genuinely new visualisation, then register it
//! there.
//!
//! The dashboard owns everything common to all widgets - the card and title,
//! sizing, and the N/A placeholder when none of a widget's metrics has data -
//! so [`Widget::draw`] only paints content into the area it is given.

use crate::config::Visual;
use crate::metrics::{MetricId, MetricStore};
use crate::poller::Diagnoses;
use crate::processes::ProcRow;
use crate::source::MetricDiagnosis;
use crate::theme::Theme;
use egui::Ui;

/// Everything a widget needs to render one frame.
///
/// Two deliberate omissions:
/// - No `dt`: value smoothing is centralised in [`MetricStore::advance`] so every
///   widget eases identically. Read eased values with [`WidgetCtx::value`].
/// - No `width`: a widget sizes itself from `ui.available_width()` *inside*
///   its own card. A grid cell's width includes the card margins, so a widget
///   allocating it would overflow the cell.
#[non_exhaustive]
pub struct WidgetCtx<'a> {
    pub theme: &'a Theme,
    /// Interval between two samples. Lets a widget state its time window
    /// accurately (e.g. a 300-sample sparkline at 1000ms = "5 min") instead of
    /// hardcoding a duration that silently lies when the user changes the rate.
    pub sample_interval_ms: u64,
    /// Top processes, newest sample. Empty unless a visible widget's
    /// [`Widget::wants_processes`] is true, since sampling them is costly.
    pub processes: &'a [ProcRow],
    store: &'a MetricStore,
    diagnostics: &'a Diagnoses,
}

impl<'a> WidgetCtx<'a> {
    #[must_use]
    pub fn new(
        store: &'a MetricStore,
        diagnostics: &'a Diagnoses,
        processes: &'a [ProcRow],
        theme: &'a Theme,
        sample_interval_ms: u64,
    ) -> Self {
        Self { theme, sample_interval_ms, processes, store, diagnostics }
    }

    /// Eased value for a metric, or `None` when unavailable.
    #[must_use]
    pub fn value(&self, id: MetricId) -> Option<f64> {
        self.store.smoothed(id)
    }

    /// Raw value for a metric, or `None` when unavailable.
    #[must_use]
    pub fn raw(&self, id: MetricId) -> Option<f64> {
        self.store.current(id)
    }

    /// A metric's history, oldest first; `NaN` marks a missing sample.
    #[must_use]
    pub fn history(&self, id: MetricId) -> &'a [f64] {
        self.store.history(id)
    }

    /// Whether any sample in the metric's history is a real reading.
    #[must_use]
    pub fn has_history(&self, id: MetricId) -> bool {
        self.history(id).iter().any(|v| !v.is_nan())
    }

    /// Why a metric is unavailable, for tooltips and placeholder text.
    #[must_use]
    pub fn diagnosis(&self, id: MetricId) -> Option<&'a MetricDiagnosis> {
        self.diagnostics.get(&id)
    }
}

/// How much horizontal space a widget wants. The dashboard groups widgets by
/// size class so gauges tile tightly while charts get room to breathe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetSize {
    /// Small, roughly square — gauges.
    Small,
    /// Half-width or wider — charts.
    Large,
    /// A full-width row: short horizontal widgets (bars). A row of their own
    /// keeps them from sharing one with a tall chart and leaving empty space
    /// under the short card.
    Wide,
}

impl WidgetSize {
    /// Default width in quarters of the dashboard (the grid has 4 columns).
    #[must_use]
    pub fn default_span(self) -> u8 {
        match self {
            WidgetSize::Small => 1,
            WidgetSize::Large => 2,
            WidgetSize::Wide => 4,
        }
    }
}

/// How a widget's card is sized by default. The user can resize a card;
/// widgets must draw into whatever size they are given.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Card {
    pub size: WidgetSize,
    /// Default card height (whole card, including its title).
    pub height: f32,
    /// Smallest height the widget stays legible at; Fit to window never
    /// shrinks below it.
    pub min_height: f32,
}

impl Card {
    #[must_use]
    pub const fn new(size: WidgetSize, height: f32, min_height: f32) -> Self {
        Self { size, height, min_height }
    }
}

/// A renderable panel.
///
/// The stable `type_id` used for persistence lives on the [`WidgetFactory`] that
/// builds the widget, not on the instance — the registry owns that mapping, so
/// implementations do not have to repeat it.
pub trait Widget: Send {
    /// Title shown in the card header.
    fn title(&self) -> &str;

    /// Metrics this widget reads. Drives the automatic N/A placeholder, and is
    /// surfaced in Settings so dependencies are inspectable at runtime.
    fn requires(&self) -> &[MetricId];

    /// Default width class and heights of this widget's card.
    fn card(&self) -> Card;

    /// Whether there is anything to draw. When false, the dashboard shows an
    /// N/A placeholder with the reason instead of calling [`Widget::draw`].
    /// Default: some required metric has data in its history. A widget with
    /// no required metrics is always drawn.
    fn has_data(&self, ctx: &WidgetCtx) -> bool {
        let req = self.requires();
        req.is_empty() || req.iter().any(|&id| ctx.has_history(id))
    }

    /// Paint the card's content into `ui`, which is exactly the area under the
    /// title; `height` is its height. Size everything from that area.
    fn draw(&mut self, ui: &mut Ui, ctx: &WidgetCtx, height: f32);

    /// The visual a single-metric widget currently uses; `None` for widgets
    /// that offer no choice. Offered in Edit layout and the card's context menu.
    fn visual(&self) -> Option<Visual> {
        None
    }

    /// Switch visual. Only called on widgets whose [`Widget::visual`] is `Some`.
    fn set_visual(&mut self, _visual: Visual) {}

    /// Whether this widget reads [`WidgetCtx::processes`]. Process sampling
    /// runs only while at least one visible widget returns true.
    fn wants_processes(&self) -> bool {
        false
    }
}

/// How to build one widget type. Kept as data so the registry can enumerate
/// available widgets for the "add widget" UI.
#[non_exhaustive]
pub struct WidgetFactory {
    /// Stable id persisted in the config. Never change it for an existing
    /// widget, or saved layouts lose that card.
    pub type_id: &'static str,
    pub display_name: &'static str,
    /// Part of the layout on a fresh install.
    pub default: bool,
    pub build: fn() -> Box<dyn Widget>,
}

/// Every widget type the build knows, in default layout order.
pub struct WidgetRegistry {
    factories: Vec<WidgetFactory>,
}

impl WidgetRegistry {
    #[must_use]
    pub fn new(factories: Vec<WidgetFactory>) -> Self {
        debug_assert!(
            factories.iter().enumerate().all(|(i, f)| factories[..i].iter().all(|g| g.type_id != f.type_id)),
            "duplicate widget type_id"
        );
        Self { factories }
    }

    #[must_use]
    pub fn factories(&self) -> &[WidgetFactory] {
        &self.factories
    }

    fn factory(&self, type_id: &str) -> Option<&WidgetFactory> {
        self.factories.iter().find(|f| f.type_id == type_id)
    }

    #[must_use]
    pub fn build(&self, type_id: &str) -> Option<Box<dyn Widget>> {
        self.factory(type_id).map(|f| (f.build)())
    }

    /// Display name of a registered type; `None` for an unknown id.
    #[must_use]
    pub fn name_of(&self, type_id: &str) -> Option<&'static str> {
        self.factory(type_id).map(|f| f.display_name)
    }

    /// Type ids placed on a fresh install, in order.
    #[must_use]
    pub fn default_layout(&self) -> Vec<&'static str> {
        self.factories.iter().filter(|f| f.default).map(|f| f.type_id).collect()
    }
}
