// Prevents terminal window in release mode on Windows:
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

fn main() {
    firefox_session_ui_gtk4::start();
}
