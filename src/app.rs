use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use crate::state::{Presence, RealtimeStatus, Screen, Toast, Workspace};
use crate::ui;

pub struct App {
    screen: Screen,
    session: Option<Session>,
    cache: Option<Cache>,
    client: SlackClient,
    transport: Option<Arc<Transport>>,
    active_team: Option<TeamId>,
    active_channel: Option<ChannelId>,
    workspaces: BTreeMap<TeamId, Workspace>,
    composer_text: String,
    errors: Vec<Toast>,
    send_seq: u64,
    last_typing: HashMap<(TeamId, ChannelId), Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
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
            workspaces: BTreeMap::new(),
            composer_text: String::new(),
            errors: Vec::new(),
            send_seq: 0,
            last_typing: HashMap::new(),
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
                            self.active_channel = self
                                .workspaces
                                .get(&team)
                                .and_then(|ws| ws.channels.keys().next().cloned());
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
        Message::ChannelSelected(id) => {
            app.active_channel = Some(id.clone());
            if let Some(team) = app.active_team.clone() {
                let needs_load = app
                    .workspaces
                    .get(&team)
                    .map(|ws| !ws.messages.get(&id).map(|cm| cm.loaded).unwrap_or(false))
                    .unwrap_or(false);
                if app.transport.is_some() && needs_load {
                    return app.load_history(&team, &id);
                }
                return mark_latest_visible(app, &team, &id);
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
                    return mark_latest_visible(app, &team, &channel);
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
                if is_auth_error(&e) {
                    app.screen = Screen::Login;
                }
                persist_workspace(app, &team);
            }
            Task::none()
        }

        Message::Realtime(team, generation, event) => {
            apply_realtime(app, &team, generation, event);
            persist_workspace(app, &team);
            if app.active_team.as_deref() == Some(&team) {
                if let Some(channel) = app.active_channel.clone() {
                    return mark_latest_visible(app, &team, &channel);
                }
            }
            Task::none()
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
    let Some(cm) = ws.messages.get_mut(&channel) else {
        return Task::none();
    };
    let adding = !cm
        .messages
        .iter()
        .find(|m| m.ts.as_deref() == Some(ts.as_str()))
        .and_then(|m| m.reactions.iter().find(|r| r.name == name))
        .is_some_and(|r| crate::state::reaction_has_user(r, &user));

    cm.apply_reaction(&ts, &user, &name, adding);
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

fn apply_realtime(app: &mut App, team: &str, generation: u64, event: RtEvent) {
    let now = Instant::now();
    let Some(ws) = app.workspaces.get_mut(team) else {
        tracing::debug!(%team, "realtime event for unknown workspace, ignoring");
        return;
    };
    if generation != ws.rt_generation {
        tracing::debug!(%team, generation, current = ws.rt_generation, "stale realtime event, ignoring");
        return;
    }
    match event {
        RtEvent::Message(msg) => {
            let Some(channel) = msg.channel.clone() else {
                return;
            };
            if let Some(user) = msg.user.as_deref() {
                ws.clear_typing_user(&channel, user);
            }
            let cm = ws.messages.entry(channel).or_default();
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
        RtEvent::MessageChanged { channel, message } => {
            ws.messages
                .entry(channel)
                .or_default()
                .merge_update(message);
        }
        RtEvent::MessageDeleted {
            channel,
            deleted_ts,
        } => {
            if let Some(cm) = ws.messages.get_mut(&channel) {
                cm.remove(&deleted_ts);
            }
        }
        RtEvent::UserTyping { channel, user } => {
            ws.set_typing(&channel, user, now);
        }
        RtEvent::PresenceChange { user, presence } => {
            ws.set_presence(user, Presence::from_slack(&presence));
        }
        RtEvent::ReactionAdded {
            channel,
            ts,
            user,
            reaction,
        } => {
            ws.messages
                .entry(channel)
                .or_default()
                .apply_reaction(&ts, &user, &reaction, true);
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
        }
        RtEvent::Unknown(raw) => {
            tracing::debug!(kind = %raw.kind, "unknown realtime event, ignoring");
        }
    }
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

    let sidebar = ui::sidebar::view(ws, app.active_channel.as_deref());

    let main: Element<'_, Message> = match app.active_channel.as_deref() {
        Some(channel_id) => {
            let label = ws
                .channels
                .get(channel_id)
                .map(crate::state::channel_label)
                .unwrap_or_else(|| channel_id.to_owned());
            column![
                container(ui::channel::view(ws, channel_id)).height(Fill),
                ui::composer::view(&app.composer_text, &label),
            ]
            .width(Fill)
            .height(Fill)
            .into()
        }
        None => center_text("Select a channel"),
    };

    row![sidebar, main].width(Fill).height(Fill).into()
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
