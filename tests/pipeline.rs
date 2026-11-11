//! End to end: sources -> poller -> store and diagnoses -> what a widget sees,
//! and the contract between the saved config and the widget registry.

use hardviz::config::{AppConfig, WidgetTypes};
use hardviz::metrics::metric::{CPU_TEMP, CPU_USAGE};
use hardviz::metrics::MetricId;
use hardviz::poller::Poller;
use hardviz::source::{MetricSource, Readings, SourceStatus};
use hardviz::theme::ThemeName;
use hardviz::widget::WidgetCtx;
use hardviz::widgets::default_registry;
use std::time::{Duration, Instant};

/// Reports fixed values for the metrics it is given.
struct Fixed {
    id: &'static str,
    provides: &'static [MetricId],
    value: f64,
    online: bool,
}

impl MetricSource for Fixed {
    fn id(&self) -> &'static str {
        self.id
    }
    fn display_name(&self) -> &'static str {
        self.id
    }
    fn provides(&self) -> &'static [MetricId] {
        self.provides
    }
    fn poll(&mut self, out: &mut Readings) {
        for &id in self.provides {
            out.set(id, self.value);
        }
    }
    fn status(&self) -> SourceStatus {
        if self.online {
            SourceStatus::Online
        } else {
            SourceStatus::Unavailable { reason: "sensor unplugged".into() }
        }
    }
}

fn sources() -> Vec<Box<dyn MetricSource>> {
    vec![
        // Highest priority, but offline: its CPU temperature must not be used.
        Box::new(Fixed { id: "offline", provides: &[CPU_TEMP], value: 99.0, online: false }),
        Box::new(Fixed { id: "primary", provides: &[CPU_TEMP], value: 55.0, online: true }),
        Box::new(Fixed { id: "fallback", provides: &[CPU_TEMP, CPU_USAGE], value: 20.0, online: true }),
    ]
}

fn wait_for(poller: &Poller, samples: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while poller.poll_count() < samples {
        assert!(Instant::now() < deadline, "poller produced no samples");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn widgets_see_the_highest_priority_live_value_and_its_explanation() {
    let poller = Poller::spawn(sources, &[], 50);
    wait_for(&poller, 2);
    let state = poller.state();
    let view = state.lock().expect("shared state");
    let theme = ThemeName::MidnightDark.theme();
    let ctx = WidgetCtx::new(&view.store, &view.diagnostics, &view.processes, &view.cores, &theme, 50);

    assert_eq!(ctx.raw(CPU_TEMP), Some(55.0), "offline source skipped, primary wins over fallback");
    assert_eq!(ctx.raw(CPU_USAGE), Some(20.0), "fallback fills what primary lacks");
    assert!(ctx.has_history(CPU_TEMP));
    let diagnosis = ctx.diagnosis(CPU_TEMP).expect("diagnosed");
    assert_eq!(diagnosis.provider, Some("primary"));
    assert!(diagnosis.explain().contains("offline: sensor unplugged"));
}

#[test]
fn disabling_the_primary_source_falls_back_without_a_restart() {
    let poller = Poller::spawn(sources, &["primary".to_string()], 50);
    wait_for(&poller, 1);
    let read = || poller.state().lock().and_then(|v| v.store.current(CPU_TEMP));
    assert_eq!(read(), Some(20.0));

    assert!(poller.set_source_enabled("primary", true));
    let start = poller.poll_count();
    wait_for(&poller, start + 2);
    assert_eq!(read(), Some(55.0));
}

#[test]
fn default_layout_only_uses_registered_widget_types() {
    let registry = default_registry();
    let known: Vec<&str> = registry.factories().iter().map(|f| f.type_id).collect();
    let defaults = registry.default_layout();
    assert_ne!(defaults, [] as [&str; 0]);
    let types = WidgetTypes::new(&known, &defaults);

    let fresh = AppConfig::with_default_widgets(&defaults);
    for w in &fresh.widgets {
        assert!(registry.build(&w.type_id).is_some(), "{} is not buildable", w.type_id);
    }
    // A saved config survives a round trip through the registry's filter.
    let json = serde_json::to_string(&fresh).expect("serialises");
    assert_eq!(AppConfig::from_json(&json, &types).expect("parses").widgets, fresh.widgets);
}

#[test]
fn every_widget_declares_metrics_from_the_catalogue() {
    for f in default_registry().factories() {
        let widget = (f.build)();
        assert!(!widget.title().is_empty(), "{}", f.type_id);
        let card = widget.card();
        assert!(card.min_height <= card.height, "{}", f.type_id);
        for id in widget.requires() {
            assert!(!id.def().label.is_empty(), "{} requires {id}", f.type_id);
        }
    }
}
