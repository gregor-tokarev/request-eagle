mod actions;
mod application;
mod assets;
mod menu;
mod window_options;

#[cfg(test)]
mod actions_tests;

fn main() {
    application::run();
}
