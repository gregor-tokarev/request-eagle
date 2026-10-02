// Release builds open without a console window on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod application;
mod assets;
mod menu;
mod window_options;

fn main() {
    application::run();
}
