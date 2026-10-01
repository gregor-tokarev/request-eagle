mod actions;
mod application;
mod assets;
mod logs;
mod menu;
mod window_options;

#[cfg(test)]
mod actions_tests;
#[cfg(test)]
mod logs_tests;

fn main() {
    logs::init();
    application::run();
}
