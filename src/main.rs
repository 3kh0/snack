#![allow(dead_code, unused_imports)]

mod app;
mod config;
mod error;
mod slack;

pub fn main() -> iced::Result {
    app::run()
}
