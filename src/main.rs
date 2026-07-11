#![allow(dead_code, unused_imports)]

mod app;
mod auth;
mod cache;
mod config;
mod error;
#[cfg(target_os = "macos")]
mod macos;
mod slack;
mod state;
mod ui;
#[cfg(target_os = "windows")]
mod windows;

pub fn main() -> iced::Result {
    #[cfg(target_os = "macos")]
    if let Err(error) = macos::ensure_app_bundle() {
        eprintln!("snack: {error}");
        std::process::exit(1);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("snack=info")),
        )
        .init();

    #[cfg(target_os = "windows")]
    if let Err(error) = windows::ensure_notification_identity() {
        tracing::warn!(%error, "could not register Windows notification identity");
    }

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
