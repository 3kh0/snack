use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill, Subscription, Task};

use crate::config::{self, Session};
use crate::slack::api::{self, HistoryArgs};
use crate::slack::events::RtEvent;
use crate::slack::models::{
    BootData, ChannelId, HistoryPage, Message as SlackMessage, SentMessage, TeamId,
};
use crate::slack::realtime::{self, ConnectParams, Connection, RtUpdate};
use crate::slack::{Error as SlackError, SlackClient, Transport};
use crate::state::{RealtimeStatus, Screen, Toast, Workspace};
use crate::ui;

pub struct App {
    screen: Screen,
    session: Option<Session>,
    client: SlackClient,
    transport: Option<Arc<Transport>>,
    active_team: Option<TeamId>,
    active_channel: Option<ChannelId>,
    workspaces: BTreeMap<TeamId, Workspace>,
    composer_text: String,
    errors: Vec<Toast>,
    send_seq: u64,
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
    HistoryLoaded(TeamId, ChannelId, Result<HistoryPage, SlackError>),
    Realtime(TeamId, RtEvent),
    RtConnected(TeamId, Connection),
    RtDisconnected(TeamId),
    SignInPressed,
    RetryAuth,
    Tick,
}

impl App {
    fn empty() -> Self {
        App {
            screen: Screen::Login,
            session: None,
            client: SlackClient::default(),
            transport: None,
            active_team: None,
            active_channel: None,
            workspaces: BTreeMap::new(),
            composer_text: String::new(),
            errors: Vec::new(),
            send_seq: 0,
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
                    for ws in session.workspaces.values() {
                        self.workspaces
                            .insert(ws.team_id.clone(), Workspace::from_session(ws));
                    }
                    self.active_team = session.workspaces.keys().next().cloned();
                    self.session = Some(session);
                    self.screen = Screen::Loading;
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
        let tasks = session.workspaces.values().map(|ws| {
            let transport = transport.clone();
            let client = self.client.clone();
            let ws = ws.clone();
            let team = ws.team_id.clone();
            Task::perform(
                async move { api::fetch_user_boot(&transport, &client, &ws).await },
                move |result| Message::BootLoaded(team.clone(), result),
            )
        });
        Task::batch(tasks)
    }

    fn load_history(&self, team: &str, channel: &ChannelId) -> Task<Message> {
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
            limit: Some(50),
            ..Default::default()
        };
        Task::perform(
            async move { api::fetch_history(&transport, &client, &ws, args).await },
            move |result| Message::HistoryLoaded(team.clone(), channel.clone(), result),
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
            }
            Task::none()
        }

        Message::ComposerChanged(value) => {
            app.composer_text = value;
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
                }
                Err(e) => app.toast(format!("history failed for {channel}: {e}")),
            }
            Task::none()
        }

        Message::Realtime(team, event) => {
            apply_realtime(app, &team, event);
            Task::none()
        }

        Message::RtConnected(team, connection) => {
            if let Some(ws) = app.workspaces.get_mut(&team) {
                ws.rt = RealtimeStatus::Connected(connection);
            }
            if app.active_team.as_deref() == Some(&team) {
                if let Some(channel) = app.active_channel.clone() {
                    if app.transport.is_some() {
                        return app.load_history(&team, &channel);
                    }
                }
            }
            Task::none()
        }

        Message::RtDisconnected(team) => {
            if let Some(ws) = app.workspaces.get_mut(&team) {
                ws.rt = RealtimeStatus::Disconnected;
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

fn apply_realtime(app: &mut App, team: &str, event: RtEvent) {
    let now = Instant::now();
    let Some(ws) = app.workspaces.get_mut(team) else {
        tracing::debug!(%team, "realtime event for unknown workspace, ignoring");
        return;
    };
    match event {
        RtEvent::Message(msg) => {
            let Some(channel) = msg.channel.clone() else {
                return;
            };
            let cm = ws.messages.entry(channel).or_default();
            if let Some(cid) = msg.client_msg_id.clone() {
                if cm.confirm(&cid, msg.clone()) {
                    return;
                }
            }
            cm.upsert(msg);
        }
        RtEvent::MessageChanged { channel, message } => {
            ws.messages.entry(channel).or_default().upsert(message);
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
        RtEvent::PresenceChange { .. } => {}
        RtEvent::Unknown(raw) => {
            tracing::debug!(kind = %raw.kind, "unknown realtime event, ignoring");
        }
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
        RtUpdate::Connected(connection) => Message::RtConnected(team, connection),
        RtUpdate::Event(event) => Message::Realtime(team, event),
        RtUpdate::Disconnected => Message::RtDisconnected(team),
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
            rt: RealtimeStatus::default(),
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
        let _ = update(&mut app, Message::Realtime(team.clone(), ev));
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
        let _ = update(&mut app, Message::Realtime(team.clone(), ev));
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
        let _ = update(&mut app, Message::RtConnected(team.clone(), conn));
        assert!(app.workspaces[&team].rt.is_connected());
        let _ = update(&mut app, Message::RtDisconnected(team.clone()));
        assert!(!app.workspaces[&team].rt.is_connected());
    }
}
