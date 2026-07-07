use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::operation::{self, RelativeOffset};

use crate::cache::Cache;
use crate::config;
use crate::slack::api::{self, SearchArgs};
use crate::slack::events::RtEvent;
use crate::slack::models::{
    ChannelId, Message as SlackMessage, MessageTs, SearchMessagesPage, TeamId, UserId,
};
use crate::slack::realtime;
use crate::slack::{Error as SlackError, Transport};
use crate::state::{ChannelMessages, Presence, RealtimeStatus, Screen, Workspace};
use crate::ui;

use super::{App, DesktopNotification, FilePreview, Message, SearchHit, SearchState, ThreadKey};

const CACHE_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);

pub(super) fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::WorkspaceSelected(team) => select_workspace(app, team),

        Message::ChannelSelected(id) => {
            app.search = None;
            if app
                .editing
                .as_ref()
                .is_some_and(|(channel, _)| channel != &id)
            {
                app.editing = None;
                app.edit_text.clear();
            }
            app.active_channel = Some(id.clone());
            if let Some(team) = app.active_team.clone() {
                app.last_active_channels.insert(team, id.clone());
            }
            if app
                .active_thread
                .as_ref()
                .is_some_and(|(channel, _)| channel != &id)
            {
                app.active_thread = None;
                app.thread_composer_text.clear();
            }
            if let Some(team) = app.active_team.clone() {
                let needs_load = app
                    .workspaces
                    .get(&team)
                    .map(|ws| !ws.messages.get(&id).map(|cm| cm.loaded).unwrap_or(false))
                    .unwrap_or(false);
                if app.transport.is_some() && needs_load {
                    return app.load_history(&team, &id);
                }
                return Task::batch([
                    hydrate_visible_missing_users(app, &team, &id),
                    mark_latest_visible(app, &team, &id),
                    load_visible_file_previews(app, &team, &id),
                    load_visible_avatar_previews(app, &team, &id),
                ]);
            }
            Task::none()
        }

        Message::ThreadOpened { channel, ts } => {
            app.active_channel = Some(channel.clone());
            app.active_thread = Some((channel.clone(), ts.clone()));
            let Some(team) = app.active_team.clone() else {
                return Task::none();
            };
            let needs_load = !app
                .threads
                .get(&(team.clone(), channel.clone(), ts.clone()))
                .map(|cm| cm.loaded)
                .unwrap_or(false);
            if needs_load {
                app.load_thread(&team, &channel, &ts)
            } else {
                Task::batch([
                    load_thread_file_previews(app, &team, &channel, &ts),
                    load_thread_avatar_previews(app, &team, &channel, &ts),
                ])
            }
        }

        Message::ThreadClosed => {
            app.active_thread = None;
            app.thread_composer_text.clear();
            Task::none()
        }

        Message::ThreadLoaded {
            team,
            channel,
            root_ts,
            result,
        } => {
            if app.active_team.as_deref() != Some(&team) {
                return Task::none();
            }
            match result {
                Ok(page) => {
                    let messages: Vec<_> = page
                        .messages
                        .into_iter()
                        .map(crate::state::visible_message)
                        .collect();
                    let key = (team.clone(), channel.clone(), root_ts.clone());
                    let cm = app.threads.entry(key).or_default();
                    let n = messages.len();
                    for msg in messages.clone() {
                        cm.upsert(msg);
                    }
                    cm.loaded = true;
                    tracing::info!(%channel, %root_ts, messages = n, "thread loaded");
                    return Task::batch([
                        hydrate_missing_users(app, &team, &messages),
                        load_avatar_previews(app, &team, messages),
                        load_thread_file_previews(app, &team, &channel, &root_ts),
                    ]);
                }
                Err(e) => {
                    app.toast(format!("thread failed for {channel}/{root_ts}: {e}"));
                    if is_auth_error(&e) {
                        app.screen = Screen::Login;
                    }
                }
            }
            Task::none()
        }

        Message::ThreadComposerChanged(value) => {
            app.thread_composer_text = value;
            Task::none()
        }

        Message::ThreadSendPressed => send_thread_pressed(app),

        Message::ThreadReplySent {
            team,
            channel,
            root_ts,
            client_msg_id,
            result,
        } => {
            if app.active_team.as_deref() != Some(&team) {
                return Task::none();
            }
            match result {
                Ok(sent) => {
                    if let Some(cm) =
                        app.threads
                            .get_mut(&(team.clone(), channel.clone(), root_ts.clone()))
                    {
                        cm.confirm(
                            &client_msg_id,
                            SlackMessage {
                                ts: Some(sent.ts),
                                thread_ts: Some(root_ts),
                                ..sent.message
                            },
                        );
                    }
                }
                Err(e) => {
                    app.toast(format!("reply failed: {e}"));
                    if is_auth_error(&e) {
                        app.screen = Screen::Login;
                    }
                }
            }
            Task::none()
        }

        Message::ComposerChanged(value) => {
            app.composer_text = value;
            maybe_send_typing(app);
            Task::none()
        }

        Message::SendPressed => send_pressed(app),

        Message::MessageSent {
            team,
            channel,
            client_msg_id,
            result,
        } => {
            match result {
                Ok(sent) => {
                    if let Some(cm) = app
                        .workspaces
                        .get_mut(&team)
                        .and_then(|ws| ws.messages.get_mut(&channel))
                    {
                        cm.confirm(
                            &client_msg_id,
                            SlackMessage {
                                ts: Some(sent.ts),
                                ..sent.message
                            },
                        );
                    }
                    mark_workspace_dirty(app, &team);
                }
                Err(e) => app.toast(format!("send failed: {e}")),
            }
            Task::none()
        }

        Message::BootLoaded(team, result) => match result {
            Ok(boot) => {
                if let Some(ws) = app.workspaces.get_mut(&team) {
                    ws.apply_boot(boot);
                    tracing::info!(%team, channels = ws.channels.len(), "boot ok");
                }
                mark_workspace_dirty(app, &team);
                app.screen = Screen::Main;
                if app.active_team.as_deref() == Some(&team) && app.active_channel.is_none() {
                    if let Some(first) = app
                        .workspaces
                        .get(&team)
                        .and_then(|ws| ws.channels.keys().next().cloned())
                    {
                        app.active_channel = Some(first.clone());
                        return app.load_history(&team, &first);
                    }
                }
                if let Some(channel) = app
                    .active_channel
                    .clone()
                    .filter(|_| app.active_team.as_deref() == Some(&team))
                {
                    return Task::batch([
                        hydrate_visible_missing_users(app, &team, &channel),
                        load_visible_file_previews(app, &team, &channel),
                        load_visible_avatar_previews(app, &team, &channel),
                    ]);
                }
                Task::none()
            }
            Err(e) => {
                app.toast(format!("boot failed for {team}: {e}"));
                if is_auth_error(&e) {
                    app.screen = Screen::Login;
                }
                Task::none()
            }
        },

        Message::CountsLoaded(team, result) => {
            match result {
                Ok(counts) => {
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        ws.apply_counts(counts);
                    }
                    mark_workspace_dirty(app, &team);
                }
                Err(e) => tracing::warn!(%team, error = %e, "counts failed"),
            }
            Task::none()
        }

        Message::HistoryLoaded(team, channel, result) => {
            match result {
                Ok(page) => {
                    let messages: Vec<_> = page
                        .messages
                        .into_iter()
                        .map(crate::state::visible_message)
                        .collect();
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        let cm = ws.messages.entry(channel.clone()).or_default();
                        let n = messages.len();
                        for msg in messages.clone() {
                            if crate::state::is_channel_timeline_visible(&msg) {
                                cm.upsert(msg);
                            }
                        }
                        cm.loaded = true;
                        tracing::info!(%channel, messages = n, "history loaded");
                    }
                    mark_workspace_dirty(app, &team);
                    return Task::batch([
                        hydrate_missing_users(app, &team, &messages),
                        load_avatar_previews(app, &team, messages),
                        mark_latest_visible(app, &team, &channel),
                        load_visible_file_previews(app, &team, &channel),
                        scroll_to_pending(app, &channel),
                    ]);
                }
                Err(e) => app.toast(format!("history failed for {channel}: {e}")),
            }
            Task::none()
        }

        Message::ChannelMarked(team, channel, ts, result) => {
            match result {
                Ok(()) => {
                    if let Some(cm) = app
                        .workspaces
                        .get_mut(&team)
                        .and_then(|ws| ws.messages.get_mut(&channel))
                    {
                        cm.last_read = Some(ts);
                        cm.unread_count = 0;
                        cm.mention_count = 0;
                    }
                    mark_workspace_dirty(app, &team);
                }
                Err(e) => tracing::warn!(%team, %channel, error = %e, "mark failed"),
            }
            Task::none()
        }

        Message::EditPressed { channel, ts } => {
            let current = find_message_text(app, &channel, &ts).unwrap_or_default();
            app.edit_text = current;
            app.editing = Some((channel, ts));
            Task::none()
        }

        Message::EditComposerChanged(value) => {
            app.edit_text = value;
            Task::none()
        }

        Message::EditCancelled => {
            app.editing = None;
            app.edit_text.clear();
            Task::none()
        }

        Message::EditSubmit => edit_submit(app),

        Message::MessageEdited {
            team,
            channel,
            ts,
            result,
        } => {
            match result {
                Ok(sent) => {
                    apply_message_edit(app, &team, &channel, &ts, sent.message.text);
                    mark_workspace_dirty(app, &team);
                }
                Err(e) => {
                    app.toast(format!("edit failed: {e}"));
                    if is_auth_error(&e) {
                        app.screen = Screen::Login;
                    }
                }
            }
            Task::none()
        }

        Message::DeletePressed { channel, ts } => delete_pressed(app, channel, ts),

        Message::MessageDeleted {
            team,
            channel,
            ts,
            result,
        } => {
            match result {
                Ok(()) => {
                    remove_message_everywhere(app, &team, &channel, &ts);
                    mark_workspace_dirty(app, &team);
                }
                Err(e) => {
                    app.toast(format!("delete failed: {e}"));
                    if is_auth_error(&e) {
                        app.screen = Screen::Login;
                    }
                }
            }
            Task::none()
        }

        Message::ReactionPressed { channel, ts, name } => toggle_reaction(app, channel, ts, name),

        Message::ReactionUpdated {
            team,
            channel,
            ts,
            user,
            name,
            added,
            result,
        } => {
            if let Err(e) = result {
                app.toast(format!("reaction failed: {e}"));
                if let Some(cm) = app
                    .workspaces
                    .get_mut(&team)
                    .and_then(|ws| ws.messages.get_mut(&channel))
                {
                    cm.apply_reaction(&ts, &user, &name, !added);
                }
                apply_thread_reaction(&mut app.threads, &team, &channel, &ts, &user, &name, !added);
                if is_auth_error(&e) {
                    app.screen = Screen::Login;
                }
                mark_workspace_dirty(app, &team);
            }
            Task::none()
        }

        Message::SearchInputChanged(value) => {
            app.search_input = value;
            Task::none()
        }

        Message::SearchSubmitted => search_submitted(app),

        Message::SearchCleared => {
            app.search = None;
            Task::none()
        }

        Message::SearchPageRequested(page) => search_page_requested(app, page),

        Message::SearchLoaded {
            team,
            query,
            page,
            result,
        } => {
            let matches = app
                .search
                .as_ref()
                .is_some_and(|s| s.team == team && s.query == query && s.page == page);
            if !matches {
                return Task::none();
            }
            match result {
                Ok(response) => {
                    if let Some(ws) = app.workspaces.get(&team) {
                        if let Some(state) = app.search.as_mut() {
                            state.hits = search_hits(ws, &response);
                            if let Some(p) = &response.pagination {
                                state.page = p.page.unwrap_or(page);
                                state.page_count = p.page_count.unwrap_or(state.page_count);
                                state.total = p.total_count.unwrap_or(state.total);
                            }
                            state.loading = false;
                        }
                    }
                }
                Err(e) => {
                    if let Some(state) = app.search.as_mut() {
                        state.loading = false;
                    }
                    app.toast(format!("search failed: {e}"));
                    if is_auth_error(&e) {
                        app.screen = Screen::Login;
                    }
                }
            }
            Task::none()
        }

        Message::SearchResultSelected {
            channel,
            ts,
            thread_ts,
        } => open_search_result(app, channel, ts, thread_ts),

        Message::FileDownloadPressed { url, filename } => download_file_pressed(app, url, filename),

        Message::FileDownloaded(result) => {
            match result {
                Ok(path) => app.toast(format!("downloaded file to {}", path.display())),
                Err(e) => app.toast(format!("download failed: {e}")),
            }
            Task::none()
        }

        Message::OpenUrl(url) => open_url_pressed(app, url),

        Message::UrlOpened(result) => {
            if let Err(e) = result {
                app.toast(format!("could not open link: {e}"));
            }
            Task::none()
        }

        Message::FilePreviewLoaded { key, result } => {
            match result {
                Ok(bytes) => {
                    app.file_previews
                        .insert(key, FilePreview::Loaded(ImageHandle::from_bytes(bytes)));
                }
                Err(e) => {
                    tracing::warn!(%key, error = %e, "file preview failed");
                    app.file_previews.insert(key, FilePreview::Failed);
                }
            }
            Task::none()
        }

        Message::AvatarLoaded { user, result } => {
            match result {
                Ok(bytes) => {
                    app.avatar_previews
                        .insert(user, FilePreview::Loaded(ImageHandle::from_bytes(bytes)));
                }
                Err(e) => {
                    tracing::debug!(%user, error = %e, "avatar failed");
                    app.avatar_previews.insert(user, FilePreview::Failed);
                }
            }
            Task::none()
        }

        Message::UsersLoaded { team, result } => {
            match result {
                Ok(users) => {
                    let ids: Vec<_> = users.iter().map(|user| user.id.clone()).collect();
                    app.avatar_profile_hydrated.extend(ids.iter().cloned());
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        for user in users {
                            ws.users.insert(user.id.clone(), user);
                        }
                    }
                    mark_workspace_dirty(app, &team);
                    return load_user_avatar_previews(app, &team, ids);
                }
                Err(e) => tracing::debug!(%team, error = %e, "users info failed"),
            }
            Task::none()
        }

        Message::DesktopNotificationShown(result) => {
            if let Err(e) = result {
                tracing::debug!(error = %e, "desktop notification failed");
            }
            Task::none()
        }

        Message::CacheSaved {
            team,
            started_at,
            result,
        } => {
            app.cache_saving.remove(&team);
            match result {
                Ok(()) => {
                    let clean = app
                        .cache_dirty
                        .get(&team)
                        .is_some_and(|dirty_at| *dirty_at <= started_at);
                    if clean {
                        app.cache_dirty.remove(&team);
                    }
                }
                Err(e) => tracing::warn!(%team, error = %e, "cache save failed"),
            }
            flush_due_cache(app, Instant::now())
        }

        Message::Realtime(team, generation, event) => {
            let cacheable = app
                .workspaces
                .get(&team)
                .is_some_and(|ws| ws.rt_generation == generation)
                && workspace_cacheable_event(&event);
            let notification = apply_realtime(app, &team, generation, event);
            if cacheable {
                mark_workspace_dirty(app, &team);
            }
            let mut tasks = Vec::new();
            if let Some(notification) = notification {
                tasks.push(show_desktop_notification_task(notification));
            }
            if app.active_team.as_deref() == Some(&team) {
                if let Some(channel) = app.active_channel.clone() {
                    tasks.push(hydrate_visible_missing_users(app, &team, &channel));
                    tasks.push(mark_latest_visible(app, &team, &channel));
                    tasks.push(load_visible_file_previews(app, &team, &channel));
                    tasks.push(load_visible_avatar_previews(app, &team, &channel));
                }
            }
            Task::batch(tasks)
        }

        Message::RtConnected(team, generation, connection) => {
            if let Some(ws) = app.workspaces.get_mut(&team) {
                ws.rt_generation = generation;
                ws.rt = RealtimeStatus::Connected(connection);
            }
            if app.active_team.as_deref() == Some(&team) {
                if let Some(channel) = app.active_channel.clone() {
                    if app.transport.is_some() {
                        let oldest = app
                            .workspaces
                            .get(&team)
                            .and_then(|ws| ws.messages.get(&channel))
                            .and_then(|cm| cm.latest_ts());
                        return app.load_history_since(&team, &channel, oldest);
                    }
                }
            }
            Task::none()
        }

        Message::RtDisconnected(team, generation) => {
            if let Some(ws) = app.workspaces.get_mut(&team) {
                if generation >= ws.rt_generation {
                    ws.rt = RealtimeStatus::Disconnected;
                }
            }
            Task::none()
        }

        Message::SignInPressed => match std::env::current_exe() {
            Ok(exe) => Task::perform(
                async move {
                    tokio::process::Command::new(exe)
                        .env("SNACK_AUTH", "1")
                        .status()
                        .await
                        .map(|s| s.success())
                        .unwrap_or(false)
                },
                |_ok| Message::RetryAuth,
            ),
            Err(e) => {
                app.toast(format!("could not locate snack binary: {e}"));
                Task::none()
            }
        },

        Message::RetryAuth => app.load_session(),

        Message::Tick => {
            let now = Instant::now();
            if let Some(ws) = app.active_workspace_mut() {
                ws.prune_typing(now, Duration::from_secs(4));
            }
            flush_due_cache(app, now)
        }
    }
}

fn select_workspace(app: &mut App, team: TeamId) -> Task<Message> {
    if app.active_team.as_deref() == Some(&team) {
        return Task::none();
    }
    if !app.workspaces.contains_key(&team) {
        return Task::none();
    }

    if let (Some(current_team), Some(current_channel)) =
        (app.active_team.clone(), app.active_channel.clone())
    {
        app.last_active_channels
            .insert(current_team, current_channel);
    }

    app.active_team = Some(team.clone());
    app.active_channel = preferred_channel(app, &team);
    app.active_thread = None;
    app.search = None;
    app.search_input.clear();
    app.editing = None;
    app.edit_text.clear();
    app.composer_text.clear();
    app.thread_composer_text.clear();

    let Some(channel) = app.active_channel.clone() else {
        return Task::none();
    };
    let needs_load = app
        .workspaces
        .get(&team)
        .map(|ws| {
            !ws.messages
                .get(&channel)
                .map(|cm| cm.loaded)
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if app.transport.is_some() && needs_load {
        app.load_history(&team, &channel)
    } else {
        mark_latest_visible(app, &team, &channel)
    }
}

pub(super) fn preferred_channel(app: &App, team: &str) -> Option<ChannelId> {
    let ws = app.workspaces.get(team)?;
    if let Some(channel) = app
        .last_active_channels
        .get(team)
        .filter(|channel| ws.channels.contains_key(*channel))
    {
        return Some(channel.clone());
    }
    app.active_channel
        .as_ref()
        .filter(|channel| ws.channels.contains_key(*channel))
        .cloned()
        .or_else(|| ws.channels.keys().next().cloned())
}

fn next_seq(app: &mut App) -> u64 {
    app.send_seq += 1;
    app.send_seq
}

fn is_auth_error(e: &SlackError) -> bool {
    matches!(e, SlackError::Api(code) if code == "invalid_auth" || code == "not_authed" || code == "token_revoked")
}

fn maybe_send_typing(app: &mut App) {
    if app.composer_text.trim().is_empty() {
        return;
    }
    let (Some(team), Some(channel)) = (app.active_team.clone(), app.active_channel.clone()) else {
        return;
    };
    let now = Instant::now();
    let key = (team.clone(), channel.clone());
    if app
        .last_typing
        .get(&key)
        .is_some_and(|last| now.duration_since(*last) < Duration::from_secs(3))
    {
        return;
    }
    let Some(ws) = app.workspaces.get(&team) else {
        return;
    };
    let RealtimeStatus::Connected(connection) = &ws.rt else {
        return;
    };
    connection.send(realtime::user_typing_frame(&channel));
    app.last_typing.insert(key, now);
}

fn mark_latest_visible(app: &App, team: &str, channel: &ChannelId) -> Task<Message> {
    let Some(ws) = app.workspaces.get(team) else {
        return Task::none();
    };
    let Some(cm) = ws.messages.get(channel) else {
        return Task::none();
    };
    if !cm.loaded {
        return Task::none();
    }
    let Some(latest) = cm.latest_ts() else {
        return Task::none();
    };
    if cm
        .last_read
        .as_ref()
        .is_some_and(|last_read| crate::state::ts_key(last_read) >= crate::state::ts_key(&latest))
    {
        return Task::none();
    }
    app.mark_channel_read(team, channel, latest)
}

fn send_pressed(app: &mut App) -> Task<Message> {
    let text = app.composer_text.trim().to_owned();
    if text.is_empty() {
        return Task::none();
    }
    let (Some(team), Some(channel)) = (app.active_team.clone(), app.active_channel.clone()) else {
        return Task::none();
    };

    let seq = next_seq(app);
    let client_msg_id = uuid::Uuid::new_v4().to_string();
    let ts = format!("{}.{:06}", chrono::Utc::now().timestamp(), seq);

    let Some(ws) = app.workspaces.get_mut(&team) else {
        return Task::none();
    };
    let self_user = ws.self_user_id.clone();

    let pending = SlackMessage {
        user: Some(self_user),
        kind: Some("message".to_owned()),
        ts: Some(ts.clone()),
        client_msg_id: Some(client_msg_id.clone()),
        text: Some(text.clone()),
        channel: Some(channel.clone()),
        ..Default::default()
    };
    let cm = ws.messages.entry(channel.clone()).or_default();
    cm.upsert(pending);
    cm.pending.push(ts);
    app.composer_text.clear();
    mark_workspace_dirty(app, &team);

    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let send_channel = channel.clone();
    Task::perform(
        async move {
            api::send_message(&transport, &client, &ws_session, send_channel, text, None).await
        },
        move |result| Message::MessageSent {
            team: team.clone(),
            channel: channel.clone(),
            client_msg_id: client_msg_id.clone(),
            result,
        },
    )
}

fn send_thread_pressed(app: &mut App) -> Task<Message> {
    let text = app.thread_composer_text.trim().to_owned();
    if text.is_empty() {
        return Task::none();
    }
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    let Some((channel, root_ts)) = app.active_thread.clone() else {
        return Task::none();
    };

    let seq = next_seq(app);
    let client_msg_id = uuid::Uuid::new_v4().to_string();
    let ts = format!("{}.{:06}", chrono::Utc::now().timestamp(), seq);

    let Some(ws) = app.workspaces.get(&team) else {
        return Task::none();
    };
    let self_user = ws.self_user_id.clone();
    let pending = SlackMessage {
        user: Some(self_user),
        kind: Some("message".to_owned()),
        ts: Some(ts.clone()),
        client_msg_id: Some(client_msg_id.clone()),
        text: Some(text.clone()),
        channel: Some(channel.clone()),
        thread_ts: Some(root_ts.clone()),
        ..Default::default()
    };
    let cm = app
        .threads
        .entry((team.clone(), channel.clone(), root_ts.clone()))
        .or_default();
    cm.upsert(pending);
    cm.pending.push(ts);
    app.thread_composer_text.clear();

    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let send_channel = channel.clone();
    let send_thread_ts = root_ts.clone();
    Task::perform(
        async move {
            api::send_message(
                &transport,
                &client,
                &ws_session,
                send_channel,
                text,
                Some(send_thread_ts),
            )
            .await
        },
        move |result| Message::ThreadReplySent {
            team: team.clone(),
            channel: channel.clone(),
            root_ts: root_ts.clone(),
            client_msg_id: client_msg_id.clone(),
            result,
        },
    )
}

fn toggle_reaction(
    app: &mut App,
    channel: ChannelId,
    ts: MessageTs,
    name: String,
) -> Task<Message> {
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let Some(ws) = app.workspaces.get_mut(&team) else {
        return Task::none();
    };
    let user = ws.self_user_id.clone();
    let active = ws
        .messages
        .get(&channel)
        .and_then(|cm| reaction_has_user_in(cm, &ts, &name, &user))
        .or_else(|| {
            app.threads
                .iter()
                .filter(|((thread_team, thread_channel, _), _)| {
                    thread_team == &team && thread_channel == &channel
                })
                .find_map(|(_, cm)| reaction_has_user_in(cm, &ts, &name, &user))
        });
    let Some(active) = active else {
        return Task::none();
    };
    let adding = !active;

    let mut applied = false;
    if let Some(cm) = ws.messages.get_mut(&channel) {
        if reaction_has_user_in(cm, &ts, &name, &user).is_some() {
            cm.apply_reaction(&ts, &user, &name, adding);
            applied = true;
        }
    }
    if !applied {
        for ((thread_team, thread_channel, _), cm) in &mut app.threads {
            if thread_team == &team
                && thread_channel == &channel
                && reaction_has_user_in(cm, &ts, &name, &user).is_some()
            {
                cm.apply_reaction(&ts, &user, &name, adding);
                break;
            }
        }
    }
    mark_workspace_dirty(app, &team);

    let send_channel = channel.clone();
    let send_ts = ts.clone();
    let send_name = name.clone();
    Task::perform(
        async move {
            if adding {
                api::add_reaction(
                    &transport,
                    &client,
                    &ws_session,
                    send_channel,
                    send_ts,
                    send_name,
                )
                .await
            } else {
                api::remove_reaction(
                    &transport,
                    &client,
                    &ws_session,
                    send_channel,
                    send_ts,
                    send_name,
                )
                .await
            }
        },
        move |result| Message::ReactionUpdated {
            team: team.clone(),
            channel: channel.clone(),
            ts: ts.clone(),
            user: user.clone(),
            name: name.clone(),
            added: adding,
            result,
        },
    )
}

fn edit_submit(app: &mut App) -> Task<Message> {
    let text = app.edit_text.trim().to_owned();
    let Some((channel, ts)) = app.editing.clone() else {
        return Task::none();
    };
    if text.is_empty() {
        app.toast("message cannot be empty");
        return Task::none();
    }
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };

    app.editing = None;
    app.edit_text.clear();
    apply_message_edit(app, &team, &channel, &ts, Some(text.clone()));
    mark_workspace_dirty(app, &team);

    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let send_channel = channel.clone();
    let send_ts = ts.clone();
    Task::perform(
        async move {
            api::edit_message(
                &transport,
                &client,
                &ws_session,
                send_channel,
                send_ts,
                text,
            )
            .await
        },
        move |result| Message::MessageEdited {
            team: team.clone(),
            channel: channel.clone(),
            ts: ts.clone(),
            result,
        },
    )
}

fn delete_pressed(app: &mut App, channel: ChannelId, ts: MessageTs) -> Task<Message> {
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    if app.editing.as_ref() == Some(&(channel.clone(), ts.clone())) {
        app.editing = None;
        app.edit_text.clear();
    }
    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let send_channel = channel.clone();
    let send_ts = ts.clone();
    Task::perform(
        async move {
            api::delete_message(&transport, &client, &ws_session, send_channel, send_ts).await
        },
        move |result| Message::MessageDeleted {
            team: team.clone(),
            channel: channel.clone(),
            ts: ts.clone(),
            result,
        },
    )
}

fn find_message_text(app: &App, channel: &str, ts: &str) -> Option<String> {
    let team = app.active_team.as_deref()?;
    let ws = app.workspaces.get(team)?;
    if let Some(text) = ws
        .messages
        .get(channel)
        .and_then(|cm| cm.messages.iter().find(|m| m.ts.as_deref() == Some(ts)))
        .and_then(|m| m.text.clone())
    {
        return Some(text);
    }
    app.threads
        .iter()
        .filter(|((t, c, _), _)| t == team && c == channel)
        .find_map(|(_, cm)| {
            cm.messages
                .iter()
                .find(|m| m.ts.as_deref() == Some(ts))
                .and_then(|m| m.text.clone())
        })
}

fn apply_message_edit(app: &mut App, team: &str, channel: &str, ts: &str, text: Option<String>) {
    if let Some(msg) = app
        .workspaces
        .get_mut(team)
        .and_then(|ws| ws.messages.get_mut(channel))
        .and_then(|cm| cm.messages.iter_mut().find(|m| m.ts.as_deref() == Some(ts)))
    {
        mark_edited(msg, text.clone());
    }
    for ((thread_team, thread_channel, _), cm) in &mut app.threads {
        if thread_team == team && thread_channel == channel {
            if let Some(msg) = cm.messages.iter_mut().find(|m| m.ts.as_deref() == Some(ts)) {
                mark_edited(msg, text.clone());
            }
        }
    }
}

fn mark_edited(msg: &mut SlackMessage, text: Option<String>) {
    if let Some(text) = text {
        msg.text = Some(text);
    }
    if msg.edited.is_none() {
        msg.edited = Some(serde_json::json!({ "ts": msg.ts.clone() }));
    }
}

fn remove_message_everywhere(app: &mut App, team: &str, channel: &str, ts: &str) {
    if let Some(cm) = app
        .workspaces
        .get_mut(team)
        .and_then(|ws| ws.messages.get_mut(channel))
    {
        cm.remove(ts);
    }
    for ((thread_team, thread_channel, _), cm) in &mut app.threads {
        if thread_team == team && thread_channel == channel {
            cm.remove(ts);
        }
    }
}

fn search_submitted(app: &mut App) -> Task<Message> {
    let query = app.search_input.trim().to_owned();
    if query.is_empty() {
        return Task::none();
    }
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    app.search = Some(SearchState {
        query: query.clone(),
        team: team.clone(),
        page: 1,
        page_count: 0,
        total: 0,
        hits: Vec::new(),
        loading: true,
    });
    run_search(app, &team, query, 1)
}

fn search_page_requested(app: &mut App, page: u32) -> Task<Message> {
    let (team, query) = match app.search.as_mut() {
        Some(state) => {
            if page < 1 || (state.page_count > 0 && page > state.page_count) {
                return Task::none();
            }
            state.page = page;
            state.loading = true;
            (state.team.clone(), state.query.clone())
        }
        None => return Task::none(),
    };
    run_search(app, &team, query, page)
}

fn run_search(app: &App, team: &str, query: String, page: u32) -> Task<Message> {
    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let team = team.to_owned();
    let args = SearchArgs {
        query: query.clone(),
        count: 20,
        page,
    };
    Task::perform(
        async move { api::fetch_search_messages(&transport, &client, &ws_session, args).await },
        move |result| Message::SearchLoaded {
            team: team.clone(),
            query: query.clone(),
            page,
            result,
        },
    )
}

fn search_hits(ws: &Workspace, response: &SearchMessagesPage) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for item in &response.items {
        let channel_id = item
            .channel
            .as_ref()
            .map(|c| c.id.clone())
            .or_else(|| item.messages.first().and_then(|m| m.channel.clone()));
        let Some(channel_id) = channel_id else {
            continue;
        };
        let label = ws
            .channels
            .get(&channel_id)
            .map(crate::state::channel_label)
            .or_else(|| item.channel.as_ref().map(crate::state::channel_label))
            .unwrap_or_else(|| channel_id.clone());
        for msg in &item.messages {
            let mut msg = crate::state::visible_message(msg.clone());
            if msg.channel.is_none() {
                msg.channel = Some(channel_id.clone());
            }
            hits.push(SearchHit {
                channel: channel_id.clone(),
                channel_label: label.clone(),
                message: msg,
            });
        }
    }
    hits
}

fn open_search_result(
    app: &mut App,
    channel: ChannelId,
    ts: MessageTs,
    thread_ts: Option<MessageTs>,
) -> Task<Message> {
    app.search = None;
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    app.active_channel = Some(channel.clone());
    app.last_active_channels
        .insert(team.clone(), channel.clone());
    app.editing = None;
    app.edit_text.clear();
    app.pending_scroll_to = Some((channel.clone(), ts));

    let mut tasks = Vec::new();
    let needs_load = app
        .workspaces
        .get(&team)
        .map(|ws| {
            !ws.messages
                .get(&channel)
                .map(|cm| cm.loaded)
                .unwrap_or(false)
        })
        .unwrap_or(true);
    if app.transport.is_some() && needs_load {
        tasks.push(app.load_history(&team, &channel));
    } else {
        tasks.push(mark_latest_visible(app, &team, &channel));
        tasks.push(load_visible_file_previews(app, &team, &channel));
        tasks.push(scroll_to_pending(app, &channel));
    }

    match thread_ts {
        Some(root) => {
            app.active_thread = Some((channel.clone(), root.clone()));
            let needs_thread = !app
                .threads
                .get(&(team.clone(), channel.clone(), root.clone()))
                .map(|cm| cm.loaded)
                .unwrap_or(false);
            if needs_thread && app.transport.is_some() {
                tasks.push(app.load_thread(&team, &channel, &root));
            }
        }
        None => {
            app.active_thread = None;
        }
    }
    Task::batch(tasks)
}

fn scroll_to_pending(app: &mut App, channel: &ChannelId) -> Task<Message> {
    let Some((pending_channel, ts)) = app.pending_scroll_to.clone() else {
        return Task::none();
    };
    if pending_channel != *channel {
        return Task::none();
    }
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    let loaded_messages = app
        .workspaces
        .get(&team)
        .and_then(|ws| ws.messages.get(channel))
        .filter(|cm| cm.loaded)
        .map(|cm| &cm.messages);
    let Some(messages) = loaded_messages else {
        return Task::none();
    };
    match crate::state::scroll_ratio_for_ts(messages, &ts) {
        Some(ratio) => {
            app.pending_scroll_to = None;
            operation::snap_to(
                ui::channel::CHANNEL_SCROLLABLE_ID,
                RelativeOffset { x: 0.0, y: ratio },
            )
        }
        None => Task::none(),
    }
}

fn open_url_pressed(app: &mut App, url: String) -> Task<Message> {
    if !crate::state::is_browser_url(&url) {
        app.toast("could not open link: unsupported URL scheme");
        return Task::none();
    }
    Task::perform(open_url_in_browser(url), Message::UrlOpened)
}

async fn open_url_in_browser(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut cmd = tokio::process::Command::new("open");
        cmd.arg(&url);
        cmd
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut cmd = tokio::process::Command::new("xdg-open");
        cmd.arg(&url);
        cmd
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args(["/C", "start", "", &url]);
        cmd
    };

    let status = command
        .status()
        .await
        .map_err(|e| format!("spawn failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}"))
    }
}

fn download_file_pressed(app: &mut App, url: String, filename: String) -> Task<Message> {
    let Some(transport) = app.transport.clone() else {
        app.toast("download failed: transport not connected");
        return Task::none();
    };
    Task::perform(
        async move { download_file_to_disk(transport, url, filename).await },
        Message::FileDownloaded,
    )
}

async fn download_file_to_disk(
    transport: Arc<Transport>,
    url: String,
    filename: String,
) -> Result<PathBuf, SlackError> {
    let user_agent = crate::slack::xparams::Identity::from_capture().user_agent;
    let bytes = transport.get_bytes(&url, &user_agent).await?;
    let dir = config::data_dir()
        .map_err(|e| SlackError::Transport(format!("download dir: {e}")))?
        .join("downloads");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| SlackError::Transport(format!("create download dir: {e}")))?;
    let path = unique_download_path(&dir, &filename).await?;
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| SlackError::Transport(format!("write download: {e}")))?;
    Ok(path)
}

pub(super) async fn unique_download_path(
    dir: &Path,
    filename: &str,
) -> Result<PathBuf, SlackError> {
    let filename = if filename.trim().is_empty() {
        "download"
    } else {
        filename
    };
    let candidate = dir.join(filename);
    if !tokio::fs::try_exists(&candidate)
        .await
        .map_err(|e| SlackError::Transport(format!("check download path: {e}")))?
    {
        return Ok(candidate);
    }

    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..1000 {
        let name = match ext {
            Some(ext) if !ext.is_empty() => format!("{stem}-{i}.{ext}"),
            _ => format!("{stem}-{i}"),
        };
        let candidate = dir.join(name);
        if !tokio::fs::try_exists(&candidate)
            .await
            .map_err(|e| SlackError::Transport(format!("check download path: {e}")))?
        {
            return Ok(candidate);
        }
    }

    Err(SlackError::Transport(
        "could not choose download path".to_owned(),
    ))
}

fn load_visible_file_previews(app: &mut App, team: &str, channel: &str) -> Task<Message> {
    let messages = visible_channel_messages(app, team, channel);
    load_file_previews(app, messages)
}

fn load_visible_avatar_previews(app: &mut App, team: &str, channel: &str) -> Task<Message> {
    let messages = visible_channel_messages(app, team, channel);
    load_avatar_previews(app, team, messages)
}

fn load_thread_file_previews(
    app: &mut App,
    team: &str,
    channel: &str,
    root_ts: &str,
) -> Task<Message> {
    let mut messages = Vec::new();
    if let Some(root) = app
        .workspaces
        .get(team)
        .and_then(|ws| ws.messages.get(channel))
        .and_then(|cm| {
            cm.messages
                .iter()
                .find(|msg| msg.ts.as_deref() == Some(root_ts))
        })
    {
        messages.push(root.clone());
    }
    if let Some(replies) =
        app.threads
            .get(&(team.to_owned(), channel.to_owned(), root_ts.to_owned()))
    {
        messages.extend(replies.messages.clone());
    }
    load_file_previews(app, messages)
}

fn load_thread_avatar_previews(
    app: &mut App,
    team: &str,
    channel: &str,
    root_ts: &str,
) -> Task<Message> {
    let mut messages = Vec::new();
    if let Some(root) = app
        .workspaces
        .get(team)
        .and_then(|ws| ws.messages.get(channel))
        .and_then(|cm| {
            cm.messages
                .iter()
                .find(|msg| msg.ts.as_deref() == Some(root_ts))
        })
    {
        messages.push(root.clone());
    }
    if let Some(replies) =
        app.threads
            .get(&(team.to_owned(), channel.to_owned(), root_ts.to_owned()))
    {
        messages.extend(replies.messages.clone());
    }
    load_avatar_previews(app, team, messages)
}

fn load_file_previews(app: &mut App, messages: Vec<SlackMessage>) -> Task<Message> {
    let Some(transport) = app.transport.clone() else {
        return Task::none();
    };
    let file_requests = messages
        .iter()
        .flat_map(|msg| &msg.files)
        .filter_map(|file| {
            let key = crate::state::file_preview_key(file)?;
            let url = crate::state::file_preview_url(file)?.to_owned();
            Some((key, url))
        });
    let attachment_requests = messages
        .iter()
        .flat_map(|msg| &msg.attachments)
        .filter_map(|att| {
            let url = crate::state::attachment_preview_url(att)?.to_owned();
            Some((url.clone(), url))
        });
    let mut seen = std::collections::HashSet::new();
    let requests: Vec<_> = file_requests
        .chain(attachment_requests)
        .filter(|(key, _)| !app.file_previews.contains_key(key) && seen.insert(key.clone()))
        .collect();

    if requests.is_empty() {
        return Task::none();
    }

    for (key, _) in &requests {
        app.file_previews.insert(key.clone(), FilePreview::Loading);
    }

    let user_agent = crate::slack::xparams::Identity::from_capture().user_agent;
    Task::batch(requests.into_iter().map(|(key, url)| {
        let transport = transport.clone();
        let user_agent = user_agent.clone();
        Task::perform(
            async move { transport.get_bytes(&url, &user_agent).await },
            move |result| Message::FilePreviewLoaded {
                key: key.clone(),
                result,
            },
        )
    }))
}

fn hydrate_missing_users(app: &App, team: &str, messages: &[SlackMessage]) -> Task<Message> {
    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(team) else {
        return Task::none();
    };
    let Some(ws) = app.workspaces.get(team) else {
        return Task::none();
    };

    let mut seen = HashSet::new();
    let users: Vec<_> = messages
        .iter()
        .flat_map(message_user_ids)
        .filter(|user| !user.trim().is_empty())
        .filter(|user| needs_user_hydration(ws, &app.avatar_profile_hydrated, user))
        .filter(|user| seen.insert(user.clone()))
        .collect();

    if users.is_empty() {
        return Task::none();
    }

    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let team = team.to_owned();
    Task::perform(
        async move { api::fetch_users_info(&transport, &client, &ws_session, users).await },
        move |result| Message::UsersLoaded {
            team: team.clone(),
            result,
        },
    )
}

pub(super) fn needs_user_hydration(
    ws: &Workspace,
    avatar_profile_hydrated: &HashSet<UserId>,
    user_id: &str,
) -> bool {
    let Some(user) = ws.users.get(user_id) else {
        return true;
    };
    ws.avatar_url(user_id).is_none() && !avatar_profile_hydrated.contains(user.id.as_str())
}

fn hydrate_visible_missing_users(app: &App, team: &str, channel: &str) -> Task<Message> {
    let messages = visible_channel_messages(app, team, channel);
    hydrate_missing_users(app, team, &messages)
}

fn visible_channel_messages(app: &App, team: &str, channel: &str) -> Vec<SlackMessage> {
    let mut messages: Vec<_> = app
        .workspaces
        .get(team)
        .and_then(|ws| ws.messages.get(channel))
        .map(|cm| {
            cm.messages
                .iter()
                .rev()
                .filter(|msg| crate::state::is_channel_timeline_visible(msg))
                .take(ui::channel::VISIBLE_MESSAGE_LIMIT)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    messages.reverse();
    messages
}

fn load_avatar_previews(app: &mut App, team: &str, messages: Vec<SlackMessage>) -> Task<Message> {
    let mut seen = HashSet::new();
    let users = messages
        .iter()
        .flat_map(message_user_ids)
        .filter(|user| seen.insert(user.clone()))
        .collect();
    load_user_avatar_previews(app, team, users)
}

fn load_user_avatar_previews(app: &mut App, team: &str, users: Vec<UserId>) -> Task<Message> {
    let Some(transport) = app.transport.clone() else {
        return Task::none();
    };
    let Some(ws) = app.workspaces.get(team) else {
        return Task::none();
    };

    let mut seen = HashSet::new();
    let requests: Vec<_> = users
        .into_iter()
        .filter(|user| !app.avatar_previews.contains_key(user))
        .filter(|user| seen.insert(user.clone()))
        .filter_map(|user| ws.avatar_url(&user).map(|url| (user, url)))
        .collect();

    if requests.is_empty() {
        return Task::none();
    }

    for (user, _) in &requests {
        app.avatar_previews
            .insert(user.clone(), FilePreview::Loading);
    }

    let user_agent = crate::slack::xparams::Identity::from_capture().user_agent;
    Task::batch(requests.into_iter().map(|(user, url)| {
        let transport = transport.clone();
        let user_agent = user_agent.clone();
        Task::perform(
            async move { transport.get_bytes(&url, &user_agent).await },
            move |result| Message::AvatarLoaded {
                user: user.clone(),
                result,
            },
        )
    }))
}

fn message_user_ids(msg: &SlackMessage) -> Vec<UserId> {
    let mut users = Vec::new();
    if let Some(user) = msg.user.clone() {
        users.push(user);
    }
    if let Some(user) = msg.parent_user_id.clone() {
        users.push(user);
    }
    users.extend(msg.reply_users.clone());
    for reaction in &msg.reactions {
        users.extend(reaction.users.clone());
    }
    users
}

fn show_desktop_notification_task(notification: DesktopNotification) -> Task<Message> {
    Task::perform(
        async move { show_desktop_notification(notification).await },
        Message::DesktopNotificationShown,
    )
}

async fn show_desktop_notification(notification: DesktopNotification) -> Result<(), String> {
    tokio::task::spawn_blocking(move || show_desktop_notification_blocking(&notification))
        .await
        .map_err(|e| format!("notification task join failed: {e}"))?
}

#[cfg(target_os = "macos")]
fn show_desktop_notification_blocking(notification: &DesktopNotification) -> Result<(), String> {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(&notification.body),
        applescript_string(&notification.title),
    );
    command_status(
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(script),
    )
}

#[cfg(target_os = "linux")]
fn show_desktop_notification_blocking(notification: &DesktopNotification) -> Result<(), String> {
    command_status(
        std::process::Command::new("notify-send")
            .arg(&notification.title)
            .arg(&notification.body),
    )
}

#[cfg(target_os = "windows")]
fn show_desktop_notification_blocking(_notification: &DesktopNotification) -> Result<(), String> {
    Err("desktop notifications are not implemented on windows".to_owned())
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_status(command: &mut std::process::Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("notification command failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("notification command exited with {status}"))
    }
}

pub(super) fn notification_for_message(
    ws: &Workspace,
    channel: &str,
    msg: &SlackMessage,
    active_channel: Option<&str>,
) -> Option<DesktopNotification> {
    if active_channel == Some(channel) || msg.user.as_deref() == Some(&ws.self_user_id) {
        return None;
    }

    let direct = ws
        .channels
        .get(channel)
        .map(|channel| channel.is_im || channel.is_mpim)
        .unwrap_or(false);
    let mentioned = message_mentions_user(msg, &ws.self_user_id);
    if !(direct || mentioned) {
        return None;
    }

    let author = msg
        .user
        .as_deref()
        .map(|user| ws.display_name(user))
        .unwrap_or_else(|| msg.bot_id.clone().unwrap_or_else(|| "Slack".to_owned()));
    let channel_label = ws
        .channels
        .get(channel)
        .map(crate::state::channel_label)
        .unwrap_or_else(|| channel.to_owned());
    let title = if direct {
        author
    } else {
        format!("{author} in {channel_label}")
    };
    let body = ui::blocks::notification_text(ws, msg);
    let body = if body.trim().is_empty() {
        "[message]".to_owned()
    } else {
        body
    };

    Some(DesktopNotification { title, body })
}

fn message_mentions_user(msg: &SlackMessage, user: &str) -> bool {
    if user.is_empty() {
        return false;
    }
    let encoded = format!("<@{user}>");
    msg.text
        .as_deref()
        .is_some_and(|text| text.contains(&encoded))
        || msg
            .blocks
            .iter()
            .any(|block| value_mentions_user(block, user, &encoded))
}

fn value_mentions_user(value: &serde_json::Value, user: &str, encoded: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s == user || s.contains(encoded),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| value_mentions_user(value, user, encoded)),
        serde_json::Value::Object(map) => {
            matches!(
                map.get("user_id")
                    .or_else(|| map.get("user"))
                    .and_then(serde_json::Value::as_str),
                Some(found) if found == user
            ) || map
                .values()
                .any(|value| value_mentions_user(value, user, encoded))
        }
        _ => false,
    }
}

fn apply_realtime(
    app: &mut App,
    team: &str,
    generation: u64,
    event: RtEvent,
) -> Option<DesktopNotification> {
    let now = Instant::now();
    let active_channel = (app.active_team.as_deref() == Some(team))
        .then(|| app.active_channel.clone())
        .flatten();
    let Some(ws) = app.workspaces.get_mut(team) else {
        tracing::debug!(%team, "realtime event for unknown workspace, ignoring");
        return None;
    };
    if generation != ws.rt_generation {
        tracing::debug!(%team, generation, current = ws.rt_generation, "stale realtime event, ignoring");
        return None;
    }
    match event {
        RtEvent::Message(msg) => {
            let Some(channel) = msg.channel.clone() else {
                return None;
            };
            let notification =
                notification_for_message(ws, &channel, &msg, active_channel.as_deref());
            if let Some(user) = msg.user.as_deref() {
                ws.clear_typing_user(&channel, user);
            }
            if let Some(root_ts) = thread_root_for_reply(&msg) {
                let key = (team.to_owned(), channel.clone(), root_ts);
                if let Some(cm) = app.threads.get_mut(&key) {
                    upsert_realtime_message(cm, msg.clone());
                }
                if msg.subtype.as_deref() != Some("thread_broadcast") {
                    return notification;
                }
            }
            let cm = ws.messages.entry(channel).or_default();
            upsert_realtime_message(cm, msg);
            notification
        }
        RtEvent::MessageChanged { channel, message } => {
            if let Some(root_ts) = thread_root_for_reply(&message) {
                if let Some(cm) = app
                    .threads
                    .get_mut(&(team.to_owned(), channel.clone(), root_ts))
                {
                    cm.merge_update(message.clone());
                }
                if message.subtype.as_deref() != Some("thread_broadcast") {
                    return None;
                }
            }
            ws.messages
                .entry(channel)
                .or_default()
                .merge_update(message);
            None
        }
        RtEvent::MessageDeleted {
            channel,
            deleted_ts,
        } => {
            if let Some(cm) = ws.messages.get_mut(&channel) {
                cm.remove(&deleted_ts);
            }
            for ((thread_team, thread_channel, _), cm) in &mut app.threads {
                if thread_team == team && thread_channel == &channel {
                    cm.remove(&deleted_ts);
                }
            }
            None
        }
        RtEvent::UserTyping { channel, user } => {
            ws.set_typing(&channel, user, now);
            None
        }
        RtEvent::PresenceChange { user, presence } => {
            ws.set_presence(user, Presence::from_slack(&presence));
            None
        }
        RtEvent::ReactionAdded {
            channel,
            ts,
            user,
            reaction,
        } => {
            ws.messages
                .entry(channel.clone())
                .or_default()
                .apply_reaction(&ts, &user, &reaction, true);
            apply_thread_reaction(
                &mut app.threads,
                team,
                &channel,
                &ts,
                &user,
                &reaction,
                true,
            );
            None
        }
        RtEvent::ReactionRemoved {
            channel,
            ts,
            user,
            reaction,
        } => {
            if let Some(cm) = ws.messages.get_mut(&channel) {
                cm.apply_reaction(&ts, &user, &reaction, false);
            }
            apply_thread_reaction(
                &mut app.threads,
                team,
                &channel,
                &ts,
                &user,
                &reaction,
                false,
            );
            None
        }
        RtEvent::Unknown(raw) => {
            tracing::debug!(kind = %raw.kind, "unknown realtime event, ignoring");
            None
        }
    }
}

fn upsert_realtime_message(cm: &mut ChannelMessages, msg: SlackMessage) {
    if let Some(cid) = msg.client_msg_id.clone() {
        if cm.confirm(&cid, msg.clone()) {
            return;
        }
    }
    if cm.confirm_matching_pending(msg.user.as_deref(), msg.text.as_deref(), msg.clone()) {
        return;
    }
    cm.upsert(msg);
}

fn thread_root_for_reply(msg: &SlackMessage) -> Option<MessageTs> {
    let root_ts = msg.thread_ts.as_deref()?;
    let ts = msg.ts.as_deref()?;
    (root_ts != ts).then(|| root_ts.to_owned())
}

fn apply_thread_reaction(
    threads: &mut HashMap<ThreadKey, ChannelMessages>,
    team: &str,
    channel: &str,
    ts: &str,
    user: &str,
    reaction: &str,
    added: bool,
) {
    for ((thread_team, thread_channel, _), cm) in threads {
        if thread_team == team && thread_channel == channel {
            cm.apply_reaction(ts, user, reaction, added);
        }
    }
}

fn reaction_has_user_in(cm: &ChannelMessages, ts: &str, name: &str, user: &str) -> Option<bool> {
    let message = cm.messages.iter().find(|m| m.ts.as_deref() == Some(ts))?;
    let reaction = message.reactions.iter().find(|r| r.name == name)?;
    Some(crate::state::reaction_has_user(reaction, user))
}

fn workspace_cacheable_event(event: &RtEvent) -> bool {
    matches!(
        event,
        RtEvent::Message(_)
            | RtEvent::MessageChanged { .. }
            | RtEvent::MessageDeleted { .. }
            | RtEvent::ReactionAdded { .. }
            | RtEvent::ReactionRemoved { .. }
    )
}

fn mark_workspace_dirty(app: &mut App, team: &str) {
    if app.cache.is_none() || !app.workspaces.contains_key(team) {
        return;
    };
    app.cache_dirty.insert(team.to_owned(), Instant::now());
}

fn flush_due_cache(app: &mut App, now: Instant) -> Task<Message> {
    if app.cache.is_none() {
        app.cache_dirty.clear();
        app.cache_saving.clear();
        return Task::none();
    }

    let due: Vec<_> = app
        .cache_dirty
        .iter()
        .filter(|(team, dirty_at)| {
            now.duration_since(**dirty_at) >= CACHE_SAVE_DEBOUNCE
                && !app.cache_saving.contains_key(*team)
        })
        .map(|(team, _)| team.clone())
        .collect();

    let tasks = due.into_iter().filter_map(|team| {
        let workspace = app.workspaces.get(&team)?.clone();
        let started_at = now;
        app.cache_saving.insert(team.clone(), started_at);
        Some(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    Cache::open_default()
                        .and_then(|cache| cache.save_workspace(&workspace))
                        .map_err(|e| e.to_string())
                })
                .await
                .unwrap_or_else(|e| Err(format!("cache save task failed: {e}")))
            },
            move |result| Message::CacheSaved {
                team: team.clone(),
                started_at,
                result,
            },
        ))
    });

    Task::batch(tasks)
}
