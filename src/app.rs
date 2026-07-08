use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::image::Handle as ImageHandle;

use crate::cache::Cache;
use crate::config::{self, Session};
use crate::slack::api::{self, HistoryArgs};
use crate::slack::events::RtEvent;
use crate::slack::models::{
    BootData, Channel, ChannelId, CountsPage, Emoji, HistoryPage, Message as SlackMessage,
    MessageTs, SearchMessagesPage, SentMessage, SidebarDmsPage, TeamId, User, UserId,
};
use crate::slack::realtime::Connection;
use crate::slack::{Error as SlackError, SlackClient, Transport};
use crate::state::{ChannelMessages, Screen, Toast, Workspace};
use crate::ui;

mod subscription;
#[cfg(test)]
mod tests;
mod update;
mod view;

use subscription::subscription;
use update::{preferred_channel, update};
use view::view;

type ActiveThreadKey = (ChannelId, MessageTs);
type ThreadKey = (TeamId, ChannelId, MessageTs);

#[derive(Debug, Clone)]
pub enum FilePreview {
    Loading,
    Loaded(ImageHandle),
    Animated {
        frames: Vec<ImageHandle>,
        delays: Vec<Duration>,
        total: Duration,
    },
    Failed,
}

#[derive(Debug, Clone)]
struct DesktopNotification {
    title: String,
    body: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub channel: ChannelId,
    pub channel_label: String,
    pub message: SlackMessage,
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub team: TeamId,
    pub page: u32,
    pub page_count: u32,
    pub total: u64,
    pub hits: Vec<SearchHit>,
    pub loading: bool,
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
    editing: Option<(ChannelId, MessageTs)>,
    edit_text: String,
    hovered_message: Option<(bool, MessageTs)>,
    search_input: String,
    search: Option<SearchState>,
    errors: Vec<Toast>,
    send_seq: u64,
    last_typing: HashMap<(TeamId, ChannelId), Instant>,
    last_active_channels: HashMap<TeamId, ChannelId>,
    file_previews: HashMap<String, FilePreview>,
    avatar_previews: HashMap<UserId, FilePreview>,
    emoji_previews: HashMap<String, FilePreview>,
    emoji_animation_started: Instant,
    emoji_hydrated: HashSet<(TeamId, String)>,
    avatar_profile_hydrated: HashSet<UserId>,
    pending_scroll_to: Option<(ChannelId, MessageTs)>,
    cache_dirty: HashMap<TeamId, Instant>,
    cache_saving: HashMap<TeamId, Instant>,
    settings: config::Settings,
    show_settings: bool,
    show_account_menu: bool,
    sidebar_resizing: bool,
    sidebar_resize_prev_x: Option<f32>,
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
    SidebarDmsLoaded(TeamId, Result<SidebarDmsPage, SlackError>),
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
    EditPressed {
        channel: ChannelId,
        ts: MessageTs,
    },
    EditComposerChanged(String),
    EditSubmit,
    CopyMessage(String),
    MessageHovered {
        in_thread: bool,
        ts: MessageTs,
    },
    MessageUnhovered,
    EditCancelled,
    MessageEdited {
        team: TeamId,
        channel: ChannelId,
        ts: MessageTs,
        result: Result<SentMessage, SlackError>,
    },
    DeletePressed {
        channel: ChannelId,
        ts: MessageTs,
    },
    MessageDeleted {
        team: TeamId,
        channel: ChannelId,
        ts: MessageTs,
        result: Result<(), SlackError>,
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
    SearchInputChanged(String),
    SearchSubmitted,
    SearchCleared,
    SearchPageRequested(u32),
    SearchLoaded {
        team: TeamId,
        query: String,
        page: u32,
        result: Result<SearchMessagesPage, SlackError>,
    },
    SearchResultSelected {
        channel: ChannelId,
        ts: MessageTs,
        thread_ts: Option<MessageTs>,
    },
    FileDownloadPressed {
        url: String,
        filename: String,
    },
    FileDownloaded(Result<PathBuf, SlackError>),
    OpenUrl(String),
    UrlOpened(Result<(), String>),
    FilePreviewLoaded {
        key: String,
        result: Result<Vec<u8>, SlackError>,
    },
    AvatarLoaded {
        user: UserId,
        result: Result<Vec<u8>, SlackError>,
    },
    EmojiPreviewLoaded {
        key: String,
        result: Result<FilePreview, SlackError>,
    },
    UsersLoaded {
        team: TeamId,
        result: Result<Vec<User>, SlackError>,
    },
    EmojisLoaded {
        team: TeamId,
        requested: Vec<String>,
        result: Result<Vec<Emoji>, SlackError>,
    },
    ChannelsLoaded {
        team: TeamId,
        result: Result<Vec<Channel>, SlackError>,
    },
    DesktopNotificationShown(Result<(), String>),
    CacheSaved {
        team: TeamId,
        started_at: Instant,
        result: Result<(), String>,
    },
    Realtime(TeamId, u64, RtEvent),
    RtConnected(TeamId, u64, Connection),
    RtDisconnected(TeamId, u64),
    SignInPressed,
    RetryAuth,
    AccountMenuToggled,
    SelfPresenceSelected(crate::state::Presence),
    SelfPresenceUpdated {
        team: TeamId,
        presence: crate::state::Presence,
        previous: Option<crate::state::Presence>,
        result: Result<(), SlackError>,
    },
    SignOutPressed,
    SettingsOpened,
    SettingsClosed,
    SettingsAccentSelected(config::AccentColor),
    SettingsGapChanged(f32),
    SettingsRadiusChanged(f32),
    SettingsBorderChanged(f32),
    SettingsReset,
    SidebarResizeStarted,
    SidebarResizeMoved(f32),
    SidebarResizeEnded,
    AnimationTick,
    Tick,
}

impl App {
    fn empty() -> Self {
        let settings = config::load_settings();
        ui::theme::apply(&settings);
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
            editing: None,
            edit_text: String::new(),
            hovered_message: None,
            search_input: String::new(),
            search: None,
            errors: Vec::new(),
            send_seq: 0,
            last_typing: HashMap::new(),
            last_active_channels: HashMap::new(),
            file_previews: HashMap::new(),
            avatar_previews: HashMap::new(),
            emoji_previews: HashMap::new(),
            emoji_animation_started: Instant::now(),
            emoji_hydrated: HashSet::new(),
            avatar_profile_hydrated: HashSet::new(),
            pending_scroll_to: None,
            cache_dirty: HashMap::new(),
            cache_saving: HashMap::new(),
            settings,
            show_settings: false,
            show_account_menu: false,
            sidebar_resizing: false,
            sidebar_resize_prev_x: None,
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

            let dms_transport = transport.clone();
            let dms_client = self.client.clone();
            let dms_ws = ws.clone();
            let team = ws.team_id.clone();
            let dms = Task::perform(
                async move { api::fetch_sidebar_dms(&dms_transport, &dms_client, &dms_ws).await },
                move |result| Message::SidebarDmsLoaded(team.clone(), result),
            );
            [boot, counts, dms]
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
        .theme(theme)
        .title("Snack")
        .run()
}

fn theme(_app: &App) -> iced::Theme {
    ui::theme::midnight()
}
