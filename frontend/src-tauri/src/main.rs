#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // The logger is installed by tauri-plugin-log inside `app_lib::run()`, which
    // writes to a file as well as to stdout. Installing one here instead would
    // claim the global logger first, the plugin would fail to register its own,
    // and a packaged app — whose stdout goes nowhere — would leave no trace at
    // all. That is exactly how a whole debugging session got lost.
    app_lib::run();
}
