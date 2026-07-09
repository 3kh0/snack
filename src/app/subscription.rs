use std::time::Duration;

use iced::Subscription;

use crate::slack::models::TeamId;
use crate::slack::realtime::{self, ConnectParams, RtUpdate};

use super::{App, Message};

pub(super) fn subscription(app: &App) -> Subscription<Message> {
    let needs_tick =
        !app.cache_dirty.is_empty() || app.workspaces.values().any(|ws| !ws.typing.is_empty());
    let mut subs = Vec::new();
    if needs_tick {
        subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick));
    }

    subs.push(iced::event::listen_with(palette_hotkey));
    if app.palette_open {
        subs.push(iced::event::listen_with(palette_navigation));
    }
    if app
        .emoji_previews
        .values()
        .any(|preview| matches!(preview, super::FilePreview::Animated { .. }))
    {
        subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::AnimationTick));
    }

    if app.sidebar_resizing {
        subs.push(iced::event::listen_with(
            |event, _status, _id| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::SidebarResizeMoved(position.x))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::SidebarResizeEnded),
                _ => None,
            },
        ));
    }

    if let Some(session) = &app.session {
        let user_agent = crate::slack::xparams::Identity::from_capture().user_agent;
        for ws in session.workspaces.values() {
            let params = ConnectParams {
                team: ws.team_id.clone(),
                ws_url: realtime::flannel_url(&ws.token, &ws.team_id),
                d_cookie: session.d_cookie.clone(),
                user_agent: user_agent.clone(),
            };
            subs.push(realtime::connect(params).map(map_rt_update));
        }
    }

    Subscription::batch(subs)
}

fn palette_hotkey(
    event: iced::Event,
    _status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{Event, Key};
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    match key {
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("k") && modifiers.command() => {
            Some(Message::PaletteToggled)
        }
        _ => None,
    }
}

fn palette_navigation(
    event: iced::Event,
    _status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Event, Key};
    let iced::Event::Keyboard(Event::KeyPressed { key, .. }) = event else {
        return None;
    };
    match key {
        Key::Named(Named::ArrowUp) => Some(Message::PaletteMoved(-1)),
        Key::Named(Named::ArrowDown) => Some(Message::PaletteMoved(1)),
        Key::Named(Named::Escape) => Some(Message::PaletteClosed),
        _ => None,
    }
}

fn map_rt_update((team, update): (TeamId, RtUpdate)) -> Message {
    match update {
        RtUpdate::Connected {
            generation,
            connection,
        } => Message::RtConnected(team, generation, connection),
        RtUpdate::Event { generation, event } => Message::Realtime(team, generation, event),
        RtUpdate::Disconnected { generation } => Message::RtDisconnected(team, generation),
    }
}
