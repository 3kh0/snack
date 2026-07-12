use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::text_editor::{self, Content};

use crate::cache::Cache;
use crate::config::{self, Session};
use crate::slack::api::{self, HistoryArgs};
use crate::slack::events::RtEvent;
use crate::slack::models::{
    ActivityFeedPage, BootData, Channel, ChannelId, CountsPage, Emoji, HistoryPage,
    Message as SlackMessage, MessageTs, MessagesListPage, SearchMessagesPage, SentMessage,
    SidebarDmsPage, TeamId, User, UserId,
};
use crate::slack::realtime::Connection;
use crate::slack::{Error as SlackError, SlackClient, Transport};
use crate::state::{ChannelMessages, Screen, Toast, Workspace};
use crate::ui;

mod agent;
mod palette;
mod subscription;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_visual;
mod update;
mod view;

pub use palette::{PaletteEntry, PaletteState, PaletteTarget};
use subscription::subscription;
use update::{preferred_channel, update};
use view::view;

type ActiveThreadKey = (ChannelId, MessageTs);
type ThreadKey = (TeamId, ChannelId, MessageTs);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSelectionSurface {
    Channel {
        channel: ChannelId,
    },
    Thread {
        channel: ChannelId,
        root_ts: MessageTs,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSelectionPoint {
    pub surface: TextSelectionSurface,
    pub message_ts: MessageTs,
    pub message_index: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: TextSelectionPoint,
    pub focus: TextSelectionPoint,
    pub dragging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingScrollTarget {
    Message(MessageTs),
    FirstUnreadAfter(MessageTs),
    Latest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryLoadKind {
    Latest,
    Since,
    Around,
    Older,
}

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

#[derive(Debug, Clone, Default)]
pub struct ActivityState {
    pub items: Vec<crate::slack::models::ActivityItem>,
    pub hydrated: HashMap<(ChannelId, MessageTs), SlackMessage>,
    pub loading: bool,
    pub loaded: bool,
    pub selected: Option<String>,
}

impl ActivityState {
    pub fn upsert(&mut self, item: crate::slack::models::ActivityItem) {
        let identity = item.identity();
        if let Some(existing) = self.items.iter_mut().find(|i| i.identity() == identity) {
            if crate::state::cmp_ts(Some(&item.feed_ts), Some(&existing.feed_ts)).is_lt() {
                return;
            }
            if self.selected.as_deref() == Some(existing.key.as_str()) {
                self.selected = Some(item.key.clone());
            }
            *existing = item;
        } else {
            self.items.push(item);
        }
        self.items
            .sort_by(|a, b| crate::state::cmp_ts(Some(&b.feed_ts), Some(&a.feed_ts)));
    }
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

#[derive(Debug, Clone)]
pub struct ComposerAttachment {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    pub uploading: bool,
    pub upload_started: Option<Instant>,
    pub upload_cancel: Option<Arc<AtomicBool>>,
    pub upload_progress: Option<Arc<AtomicU64>>,
    pub preview_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PendingFileMessage {
    pub team: TeamId,
    pub channel: ChannelId,
    pub thread_ts: Option<MessageTs>,
    pub message_ts: MessageTs,
    pub client_msg_id: String,
    pub text: String,
    pub attachments: Vec<ComposerAttachment>,
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
    thread_open: bool,
    main_view: crate::state::MainView,
    activity: ActivityState,
    workspaces: BTreeMap<TeamId, Workspace>,
    threads: HashMap<ThreadKey, ChannelMessages>,
    composer: Content,
    thread_composer: Content,
    composer_attachments: Vec<ComposerAttachment>,
    thread_composer_attachments: Vec<ComposerAttachment>,
    pending_file_messages: Vec<PendingFileMessage>,
    attachment_seq: u64,
    editing: Option<(ChannelId, MessageTs)>,
    edit_text: String,
    hovered_message: Option<(bool, MessageTs)>,
    text_selection: Option<TextSelection>,
    search_input: String,
    search: Option<SearchState>,
    palette: Option<PaletteState>,
    palette_open: bool,
    errors: Vec<Toast>,
    send_seq: u64,
    last_typing: HashMap<(TeamId, ChannelId), Instant>,
    last_active_channels: HashMap<TeamId, ChannelId>,
    file_previews: HashMap<String, FilePreview>,
    avatar_previews: HashMap<UserId, FilePreview>,
    emoji_previews: HashMap<String, FilePreview>,
    emoji_animation_started: Instant,
    emoji_hydrated: HashSet<(TeamId, String)>,
    channel_hydrated: HashSet<(TeamId, ChannelId)>,
    avatar_profile_hydrated: HashSet<UserId>,
    pending_scroll_to: Option<(ChannelId, PendingScrollTarget)>,
    pending_marks: HashSet<(TeamId, ChannelId, MessageTs)>,
    mark_blocked: HashSet<(TeamId, ChannelId)>,
    cache_dirty: HashMap<TeamId, Instant>,
    cache_saving: HashMap<TeamId, Instant>,
    settings: config::Settings,
    show_settings: bool,
    settings_open: bool,
    show_account_menu: bool,
    account_menu_open: bool,
    sidebar_resizing: bool,
    sidebar_resize_prev_x: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerTarget {
    Channel,
    Thread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatMark {
    Bold,
    Italic,
    Strike,
    Code,
    CodeBlock,
    Quote,
}

#[derive(Debug, Clone)]
pub enum Message {
    WorkspaceSelected(TeamId),
    ChannelSelected(ChannelId),
    ComposerAction {
        target: ComposerTarget,
        action: text_editor::Action,
    },
    ComposerFormat {
        target: ComposerTarget,
        mark: FormatMark,
    },
    AttachmentPickerOpened(ComposerTarget),
    AttachmentsPicked {
        target: ComposerTarget,
        paths: Vec<PathBuf>,
    },
    FilesDropped(Vec<PathBuf>),
    VideoPreviewReady {
        source: PathBuf,
        result: Result<PathBuf, String>,
    },
    AttachmentRemoved {
        target: ComposerTarget,
        id: u64,
    },
    PasteAttachmentsRequested(ComposerTarget),
    ClipboardFilesRead {
        target: ComposerTarget,
        result: Result<Vec<PathBuf>, String>,
    },
    ClipboardTextRead {
        target: ComposerTarget,
        result: Result<String, String>,
    },
    AttachmentsSent {
        target: ComposerTarget,
        team: TeamId,
        channel: ChannelId,
        thread_ts: Option<MessageTs>,
        message_ts: MessageTs,
        client_msg_id: String,
        result: Result<(), SlackError>,
    },
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
    HistoryLoaded(
        TeamId,
        ChannelId,
        HistoryLoadKind,
        Result<HistoryPage, SlackError>,
    ),
    ChannelScrolled {
        channel: ChannelId,
        y: f32,
    },
    ChannelMarked(TeamId, ChannelId, MessageTs, Result<(), SlackError>),
    ThreadOpened {
        channel: ChannelId,
        ts: MessageTs,
    },
    ThreadClosed,
    ThreadDismissed,
    ThreadLoaded {
        team: TeamId,
        channel: ChannelId,
        root_ts: MessageTs,
        result: Result<HistoryPage, SlackError>,
    },
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
    TextSelectionStarted(TextSelectionPoint),
    TextSelectionDragged(TextSelectionPoint),
    TextSelectionEnded,
    TextSelectionCopyRequested,
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
    PaletteToggled,
    PaletteClosed,
    PaletteDismissed,
    PaletteQueryChanged(String),
    PaletteMoved(isize),
    PaletteSubmitted,
    PaletteEntryPressed(usize),
    PaletteRemoteUsersLoaded {
        team: TeamId,
        seq: u64,
        result: Result<Vec<User>, SlackError>,
    },
    PaletteRemoteChannelsLoaded {
        team: TeamId,
        seq: u64,
        result: Result<Vec<Channel>, SlackError>,
    },
    DmOpened {
        team: TeamId,
        user: UserId,
        result: Result<ChannelId, SlackError>,
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
        requested: Vec<ChannelId>,
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
    MainViewSelected(crate::state::MainView),
    ActivityLoaded(TeamId, Result<ActivityFeedPage, SlackError>),
    ActivityMessagesLoaded(TeamId, Result<MessagesListPage, SlackError>),
    ActivitySelected(String),
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
    SettingsDismissed,
    SettingsAccentSelected(config::AccentColor),
    SettingsGapChanged(f32),
    SettingsRadiusChanged(f32),
    SettingsBorderChanged(f32),
    SettingsReset,
    AccountMenuDismissed,
    SidebarResizeStarted,
    SidebarResizeMoved(f32),
    SidebarResizeEnded,
    AnimationTick,
    Tick,
    AgentRequest {
        id: u64,
        command: agent::AgentCommand,
    },
    AgentScreenshotCaptured {
        id: u64,
        path: PathBuf,
        result: Result<iced::window::Screenshot, String>,
    },
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
            thread_open: false,
            main_view: crate::state::MainView::Home,
            activity: ActivityState::default(),
            workspaces: BTreeMap::new(),
            threads: HashMap::new(),
            composer: Content::new(),
            thread_composer: Content::new(),
            composer_attachments: Vec::new(),
            thread_composer_attachments: Vec::new(),
            pending_file_messages: Vec::new(),
            attachment_seq: 0,
            editing: None,
            edit_text: String::new(),
            hovered_message: None,
            search_input: String::new(),
            search: None,
            palette: None,
            palette_open: false,
            errors: Vec::new(),
            send_seq: 0,
            last_typing: HashMap::new(),
            last_active_channels: HashMap::new(),
            file_previews: HashMap::new(),
            avatar_previews: HashMap::new(),
            emoji_previews: HashMap::new(),
            emoji_animation_started: Instant::now(),
            emoji_hydrated: HashSet::new(),
            channel_hydrated: HashSet::new(),
            avatar_profile_hydrated: HashSet::new(),
            text_selection: None,
            pending_scroll_to: None,
            pending_marks: HashSet::new(),
            mark_blocked: HashSet::new(),
            cache_dirty: HashMap::new(),
            cache_saving: HashMap::new(),
            settings,
            show_settings: false,
            settings_open: false,
            show_account_menu: false,
            account_menu_open: false,
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
        if let Some(anchor) = self.unread_anchor(team, channel) {
            return self.load_history_around(team, channel, anchor);
        }
        self.load_history_page(team, channel, HistoryLoadKind::Latest, None, None)
    }

    fn unread_anchor(&self, team: &str, channel: &ChannelId) -> Option<MessageTs> {
        let ws = self.workspaces.get(team)?;
        let unread = ws
            .channels
            .get(channel)
            .map(|c| ws.unread_total(c) > 0)
            .unwrap_or_else(|| {
                ws.messages
                    .get(channel)
                    .is_some_and(|cm| cm.unread_count > 0 || cm.mention_count > 0)
            });
        if !unread {
            return None;
        }
        ws.messages.get(channel)?.last_read.clone()
    }

    fn load_history_around(
        &self,
        team: &str,
        channel: &ChannelId,
        anchor: MessageTs,
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
        let fetch_channel = channel.clone();
        Task::perform(
            async move {
                let before = api::fetch_history(
                    &transport,
                    &client,
                    &ws,
                    HistoryArgs {
                        channel: fetch_channel.clone(),
                        latest: Some(anchor.clone()),
                        limit: Some(50),
                        inclusive: true,
                        ..Default::default()
                    },
                )
                .await?;
                let after = api::fetch_history(
                    &transport,
                    &client,
                    &ws,
                    HistoryArgs {
                        channel: fetch_channel,
                        oldest: Some(anchor),
                        limit: Some(50),
                        inclusive: true,
                        ..Default::default()
                    },
                )
                .await?;
                Ok(merge_history_pages(before, after))
            },
            move |result| {
                Message::HistoryLoaded(
                    team.clone(),
                    channel.clone(),
                    HistoryLoadKind::Around,
                    result,
                )
            },
        )
    }

    fn load_history_since(
        &self,
        team: &str,
        channel: &ChannelId,
        oldest: Option<MessageTs>,
    ) -> Task<Message> {
        self.load_history_page(team, channel, HistoryLoadKind::Since, None, oldest)
    }

    fn load_older_history(
        &self,
        team: &str,
        channel: &ChannelId,
        latest: MessageTs,
    ) -> Task<Message> {
        self.load_history_page(team, channel, HistoryLoadKind::Older, Some(latest), None)
    }

    fn load_history_page(
        &self,
        team: &str,
        channel: &ChannelId,
        kind: HistoryLoadKind,
        latest: Option<MessageTs>,
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
            latest,
            oldest,
            limit: Some(50),
            ..Default::default()
        };
        Task::perform(
            async move { api::fetch_history(&transport, &client, &ws, args).await },
            move |result| Message::HistoryLoaded(team.clone(), channel.clone(), kind, result),
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

fn merge_history_pages(mut before: HistoryPage, after: HistoryPage) -> HistoryPage {
    let mut seen: HashSet<_> = before
        .messages
        .iter()
        .filter_map(|message| message.ts.clone())
        .collect();
    before.messages.extend(
        after
            .messages
            .into_iter()
            .filter(|message| message.ts.as_ref().is_none_or(|ts| seen.insert(ts.clone()))),
    );
    before.has_more |= after.has_more;
    before.latest_updates.extend(after.latest_updates);
    before.unchanged_messages.extend(after.unchanged_messages);
    before
}

pub fn run() -> iced::Result {
    iced::application(App::boot, update, view)
        .subscription(subscription)
        .theme(theme)
        .title("Snack")
        .window(window_settings())
        .run()
}

fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        icon: app_icon(),
        ..iced::window::Settings::default()
    }
}

fn app_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_file_data(include_bytes!("../assets/icons/icon-256.png"), None).ok()
}

fn theme(_app: &App) -> iced::Theme {
    ui::theme::midnight()
}
