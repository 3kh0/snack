#![allow(dead_code, unused_imports)]

mod app;
mod auth;
mod cache;
mod config;
mod error;
mod slack;
mod state;
mod ui;

pub fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("snack=info")),
        )
        .init();

    if std::env::var_os("SNACK_AUTH").is_some() {
        match auth::login() {
            Ok(session) => {
                if let Err(e) = config::save_session(&session) {
                    eprintln!("snack auth: failed to save session: {e}");
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("snack auth: {e}");
                std::process::exit(1);
            }
        }
    }

    app::run()
}
