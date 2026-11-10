// Release builds on Windows run as a GUI app with no console window. Debug
// builds keep the console for stderr logging; the log file covers release.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    hardviz::startup::run()
}
