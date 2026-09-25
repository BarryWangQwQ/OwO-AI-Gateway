// No console window next to the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = owo_desktop_lib::cli_mode() {
        std::process::exit(code);
    }
    owo_desktop_lib::run();
}
