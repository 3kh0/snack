use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::operation::{self, RelativeOffset};
use unicode_segmentation::UnicodeSegmentation;

use crate::cache::Cache;
use crate::config;
use crate::slack::api::{self, SearchArgs};
use crate::slack::events::RtEvent;
use crate::slack::models::{
    Channel, ChannelId, Emoji, Message as SlackMessage, MessageTs, SearchMessagesPage, TeamId,
    UserId,
};
use crate::slack::realtime;
use crate::slack::{Error as SlackError, Transport};
use crate::state::{ChannelMessages, Presence, RealtimeStatus, Screen, Workspace};
use crate::ui;

use super::palette::{self, PaletteEntry, PaletteState, PaletteTarget};
use super::{
    App, ComposerAttachment, ComposerTarget, DesktopNotification, FilePreview, HistoryLoadKind,
    Message, PendingFileMessage, PendingScrollTarget, SearchHit, SearchState, TextSelection,
    TextSelectionPoint, TextSelectionSurface, ThreadKey,
};
use iced::widget::text_editor::Content;

const CACHE_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);
const LOAD_OLDER_SCROLL_TOP_PX: f32 = 48.0;

pub(super) fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::WorkspaceSelected(team) => select_workspace(app, team),

        Message::ChannelSelected(id) => {
            app.search = None;
            app.text_selection = None;
            let same_channel = app.active_channel.as_deref() == Some(id.as_str());
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
                app.last_active_channels.insert(team.clone(), id.clone());
                if let Some(ws) = app.workspaces.get_mut(&team) {
                    ws.last_active_channel = Some(id.clone());
                    ws.touch_recent(&id);
                    ws.record_visit(&id, crate::state::now_secs());
                }
                mark_workspace_dirty(app, &team);
                if !same_channel {
                    app.pending_scroll_to = channel_open_scroll_target(app, &team, &id)
                        .map(|target| (id.clone(), target));
                }
            }
            if app
                .active_thread
                .as_ref()
                .is_some_and(|(channel, _)| channel != &id)
            {
                app.active_thread = None;
                app.thread_open = false;
                app.thread_composer = Content::new();
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
                    hydrate_visible_channels(app, &team, &id),
                    mark_latest_visible(app, &team, &id),
                    load_visible_file_previews(app, &team, &id),
                    load_visible_avatar_previews(app, &team, &id),
                    hydrate_visible_emojis(app, &team, &id),
                    load_visible_emoji_previews(app, &team, &id),
                    scroll_to_pending(app, &id),
                ]);
            }
            Task::none()
        }

        Message::ThreadOpened { channel, ts } => {
            app.text_selection = None;
            app.active_channel = Some(channel.clone());
            app.active_thread = Some((channel.clone(), ts.clone()));
            app.thread_open = true;
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
                    hydrate_thread_emojis(app, &team, &channel, &ts),
                    load_thread_emoji_previews(app, &team, &channel, &ts),
                ])
            }
        }

        Message::ThreadClosed => {
            app.text_selection = None;
            app.thread_open = false;
            Task::none()
        }

        Message::ThreadDismissed => {
            if !app.thread_open {
                app.active_thread = None;
                app.thread_composer = Content::new();
            }
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
                        hydrate_message_channels(app, &team, &messages),
                        hydrate_emojis(app, &team, &messages),
                        load_avatar_previews(app, &team, messages),
                        load_thread_file_previews(app, &team, &channel, &root_ts),
                        load_thread_emoji_previews(app, &team, &channel, &root_ts),
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

        Message::ComposerAction { target, action } => {
            let is_edit = action.is_edit();
            match target {
                ComposerTarget::Channel => app.composer.perform(action),
                ComposerTarget::Thread => app.thread_composer.perform(action),
            }
            if is_edit && target == ComposerTarget::Channel {
                maybe_send_typing(app);
            }
            Task::none()
        }

        Message::ComposerFormat { target, mark } => {
            match target {
                ComposerTarget::Channel => {
                    ui::composer::apply_format(&mut app.composer, mark);
                    maybe_send_typing(app);
                }
                ComposerTarget::Thread => {
                    ui::composer::apply_format(&mut app.thread_composer, mark);
                }
            }
            Task::none()
        }

        Message::AttachmentPickerOpened(target) => Task::perform(
            async move {
                rfd::AsyncFileDialog::new()
                    .set_title("Attach files")
                    .pick_files()
                    .await
                    .into_iter()
                    .flatten()
                    .map(|file| file.path().to_owned())
                    .collect()
            },
            move |paths| Message::AttachmentsPicked { target, paths },
        ),

        Message::AttachmentsPicked { target, paths } => {
            let video_paths = paths
                .iter()
                .filter(|path| is_video(path))
                .cloned()
                .collect::<Vec<_>>();
            add_attachments(app, target, paths);
            video_preview_tasks(video_paths)
        }

        Message::FilesDropped(paths) => {
            let video_paths = paths
                .iter()
                .filter(|path| is_video(path))
                .cloned()
                .collect::<Vec<_>>();
            let target = if app.active_thread.is_some() {
                ComposerTarget::Thread
            } else {
                ComposerTarget::Channel
            };
            add_attachments(app, target, paths);
            video_preview_tasks(video_paths)
        }

        Message::VideoPreviewReady { source, result } => {
            if let Ok(preview_path) = result {
                for attachment in app
                    .composer_attachments
                    .iter_mut()
                    .chain(app.thread_composer_attachments.iter_mut())
                    .chain(
                        app.pending_file_messages
                            .iter_mut()
                            .flat_map(|pending| pending.attachments.iter_mut()),
                    )
                    .filter(|attachment| attachment.path == source)
                {
                    attachment.preview_path = Some(preview_path.clone());
                }
            }
            Task::none()
        }

        Message::AttachmentRemoved { target, id } => {
            for attachment in attachments_mut(app, target)
                .iter()
                .filter(|attachment| attachment.uploading)
            {
                if let Some(cancel) = &attachment.upload_cancel {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            attachments_mut(app, target).retain(|attachment| attachment.id != id);
            Task::none()
        }

        Message::PasteAttachmentsRequested(target) => {
            iced::clipboard::read_files().map(move |result| Message::ClipboardFilesRead {
                target,
                result: result
                    .map(|files| files.iter().cloned().collect())
                    .map_err(|error| format!("{error:?}")),
            })
        }

        Message::ClipboardFilesRead { target, result } => match result {
            Ok(paths) if !paths.is_empty() => {
                add_attachments(app, target, paths);
                Task::none()
            }
            _ => iced::clipboard::read_text().map(move |result| Message::ClipboardTextRead {
                target,
                result: result
                    .map(|text| text.as_ref().clone())
                    .map_err(|error| format!("{error:?}")),
            }),
        },

        Message::ClipboardTextRead { target, result } => {
            if let Ok(text) = result {
                let content = match target {
                    ComposerTarget::Channel => &mut app.composer,
                    ComposerTarget::Thread => &mut app.thread_composer,
                };
                content.perform(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Paste(Arc::new(text)),
                ));
            }
            Task::none()
        }

        Message::AttachmentsSent {
            target,
            team,
            channel,
            thread_ts,
            message_ts,
            client_msg_id,
            result,
        } => {
            let pending_index = app
                .pending_file_messages
                .iter()
                .position(|pending| pending.client_msg_id == client_msg_id);
            match result {
                Ok(()) => {
                    if let Some(index) = pending_index {
                        for attachment in &mut app.pending_file_messages[index].attachments {
                            attachment.uploading = false;
                            attachment.upload_started = None;
                            if let Some(progress) = &attachment.upload_progress {
                                progress.store(attachment.bytes, Ordering::Relaxed);
                            }
                        }
                    }
                    mark_workspace_dirty(app, &team);
                }
                Err(error) => {
                    let pending =
                        pending_index.map(|index| app.pending_file_messages.remove(index));
                    let messages = match thread_ts.as_ref() {
                        Some(root_ts) => {
                            app.threads
                                .get_mut(&(team.clone(), channel.clone(), root_ts.clone()))
                        }
                        None => app
                            .workspaces
                            .get_mut(&team)
                            .and_then(|ws| ws.messages.get_mut(&channel)),
                    };
                    if let Some(messages) = messages {
                        messages.remove(&message_ts);
                    }
                    let mut attachments = pending
                        .map(|message| message.attachments)
                        .unwrap_or_default();
                    for attachment in &mut attachments {
                        attachment.uploading = false;
                        attachment.upload_started = None;
                        attachment.upload_cancel = None;
                        attachment.upload_progress = None;
                    }
                    attachments_mut(app, target).extend(attachments);
                    if !matches!(error, SlackError::UploadCanceled) {
                        app.toast(format!("file upload failed: {error}"));
                    }
                }
            }
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
                let mut self_user = None;
                if let Some(ws) = app.workspaces.get_mut(&team) {
                    ws.apply_boot(boot);
                    self_user = Some(ws.self_user_id.clone());
                    tracing::info!(%team, channels = ws.channels.len(), "boot ok");
                }
                mark_workspace_dirty(app, &team);
                app.screen = Screen::Main;
                let mut tasks = vec![
                    hydrate_sidebar_channels(app, &team),
                    hydrate_sidebar_dm_users(app, &team),
                ];
                if let Some(self_user) = self_user {
                    tasks.push(load_user_avatar_previews(app, &team, vec![self_user]));
                }
                if app.active_team.as_deref() == Some(&team) && app.active_channel.is_none() {
                    if let Some(channel) = preferred_channel(app, &team) {
                        app.active_channel = Some(channel.clone());
                        if let Some(ws) = app.workspaces.get_mut(&team) {
                            ws.last_active_channel = Some(channel.clone());
                        }
                        app.pending_scroll_to = channel_open_scroll_target(app, &team, &channel)
                            .map(|target| (channel.clone(), target));
                        tasks.push(app.load_history(&team, &channel));
                    }
                }
                if let Some(channel) = app
                    .active_channel
                    .clone()
                    .filter(|_| app.active_team.as_deref() == Some(&team))
                {
                    tasks.extend([
                        hydrate_visible_missing_users(app, &team, &channel),
                        load_visible_file_previews(app, &team, &channel),
                        load_visible_avatar_previews(app, &team, &channel),
                        hydrate_visible_emojis(app, &team, &channel),
                        load_visible_emoji_previews(app, &team, &channel),
                    ]);
                }
                Task::batch(tasks)
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
                    return Task::batch([
                        hydrate_sidebar_channels(app, &team),
                        hydrate_sidebar_dm_users(app, &team),
                    ]);
                }
                Err(e) => tracing::warn!(%team, error = %e, "counts failed"),
            }
            Task::none()
        }

        Message::SidebarDmsLoaded(team, result) => {
            match result {
                Ok(dms) => {
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        ws.apply_sidebar_dms(dms);
                    }
                    mark_workspace_dirty(app, &team);
                    return hydrate_sidebar_dm_users(app, &team);
                }
                Err(e) => tracing::warn!(%team, error = %e, "sidebar.dms failed"),
            }
            Task::none()
        }

        Message::HistoryLoaded(team, channel, kind, result) => {
            match result {
                Ok(page) => {
                    let has_more = page.has_more;
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
                        if matches!(
                            kind,
                            HistoryLoadKind::Latest
                                | HistoryLoadKind::Around
                                | HistoryLoadKind::Older
                        ) {
                            cm.has_more_older = has_more;
                        }
                        if kind == HistoryLoadKind::Older {
                            cm.history_loading_older = false;
                        }
                        tracing::info!(%channel, messages = n, "history loaded");
                    }
                    mark_workspace_dirty(app, &team);
                    let mut tasks = vec![
                        hydrate_missing_users(app, &team, &messages),
                        hydrate_message_channels(app, &team, &messages),
                        hydrate_emojis(app, &team, &messages),
                        load_avatar_previews(app, &team, messages),
                        load_visible_file_previews(app, &team, &channel),
                        load_visible_emoji_previews(app, &team, &channel),
                        scroll_to_pending(app, &channel),
                    ];
                    if kind != HistoryLoadKind::Older {
                        tasks.push(mark_latest_visible(app, &team, &channel));
                    }
                    return Task::batch(tasks);
                }
                Err(e) => {
                    if kind == HistoryLoadKind::Older {
                        if let Some(cm) = app
                            .workspaces
                            .get_mut(&team)
                            .and_then(|ws| ws.messages.get_mut(&channel))
                        {
                            cm.history_loading_older = false;
                        }
                        if app
                            .pending_scroll_to
                            .as_ref()
                            .is_some_and(|(pending_channel, _)| pending_channel == &channel)
                        {
                            app.pending_scroll_to = None;
                        }
                    }
                    app.toast(format!("history failed for {channel}: {e}"));
                }
            }
            Task::none()
        }

        Message::ChannelScrolled { channel, y } => channel_scrolled(app, channel, y),

        Message::ChannelMarked(team, channel, ts, result) => {
            app.pending_marks
                .remove(&(team.clone(), channel.clone(), ts.clone()));
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
                Err(e) => {
                    if is_permanent_mark_error(&e) {
                        app.mark_blocked.insert((team.clone(), channel.clone()));
                        tracing::warn!(
                            %team,
                            %channel,
                            error = %e,
                            "mark failed permanently; blocking further attempts this session"
                        );
                    } else {
                        tracing::warn!(%team, %channel, error = %e, "mark failed");
                    }
                }
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

        Message::CopyMessage(text) => iced::clipboard::write(text).discard(),

        Message::TextSelectionStarted(point) => {
            app.text_selection = Some(TextSelection {
                anchor: point.clone(),
                focus: point,
                dragging: true,
            });
            Task::none()
        }

        Message::TextSelectionDragged(point) => {
            if let Some(selection) = app
                .text_selection
                .as_mut()
                .filter(|selection| selection.dragging && selection.anchor.surface == point.surface)
            {
                selection.focus = point;
            }
            Task::none()
        }

        Message::TextSelectionEnded => {
            if let Some(selection) = app.text_selection.as_mut() {
                if selection.anchor == selection.focus {
                    app.text_selection = None;
                } else {
                    selection.dragging = false;
                }
            }
            Task::none()
        }

        Message::TextSelectionCopyRequested => match selected_text(app) {
            Some(text) if !text.is_empty() => iced::clipboard::write(text).discard(),
            _ => Task::none(),
        },

        Message::MessageHovered { in_thread, ts } => {
            app.hovered_message = Some((in_thread, ts));
            Task::none()
        }

        Message::MessageUnhovered => {
            app.hovered_message = None;
            Task::none()
        }

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

        Message::PaletteToggled => palette_toggled(app),
        Message::PaletteClosed => {
            app.palette_open = false;
            Task::none()
        }
        Message::PaletteDismissed => {
            if !app.palette_open {
                app.palette = None;
            }
            Task::none()
        }
        Message::PaletteQueryChanged(query) => palette_query_changed(app, query),
        Message::PaletteMoved(delta) => {
            palette_moved(app, delta);
            Task::none()
        }
        Message::PaletteSubmitted => palette_activate_selected(app),
        Message::PaletteEntryPressed(index) => palette_activate(app, index),
        Message::PaletteRemoteUsersLoaded { team, seq, result } => {
            palette_remote_users_loaded(app, team, seq, result)
        }
        Message::PaletteRemoteChannelsLoaded { team, seq, result } => {
            palette_remote_channels_loaded(app, team, seq, result)
        }
        Message::DmOpened { team, user, result } => dm_opened(app, team, user, result),

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

        Message::EmojiPreviewLoaded { key, result } => {
            match result {
                Ok(preview) => {
                    app.emoji_previews.insert(key, preview);
                }
                Err(e) => {
                    tracing::debug!(%key, error = %e, "emoji preview failed");
                    app.emoji_previews.insert(key, FilePreview::Failed);
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

        Message::EmojisLoaded {
            team,
            requested,
            result,
        } => {
            app.emoji_hydrated
                .extend(requested.iter().map(|name| (team.clone(), name.clone())));
            match result {
                Ok(emojis) => {
                    let names: Vec<_> = emojis.iter().map(|emoji| emoji.name.clone()).collect();
                    let alias_targets: Vec<_> = emojis
                        .iter()
                        .filter_map(|emoji| emoji.value.strip_prefix("alias:"))
                        .filter(|name| !crate::state::is_standard_emoji(name))
                        .map(str::to_owned)
                        .collect();
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        ws.apply_emojis(emojis);
                    }
                    return Task::batch([
                        load_emoji_previews_for_names(app, &team, names),
                        hydrate_emoji_names(app, &team, alias_targets),
                    ]);
                }
                Err(e) => tracing::debug!(%team, error = %e, "emojis info failed"),
            }
            Task::none()
        }

        Message::ChannelsLoaded {
            team,
            requested,
            result,
        } => {
            app.channel_hydrated
                .extend(requested.into_iter().map(|channel| (team.clone(), channel)));
            match result {
                Ok(channels) => {
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        ws.apply_channels_info(channels);
                    }
                    mark_workspace_dirty(app, &team);
                }
                Err(e) => tracing::debug!(%team, error = %e, "channels info failed"),
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
                    tasks.push(hydrate_visible_channels(app, &team, &channel));
                    tasks.push(mark_latest_visible(app, &team, &channel));
                    tasks.push(load_visible_file_previews(app, &team, &channel));
                    tasks.push(load_visible_avatar_previews(app, &team, &channel));
                    tasks.push(hydrate_visible_emojis(app, &team, &channel));
                    tasks.push(load_visible_emoji_previews(app, &team, &channel));
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

        Message::AccountMenuToggled => {
            if app.account_menu_open {
                app.account_menu_open = false;
            } else {
                app.show_account_menu = true;
                app.account_menu_open = true;
            }
            Task::none()
        }

        Message::AccountMenuDismissed => {
            if !app.account_menu_open {
                app.show_account_menu = false;
            }
            Task::none()
        }

        Message::SelfPresenceSelected(presence) => set_self_presence(app, presence),

        Message::SelfPresenceUpdated {
            team,
            presence,
            previous,
            result,
        } => {
            if let Err(e) = result {
                if let Some(ws) = app.workspaces.get_mut(&team) {
                    if let Some(previous) = previous {
                        ws.set_presence(ws.self_user_id.clone(), previous);
                    } else {
                        let self_user = ws.self_user_id.clone();
                        ws.presence.remove(&self_user);
                    }
                }
                app.toast(format!("presence update failed: {e}"));
                if is_auth_error(&e) {
                    app.screen = Screen::Login;
                }
                mark_workspace_dirty(app, &team);
            } else {
                tracing::debug!(%team, ?presence, "presence updated");
            }
            Task::none()
        }

        Message::SignOutPressed => sign_out(app),

        Message::SettingsOpened => {
            app.account_menu_open = false;
            app.show_settings = true;
            app.settings_open = true;
            Task::none()
        }

        Message::SettingsClosed => {
            app.settings_open = false;
            Task::none()
        }

        Message::SettingsDismissed => {
            if !app.settings_open {
                app.show_settings = false;
            }
            Task::none()
        }

        Message::SettingsAccentSelected(accent) => {
            app.settings.accent = accent;
            apply_settings(app);
            Task::none()
        }

        Message::SettingsGapChanged(value) => {
            app.settings.gap = value;
            apply_settings(app);
            Task::none()
        }

        Message::SettingsRadiusChanged(value) => {
            app.settings.panel_radius = value;
            apply_settings(app);
            Task::none()
        }

        Message::SettingsBorderChanged(value) => {
            app.settings.border_thickness = value;
            apply_settings(app);
            Task::none()
        }

        Message::SettingsReset => {
            app.settings = config::Settings::default();
            apply_settings(app);
            Task::none()
        }

        Message::SidebarResizeStarted => {
            app.sidebar_resizing = true;
            app.sidebar_resize_prev_x = None;
            Task::none()
        }

        Message::SidebarResizeMoved(x) => {
            if app.sidebar_resizing {
                if let Some(prev) = app.sidebar_resize_prev_x {
                    let next = (app.settings.sidebar_width + (x - prev))
                        .clamp(config::SIDEBAR_WIDTH_MIN, config::SIDEBAR_WIDTH_MAX);
                    app.settings.sidebar_width = next;
                }
                app.sidebar_resize_prev_x = Some(x);
            }
            Task::none()
        }

        Message::SidebarResizeEnded => {
            if app.sidebar_resizing {
                app.sidebar_resizing = false;
                app.sidebar_resize_prev_x = None;
                if let Err(e) = config::save_settings(&app.settings) {
                    app.toast(format!("could not save settings: {e}"));
                }
            }
            Task::none()
        }

        Message::AnimationTick => Task::none(),

        Message::Tick => {
            let now = Instant::now();
            if let Some(ws) = app.active_workspace_mut() {
                ws.prune_typing(now, Duration::from_secs(4));
            }
            flush_due_cache(app, now)
        }
    }
}

fn apply_settings(app: &mut App) {
    ui::theme::apply(&app.settings);
    if let Err(e) = config::save_settings(&app.settings) {
        app.toast(format!("could not save settings: {e}"));
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
            .insert(current_team.clone(), current_channel.clone());
        if let Some(ws) = app.workspaces.get_mut(&current_team) {
            ws.last_active_channel = Some(current_channel);
        }
        mark_workspace_dirty(app, &current_team);
    }

    app.active_team = Some(team.clone());
    app.show_account_menu = false;
    app.account_menu_open = false;
    app.active_channel = preferred_channel(app, &team);
    if let (Some(ws), Some(channel)) = (app.workspaces.get_mut(&team), app.active_channel.clone()) {
        ws.last_active_channel = Some(channel);
        mark_workspace_dirty(app, &team);
    }
    app.active_thread = None;
    app.thread_open = false;
    app.search = None;
    app.search_input.clear();
    app.editing = None;
    app.edit_text.clear();
    app.text_selection = None;
    app.composer = Content::new();
    app.thread_composer = Content::new();

    let Some(channel) = app.active_channel.clone() else {
        return Task::none();
    };
    app.pending_scroll_to =
        channel_open_scroll_target(app, &team, &channel).map(|target| (channel.clone(), target));
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
        Task::batch([
            mark_latest_visible(app, &team, &channel),
            scroll_to_pending(app, &channel),
        ])
    }
}

fn sign_out(app: &mut App) -> Task<Message> {
    if let Err(e) = config::clear_session() {
        app.toast(format!("could not sign out: {e}"));
        return Task::none();
    }

    app.session = None;
    app.cache = None;
    app.transport = None;
    app.active_team = None;
    app.active_channel = None;
    app.active_thread = None;
    app.thread_open = false;
    app.workspaces.clear();
    app.threads.clear();
    app.composer = Content::new();
    app.thread_composer = Content::new();
    app.editing = None;
    app.edit_text.clear();
    app.hovered_message = None;
    app.search_input.clear();
    app.search = None;
    app.file_previews.clear();
    app.avatar_previews.clear();
    app.emoji_previews.clear();
    app.emoji_hydrated.clear();
    app.avatar_profile_hydrated.clear();
    app.pending_scroll_to = None;
    app.cache_dirty.clear();
    app.cache_saving.clear();
    app.show_account_menu = false;
    app.account_menu_open = false;
    app.show_settings = false;
    app.settings_open = false;
    app.palette = None;
    app.palette_open = false;
    app.screen = Screen::Login;
    Task::none()
}

fn set_self_presence(app: &mut App, presence: Presence) -> Task<Message> {
    let Some(team) = app.active_team.clone() else {
        app.account_menu_open = false;
        return Task::none();
    };
    let previous = app
        .workspaces
        .get(&team)
        .and_then(|ws| ws.presence.get(&ws.self_user_id).copied());
    if let Some(ws) = app.workspaces.get_mut(&team) {
        ws.set_presence(ws.self_user_id.clone(), presence);
    }
    app.account_menu_open = false;
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
    let slack_presence = slack_presence_value(presence).to_owned();
    Task::perform(
        async move { api::set_presence(&transport, &client, &ws_session, slack_presence).await },
        move |result| Message::SelfPresenceUpdated {
            team: team.clone(),
            presence,
            previous,
            result,
        },
    )
}

fn slack_presence_value(presence: Presence) -> &'static str {
    match presence {
        Presence::Active => "auto",
        Presence::Away | Presence::Unknown => "away",
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
    if let Some(channel) = ws
        .last_active_channel
        .as_ref()
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

fn attachments_mut(app: &mut App, target: ComposerTarget) -> &mut Vec<ComposerAttachment> {
    match target {
        ComposerTarget::Channel => &mut app.composer_attachments,
        ComposerTarget::Thread => &mut app.thread_composer_attachments,
    }
}

fn add_attachments(app: &mut App, target: ComposerTarget, paths: Vec<PathBuf>) {
    for path in paths {
        if !path.is_file()
            || attachments_mut(app, target)
                .iter()
                .any(|attachment| attachment.path == path)
        {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        app.attachment_seq += 1;
        let id = app.attachment_seq;
        attachments_mut(app, target).push(ComposerAttachment {
            id,
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment")
                .to_owned(),
            path,
            bytes: metadata.len(),
            uploading: false,
            upload_started: None,
            upload_cancel: None,
            upload_progress: None,
            preview_path: None,
        });
    }
}

fn is_video(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if matches!(
            extension.to_ascii_lowercase().as_str(),
            "mp4" | "mov" | "m4v" | "webm"
        )
    )
}

fn is_local_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if matches!(
            extension.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp"
        )
    )
}

fn video_preview_tasks(paths: Vec<PathBuf>) -> Task<Message> {
    Task::batch(paths.into_iter().map(|source| {
        Task::perform(video_thumbnail(source.clone()), move |result| {
            Message::VideoPreviewReady {
                source: source.clone(),
                result,
            }
        })
    }))
}

async fn video_thumbnail(source: PathBuf) -> Result<PathBuf, String> {
    let output =
        std::env::temp_dir().join(format!("snack-video-preview-{}.jpg", uuid::Uuid::new_v4()));
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-ss",
            "0.1",
            "-i",
            source.to_string_lossy().as_ref(),
            "-frames:v",
            "1",
            "-vf",
            "scale=128:-1",
            output.to_string_lossy().as_ref(),
        ])
        .status()
        .await
        .map_err(|error| format!("start ffmpeg: {error}"))?;
    if status.success() {
        Ok(output)
    } else {
        Err("ffmpeg could not extract a video preview".to_owned())
    }
}

fn is_auth_error(e: &SlackError) -> bool {
    matches!(e, SlackError::Api(code) if code == "invalid_auth" || code == "not_authed" || code == "token_revoked")
}

fn maybe_send_typing(app: &mut App) {
    if app.composer.text().trim().is_empty() {
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

fn mark_latest_visible(app: &mut App, team: &str, channel: &ChannelId) -> Task<Message> {
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
    if app.live().is_none() {
        return Task::none();
    }
    if !begin_mark(app, team, channel, &latest) {
        return Task::none();
    }
    app.mark_channel_read(team, channel, latest)
}

pub(super) fn begin_mark(app: &mut App, team: &str, channel: &ChannelId, ts: &MessageTs) -> bool {
    let team_channel = (team.to_owned(), channel.clone());
    if app.mark_blocked.contains(&team_channel) {
        return false;
    }
    let key = (team.to_owned(), channel.clone(), ts.clone());
    if app.pending_marks.contains(&key) {
        return false;
    }
    app.pending_marks.insert(key);
    true
}

pub(super) fn is_permanent_mark_error(error: &SlackError) -> bool {
    match error {
        SlackError::Api(code) => matches!(
            code.as_str(),
            "channel_not_found" | "is_archived" | "not_in_channel" | "invalid_channel"
        ),
        _ => false,
    }
}

fn send_pressed(app: &mut App) -> Task<Message> {
    let text = app.composer.text().trim().to_owned();
    if text.is_empty() && app.composer_attachments.is_empty() {
        return Task::none();
    }
    let (Some(team), Some(channel)) = (app.active_team.clone(), app.active_channel.clone()) else {
        return Task::none();
    };

    if !app.composer_attachments.is_empty() {
        return send_attachments(app, ComposerTarget::Channel, team, channel, None, text);
    }

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
    app.composer = Content::new();
    mark_workspace_dirty(app, &team);
    app.pending_scroll_to = Some((channel.clone(), PendingScrollTarget::Latest));
    let scroll = scroll_to_pending(app, &channel);

    let Some((transport, session)) = app.live() else {
        return scroll;
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return scroll;
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let send_channel = channel.clone();
    let send = Task::perform(
        async move {
            api::send_message(&transport, &client, &ws_session, send_channel, text, None).await
        },
        move |result| Message::MessageSent {
            team: team.clone(),
            channel: channel.clone(),
            client_msg_id: client_msg_id.clone(),
            result,
        },
    );
    Task::batch([scroll, send])
}

fn send_thread_pressed(app: &mut App) -> Task<Message> {
    let text = app.thread_composer.text().trim().to_owned();
    if text.is_empty() && app.thread_composer_attachments.is_empty() {
        return Task::none();
    }
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    let Some((channel, root_ts)) = app.active_thread.clone() else {
        return Task::none();
    };

    if !app.thread_composer_attachments.is_empty() {
        return send_attachments(
            app,
            ComposerTarget::Thread,
            team,
            channel,
            Some(root_ts),
            text,
        );
    }

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
    app.thread_composer = Content::new();

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

fn send_attachments(
    app: &mut App,
    target: ComposerTarget,
    team: TeamId,
    channel: ChannelId,
    thread_ts: Option<MessageTs>,
    text: String,
) -> Task<Message> {
    let Some((transport, session)) = app.live() else {
        app.toast("file upload unavailable while offline");
        return Task::none();
    };
    let Some(workspace) = session.workspaces.get(&team) else {
        app.toast("file upload unavailable for this workspace");
        return Task::none();
    };
    let transport = transport.clone();
    let workspace = workspace.clone();
    let client = app.client.clone();
    let upload_cancel = Arc::new(AtomicBool::new(false));
    if attachments_mut(app, target)
        .iter()
        .any(|attachment| attachment.uploading)
    {
        return Task::none();
    }
    let mut attachments = std::mem::take(attachments_mut(app, target));
    let files = attachments
        .iter_mut()
        .map(|attachment| {
            attachment.uploading = true;
            attachment.upload_started = Some(Instant::now());
            attachment.upload_cancel = Some(upload_cancel.clone());
            let progress = Arc::new(AtomicU64::new(0));
            attachment.upload_progress = Some(progress.clone());
            (attachment.path.clone(), progress)
        })
        .collect::<Vec<_>>();
    let seq = next_seq(app);
    let client_msg_id = uuid::Uuid::new_v4().to_string();
    let message_ts = format!("{}.{:06}", chrono::Utc::now().timestamp(), seq);
    let Some(ws) = app.workspaces.get(&team) else {
        attachments_mut(app, target).extend(attachments);
        return Task::none();
    };
    let pending_message = SlackMessage {
        user: Some(ws.self_user_id.clone()),
        kind: Some("message".to_owned()),
        ts: Some(message_ts.clone()),
        client_msg_id: Some(client_msg_id.clone()),
        text: Some(text.clone()),
        channel: Some(channel.clone()),
        thread_ts: thread_ts.clone(),
        ..Default::default()
    };
    match thread_ts.as_ref() {
        Some(root_ts) => {
            let messages = app
                .threads
                .entry((team.clone(), channel.clone(), root_ts.clone()))
                .or_default();
            messages.upsert(pending_message);
            messages.pending.push(message_ts.clone());
        }
        None => {
            let messages = app
                .workspaces
                .get_mut(&team)
                .expect("workspace checked above")
                .messages
                .entry(channel.clone())
                .or_default();
            messages.upsert(pending_message);
            messages.pending.push(message_ts.clone());
        }
    }
    app.pending_file_messages.push(PendingFileMessage {
        team: team.clone(),
        channel: channel.clone(),
        thread_ts: thread_ts.clone(),
        message_ts: message_ts.clone(),
        client_msg_id: client_msg_id.clone(),
        text: text.clone(),
        attachments,
    });
    match target {
        ComposerTarget::Channel => app.composer = Content::new(),
        ComposerTarget::Thread => app.thread_composer = Content::new(),
    }
    mark_workspace_dirty(app, &team);
    let scroll = if target == ComposerTarget::Channel {
        app.pending_scroll_to = Some((channel.clone(), PendingScrollTarget::Latest));
        scroll_to_pending(app, &channel)
    } else {
        Task::none()
    };
    let sent_team = team.clone();
    let sent_channel = channel.clone();
    let sent_thread_ts = thread_ts.clone();
    let sent_message_ts = message_ts.clone();
    let sent_client_msg_id = client_msg_id.clone();
    let upload = Task::perform(
        async move {
            api::upload_files(
                &transport,
                &client,
                &workspace,
                channel,
                thread_ts,
                text,
                files,
                upload_cancel,
            )
            .await
        },
        move |result| Message::AttachmentsSent {
            target,
            team: sent_team.clone(),
            channel: sent_channel.clone(),
            thread_ts: sent_thread_ts.clone(),
            message_ts: sent_message_ts.clone(),
            client_msg_id: sent_client_msg_id.clone(),
            result,
        },
    );
    Task::batch([scroll, upload])
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

fn selected_text(app: &App) -> Option<String> {
    let selection = app.text_selection.as_ref()?;
    if selection.anchor.surface != selection.focus.surface {
        return None;
    }
    let ws = app.active_workspace()?;
    let messages = selected_surface_messages(app, ws, &selection.anchor.surface)?;

    let mut parts = Vec::new();
    for (index, msg) in messages.into_iter().enumerate() {
        let full = ui::message::selectable_copy_text(ws, msg);
        if full.is_empty() {
            continue;
        }
        let len = full.graphemes(true).count();
        if let Some((lo, hi)) = selection_range_for_index(selection, index, len) {
            let text = slice_graphemes(&full, lo, hi);
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }

    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn selected_surface_messages<'a>(
    app: &'a App,
    ws: &'a Workspace,
    surface: &TextSelectionSurface,
) -> Option<Vec<&'a SlackMessage>> {
    match surface {
        TextSelectionSurface::Channel { channel } => {
            let cm = ws.messages.get(channel)?;
            let mut visible: Vec<_> = cm
                .messages
                .iter()
                .rev()
                .filter(|m| crate::state::is_channel_timeline_visible(m))
                .take(ui::channel::VISIBLE_MESSAGE_LIMIT)
                .collect();
            visible.reverse();
            Some(visible)
        }
        TextSelectionSurface::Thread { channel, root_ts } => {
            let team = app.active_team.as_ref()?;
            let replies = app
                .threads
                .get(&(team.clone(), channel.clone(), root_ts.clone()));
            match replies {
                Some(cm) if !cm.messages.is_empty() => Some(cm.messages.iter().collect()),
                _ => ui::thread::root_message(ws, channel, root_ts).map(|root| vec![root]),
            }
        }
    }
}

fn selection_range_for_index(
    selection: &TextSelection,
    index: usize,
    len: usize,
) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let anchor = &selection.anchor;
    let focus = &selection.focus;
    let start_index = anchor.message_index.min(focus.message_index);
    let end_index = anchor.message_index.max(focus.message_index);
    if index < start_index || index > end_index {
        return None;
    }

    let anchor_offset = anchor.offset.min(len - 1);
    let focus_offset = focus.offset.min(len - 1);
    let forward = anchor.message_index < focus.message_index
        || (anchor.message_index == focus.message_index && anchor.offset <= focus.offset);

    if anchor.message_index == focus.message_index {
        return Some((
            anchor_offset.min(focus_offset),
            anchor_offset.max(focus_offset),
        ));
    }
    if index != anchor.message_index && index != focus.message_index {
        return Some((0, len - 1));
    }
    if forward {
        if index == anchor.message_index {
            Some((anchor_offset, len - 1))
        } else {
            Some((0, focus_offset))
        }
    } else if index == focus.message_index {
        Some((focus_offset, len - 1))
    } else {
        Some((0, anchor_offset))
    }
}

fn slice_graphemes(value: &str, lo: usize, hi: usize) -> String {
    value
        .graphemes(true)
        .skip(lo)
        .take(hi.saturating_sub(lo) + 1)
        .collect()
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
    if let Some(ws) = app.workspaces.get_mut(&team) {
        ws.last_active_channel = Some(channel.clone());
    }
    mark_workspace_dirty(app, &team);
    app.editing = None;
    app.edit_text.clear();
    app.pending_scroll_to = Some((channel.clone(), PendingScrollTarget::Message(ts)));

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
            app.thread_open = true;
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
            app.thread_open = false;
        }
    }
    Task::batch(tasks)
}

fn palette_toggled(app: &mut App) -> Task<Message> {
    if app.palette_open {
        app.palette_open = false;
        return Task::none();
    }
    let entries = app
        .active_workspace()
        .map(|ws| palette::recents(ws, app.active_channel.as_deref()))
        .unwrap_or_default();
    app.palette = Some(PaletteState {
        entries,
        ..PaletteState::default()
    });
    app.palette_open = true;
    let team = app.active_team.clone();
    let avatars = team
        .map(|team| load_palette_avatar_previews(app, &team))
        .unwrap_or_else(Task::none);
    Task::batch([operation::focus(ui::palette::INPUT_ID), avatars])
}

fn palette_query_changed(app: &mut App, query: String) -> Task<Message> {
    let Some(ws) = app.active_workspace() else {
        return Task::none();
    };
    let remote = std::collections::BTreeMap::new();
    let entries = if query.trim().is_empty() {
        palette::recents(ws, app.active_channel.as_deref())
    } else {
        palette::rank(ws, &query, &remote)
    };
    let Some(state) = app.palette.as_mut() else {
        return Task::none();
    };
    state.query = query.clone();
    state.remote_channels = remote;
    state.entries = entries;
    state.selected = 0;
    state.remote_seq += 1;
    let seq = state.remote_seq;

    let local_avatars = match app.active_team.clone() {
        Some(team) => load_palette_avatar_previews(app, &team),
        None => Task::none(),
    };

    if query.trim().chars().count() < 2 {
        return local_avatars;
    }
    let Some((transport, session)) = app.live() else {
        return Task::none();
    };
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    let Some(ws_session) = session.workspaces.get(&team) else {
        return Task::none();
    };
    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let users_task = {
        let transport = transport.clone();
        let client = client.clone();
        let ws_session = ws_session.clone();
        let team = team.clone();
        let query = query.clone();
        Task::perform(
            async move { api::fetch_users_search(&transport, &client, &ws_session, query).await },
            move |result| Message::PaletteRemoteUsersLoaded {
                team: team.clone(),
                seq,
                result,
            },
        )
    };
    let channels_task = Task::perform(
        async move { api::fetch_channels_search(&transport, &client, &ws_session, query).await },
        move |result| Message::PaletteRemoteChannelsLoaded {
            team: team.clone(),
            seq,
            result,
        },
    );
    Task::batch([local_avatars, users_task, channels_task])
}

fn palette_moved(app: &mut App, delta: isize) {
    let Some(state) = app.palette.as_mut() else {
        return;
    };
    let len = state.entries.len();
    if len == 0 {
        return;
    }
    let cur = state.selected as isize;
    let next = (cur + delta).rem_euclid(len as isize);
    state.selected = next as usize;
}

fn palette_activate_selected(app: &mut App) -> Task<Message> {
    let index = app.palette.as_ref().map(|s| s.selected).unwrap_or(0);
    palette_activate(app, index)
}

fn palette_activate(app: &mut App, index: usize) -> Task<Message> {
    let Some(entry) = app
        .palette
        .as_ref()
        .and_then(|s| s.entries.get(index).cloned())
    else {
        return Task::none();
    };
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    match entry.target {
        PaletteTarget::Channel(id) => {
            if let Some(remote) = app
                .palette
                .as_ref()
                .and_then(|s| s.remote_channels.get(&id).cloned())
            {
                if let Some(ws) = app.workspaces.get_mut(&team) {
                    ws.channels.entry(id.clone()).or_insert(remote);
                }
            }
            app.palette_open = false;
            app.palette = None;
            Task::done(Message::ChannelSelected(id))
        }
        PaletteTarget::User { dm: Some(id), .. } => {
            app.palette_open = false;
            app.palette = None;
            Task::done(Message::ChannelSelected(id))
        }
        PaletteTarget::User { user, dm: None } => {
            app.palette_open = false;
            app.palette = None;
            let Some((transport, session)) = app.live() else {
                return Task::none();
            };
            let Some(ws_session) = session.workspaces.get(&team) else {
                return Task::none();
            };
            let transport = transport.clone();
            let client = app.client.clone();
            let ws_session = ws_session.clone();
            let dm_user = user.clone();
            Task::perform(
                async move { api::open_dm(&transport, &client, &ws_session, dm_user).await },
                move |result| Message::DmOpened {
                    team: team.clone(),
                    user: user.clone(),
                    result,
                },
            )
        }
    }
}

fn palette_remote_users_loaded(
    app: &mut App,
    team: TeamId,
    seq: u64,
    result: Result<Vec<crate::slack::models::User>, SlackError>,
) -> Task<Message> {
    if app.active_team.as_deref() != Some(&team) {
        return Task::none();
    }
    let users = match result {
        Ok(users) => users,
        Err(e) => {
            tracing::debug!(error = %e, "palette user search failed");
            return Task::none();
        }
    };
    if app.palette.as_ref().map(|s| s.remote_seq) != Some(seq) {
        return Task::none();
    }
    if let Some(ws) = app.workspaces.get_mut(&team) {
        for user in users {
            merge_searched_user(ws, user);
        }
    }
    mark_workspace_dirty(app, &team);
    rerank_palette(app, &team);
    load_palette_avatar_previews(app, &team)
}

fn palette_remote_channels_loaded(
    app: &mut App,
    team: TeamId,
    seq: u64,
    result: Result<Vec<Channel>, SlackError>,
) -> Task<Message> {
    if app.active_team.as_deref() != Some(&team) {
        return Task::none();
    }
    let channels = match result {
        Ok(channels) => channels,
        Err(e) => {
            tracing::debug!(error = %e, "palette channel search failed");
            return Task::none();
        }
    };
    if app.palette.as_ref().map(|s| s.remote_seq) != Some(seq) {
        return Task::none();
    }

    let mut known_updates = Vec::new();
    let mut remote_only = Vec::new();
    if let Some(ws) = app.workspaces.get(&team) {
        for channel in channels {
            if channel.is_archived || channel.is_im {
                continue;
            }
            if ws.channels.contains_key(&channel.id) {
                known_updates.push(channel);
            } else {
                remote_only.push(channel);
            }
        }
    } else {
        remote_only = channels
            .into_iter()
            .filter(|c| !c.is_archived && !c.is_im)
            .collect();
    }

    if let Some(ws) = app.workspaces.get_mut(&team) {
        for channel in known_updates {
            if let Some(existing) = ws.channels.get_mut(&channel.id) {
                if !channel.previous_names.is_empty() {
                    existing.previous_names = channel.previous_names;
                }
                if existing.name.as_ref().is_none_or(|n| n.trim().is_empty()) {
                    if channel.name.as_ref().is_some_and(|n| !n.trim().is_empty()) {
                        existing.name = channel.name;
                    }
                }
            }
        }
    }
    if let Some(state) = app.palette.as_mut() {
        for channel in remote_only {
            state.remote_channels.insert(channel.id.clone(), channel);
        }
    }
    mark_workspace_dirty(app, &team);
    rerank_palette(app, &team);
    Task::none()
}

fn rerank_palette(app: &mut App, team: &str) {
    let Some(state) = app.palette.as_ref() else {
        return;
    };
    let query = state.query.clone();
    let remote = state.remote_channels.clone();
    let active = app.active_channel.clone();
    let Some(ws) = app.workspaces.get(team) else {
        return;
    };
    let entries = if query.trim().is_empty() {
        palette::recents(ws, active.as_deref())
    } else {
        palette::rank(ws, &query, &remote)
    };
    if let Some(state) = app.palette.as_mut() {
        state.entries = entries;
        if state.selected >= state.entries.len() {
            state.selected = 0;
        }
    }
}

fn merge_searched_user(ws: &mut Workspace, incoming: crate::slack::models::User) {
    match ws.users.get_mut(&incoming.id) {
        Some(existing) if crate::state::user_avatar_url(existing).is_none() => {
            if crate::state::user_avatar_url(&incoming).is_some() {
                existing.profile = incoming.profile;
            }
        }
        Some(_) => {}
        None => {
            ws.users.insert(incoming.id.clone(), incoming);
        }
    }
}

fn load_palette_avatar_previews(app: &mut App, team: &str) -> Task<Message> {
    let Some(state) = app.palette.as_ref() else {
        return Task::none();
    };
    let users: Vec<UserId> = state
        .entries
        .iter()
        .filter_map(|entry| match &entry.target {
            PaletteTarget::User { user, .. } => Some(user.clone()),
            PaletteTarget::Channel(_) => None,
        })
        .collect();
    if users.is_empty() {
        return Task::none();
    }
    if let Some(crate::state::RealtimeStatus::Connected(conn)) =
        app.workspaces.get(team).map(|ws| &ws.rt)
    {
        conn.send(crate::slack::realtime::presence_query_frame(&users));
    }
    load_user_avatar_previews(app, team, users)
}

fn dm_opened(
    app: &mut App,
    team: TeamId,
    user: UserId,
    result: Result<ChannelId, SlackError>,
) -> Task<Message> {
    let channel = match result {
        Ok(channel) => channel,
        Err(e) => {
            app.toast(format!("could not open DM: {e}"));
            return Task::none();
        }
    };
    if let Some(ws) = app.workspaces.get_mut(&team) {
        ws.channels
            .entry(channel.clone())
            .or_insert_with(|| Channel {
                id: channel.clone(),
                is_im: true,
                user: Some(user.clone()),
                ..Default::default()
            });
        if !ws.dm_order.iter().any(|id| id == &channel) {
            ws.dm_order.push(channel.clone());
        }
    }
    mark_workspace_dirty(app, &team);
    Task::done(Message::ChannelSelected(channel))
}

fn channel_scrolled(app: &mut App, channel: ChannelId, y: f32) -> Task<Message> {
    if app.active_channel.as_deref() != Some(channel.as_str()) {
        return Task::none();
    }
    let Some(team) = app.active_team.clone() else {
        return Task::none();
    };
    if app.transport.is_none() {
        return Task::none();
    }
    if app
        .pending_scroll_to
        .as_ref()
        .is_some_and(|(pending_channel, _)| pending_channel == &channel)
    {
        return Task::none();
    }
    let oldest = {
        let Some(cm) = app
            .workspaces
            .get_mut(&team)
            .and_then(|ws| ws.messages.get_mut(&channel))
        else {
            return Task::none();
        };
        if !should_load_older_history(cm, y) {
            return Task::none();
        }
        let Some(oldest) = cm.oldest_ts() else {
            return Task::none();
        };
        cm.history_loading_older = true;
        oldest
    };
    app.pending_scroll_to = Some((
        channel.clone(),
        PendingScrollTarget::Message(oldest.clone()),
    ));
    app.load_older_history(&team, &channel, oldest)
}

pub(super) fn should_load_older_history(cm: &ChannelMessages, y: f32) -> bool {
    y <= LOAD_OLDER_SCROLL_TOP_PX && cm.loaded && cm.has_more_older && !cm.history_loading_older
}

fn scroll_to_pending(app: &mut App, channel: &ChannelId) -> Task<Message> {
    let Some((pending_channel, target)) = app.pending_scroll_to.clone() else {
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
    let Some(ts) = pending_target_ts(messages, target) else {
        return Task::none();
    };
    match crate::state::scroll_ratio_for_ts(messages, &ts) {
        Some(ratio) => {
            app.pending_scroll_to = None;
            operation::snap_to(
                ui::channel::scrollable_id(channel),
                RelativeOffset { x: 0.0, y: ratio },
            )
        }
        None => Task::none(),
    }
}

pub(super) fn channel_open_scroll_target(
    app: &App,
    team: &str,
    channel: &ChannelId,
) -> Option<PendingScrollTarget> {
    let ws = app.workspaces.get(team)?;
    let cm = ws.messages.get(channel);
    let unread = ws
        .channels
        .get(channel)
        .map(|channel| ws.unread_total(channel) > 0)
        .unwrap_or_else(|| cm.is_some_and(|cm| cm.unread_count > 0 || cm.mention_count > 0));
    if unread {
        if let Some(last_read) = cm.and_then(|cm| cm.last_read.clone()) {
            return Some(PendingScrollTarget::FirstUnreadAfter(last_read));
        }
    }
    Some(PendingScrollTarget::Latest)
}

pub(super) fn pending_target_ts(
    messages: &[SlackMessage],
    target: PendingScrollTarget,
) -> Option<MessageTs> {
    match target {
        PendingScrollTarget::Message(ts) => Some(ts),
        PendingScrollTarget::FirstUnreadAfter(last_read) => messages
            .iter()
            .filter_map(|message| message.ts.as_deref())
            .find(|ts| crate::state::ts_key(ts) > crate::state::ts_key(&last_read))
            .map(str::to_owned)
            .or_else(|| {
                messages
                    .iter()
                    .filter_map(|message| message.ts.clone())
                    .last()
            }),
        PendingScrollTarget::Latest => messages
            .iter()
            .filter_map(|message| message.ts.clone())
            .last(),
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

fn hydrate_visible_emojis(app: &App, team: &str, channel: &str) -> Task<Message> {
    let messages = visible_channel_messages(app, team, channel);
    hydrate_emojis(app, team, &messages)
}

fn hydrate_thread_emojis(app: &App, team: &str, channel: &str, root_ts: &str) -> Task<Message> {
    let messages = thread_messages(app, team, channel, root_ts);
    hydrate_emojis(app, team, &messages)
}

fn hydrate_emojis(app: &App, team: &str, messages: &[SlackMessage]) -> Task<Message> {
    let names = messages.iter().flat_map(message_emoji_names).collect();
    hydrate_emoji_names(app, team, names)
}

fn hydrate_emoji_names(app: &App, team: &str, names: Vec<String>) -> Task<Message> {
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
    let names: Vec<_> = names
        .into_iter()
        .filter(|name| !crate::state::is_standard_emoji(name))
        .filter(|name| !ws.custom_emoji.contains_key(name))
        .filter(|name| {
            !app.emoji_hydrated
                .contains(&(team.to_owned(), name.clone()))
        })
        .filter(|name| seen.insert(name.clone()))
        .collect();

    if names.is_empty() {
        return Task::none();
    }

    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let team = team.to_owned();
    Task::perform(
        {
            let names = names.clone();
            async move { api::fetch_emojis_info(&transport, &client, &ws_session, names).await }
        },
        move |result| Message::EmojisLoaded {
            team: team.clone(),
            requested: names.clone(),
            result,
        },
    )
}

fn load_visible_emoji_previews(app: &mut App, team: &str, channel: &str) -> Task<Message> {
    let messages = visible_channel_messages(app, team, channel);
    load_emoji_previews(app, team, &messages)
}

fn load_thread_emoji_previews(
    app: &mut App,
    team: &str,
    channel: &str,
    root_ts: &str,
) -> Task<Message> {
    let messages = thread_messages(app, team, channel, root_ts);
    load_emoji_previews(app, team, &messages)
}

fn load_emoji_previews(app: &mut App, team: &str, messages: &[SlackMessage]) -> Task<Message> {
    let mut seen = HashSet::new();
    let names = messages
        .iter()
        .flat_map(message_emoji_names)
        .filter(|name| seen.insert(name.clone()))
        .collect();
    load_emoji_previews_for_names(app, team, names)
}

fn load_emoji_previews_for_names(app: &mut App, team: &str, names: Vec<String>) -> Task<Message> {
    let Some(transport) = app.transport.clone() else {
        return Task::none();
    };
    let Some(ws) = app.workspaces.get(team) else {
        return Task::none();
    };

    let mut seen = HashSet::new();
    let requests: Vec<_> = names
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .filter_map(|name| {
            let key = crate::state::emoji_preview_key(team, &name);
            if app.emoji_previews.contains_key(&key) {
                return None;
            }
            let url = ws.custom_emoji_url(&name)?.to_owned();
            Some((key, url))
        })
        .collect();

    if requests.is_empty() {
        return Task::none();
    }

    for (key, _) in &requests {
        app.emoji_previews.insert(key.clone(), FilePreview::Loading);
    }

    let user_agent = crate::slack::xparams::Identity::from_capture().user_agent;
    Task::batch(requests.into_iter().map(|(key, url)| {
        let transport = transport.clone();
        let user_agent = user_agent.clone();
        Task::perform(
            async move {
                let bytes = transport.get_bytes(&url, &user_agent).await?;
                Ok(emoji_preview_from_bytes(bytes))
            },
            move |result| Message::EmojiPreviewLoaded {
                key: key.clone(),
                result,
            },
        )
    }))
}

pub(super) fn emoji_preview_from_bytes(bytes: Vec<u8>) -> FilePreview {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        if let Some(preview) = decode_gif_preview(&bytes) {
            return preview;
        }
    }
    FilePreview::Loaded(ImageHandle::from_bytes(bytes))
}

fn decode_gif_preview(bytes: &[u8]) -> Option<FilePreview> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(std::io::Cursor::new(bytes)).ok()?;

    let width = decoder.width() as usize;
    let height = decoder.height() as usize;
    if width == 0 || height == 0 {
        return None;
    }

    let mut frames = Vec::new();
    let mut delays = Vec::new();
    let mut canvas = vec![0u8; width * height * 4];
    while let Some(frame) = decoder.read_next_frame().ok()? {
        let snapshot =
            matches!(frame.dispose, gif::DisposalMethod::Previous).then(|| canvas.clone());

        composite_frame(&mut canvas, width, height, frame);
        frames.push(ImageHandle::from_rgba(
            width as u32,
            height as u32,
            canvas.clone(),
        ));
        delays.push(gif_delay(frame.delay));

        match frame.dispose {
            gif::DisposalMethod::Background => {
                clear_frame_rect(&mut canvas, width, height, frame);
            }
            gif::DisposalMethod::Previous => {
                if let Some(prev) = snapshot {
                    canvas = prev;
                }
            }
            gif::DisposalMethod::Keep | gif::DisposalMethod::Any => {}
        }
    }

    match frames.len() {
        0 => None,
        1 => Some(FilePreview::Loaded(frames.remove(0))),
        _ => {
            let total = delays.iter().copied().sum();
            Some(FilePreview::Animated {
                frames,
                delays,
                total,
            })
        }
    }
}

fn gif_delay(delay_cs: u16) -> Duration {
    if delay_cs == 0 {
        Duration::from_millis(100)
    } else {
        Duration::from_millis((delay_cs as u64 * 10).max(20))
    }
}

fn composite_frame(canvas: &mut [u8], width: usize, height: usize, frame: &gif::Frame) {
    let fx = frame.left as usize;
    let fy = frame.top as usize;
    let fw = frame.width as usize;
    let fh = frame.height as usize;
    for row in 0..fh {
        let cy = fy + row;
        if cy >= height {
            break;
        }
        for col in 0..fw {
            let cx = fx + col;
            if cx >= width {
                break;
            }
            let src = (row * fw + col) * 4;
            if frame.buffer[src + 3] == 0 {
                continue;
            }
            let dst = (cy * width + cx) * 4;
            canvas[dst..dst + 4].copy_from_slice(&frame.buffer[src..src + 4]);
        }
    }
}

fn clear_frame_rect(canvas: &mut [u8], width: usize, height: usize, frame: &gif::Frame) {
    let fx = frame.left as usize;
    let fy = frame.top as usize;
    let fw = frame.width as usize;
    let fh = frame.height as usize;
    for row in 0..fh {
        let cy = fy + row;
        if cy >= height {
            break;
        }
        let start = (cy * width + fx.min(width)) * 4;
        let end = (cy * width + (fx + fw).min(width)) * 4;
        canvas[start..end].fill(0);
    }
}

fn thread_messages(app: &App, team: &str, channel: &str, root_ts: &str) -> Vec<SlackMessage> {
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
    messages
}

fn message_emoji_names(msg: &SlackMessage) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(text) = msg.text.as_deref() {
        names.extend(crate::state::emoji_names_in_text(text));
    }
    for reaction in &msg.reactions {
        names.push(reaction.name.clone());
    }
    for block in &msg.blocks {
        collect_value_emoji_names(block, &mut names);
    }
    for att in &msg.attachments {
        for text in [
            att.service_name.as_deref(),
            att.author_name.as_deref(),
            att.title.as_deref(),
            att.pretext.as_deref(),
            att.text.as_deref(),
            att.footer.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            names.extend(crate::state::emoji_names_in_text(text));
        }
    }
    names
}

fn collect_value_emoji_names(value: &serde_json::Value, names: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => names.extend(crate::state::emoji_names_in_text(text)),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_value_emoji_names(value, names);
            }
        }
        serde_json::Value::Object(map) => {
            if map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "emoji")
            {
                if let Some(name) = map.get("name").and_then(serde_json::Value::as_str) {
                    names.push(name.to_owned());
                }
            }
            for value in map.values() {
                collect_value_emoji_names(value, names);
            }
        }
        _ => {}
    }
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

fn hydrate_sidebar_channels(app: &App, team: &str) -> Task<Message> {
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
    let mut channels = Vec::new();
    for id in ws.starred_order.iter().chain(ws.priority_scores.keys()) {
        if !seen.insert(id.clone()) {
            continue;
        }
        let needs_hydration = ws
            .channels
            .get(id)
            .map(channel_needs_hydration)
            .unwrap_or_else(|| ws.starred_order.iter().any(|starred| starred == id));
        if needs_hydration {
            channels.push(id.clone());
        }
    }

    if channels.is_empty() {
        return Task::none();
    }

    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let team = team.to_owned();
    Task::perform(
        {
            let channels = channels.clone();
            async move { api::fetch_channels_info(&transport, &client, &ws_session, channels).await }
        },
        move |result| Message::ChannelsLoaded {
            team: team.clone(),
            requested: channels.clone(),
            result,
        },
    )
}

fn channel_needs_hydration(channel: &Channel) -> bool {
    channel
        .name
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
}

fn hydrate_visible_channels(app: &App, team: &str, channel: &str) -> Task<Message> {
    let messages = visible_channel_messages(app, team, channel);
    hydrate_message_channels(app, team, &messages)
}

fn hydrate_message_channels(app: &App, team: &str, messages: &[SlackMessage]) -> Task<Message> {
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
    let channels: Vec<_> = messages
        .iter()
        .flat_map(ui::blocks::mentioned_channel_ids)
        .filter(|channel| ws.channels.get(channel).is_none_or(channel_needs_hydration))
        .filter(|channel| {
            !app.channel_hydrated
                .contains(&(team.to_owned(), channel.clone()))
        })
        .filter(|channel| seen.insert(channel.clone()))
        .collect();

    if channels.is_empty() {
        return Task::none();
    }

    let transport = transport.clone();
    let client = app.client.clone();
    let ws_session = ws_session.clone();
    let team = team.to_owned();
    Task::perform(
        {
            let channels = channels.clone();
            async move { api::fetch_channels_info(&transport, &client, &ws_session, channels).await }
        },
        move |result| Message::ChannelsLoaded {
            team: team.clone(),
            requested: channels.clone(),
            result,
        },
    )
}

fn hydrate_sidebar_dm_users(app: &App, team: &str) -> Task<Message> {
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
    let users: Vec<_> = ws
        .channels
        .values()
        .filter(|channel| channel.is_im)
        .filter(|channel| {
            ws.should_show_unstarred_read_channels()
                || ws.unread_total(channel) > 0
                || app.active_channel.as_deref() == Some(channel.id.as_str())
                || ws.is_starred_channel(channel)
        })
        .filter_map(crate::state::dm_user_id)
        .filter(|user| !user.trim().is_empty())
        .filter(|user| needs_user_hydration(ws, &app.avatar_profile_hydrated, user))
        .filter(|user| seen.insert((*user).to_owned()))
        .map(str::to_owned)
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
    let mut seen_bots = HashSet::new();
    let bot_requests = messages
        .iter()
        .filter_map(crate::state::message_bot_avatar)
        .filter(|(key, _)| seen_bots.insert(key.clone()))
        .collect();
    Task::batch([
        load_avatar_url_previews(app, bot_requests),
        load_user_avatar_previews(app, team, users),
    ])
}

fn load_user_avatar_previews(app: &mut App, team: &str, users: Vec<UserId>) -> Task<Message> {
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

    load_avatar_url_previews(app, requests)
}

fn load_avatar_url_previews(app: &mut App, requests: Vec<(String, String)>) -> Task<Message> {
    let Some(transport) = app.transport.clone() else {
        return Task::none();
    };

    let requests: Vec<_> = requests
        .into_iter()
        .filter(|(key, _)| !app.avatar_previews.contains_key(key))
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

    let author = ws.message_author_name(msg);
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
            let pending_file_message = (!msg.files.is_empty()
                && msg.user.as_deref() == Some(ws.self_user_id.as_str()))
            .then(|| {
                app.pending_file_messages.iter().position(|pending| {
                    pending.team == team
                        && pending.channel == channel
                        && pending.thread_ts == msg.thread_ts
                        && msg.text.as_deref().unwrap_or_default() == pending.text
                })
            })
            .flatten()
            .map(|index| app.pending_file_messages.remove(index));
            if let Some(pending) = pending_file_message.as_ref() {
                for (file, attachment) in msg.files.iter().zip(&pending.attachments) {
                    let preview_path = attachment
                        .preview_path
                        .as_ref()
                        .or_else(|| is_local_image(&attachment.path).then_some(&attachment.path));
                    if let (Some(key), Some(path)) =
                        (crate::state::file_preview_key(file), preview_path)
                    {
                        app.file_previews.insert(
                            key,
                            FilePreview::Loaded(ImageHandle::from_path(path.clone())),
                        );
                    }
                }
            }
            let pending_message_ts = pending_file_message
                .as_ref()
                .map(|pending| pending.message_ts.as_str());
            let notification =
                notification_for_message(ws, &channel, &msg, active_channel.as_deref());
            if let Some(user) = msg.user.as_deref() {
                ws.clear_typing_user(&channel, user);
            }
            if let Some(root_ts) = thread_root_for_reply(&msg) {
                let key = (team.to_owned(), channel.clone(), root_ts);
                if let Some(cm) = app.threads.get_mut(&key) {
                    if let Some(pending_ts) = pending_message_ts {
                        cm.remove(pending_ts);
                    }
                    upsert_realtime_message(cm, msg.clone());
                }
                if msg.subtype.as_deref() != Some("thread_broadcast") {
                    return notification;
                }
            }
            let cm = ws.messages.entry(channel).or_default();
            if let Some(pending_ts) = pending_message_ts {
                cm.remove(pending_ts);
            }
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
