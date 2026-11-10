//! `HardViz`: a hardware monitoring dashboard.
//!
//! The binaries only call into this library, so it can be tested end to end
//! (`tests/`): `hardviz` (`main.rs`) runs [`startup::run`], the window;
//! `hardviz-tui` (`bin/hardviz-tui.rs`) runs [`tui::run`], the terminal
//! dashboard.
//!
//! Data flows one way: [`sources`] produce readings for [`metrics`] ids, the
//! [`poller`] thread resolves them by priority ([`source`]) into the store,
//! and [`widgets`] draw what the store holds, inside the [`app`] shell. See
//! `docs/architecture.md`.

pub mod app;
/// The build script's copy rules, compiled here so `cargo test` covers them.
#[cfg(test)]
mod bundle;
pub mod config;
pub mod log;
pub mod metrics;
pub mod num;
pub mod nvidia;
pub mod poller;
pub mod processes;
pub mod source;
pub mod sources;
pub mod startup;
pub mod theme;
pub mod tui;
pub mod update;
pub mod widget;
pub mod widgets;
