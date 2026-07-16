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
    subs.push(iced::event::listen_with(file_drop));
    if app.text_selection.is_some() {
        subs.push(iced::event::listen_with(selection_copy_hotkey));
    }
    if app
        .text_selection
        .as_ref()
        .is_some_and(|selection| selection.dragging)
    {
        subs.push(iced::event::listen_with(selection_mouse_release));
    }
    if app.palette_open {
        subs.push(iced::event::listen_with(palette_navigation));
    }
    if app
        .emoji_previews
        .values()
        .any(|preview| matches!(preview, super::FilePreview::Animated { .. }))
        || has_pending_sends(app)
        || app
            .composer_attachments
            .iter()
            .any(|attachment| attachment.uploading)
        || app
            .thread_composer_attachments
            .iter()
            .any(|attachment| attachment.uploading)
        || app
            .pending_file_messages
            .iter()
            .flat_map(|pending| &pending.attachments)
            .any(|attachment| attachment.uploading)
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
            subs.push(
                realtime::connect(params)
                    .with(app.account_epoch)
                    .map(map_scoped_rt_update),
            );
        }
    }

    if super::agent::enabled() {
        subs.push(super::agent::subscription());
    }

    Subscription::batch(subs)
}

fn file_drop(
    event: iced::Event,
    _status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => {
            Some(Message::FilesDropped(vec![path]))
        }
        _ => None,
    }
}

fn selection_copy_hotkey(
    event: iced::Event,
    status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{Event, Key};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    match key {
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("c") && modifiers.command() => {
            Some(Message::TextSelectionCopyRequested)
        }
        _ => None,
    }
}

fn selection_mouse_release(
    event: iced::Event,
    _status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Message::TextSelectionEnded)
        }
        _ => None,
    }
}

fn has_pending_sends(app: &App) -> bool {
    app.workspaces
        .values()
        .flat_map(|ws| ws.messages.values())
        .any(|cm| !cm.pending.is_empty())
        || app.threads.values().any(|cm| !cm.pending.is_empty())
}

fn palette_hotkey(
    event: iced::Event,
    status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{Event, Key};
    if status == iced::event::Status::Captured {
        return None;
    }
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

fn map_scoped_rt_update((epoch, update): (u64, (TeamId, RtUpdate))) -> Message {
    Message::AccountScoped(epoch, Box::new(map_rt_update(update)))
}
