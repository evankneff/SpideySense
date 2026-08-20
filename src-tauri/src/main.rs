// Release builds have no console window - the app lives entirely in the tray.
// Debug builds keep one so `cargo tauri dev` shows the log stream.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    frigate_popup_lib::run()
}
