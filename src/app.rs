use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill, Subscription, Task};

use crate::cache::Cache;
use crate::config::{self, Session};
use crate::slack::api::{self, HistoryArgs};
use crate::slack::events::RtEvent;
use crate::slack::models::{
    BootData, ChannelId, CountsPage, HistoryPage, Message as SlackMessage, MessageTs, SentMessage,
    TeamId,
};
use crate::slack::realtime::{self, ConnectParams, Connection, RtUpdate};
use crate::slack::{Error as SlackError, SlackClient, Transport};
use crate::state::{ChannelMessages, Presence, RealtimeStatus, Screen, Toast, Workspace};
use crate::ui;

type ActiveThreadKey = (ChannelId, MessageTs);
type ThreadKey = (TeamId, ChannelId, MessageTs);

#[derive(Debug, Clone)]
pub enum FilePreview {
    Loading,
    Loaded(ImageHandle),
    Failed,
}

#[derive(Debug, Clone)]
struct DesktopNotification {
    title: String,
    body: String,
}

pub struct App {
    screen: Screen,
    session: Option<Session>,
    cache: Option<Cache>,
    client: SlackClient,
    transport: Option<Arc<Transport>>,
    active_team: Option<TeamId>,
    active_channel: Option<ChannelId>,
    active_thread: Option<ActiveThreadKey>,
    workspaces: BTreeMap<TeamId, Workspace>,
    threads: HashMap<ThreadKey, ChannelMessages>,
    composer_text: String,
    thread_composer_text: String,
    errors: Vec<Toast>,
    send_seq: u64,
    last_typing: HashMap<(TeamId, ChannelId), Instant>,
    last_active_channels: HashMap<TeamId, ChannelId>,
    file_previews: HashMap<String, FilePreview>,
}

#[derive(Debug, Clone)]
pub enum Message {
    WorkspaceSelected(TeamId),
    ChannelSelected(ChannelId),
    ComposerChanged(String),
    SendPressed,
    MessageSent {
        team: TeamId,
        channel: ChannelId,
        client_msg_id: String,
        result: Result<SentMessage, SlackError>,
    },
    BootLoaded(TeamId, Result<BootData, SlackError>),
    CountsLoaded(TeamId, Result<CountsPage, SlackError>),
    HistoryLoaded(TeamId, ChannelId, Result<HistoryPage, SlackError>),
    ChannelMarked(TeamId, ChannelId, MessageTs, Result<(), SlackError>),
    ThreadOpened {
        channel: ChannelId,
        ts: MessageTs,
    },
    ThreadClosed,
    ThreadLoaded {
        team: TeamId,
        channel: ChannelId,
        root_ts: MessageTs,
        result: Result<HistoryPage, SlackError>,
    },
    ThreadComposerChanged(String),
    ThreadSendPressed,
    ThreadReplySent {
        team: TeamId,
        channel: ChannelId,
        root_ts: MessageTs,
        client_msg_id: String,
        result: Result<SentMessage, SlackError>,
    },
    ReactionPressed {
        channel: ChannelId,
        ts: MessageTs,
        name: String,
    },
    ReactionUpdated {
        team: TeamId,
        channel: ChannelId,
        ts: MessageTs,
        user: String,
        name: String,
        added: bool,
        result: Result<(), SlackError>,
    },
    FileDownloadPressed {
        url: String,
        filename: String,
    },
    FileDownloaded(Result<PathBuf, SlackError>),
    FilePreviewLoaded {
        key: String,
        result: Result<Vec<u8>, SlackError>,
    },
    DesktopNotificationShown(Result<(), String>),
    Realtime(TeamId, u64, RtEvent),
    RtConnected(TeamId, u64, Connection),
    RtDisconnected(TeamId, u64),
    SignInPressed,
    RetryAuth,
    Tick,
}

impl App {
    fn empty() -> Self {
        App {
            screen: Screen::Login,
            session: None,
            cache: None,
            client: SlackClient::default(),
            transport: None,
            active_team: None,
            active_channel: None,
            active_thread: None,
            workspaces: BTreeMap::new(),
            threads: HashMap::new(),
            composer_text: String::new(),
            thread_composer_text: String::new(),
            errors: Vec::new(),
            send_seq: 0,
            last_typing: HashMap::new(),
            last_active_channels: HashMap::new(),
            file_previews: HashMap::new(),
        }
    }

    fn boot() -> (Self, Task<Message>) {
        let mut app = App::empty();
        let task = app.load_session();
        (app, task)
    }

    fn load_session(&mut self) -> Task<Message> {
        match config::load_session() {
            Ok(Some(session)) => match Transport::new(session.d_cookie.clone()) {
                Ok(transport) => {
                    self.transport = Some(Arc::new(transport));
                    let cache = match Cache::open_default() {
                        Ok(cache) => Some(cache),
                        Err(e) => {
                            self.toast(format!("cache unavailable: {e}"));
                            None
                        }
                    };
                    let mut warm = false;
                    for ws in session.workspaces.values() {
                        let workspace = cache
                            .as_ref()
                            .and_then(|cache| match cache.load_workspace(ws) {
                                Ok(workspace) => workspace,
                                Err(e) => {
                                    tracing::warn!(team = %ws.team_id, error = %e, "cache load failed");
                                    None
                                }
                            })
                            .unwrap_or_else(|| Workspace::from_session(ws));
                        warm |= !workspace.channels.is_empty();
                        self.workspaces.insert(ws.team_id.clone(), workspace);
                    }
                    self.active_team = session.workspaces.keys().next().cloned();
                    if warm {
                        self.screen = Screen::Main;
                        if let Some(team) = self.active_team.clone() {
                            self.active_channel = preferred_channel(self, &team);
                        }
                    } else {
                        self.screen = Screen::Loading;
                    }
                    self.cache = cache;
                    self.session = Some(session);
                    self.boot_all()
                }
                Err(e) => {
                    self.toast(format!("transport init failed: {e}"));
                    self.screen = Screen::Login;
                    Task::none()
                }
            },
            Ok(None) => {
                self.screen = Screen::Login;
                Task::none()
            }
            Err(e) => {
                self.toast(format!("could not load session: {e}"));
                self.screen = Screen::Login;
                Task::none()
            }
        }
    }

    fn boot_all(&self) -> Task<Message> {
        let Some((transport, session)) = self.live() else {
            return Task::none();
        };
        let tasks = session.workspaces.values().flat_map(|ws| {
            let boot_transport = transport.clone();
            let boot_client = self.client.clone();
            let boot_ws = ws.clone();
            let team = ws.team_id.clone();
            let boot = Task::perform(
                async move { api::fetch_user_boot(&boot_transport, &boot_client, &boot_ws).await },
                move |result| Message::BootLoaded(team.clone(), result),
            );

            let counts_transport = transport.clone();
            let counts_client = self.client.clone();
            let counts_ws = ws.clone();
            let team = ws.team_id.clone();
            let counts = Task::perform(
                async move { api::fetch_counts(&counts_transport, &counts_client, &counts_ws).await },
                move |result| Message::CountsLoaded(team.clone(), result),
            );
            [boot, counts]
        });
        Task::batch(tasks)
    }

    fn load_history(&self, team: &str, channel: &ChannelId) -> Task<Message> {
        self.load_history_since(team, channel, None)
    }

    fn load_history_since(
        &self,
        team: &str,
        channel: &ChannelId,
        oldest: Option<MessageTs>,
    ) -> Task<Message> {
        let Some((transport, session)) = self.live() else {
            return Task::none();
        };
        let Some(ws) = session.workspaces.get(team) else {
            return Task::none();
        };
        let transport = transport.clone();
        let client = self.client.clone();
        let ws = ws.clone();
        let team = team.to_owned();
        let channel = channel.clone();
        let args = HistoryArgs {
            channel: channel.clone(),
            oldest,
            limit: Some(50),
            ..Default::default()
        };
        Task::perform(
            async move { api::fetch_history(&transport, &client, &ws, args).await },
            move |result| Message::HistoryLoaded(team.clone(), channel.clone(), result),
        )
    }

    fn mark_channel_read(&self, team: &str, channel: &ChannelId, ts: MessageTs) -> Task<Message> {
        let Some((transport, session)) = self.live() else {
            return Task::none();
        };
        let Some(ws) = session.workspaces.get(team) else {
            return Task::none();
        };
        let transport = transport.clone();
        let client = self.client.clone();
        let ws = ws.clone();
        let team = team.to_owned();
        let channel = channel.clone();
        let send_channel = channel.clone();
        let mark_ts = ts.clone();
        Task::perform(
            async move { api::mark_channel(&transport, &client, &ws, send_channel, ts).await },
            move |result| {
                Message::ChannelMarked(team.clone(), channel.clone(), mark_ts.clone(), result)
            },
        )
    }

    fn load_thread(&self, team: &str, channel: &ChannelId, root_ts: &MessageTs) -> Task<Message> {
        let Some((transport, session)) = self.live() else {
            return Task::none();
        };
        let Some(ws) = session.workspaces.get(team) else {
            return Task::none();
        };
        let transport = transport.clone();
        let client = self.client.clone();
        let ws = ws.clone();
        let team = team.to_owned();
        let channel = channel.clone();
        let root_ts = root_ts.clone();
        let fetch_channel = channel.clone();
        let fetch_ts = root_ts.clone();
        Task::perform(
            async move {
                api::fetch_replies(&transport, &client, &ws, fetch_channel, fetch_ts, None).await
            },
            move |result| Message::ThreadLoaded {
                team: team.clone(),
                channel: channel.clone(),
                root_ts: root_ts.clone(),
                result,
            },
        )
    }

    fn active_workspace(&self) -> Option<&Workspace> {
        self.workspaces.get(self.active_team.as_ref()?)
    }

    fn active_workspace_mut(&mut self) -> Option<&mut Workspace> {
        let team = self.active_team.clone()?;
        self.workspaces.get_mut(&team)
    }

    fn live(&self) -> Option<(&Arc<Transport>, &Session)> {
        Some((self.transport.as_ref()?, self.session.as_ref()?))
    }

    fn toast(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!(%text, "toast");
        self.errors.push(Toast { text });
    }
}

pub fn run() -> iced::Result {
    iced::application(App::boot, update, view)
        .subscription(subscription)
        .title("Snack")
        .run()
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::WorkspaceSelected(team) => select_workspace(app, team),

        Message::ChannelSelected(id) => {
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
                    mark_latest_visible(app, &team, &id),
                    load_visible_file_previews(app, &team, &id),
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
                load_thread_file_previews(app, &team, &channel, &ts)
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
                    let key = (team.clone(), channel.clone(), root_ts.clone());
                    let cm = app.threads.entry(key).or_default();
                    let n = page.messages.len();
                    for msg in page.messages {
                        cm.upsert(msg);
                    }
                    cm.loaded = true;
                    tracing::info!(%channel, %root_ts, messages = n, "thread loaded");
                    return load_thread_file_previews(app, &team, &channel, &root_ts);
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
                    persist_workspace(app, &team);
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
                persist_workspace(app, &team);
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
                    return load_visible_file_previews(app, &team, &channel);
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
                    persist_workspace(app, &team);
                }
                Err(e) => tracing::warn!(%team, error = %e, "counts failed"),
            }
            Task::none()
        }

        Message::HistoryLoaded(team, channel, result) => {
            match result {
                Ok(page) => {
                    if let Some(ws) = app.workspaces.get_mut(&team) {
                        let cm = ws.messages.entry(channel.clone()).or_default();
                        let n = page.messages.len();
                        for msg in page.messages {
                            cm.upsert(msg);
                        }
                        cm.loaded = true;
                        tracing::info!(%channel, messages = n, "history loaded");
                    }
                    persist_workspace(app, &team);
                    return Task::batch([
                        mark_latest_visible(app, &team, &channel),
                        load_visible_file_previews(app, &team, &channel),
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
                    persist_workspace(app, &team);
                }
                Err(e) => tracing::warn!(%team, %channel, error = %e, "mark failed"),
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
                persist_workspace(app, &team);
            }
            Task::none()
        }

        Message::FileDownloadPressed { url, filename } => download_file_pressed(app, url, filename),

        Message::FileDownloaded(result) => {
            match result {
                Ok(path) => app.toast(format!("downloaded file to {}", path.display())),
                Err(e) => app.toast(format!("download failed: {e}")),
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

        Message::DesktopNotificationShown(result) => {
            if let Err(e) = result {
                tracing::debug!(error = %e, "desktop notification failed");
            }
            Task::none()
        }

        Message::Realtime(team, generation, event) => {
            let notification = apply_realtime(app, &team, generation, event);
            persist_workspace(app, &team);
            let mut tasks = Vec::new();
            if let Some(notification) = notification {
                tasks.push(show_desktop_notification_task(notification));
            }
            if app.active_team.as_deref() == Some(&team) {
                if let Some(channel) = app.active_channel.clone() {
                    tasks.push(mark_latest_visible(app, &team, &channel));
                    tasks.push(load_visible_file_previews(app, &team, &channel));
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
            Task::none()
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

fn preferred_channel(app: &App, team: &str) -> Option<ChannelId> {
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
    persist_workspace(app, &team);

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
    persist_workspace(app, &team);

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

async fn unique_download_path(dir: &Path, filename: &str) -> Result<PathBuf, SlackError> {
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
    let messages = app
        .workspaces
        .get(team)
        .and_then(|ws| ws.messages.get(channel))
        .map(|cm| cm.messages.clone())
        .unwrap_or_default();
    load_file_previews(app, messages)
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

fn load_file_previews(app: &mut App, messages: Vec<SlackMessage>) -> Task<Message> {
    let Some(transport) = app.transport.clone() else {
        return Task::none();
    };
    let requests: Vec<_> = messages
        .iter()
        .flat_map(|msg| &msg.files)
        .filter_map(|file| {
            let key = crate::state::file_preview_key(file)?;
            let url = crate::state::file_preview_url(file)?.to_owned();
            Some((key, url))
        })
        .filter(|(key, _)| !app.file_previews.contains_key(key))
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

fn notification_for_message(
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

fn persist_workspace(app: &App, team: &str) {
    let (Some(cache), Some(ws)) = (app.cache.as_ref(), app.workspaces.get(team)) else {
        return;
    };
    if let Err(e) = cache.save_workspace(ws) {
        tracing::warn!(%team, error = %e, "cache save failed");
    }
}

fn view(app: &App) -> Element<'_, Message> {
    match app.screen {
        Screen::Login => login_view(),
        Screen::Loading => center_text("Loading…"),
        Screen::Main => main_view(app),
    }
}

fn login_view() -> Element<'static, Message> {
    container(
        column![
            text("Snack").size(ui::theme::TEXT_LG),
            text("Sign in to your Slack workspace.").size(ui::theme::TEXT_MD),
            text("Opens a new window for the Slack sign-in flow.").size(ui::theme::TEXT_SM),
            button(text("Sign in").size(ui::theme::TEXT_MD)).on_press(Message::SignInPressed),
            button(text("Reload session").size(ui::theme::TEXT_SM)).on_press(Message::RetryAuth),
        ]
        .spacing(12),
    )
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn main_view(app: &App) -> Element<'_, Message> {
    let Some(ws) = app.active_workspace() else {
        return center_text("No workspace");
    };

    let sidebar = ui::sidebar::view(
        &app.workspaces,
        app.active_team.as_deref(),
        ws,
        app.active_channel.as_deref(),
    );

    let main: Element<'_, Message> = match app.active_channel.as_deref() {
        Some(channel_id) => {
            let label = ws
                .channels
                .get(channel_id)
                .map(crate::state::channel_label)
                .unwrap_or_else(|| channel_id.to_owned());
            column![
                container(ui::channel::view(ws, channel_id, &app.file_previews)).height(Fill),
                ui::composer::view(&app.composer_text, &label),
            ]
            .width(Fill)
            .height(Fill)
            .into()
        }
        None => center_text("Select a channel"),
    };

    let content = if let (Some(team), Some((channel, root_ts))) =
        (app.active_team.as_ref(), app.active_thread.as_ref())
    {
        let replies = app
            .threads
            .get(&(team.clone(), channel.clone(), root_ts.clone()));
        let root = ui::thread::root_message(ws, channel, root_ts);
        row![
            main,
            ui::thread::view(
                ws,
                channel,
                root,
                replies,
                &app.thread_composer_text,
                &app.file_previews,
            )
        ]
        .width(Fill)
        .height(Fill)
    } else {
        row![main].width(Fill).height(Fill)
    };

    row![sidebar, content].width(Fill).height(Fill).into()
}

fn center_text(label: &str) -> Element<'_, Message> {
    container(text(label.to_owned()).size(ui::theme::TEXT_LG))
        .center_x(Fill)
        .center_y(Fill)
        .into()
}

fn subscription(app: &App) -> Subscription<Message> {
    let mut subs = vec![iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)];

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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use serde_json::json;

    use super::*;
    use crate::slack::models::Channel;
    use crate::state::ChannelMessages;

    const SELF_USER: &str = "U_SELF";

    fn msg(user: &str, ts: &str, text: &str) -> SlackMessage {
        SlackMessage {
            user: Some(user.into()),
            ts: Some(ts.into()),
            text: Some(text.into()),
            ..Default::default()
        }
    }

    fn loaded_channel(user: &str, ts: &str, text: &str) -> ChannelMessages {
        let mut cm = ChannelMessages::default();
        cm.upsert(msg(user, ts, text));
        cm.loaded = true;
        cm
    }

    fn test_workspace() -> Workspace {
        let mut channels = BTreeMap::new();
        for (id, name) in [("C_GENERAL", "general"), ("C_DEV", "dev")] {
            channels.insert(
                id.into(),
                Channel {
                    id: id.into(),
                    name: Some(name.into()),
                    is_channel: true,
                    ..Default::default()
                },
            );
        }

        let messages = HashMap::from([
            (
                "C_GENERAL".into(),
                loaded_channel("U_ALICE", "1783372300.000100", "morning"),
            ),
            (
                "C_DEV".into(),
                loaded_channel("U_BOB", "1783370000.000100", "question"),
            ),
        ]);

        Workspace {
            team_id: "T_TEST".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            self_user_id: SELF_USER.into(),
            channels,
            users: HashMap::new(),
            messages,
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 1,
        }
    }

    fn test_app() -> App {
        let mut app = App::empty();
        let ws = test_workspace();
        let team = ws.team_id.clone();
        app.active_channel = ws.channels.keys().next().cloned();
        app.workspaces.insert(team.clone(), ws);
        app.active_team = Some(team);
        app.screen = Screen::Main;
        app
    }

    fn add_second_workspace(app: &mut App) {
        let mut ws = test_workspace();
        ws.team_id = "T_SECOND".into();
        ws.name = "Second".into();
        ws.url = "https://second.slack.com".into();
        ws.channels.clear();
        ws.channels.insert(
            "C_SECOND".into(),
            Channel {
                id: "C_SECOND".into(),
                name: Some("second".into()),
                is_channel: true,
                ..Default::default()
            },
        );
        ws.messages = HashMap::from([(
            "C_SECOND".into(),
            loaded_channel("U_SECOND", "1783375000.000100", "second workspace"),
        )]);
        app.workspaces.insert(ws.team_id.clone(), ws);
    }

    #[test]
    fn notification_created_for_inactive_channel_mention() {
        let app = test_app();
        let ws = app.active_workspace().unwrap();
        let message = SlackMessage {
            user: Some("U_ALICE".into()),
            text: Some("hi <@U_SELF>".into()),
            ..Default::default()
        };

        let notification = notification_for_message(ws, "C_DEV", &message, Some("C_GENERAL"))
            .expect("mention should notify");

        assert_eq!(notification.title, "U_ALICE in #dev");
        assert_eq!(notification.body, "hi <@U_SELF>");
    }

    #[test]
    fn notification_suppresses_self_and_active_channel_messages() {
        let app = test_app();
        let ws = app.active_workspace().unwrap();
        let self_message = SlackMessage {
            user: Some(SELF_USER.into()),
            text: Some("self <@U_SELF>".into()),
            ..Default::default()
        };
        assert!(notification_for_message(ws, "C_DEV", &self_message, Some("C_GENERAL")).is_none());

        let active_message = SlackMessage {
            user: Some("U_ALICE".into()),
            text: Some("active <@U_SELF>".into()),
            ..Default::default()
        };
        assert!(
            notification_for_message(ws, "C_GENERAL", &active_message, Some("C_GENERAL")).is_none()
        );
    }

    #[test]
    fn notification_detects_block_user_mentions() {
        let app = test_app();
        let ws = app.active_workspace().unwrap();
        let message = SlackMessage {
            user: Some("U_ALICE".into()),
            blocks: vec![json!({
                "type": "rich_text",
                "elements": [{
                    "type": "rich_text_section",
                    "elements": [
                        {"type": "text", "text": "cc "},
                        {"type": "user", "user_id": "U_SELF"}
                    ]
                }]
            })],
            ..Default::default()
        };

        let notification = notification_for_message(ws, "C_DEV", &message, Some("C_GENERAL"))
            .expect("block mention should notify");

        assert_eq!(notification.body, "cc @U_SELF");
    }

    #[test]
    fn boot_selects_first_channel() {
        let app = test_app();
        assert_eq!(app.screen, Screen::Main);
        assert!(app.active_team.is_some());
        assert!(app.active_channel.is_some());
        assert!(!app.active_workspace().unwrap().channels.is_empty());
    }

    #[test]
    fn channel_selection_preserves_loaded_messages() {
        let mut app = test_app();
        let _ = update(&mut app, Message::ChannelSelected("C_DEV".into()));
        assert_eq!(app.active_channel.as_deref(), Some("C_DEV"));
        let ws = app.active_workspace().unwrap();
        assert!(!ws.messages.get("C_GENERAL").unwrap().messages.is_empty());
        assert!(!ws.messages.get("C_DEV").unwrap().messages.is_empty());
    }

    #[test]
    fn workspace_selection_switches_active_workspace_and_channel() {
        let mut app = test_app();
        add_second_workspace(&mut app);
        app.composer_text = "draft".into();
        app.thread_composer_text = "reply draft".into();
        app.active_thread = Some(("C_GENERAL".into(), "1783372300.000100".into()));

        let _ = update(&mut app, Message::WorkspaceSelected("T_SECOND".into()));

        assert_eq!(app.active_team.as_deref(), Some("T_SECOND"));
        assert_eq!(app.active_channel.as_deref(), Some("C_SECOND"));
        assert!(app.active_thread.is_none());
        assert!(app.composer_text.is_empty());
        assert!(app.thread_composer_text.is_empty());
    }

    #[test]
    fn workspace_selection_remembers_previous_channel() {
        let mut app = test_app();
        add_second_workspace(&mut app);
        let _ = update(&mut app, Message::ChannelSelected("C_DEV".into()));
        let _ = update(&mut app, Message::WorkspaceSelected("T_SECOND".into()));
        let _ = update(&mut app, Message::WorkspaceSelected("T_TEST".into()));

        assert_eq!(app.active_team.as_deref(), Some("T_TEST"));
        assert_eq!(app.active_channel.as_deref(), Some("C_DEV"));
    }

    #[tokio::test]
    async fn unique_download_path_avoids_overwrite() {
        let dir = std::env::temp_dir().join(format!("snack-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("report.pdf"), b"old")
            .await
            .unwrap();

        let path = unique_download_path(&dir, "report.pdf").await.unwrap();

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("report-1.pdf")
        );
    }

    #[test]
    fn thread_open_tracks_selected_root() {
        let mut app = test_app();
        let _ = update(
            &mut app,
            Message::ThreadOpened {
                channel: "C_GENERAL".into(),
                ts: "1783372300.000100".into(),
            },
        );

        assert_eq!(app.active_channel.as_deref(), Some("C_GENERAL"));
        assert_eq!(
            app.active_thread.as_ref(),
            Some(&("C_GENERAL".into(), "1783372300.000100".into()))
        );
    }

    #[test]
    fn thread_loaded_stores_replies() {
        let mut app = test_app();
        let team = app.active_team.clone().unwrap();
        let root_ts = "1783372300.000100".to_owned();
        let _ = update(
            &mut app,
            Message::ThreadLoaded {
                team: team.clone(),
                channel: "C_GENERAL".into(),
                root_ts: root_ts.clone(),
                result: Ok(HistoryPage {
                    messages: vec![
                        msg("U_ALICE", &root_ts, "morning"),
                        SlackMessage {
                            thread_ts: Some(root_ts.clone()),
                            ..msg("U_BOB", "1783372310.000100", "reply")
                        },
                    ],
                    ..Default::default()
                }),
            },
        );

        let cm = &app.threads[&(team, "C_GENERAL".into(), root_ts)];
        assert!(cm.loaded);
        assert_eq!(cm.messages.len(), 2);
    }

    #[test]
    fn optimistic_thread_reply_inserts_pending_without_transport() {
        let mut app = test_app();
        let root_ts = "1783372300.000100".to_owned();
        app.active_thread = Some(("C_GENERAL".into(), root_ts.clone()));
        app.thread_composer_text = "thread answer".into();
        let _ = update(&mut app, Message::ThreadSendPressed);

        assert!(app.thread_composer_text.is_empty());
        let team = app.active_team.clone().unwrap();
        let cm = &app.threads[&(team, "C_GENERAL".into(), root_ts)];
        let reply = cm.messages.last().unwrap();
        assert_eq!(reply.text.as_deref(), Some("thread answer"));
        assert_eq!(reply.thread_ts.as_deref(), Some("1783372300.000100"));
        assert!(cm.is_pending(reply.ts.as_deref().unwrap()));
    }

    #[test]
    fn realtime_thread_reply_updates_open_thread_not_channel() {
        let mut app = test_app();
        let team = app.active_team.clone().unwrap();
        let root_ts = "1783372300.000100".to_owned();
        app.active_thread = Some(("C_GENERAL".into(), root_ts.clone()));
        app.threads.insert(
            (team.clone(), "C_GENERAL".into(), root_ts.clone()),
            ChannelMessages::default(),
        );
        let before = app.workspaces[&team].messages["C_GENERAL"].messages.len();
        let ev = RtEvent::Message(SlackMessage {
            user: Some("U_BOB".into()),
            ts: Some("1783372310.000100".into()),
            channel: Some("C_GENERAL".into()),
            thread_ts: Some(root_ts.clone()),
            text: Some("reply".into()),
            ..Default::default()
        });
        let _ = update(&mut app, Message::Realtime(team.clone(), 1, ev));

        assert_eq!(
            app.workspaces[&team].messages["C_GENERAL"].messages.len(),
            before
        );
        assert_eq!(
            app.threads[&(team, "C_GENERAL".into(), root_ts)].messages[0]
                .text
                .as_deref(),
            Some("reply")
        );
    }

    #[test]
    fn empty_send_is_noop() {
        let mut app = test_app();
        app.active_channel = Some("C_GENERAL".into());
        let before = app.active_workspace().unwrap().messages["C_GENERAL"]
            .messages
            .len();
        app.composer_text = "   ".into();
        let _ = update(&mut app, Message::SendPressed);
        let after = app.active_workspace().unwrap().messages["C_GENERAL"]
            .messages
            .len();
        assert_eq!(before, after);
    }

    #[test]
    fn optimistic_send_inserts_pending_without_transport() {
        let mut app = test_app();
        app.active_channel = Some("C_GENERAL".into());
        app.composer_text = "hello world".into();
        let _ = update(&mut app, Message::SendPressed);

        assert!(app.composer_text.is_empty());
        let cm = &app.active_workspace().unwrap().messages["C_GENERAL"];
        let last = cm.messages.last().unwrap();
        assert_eq!(last.text.as_deref(), Some("hello world"));
        assert_eq!(last.user.as_deref(), Some(SELF_USER));
        let ts = last.ts.clone().unwrap();
        assert!(cm.is_pending(&ts));
    }

    #[test]
    fn message_sent_clears_pending() {
        let mut app = test_app();
        app.active_channel = Some("C_GENERAL".into());
        app.composer_text = "confirm me".into();
        let _ = update(&mut app, Message::SendPressed);

        let cid = {
            let cm = &app.active_workspace().unwrap().messages["C_GENERAL"];
            cm.messages
                .iter()
                .find(|m| m.text.as_deref() == Some("confirm me"))
                .unwrap()
                .client_msg_id
                .clone()
                .unwrap()
        };

        let team = app.active_team.clone().unwrap();
        let _ = update(
            &mut app,
            Message::MessageSent {
                team,
                channel: "C_GENERAL".into(),
                client_msg_id: cid,
                result: Ok(SentMessage {
                    channel: "C_GENERAL".into(),
                    ts: "1783372400.111111".into(),
                    message: SlackMessage {
                        user: Some(SELF_USER.into()),
                        text: Some("confirm me".into()),
                        ts: Some("1783372400.111111".into()),
                        ..Default::default()
                    },
                }),
            },
        );

        let cm = &app.active_workspace().unwrap().messages["C_GENERAL"];
        let msg = cm
            .messages
            .iter()
            .find(|m| m.text.as_deref() == Some("confirm me"))
            .unwrap();
        assert!(!cm.is_pending(msg.ts.as_deref().unwrap()));
    }

    #[test]
    fn realtime_message_upserts_into_channel() {
        let mut app = test_app();
        let team = app.active_team.clone().unwrap();
        let before = app.workspaces[&team].messages["C_GENERAL"].messages.len();
        let ev = RtEvent::Message(SlackMessage {
            user: Some("U_ALICE".into()),
            ts: Some("9999999999.000001".into()),
            channel: Some("C_GENERAL".into()),
            text: Some("live!".into()),
            ..Default::default()
        });
        let _ = update(&mut app, Message::Realtime(team.clone(), 1, ev));
        let after = app.workspaces[&team].messages["C_GENERAL"].messages.len();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn realtime_delete_removes_message() {
        let mut app = test_app();
        let team = app.active_team.clone().unwrap();
        let ev = RtEvent::MessageDeleted {
            channel: "C_GENERAL".into(),
            deleted_ts: "1783372300.000100".into(),
        };
        let _ = update(&mut app, Message::Realtime(team.clone(), 1, ev));
        let exists = app.workspaces[&team].messages["C_GENERAL"]
            .messages
            .iter()
            .any(|m| m.ts.as_deref() == Some("1783372300.000100"));
        assert!(!exists);
    }

    #[test]
    fn rt_connected_stores_connection() {
        let mut app = test_app();
        let team = app.active_team.clone().unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let conn = Connection::from_sender(tx);
        let _ = update(&mut app, Message::RtConnected(team.clone(), 2, conn));
        assert!(app.workspaces[&team].rt.is_connected());
        assert_eq!(app.workspaces[&team].rt_generation, 2);
        let _ = update(&mut app, Message::RtDisconnected(team.clone(), 1));
        assert!(app.workspaces[&team].rt.is_connected());
        let _ = update(&mut app, Message::RtDisconnected(team.clone(), 2));
        assert!(!app.workspaces[&team].rt.is_connected());
    }

    #[test]
    fn stale_realtime_event_is_ignored() {
        let mut app = test_app();
        let team = app.active_team.clone().unwrap();
        app.workspaces.get_mut(&team).unwrap().rt_generation = 5;
        let before = app.workspaces[&team].messages["C_GENERAL"].messages.len();
        let ev = RtEvent::Message(SlackMessage {
            user: Some("U_ALICE".into()),
            ts: Some("9999999999.000002".into()),
            channel: Some("C_GENERAL".into()),
            text: Some("stale".into()),
            ..Default::default()
        });
        let _ = update(&mut app, Message::Realtime(team.clone(), 4, ev));
        let after = app.workspaces[&team].messages["C_GENERAL"].messages.len();
        assert_eq!(after, before);
    }
}
