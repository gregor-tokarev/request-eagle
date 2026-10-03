// Release builds open without a console window on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod application;
mod assets;
mod logs;
mod menu;
mod window_options;

#[cfg(test)]
mod logs_tests;

fn main() {
    logs::init();
    application::run();
}
