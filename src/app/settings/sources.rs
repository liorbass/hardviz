//! Settings -> Data sources: each source's state and toggle, and on Windows
//! the `LibreHardwareMonitor` startup task.

use super::super::style::section_label;
use super::{HardVizApp, SettingsAction};
use crate::source::{SourceInfo, SourceStatus};
use eframe::egui;
#[cfg(windows)]
use std::fmt::Write as _;

impl HardVizApp {
    pub(super) fn settings_sources(
        &self,
        ui: &mut egui::Ui,
        infos: &[SourceInfo],
        action: &mut Option<SettingsAction>,
    ) {
        section_label(ui, &self.theme, "Data sources");
        egui::Grid::new("sources_grid").num_columns(3).spacing([6.0, 4.0]).show(ui, |ui| {
            for info in infos {
                let mut on = info.enabled;
                if ui.checkbox(&mut on, "").on_hover_text("Enable or disable this source").changed() {
                    *action = Some(SettingsAction::ToggleSource(info.id, on));
                }
                let online = info.status.is_online();
                ui.colored_label(if online { self.theme.success } else { self.theme.danger }, "\u{25CF}");
                // Name on top, status underneath: keeps the column narrow.
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(info.display_name).size(self.theme.fs(11.0)).color(if online {
                        self.theme.text_primary
                    } else {
                        self.theme.text_muted
                    }))
                    .on_hover_text(format!(
                        "provides: {}",
                        info.provides.iter().map(|id| id.key()).collect::<Vec<_>>().join(", ")
                    ));
                    ui.label(
                        egui::RichText::new(status_line(&info.status))
                            .size(self.theme.fs(9.5))
                            .color(self.theme.text_muted),
                    )
                    .on_hover_text(info.status.to_string());
                });
                ui.end_row();
            }
        });
        #[cfg(windows)]
        self.lhm_task_row(ui);
    }

    /// The `LibreHardwareMonitor` startup task: its state, a button to set it
    /// up or remove it, and a warning when it would elevate a file the user's
    /// own programs could replace.
    #[cfg(windows)]
    fn lhm_task_row(&self, ui: &mut egui::Ui) {
        use crate::sources::lhm::task::{is_protected_location, TaskState};
        let theme = &self.theme;
        let status = self.lhm_task.status();
        let small = |text: &str, color| egui::RichText::new(text).size(theme.fs(10.0)).color(color);

        ui.horizontal_wrapped(|ui| {
            let label = match (&status.task, status.busy) {
                (Some(TaskState::Current), _) => "Startup task: LibreHardwareMonitor starts at login, no UAC prompts",
                (Some(TaskState::Stale { .. }), _) => {
                    "Startup task: set up for another copy of LibreHardwareMonitor (not used)"
                }
                (Some(TaskState::Missing), _) => {
                    "Startup task: not set up (LibreHardwareMonitor asks for admin rights each time)"
                }
                (None, true) => "Startup task: checking\u{2026}",
                (None, false) => "Startup task: unavailable",
            };
            ui.label(small(label, theme.text_secondary));
            if status.busy {
                ui.spinner();
                return;
            }
            let Some(task) = &status.task else { return };
            if *task == TaskState::Current {
                if ui
                    .small_button("Remove")
                    .on_hover_text("Delete the scheduled task (one administrator prompt)")
                    .clicked()
                {
                    self.lhm_task.uninstall();
                }
                return;
            }
            let mut tip = String::from(
                "Create a scheduled task that starts LibreHardwareMonitor with admin rights at login \
                 (one administrator prompt, then never again)",
            );
            if let TaskState::Stale { command } = task {
                let _ = write!(tip, "\n\nThe existing task runs {command}; this replaces it.");
            }
            if ui.small_button("Set up").on_hover_text(tip).clicked() {
                self.lhm_task.install();
            }
        });
        if let (Some(Ok(exe)), Some(_)) = (&status.bundle, &status.task) {
            if !is_protected_location(exe) {
                ui.label(small(
                    "Note: LibreHardwareMonitor is in a folder your account can change. With the startup \
                     task set up, a program running as you could replace it and get administrator rights \
                     at login. Installing HardViz under Program Files avoids this.",
                    theme.warning,
                ))
                .on_hover_text(exe.display().to_string());
            }
        }
        if let Some(m) = &status.message {
            ui.label(small(m, theme.danger));
        }
    }
}

/// One short line for a source's status: `online`, or the first line of its
/// reason, clipped by characters (reasons contain multi-byte text).
fn status_line(status: &SourceStatus) -> String {
    match status {
        SourceStatus::Online => "online".to_string(),
        SourceStatus::Unavailable { reason } => {
            crate::widgets::clip_chars(reason.lines().next().unwrap_or("unavailable"), 44)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_is_short_and_char_safe() {
        assert_eq!(status_line(&SourceStatus::Online), "online");
        let long = SourceStatus::Unavailable { reason: format!("{}\nsecond line", "—".repeat(60)) };
        let line = status_line(&long);
        assert_eq!(line.chars().count(), 44);
        assert!(!line.contains("second"));
    }
}
