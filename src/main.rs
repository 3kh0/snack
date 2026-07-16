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

    if let Some(channel) = std::env::var_os("SNACK_HUDDLE_CAPTURE") {
        let channel = channel.to_string_lossy().into_owned();
        std::process::exit(run_huddle_capture(&channel));
    }

    if std::env::var_os("SNACK_HUDDLE_TRACE").is_some() {
        std::process::exit(run_huddle_trace());
    }

    if let Some(channel) = std::env::var_os("SNACK_HUDDLE_JOIN") {
        let channel = channel.to_string_lossy().into_owned();
        std::process::exit(run_huddle_join(&channel));
    }

    if std::env::var_os("SNACK_HUDDLE_WEBVIEW").is_some() {
        match auth::capture_huddle_webview() {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("snack huddle-webview: {e}");
                std::process::exit(1);
            }
        }
    }

    if std::env::var_os("SNACK_AUTH").is_some() {
        let add_account = std::env::var_os("SNACK_AUTH_ADD").is_some();
        match auth::login(add_account) {
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

fn run_huddle_capture(channel: &str) -> i32 {
    let session = match config::load_session() {
        Ok(Some(session)) => session,
        Ok(None) => {
            eprintln!("snack huddle-capture: no saved session; run SNACK_AUTH=1 snack first");
            return 1;
        }
        Err(e) => {
            eprintln!("snack huddle-capture: failed to load session: {e}");
            return 1;
        }
    };

    let workspace = match std::env::var("SNACK_HUDDLE_TEAM") {
        Ok(team) => session.workspaces.get(&team),
        Err(_) => session.workspaces.values().next(),
    };
    let Some(workspace) = workspace.cloned() else {
        eprintln!("snack huddle-capture: no matching workspace in session");
        return 1;
    };

    let transport = match slack::Transport::new(session.d_cookie.clone()) {
        Ok(transport) => transport,
        Err(e) => {
            eprintln!("snack huddle-capture: transport init failed: {e}");
            return 1;
        }
    };
    let client = slack::SlackClient::default();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("snack huddle-capture: runtime init failed: {e}");
            return 1;
        }
    };

    match runtime.block_on(slack::huddle_api::capture_channel_huddle(
        &transport,
        &client,
        &workspace,
        channel.to_owned(),
    )) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("snack huddle-capture: {e}");
            1
        }
    }
}

fn run_huddle_join(channel: &str) -> i32 {
    let session = match config::load_session() {
        Ok(Some(session)) => session,
        Ok(None) => {
            eprintln!("snack huddle-join: no saved session; run SNACK_AUTH=1 snack first");
            return 1;
        }
        Err(e) => {
            eprintln!("snack huddle-join: failed to load session: {e}");
            return 1;
        }
    };

    let workspace = match std::env::var("SNACK_HUDDLE_TEAM") {
        Ok(team) => session.workspaces.get(&team),
        Err(_) => session.workspaces.values().next(),
    };
    let Some(workspace) = workspace.cloned() else {
        eprintln!("snack huddle-join: no matching workspace in session");
        return 1;
    };

    let transport = match slack::Transport::new(session.d_cookie.clone()) {
        Ok(transport) => transport,
        Err(e) => {
            eprintln!("snack huddle-join: transport init failed: {e}");
            return 1;
        }
    };
    let client = slack::SlackClient::default();
    let room_override = std::env::var("SNACK_HUDDLE_ROOM").ok();
    let secs = std::env::var("SNACK_HUDDLE_JOIN_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);
    let user_agent = slack::xparams::Identity::from_capture().user_agent;

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("snack huddle-join: runtime init failed: {e}");
            return 1;
        }
    };

    match runtime.block_on(slack::huddle_api::capture_rooms_join(
        &transport,
        &client,
        &workspace,
        &session.d_cookie,
        &user_agent,
        channel,
        room_override,
        std::time::Duration::from_secs(secs),
    )) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("snack huddle-join: {e}");
            1
        }
    }
}

fn run_huddle_trace() -> i32 {
    let session = match config::load_session() {
        Ok(Some(session)) => session,
        Ok(None) => {
            eprintln!("snack huddle-trace: no saved session; run SNACK_AUTH=1 snack first");
            return 1;
        }
        Err(e) => {
            eprintln!("snack huddle-trace: failed to load session: {e}");
            return 1;
        }
    };

    let workspace = match std::env::var("SNACK_HUDDLE_TEAM") {
        Ok(team) => session.workspaces.get(&team),
        Err(_) => session.workspaces.values().next(),
    };
    let Some(workspace) = workspace.cloned() else {
        eprintln!("snack huddle-trace: no matching workspace in session");
        return 1;
    };

    let secs = std::env::var("SNACK_HUDDLE_TRACE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(90);
    let user_agent = slack::xparams::Identity::from_capture().user_agent;

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("snack huddle-trace: runtime init failed: {e}");
            return 1;
        }
    };

    match runtime.block_on(slack::huddle_api::trace_realtime(
        &workspace,
        &session.d_cookie,
        &user_agent,
        std::time::Duration::from_secs(secs),
    )) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("snack huddle-trace: {e}");
            1
        }
    }
}
