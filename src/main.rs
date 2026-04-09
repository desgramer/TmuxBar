#![allow(dead_code)]

mod app;
mod core;
mod infra;
mod models;
mod ui;

fn main() {
    // Initialize tracing subscriber for structured logging.
    tracing_subscriber::fmt::init();

    app::run();
}
