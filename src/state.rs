use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use crate::slack::models::{
    Channel, ChannelId, Emoji, File, Message as SlackMessage, MessageTs, Reaction, TeamId, User,
    UserId,
};
use crate::slack::realtime::Connection;

pub const RECENT_CHANNELS_MAX: usize = 20;

pub const FRECENCY_HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrecencyEntry {
    pub score: f64,
    pub last_visit: i64,
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn decayed(score: f64, last_visit: i64, now: i64) -> f64 {
    let elapsed = (now - last_visit).max(0) as f64;
    score * 0.5_f64.powf(elapsed / FRECENCY_HALF_LIFE_SECS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    Loading,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainView {
    #[default]
    Home,
    Dms,
    Activity,
}

#[derive(Debug, Clone, Default)]
pub enum RealtimeStatus {
    #[default]
    Disconnected,
    Connected(Connection),
}

impl RealtimeStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self, RealtimeStatus::Connected(_))
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Presence {
    Active,
    Away,
    #[default]
    Unknown,
}

impl Presence {
    pub fn from_slack(value: &str) -> Self {
        match value {
            "active" => Presence::Active,
            "away" => Presence::Away,
            _ => Presence::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChannelMessages {
    pub messages: Vec<SlackMessage>,
    pub loaded: bool,
    pub has_more_older: bool,
    pub history_loading_older: bool,
    pub history_failed: bool,
    pub pending: Vec<MessageTs>,
    pub last_read: Option<MessageTs>,
    pub unread_count: u32,
    pub mention_count: u32,
}

impl ChannelMessages {
    pub fn upsert(&mut self, msg: SlackMessage) -> bool {
        let Some(ts) = msg.ts.clone() else {
            self.messages.push(msg);
            return true;
        };
        match self.index_of(&ts) {
            Some(i) => {
                self.messages[i] = msg;
                false
            }
            None => {
                let pos = self
                    .messages
                    .binary_search_by(|m| cmp_ts(m.ts.as_deref(), Some(&ts)))
                    .unwrap_or_else(|e| e);
                self.messages.insert(pos, msg);
                true
            }
        }
    }

    pub fn remove(&mut self, ts: &str) -> bool {
        match self.index_of(ts) {
            Some(i) => {
                self.messages.remove(i);
                self.pending.retain(|p| p != ts);
                true
            }
            None => false,
        }
    }

    pub fn confirm(&mut self, client_msg_id: &str, confirmed: SlackMessage) -> bool {
        let temp_ts = self.messages.iter().find_map(|m| {
            match (m.client_msg_id.as_deref(), m.ts.as_deref()) {
                (Some(cid), Some(ts))
                    if cid == client_msg_id && self.pending.iter().any(|p| p == ts) =>
                {
                    Some(ts.to_owned())
                }
                _ => None,
            }
        });
        if let Some(ts) = temp_ts {
            self.remove(&ts);
            self.upsert(confirmed);
            true
        } else {
            false
        }
    }

    pub fn confirm_matching_pending(
        &mut self,
        user: Option<&str>,
        text: Option<&str>,
        confirmed: SlackMessage,
    ) -> bool {
        let matches = self
            .messages
            .iter()
            .filter_map(|m| {
                let ts = m.ts.as_deref()?;
                if !self.pending.iter().any(|p| p == ts) {
                    return None;
                }
                (m.user.as_deref() == user && m.text.as_deref() == text).then(|| ts.to_owned())
            })
            .take(2)
            .collect::<Vec<_>>();
        if let [ts] = matches.as_slice() {
            self.remove(&ts);
            self.upsert(confirmed);
            true
        } else {
            false
        }
    }

    pub fn merge_update(&mut self, update: SlackMessage) -> bool {
        let Some(ts) = update.ts.clone() else {
            self.messages.push(update);
            return true;
        };
        match self.index_of(&ts) {
            Some(i) => {
                merge_message(&mut self.messages[i], update);
                false
            }
            None => self.upsert(update),
        }
    }

    pub fn is_pending(&self, ts: &str) -> bool {
        self.pending.iter().any(|p| p == ts)
    }

    pub fn latest_ts(&self) -> Option<MessageTs> {
        self.messages
            .iter()
            .filter_map(|m| m.ts.clone())
            .max_by(|a, b| ts_key(a).cmp(&ts_key(b)))
    }

    pub fn oldest_ts(&self) -> Option<MessageTs> {
        self.messages
            .iter()
            .filter_map(|m| m.ts.clone())
            .min_by(|a, b| ts_key(a).cmp(&ts_key(b)))
    }

    pub fn apply_reaction(&mut self, ts: &str, user: &str, name: &str, added: bool) -> bool {
        let Some(i) = self.index_of(ts) else {
            return false;
        };
        apply_message_reaction(&mut self.messages[i], user, name, added)
    }

    fn index_of(&self, ts: &str) -> Option<usize> {
        self.messages
            .iter()
            .position(|m| m.ts.as_deref() == Some(ts))
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub team_id: TeamId,
    pub name: String,
    pub url: String,
    pub self_user_id: UserId,
    pub activity_unread_count: Option<u32>,
    pub channels: BTreeMap<ChannelId, Channel>,
    pub starred_order: Vec<ChannelId>,
    pub dm_order: Vec<ChannelId>,
    pub recent_channels: Vec<ChannelId>,
    pub last_active_channel: Option<ChannelId>,
    pub priority_scores: BTreeMap<ChannelId, f64>,
    pub frecency: BTreeMap<ChannelId, FrecencyEntry>,
    pub hide_read_channels_unless_starred: bool,
    pub priority_sidebar_section: bool,
    pub users: HashMap<UserId, User>,
    pub custom_emoji: HashMap<String, Emoji>,
    pub messages: HashMap<ChannelId, ChannelMessages>,
    pub typing: HashMap<ChannelId, Vec<(UserId, Instant)>>,
    pub presence: HashMap<UserId, Presence>,
    pub rt: RealtimeStatus,
    pub rt_generation: u64,
}

impl Workspace {
    pub fn from_session(s: &crate::config::WorkspaceSession) -> Self {
        Workspace {
            team_id: s.team_id.clone(),
            name: s.name.clone(),
            url: s.url.clone(),
            self_user_id: s.user_id.clone(),
            activity_unread_count: None,
            channels: BTreeMap::new(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        }
    }

    pub fn apply_boot(&mut self, boot: crate::slack::models::BootData) {
        let self_user = if boot.self_user.id.is_empty() {
            None
        } else {
            Some(User {
                id: boot.self_user.id.clone(),
                name: boot.self_user.name.clone(),
                real_name: boot
                    .self_user
                    .profile
                    .as_ref()
                    .and_then(|profile| profile.real_name.clone()),
                profile: boot.self_user.profile.clone(),
                extra: boot.self_user.extra.clone(),
                ..Default::default()
            })
        };
        if !boot.self_user.id.is_empty() {
            self.self_user_id = boot.self_user.id.clone();
        }
        if let Some(team) = &boot.team {
            if let Some(name) = &team.name {
                self.name = name.clone();
            }
            if let Some(url) = &team.url {
                self.url = url.clone();
            }
        }
        self.starred_order = boot.starred.clone();
        self.priority_scores = boot.channels_priority.clone();
        self.hide_read_channels_unless_starred =
            boot.prefs.sidebar_behavior.as_deref() == Some("hide_read_channels_unless_starred");
        self.priority_sidebar_section = boot.prefs.priority_sidebar_section;

        for channel in boot.all_channels() {
            if channel.is_im || channel.is_mpim {
                append_unique(&mut self.dm_order, channel.id.clone());
            }
            self.apply_channel_read_state(&channel);
            self.channels.insert(channel.id.clone(), channel);
        }
        for id in &self.starred_order {
            if let Some(channel) = self.channels.get_mut(id) {
                channel.is_starred = true;
            }
        }
        for user in boot.users {
            self.users.insert(user.id.clone(), user);
        }
        if let Some(user) = self_user {
            self.users.insert(user.id.clone(), user);
        }
    }

    pub fn apply_counts(&mut self, counts: crate::slack::models::CountsPage) {
        if let Some(unread_count) = counts.activity_unread_count() {
            self.activity_unread_count = Some(unread_count);
        }
        for channel in counts.all_channels() {
            self.apply_channel_read_state(&channel);
            if let Some(existing) = self.channels.get_mut(&channel.id) {
                if channel.is_starred {
                    existing.is_starred = true;
                }
                existing.unread_count = channel.unread_count.or(existing.unread_count);
                existing.unread_count_display = channel
                    .unread_count_display
                    .or(existing.unread_count_display);
                existing.mention_count = channel.mention_count.or(existing.mention_count);
                existing.has_unreads |= channel.has_unreads;
                existing.last_read = channel.last_read.or_else(|| existing.last_read.take());
            } else {
                self.channels.insert(channel.id.clone(), channel);
            }
        }
    }

    pub fn apply_sidebar_dms(&mut self, dms: crate::slack::models::SidebarDmsPage) {
        for channel in dms.all_channels() {
            append_unique(&mut self.dm_order, channel.id.clone());
            self.apply_channel_read_state(&channel);
            if let Some(existing) = self.channels.get_mut(&channel.id) {
                merge_channel_metadata(existing, channel);
            } else {
                self.channels.insert(channel.id.clone(), channel);
            }
        }
    }

    pub fn apply_channels_info(&mut self, channels: Vec<Channel>) {
        for channel in channels {
            self.apply_channel_read_state(&channel);
            if let Some(existing) = self.channels.get_mut(&channel.id) {
                merge_channel_metadata(existing, channel);
            } else {
                self.channels.insert(channel.id.clone(), channel);
            }
        }
        for id in &self.starred_order {
            if let Some(channel) = self.channels.get_mut(id) {
                channel.is_starred = true;
            }
        }
    }

    pub fn display_name(&self, user_id: &str) -> String {
        display_name(self.users.get(user_id), user_id)
    }

    pub fn avatar_url(&self, user_id: &str) -> Option<String> {
        user_avatar_url(self.users.get(user_id)?).map(str::to_owned)
    }

    pub fn message_author_name(&self, msg: &SlackMessage) -> String {
        message_author_name(self, msg)
    }

    pub fn message_avatar(&self, msg: &SlackMessage) -> (Option<String>, Option<String>) {
        message_avatar(self, msg)
    }

    pub fn custom_emoji_url(&self, name: &str) -> Option<&str> {
        custom_emoji_url(&self.custom_emoji, name)
    }

    pub fn apply_emojis(&mut self, emojis: Vec<Emoji>) {
        for emoji in emojis {
            self.custom_emoji.insert(emoji.name.clone(), emoji);
        }
    }

    pub fn set_typing(&mut self, channel: &str, user: UserId, now: Instant) {
        if user == self.self_user_id {
            return;
        }
        let entry = self.typing.entry(channel.to_owned()).or_default();
        entry.retain(|(u, _)| u != &user);
        entry.push((user, now));
    }

    pub fn clear_typing_user(&mut self, channel: &str, user: &str) {
        if let Some(entry) = self.typing.get_mut(channel) {
            entry.retain(|(u, _)| u != user);
        }
        self.typing.retain(|_, v| !v.is_empty());
    }

    pub fn prune_typing(&mut self, now: Instant, ttl: Duration) -> bool {
        let mut changed = false;
        for entry in self.typing.values_mut() {
            let before = entry.len();
            entry.retain(|(_, seen)| now.duration_since(*seen) < ttl);
            changed |= entry.len() != before;
        }
        self.typing.retain(|_, v| !v.is_empty());
        changed
    }

    pub fn typing_names(&self, channel: &str) -> Vec<String> {
        self.typing
            .get(channel)
            .into_iter()
            .flatten()
            .filter(|(u, _)| u != &self.self_user_id)
            .map(|(u, _)| self.display_name(u))
            .collect()
    }

    pub fn set_presence(&mut self, user: UserId, presence: Presence) {
        if user.is_empty() {
            return;
        }
        self.presence.insert(user, presence);
    }

    pub fn presence_for_channel(&self, channel: &Channel) -> Presence {
        if !(channel.is_im || channel.is_mpim) {
            return Presence::Unknown;
        }
        dm_user_id(channel)
            .and_then(|user| self.presence.get(user).copied())
            .unwrap_or(Presence::Unknown)
    }

    pub fn touch_recent(&mut self, id: &ChannelId) {
        self.recent_channels.retain(|existing| existing != id);
        self.recent_channels.insert(0, id.clone());
        self.recent_channels.truncate(RECENT_CHANNELS_MAX);
    }

    pub fn record_visit(&mut self, id: &ChannelId, now: i64) {
        let entry = self.frecency.entry(id.clone()).or_insert(FrecencyEntry {
            score: 0.0,
            last_visit: now,
        });
        entry.score = decayed(entry.score, entry.last_visit, now) + 1.0;
        entry.last_visit = now;
    }

    pub fn frecency_score(&self, id: &str, now: i64) -> f64 {
        self.frecency
            .get(id)
            .map(|e| decayed(e.score, e.last_visit, now))
            .unwrap_or(0.0)
    }

    pub fn max_frecency_score(&self, now: i64) -> f64 {
        self.frecency
            .values()
            .map(|e| decayed(e.score, e.last_visit, now))
            .fold(0.0, f64::max)
    }

    pub fn is_starred_channel(&self, channel: &Channel) -> bool {
        channel.is_starred
            || self.starred_order.iter().any(|id| id == &channel.id)
            || channel
                .extra
                .get("is_starred")
                .or_else(|| channel.extra.get("starred"))
                .or_else(|| channel.extra.get("is_favorite"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    }

    pub fn priority_score(&self, channel_id: &str) -> Option<f64> {
        self.priority_scores.get(channel_id).copied()
    }

    pub fn unread_total(&self, channel: &Channel) -> u32 {
        self.messages
            .get(&channel.id)
            .map(|cm| cm.mention_count.max(cm.unread_count))
            .unwrap_or_else(|| {
                let count = channel
                    .mention_count
                    .or(channel.unread_count)
                    .or(channel.unread_count_display)
                    .unwrap_or(0);
                if count == 0 && channel.has_unreads {
                    1
                } else {
                    count
                }
            })
    }

    pub fn should_show_unstarred_read_channels(&self) -> bool {
        !self.hide_read_channels_unless_starred
    }

    fn apply_channel_read_state(&mut self, channel: &Channel) {
        let cm = self.messages.entry(channel.id.clone()).or_default();
        if let Some(last_read) = &channel.last_read {
            cm.last_read = Some(last_read.clone());
        }
        if let Some(unread) = channel.unread_count.or(channel.unread_count_display) {
            cm.unread_count = unread;
        }
        if let Some(mentions) = channel.mention_count {
            cm.mention_count = mentions;
        }
        if channel.has_unreads && cm.unread_count == 0 && cm.mention_count == 0 {
            cm.unread_count = 1;
        }
    }
}

fn append_unique(ids: &mut Vec<ChannelId>, id: ChannelId) {
    if !ids.iter().any(|existing| existing == &id) {
        ids.push(id);
    }
}

fn merge_channel_metadata(existing: &mut Channel, update: Channel) {
    if update
        .name
        .as_ref()
        .is_some_and(|name| !name.trim().is_empty())
    {
        existing.name = update.name;
    }
    existing.is_channel |= update.is_channel;
    existing.is_group |= update.is_group;
    existing.is_im |= update.is_im;
    existing.is_mpim |= update.is_mpim;
    existing.is_private |= update.is_private;
    existing.is_archived |= update.is_archived;
    existing.is_starred |= update.is_starred;
    existing.has_unreads |= update.has_unreads;
    existing.updated = update.updated.or(existing.updated);
    existing.user = update.user.or_else(|| existing.user.take());
    existing.unread_count = update.unread_count.or(existing.unread_count);
    existing.unread_count_display = update
        .unread_count_display
        .or(existing.unread_count_display);
    existing.mention_count = update.mention_count.or(existing.mention_count);
    existing.last_read = update.last_read.or_else(|| existing.last_read.take());
    existing.extra.extend(update.extra);
}

pub fn dm_user_id(channel: &Channel) -> Option<&str> {
    if let Some(user) = channel.user.as_deref() {
        return Some(user);
    }
    for key in ["user", "user_id"] {
        if let Some(user) = channel.extra.get(key).and_then(serde_json::Value::as_str) {
            return Some(user);
        }
    }
    None
}

pub fn display_name(user: Option<&User>, user_id: &str) -> String {
    let Some(user) = user else {
        return user_id.to_owned();
    };
    if let Some(profile) = &user.profile {
        if let Some(dn) = non_empty(profile.display_name.as_deref()) {
            return dn.to_owned();
        }
        if let Some(rn) = non_empty(profile.real_name.as_deref()) {
            return rn.to_owned();
        }
    }
    if let Some(rn) = non_empty(user.real_name.as_deref()) {
        return rn.to_owned();
    }
    if let Some(name) = non_empty(user.name.as_deref()) {
        return name.to_owned();
    }
    user_id.to_owned()
}

pub fn user_avatar_url(user: &User) -> Option<&str> {
    let profile = user.profile.as_ref()?;
    non_empty(profile.image_48.as_deref())
        .or_else(|| non_empty(profile.image_72.as_deref()))
        .or_else(|| non_empty(profile.image_32.as_deref()))
        .or_else(|| non_empty(profile.image_24.as_deref()))
        .or_else(|| non_empty(profile.image_192.as_deref()))
        .or_else(|| non_empty(profile.image_512.as_deref()))
        .or_else(|| non_empty(profile.image_original.as_deref()))
}

pub fn message_author_name(ws: &Workspace, msg: &SlackMessage) -> String {
    if let Some(user) = msg
        .user
        .as_deref()
        .filter(|_| msg.bot_profile.is_none() && msg.bot_id.is_none())
    {
        return ws.display_name(user);
    }
    if let Some(name) = non_empty(msg.username.as_deref()) {
        return name.to_owned();
    }
    if let Some(name) = msg
        .bot_profile
        .as_ref()
        .and_then(|profile| non_empty(profile.name.as_deref()))
    {
        return name.to_owned();
    }
    if let Some(user) = msg.user.as_deref() {
        return ws.display_name(user);
    }
    msg.bot_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

pub fn message_avatar(ws: &Workspace, msg: &SlackMessage) -> (Option<String>, Option<String>) {
    if let Some(user) = msg
        .user
        .as_deref()
        .filter(|_| msg.bot_profile.is_none() && msg.bot_id.is_none())
    {
        return (Some(user.to_owned()), ws.avatar_url(user));
    }
    if let Some((key, url)) = message_bot_avatar(msg) {
        return (Some(key), Some(url));
    }
    if let Some(user) = msg.user.as_deref() {
        return (Some(user.to_owned()), ws.avatar_url(user));
    }
    (None, None)
}

pub fn message_bot_avatar(msg: &SlackMessage) -> Option<(String, String)> {
    let source = msg
        .bot_id
        .as_deref()
        .or_else(|| {
            msg.bot_profile
                .as_ref()
                .and_then(|profile| profile.id.as_deref())
        })
        .or_else(|| {
            msg.bot_profile
                .as_ref()
                .and_then(|profile| profile.user_id.as_deref())
        })?;
    let url = msg
        .bot_profile
        .as_ref()
        .and_then(|profile| profile.icons.as_ref())
        .and_then(message_icon_url)
        .or_else(|| msg.icons.as_ref().and_then(message_icon_url))?
        .to_owned();
    let key = format!("bot-icon:{source}:{url}");
    Some((key, url))
}

pub fn message_icon_url(icons: &crate::slack::models::MessageIcons) -> Option<&str> {
    non_empty(icons.image_48.as_deref())
        .or_else(|| non_empty(icons.image_72.as_deref()))
        .or_else(|| non_empty(icons.image_64.as_deref()))
        .or_else(|| non_empty(icons.image_36.as_deref()))
        .or_else(|| non_empty(icons.image_192.as_deref()))
        .or_else(|| non_empty(icons.image_512.as_deref()))
        .or_else(|| non_empty(icons.image_original.as_deref()))
        .or_else(|| non_empty(icons.icon_url.as_deref()))
}

pub fn channel_label(channel: &Channel) -> String {
    if let Some(name) = non_empty(channel.name.as_deref()) {
        if channel.is_im {
            return name.to_owned();
        }
        return format!("#{name}");
    }
    if channel.is_im {
        return "direct message".to_owned();
    }
    channel.id.clone()
}

pub fn channel_display_name(ws: &Workspace, c: &Channel) -> String {
    if c.is_im || c.is_mpim {
        dm_label(ws, c)
    } else {
        channel_name(c)
    }
}

pub fn channel_name(c: &Channel) -> String {
    c.name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(c.id.as_str())
        .to_owned()
}

pub fn dm_label(ws: &Workspace, c: &Channel) -> String {
    if c.is_im {
        if let Some(user) = dm_user_id(c) {
            return ws.display_name(user);
        }
    }
    if c.is_mpim {
        if let Some(name) = c.name.as_deref().and_then(mpdm_name_label) {
            return name;
        }
    }
    channel_label(c).trim_start_matches('#').to_owned()
}

/// Whether this conversation is treated as VIP (priority sidebar / channel metadata).
pub fn is_vip_channel(ws: &Workspace, c: &Channel) -> bool {
    if ws.priority_sidebar_section && ws.priority_score(&c.id).is_some() {
        return true;
    }
    c.extra.iter().any(|(key, value)| {
        let key = key.to_ascii_lowercase();
        key.contains("vip")
            || (key.contains("priority") && value.as_bool().unwrap_or(false))
            || value_names_vip(value)
    })
}

fn value_names_vip(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => value.to_ascii_lowercase().contains("vip"),
        serde_json::Value::Array(values) => values.iter().any(value_names_vip),
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.to_ascii_lowercase().contains("vip") || value_names_vip(value)),
        _ => false,
    }
}

pub fn mpdm_name_label(name: &str) -> Option<String> {
    let rest = name.strip_prefix("mpdm-")?;
    let rest = rest
        .rsplit_once('-')
        .and_then(|(prefix, suffix)| suffix.parse::<u32>().ok().map(|_| prefix))
        .unwrap_or(rest);
    let names: Vec<_> = rest
        .split("--")
        .map(|name| name.replace('.', " "))
        .filter(|name| !name.trim().is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

pub fn message_text(msg: &SlackMessage) -> String {
    if let Some(text) = non_empty(msg.text.as_deref()) {
        return text.to_owned();
    }
    if !msg.files.is_empty() {
        return String::new();
    }
    if let Some(subtype) = non_empty(msg.subtype.as_deref()) {
        return format!("[{subtype}]");
    }
    if !msg.blocks.is_empty() {
        return "[rich message]".to_owned();
    }
    "[no text]".to_owned()
}

pub fn visible_message(msg: SlackMessage) -> SlackMessage {
    if msg.subtype.as_deref() != Some("message_replied") {
        return msg;
    }

    let Some(nested) = msg.message.clone() else {
        return msg;
    };
    let mut nested = *nested;
    nested.team = nested.team.or(msg.team);
    nested.channel = nested.channel.or(msg.channel);
    nested.reply_count = nested.reply_count.or(msg.reply_count);
    nested.reply_users_count = nested.reply_users_count.or(msg.reply_users_count);
    nested.latest_reply = nested.latest_reply.or(msg.latest_reply);
    if nested.reply_users.is_empty() {
        nested.reply_users = msg.reply_users;
    }
    if nested.reactions.is_empty() {
        nested.reactions = msg.reactions;
    }
    if nested.files.is_empty() {
        nested.files = msg.files;
    }
    if nested.attachments.is_empty() {
        nested.attachments = msg.attachments;
    }
    nested
}

pub fn is_channel_timeline_visible(msg: &SlackMessage) -> bool {
    if msg.subtype.as_deref() == Some("message_replied") {
        return false;
    }
    match (msg.thread_ts.as_deref(), msg.ts.as_deref()) {
        (Some(root), Some(ts)) if root != ts => msg.subtype.as_deref() == Some("thread_broadcast"),
        _ => true,
    }
}

pub fn file_title(file: &File) -> String {
    non_empty(file.title.as_deref())
        .or_else(|| non_empty(file.name.as_deref()))
        .or_else(|| non_empty(file.id.as_deref()))
        .unwrap_or("file")
        .to_owned()
}

pub fn file_summary(file: &File) -> String {
    let mut parts = Vec::new();
    if let Some(kind) = non_empty(file.pretty_type.as_deref())
        .or_else(|| non_empty(file.filetype.as_deref()))
        .or_else(|| non_empty(file.mimetype.as_deref()))
    {
        parts.push(kind.to_owned());
    }
    if let Some(size) = file.size {
        parts.push(format_file_size(size));
    }
    if parts.is_empty() {
        "attachment".to_owned()
    } else {
        parts.join(" - ")
    }
}

pub fn file_download_name(file: &File) -> String {
    sanitize_file_name(
        non_empty(file.name.as_deref())
            .or_else(|| non_empty(file.title.as_deref()))
            .or_else(|| non_empty(file.id.as_deref()))
            .unwrap_or("download"),
    )
}

pub fn file_preview_key(file: &File) -> Option<String> {
    non_empty(file.id.as_deref())
        .or_else(|| file_preview_url(file))
        .map(str::to_owned)
}

pub fn attachment_preview_url(att: &crate::slack::models::Attachment) -> Option<&str> {
    non_empty(att.thumb_url.as_deref()).or_else(|| non_empty(att.image_url.as_deref()))
}

pub fn is_browser_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

pub fn file_preview_url(file: &File) -> Option<&str> {
    non_empty(file.thumb_360.as_deref())
        .or_else(|| non_empty(file.thumb_160.as_deref()))
        .or_else(|| non_empty(file.thumb_80.as_deref()))
        .or_else(|| non_empty(file.thumb_64.as_deref()))
}

fn sanitize_file_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| {
            if c.is_ascii_control()
                || matches!(c, '/' | '\\' | ':' | '*')
                || matches!(c, '?' | '"' | '<' | '>' | '|')
            {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim_matches(|c| c == ' ' || c == '.')
        .to_owned();
    if sanitized.is_empty() {
        "download".to_owned()
    } else {
        sanitized
    }
}

pub fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes < 1024 {
        format!("{bytes} B")
    } else if (bytes as f64) < MB {
        format!("{:.1} KB", bytes as f64 / KB)
    } else if (bytes as f64) < GB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{:.1} GB", bytes as f64 / GB)
    }
}

pub fn reaction_summary(reaction: &crate::slack::models::Reaction) -> String {
    format!("{} {}", emoji_glyph(&reaction.name), reaction.count.max(1))
}

/// Map a Slack emoji name to its unicode glyph, falling back to `:name:` for
/// custom/unknown emoji. Slack appends skin-tone modifiers like
/// `thumbsup::skin-tone-3`; the base name resolves the glyph.
pub fn emoji_glyph(name: &str) -> String {
    let base = name.split("::").next().unwrap_or(name);
    emojis::get_by_shortcode(base)
        .map(|e| e.as_str().to_owned())
        .unwrap_or_else(|| format!(":{name}:"))
}

pub fn is_standard_emoji(name: &str) -> bool {
    let base = name.split("::").next().unwrap_or(name);
    emojis::get_by_shortcode(base).is_some()
}

pub fn emoji_text_to_display(text: &str) -> String {
    emoji_text_tokens(text)
        .into_iter()
        .map(|token| match token {
            EmojiTextToken::Text(text) => text,
            EmojiTextToken::Emoji(name) => emoji_glyph(&name),
        })
        .collect()
}

pub fn emoji_preview_key(team: &str, name: &str) -> String {
    format!("{team}:{name}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmojiTextToken {
    Text(String),
    Emoji(String),
}

pub fn emoji_text_tokens(text: &str) -> Vec<EmojiTextToken> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        let (before, after_start) = rest.split_at(start);
        if !before.is_empty() {
            out.push(EmojiTextToken::Text(before.to_owned()));
        }

        let after_start = &after_start[1..];
        let Some(end) = after_start.find(':') else {
            out.push(EmojiTextToken::Text(":".to_owned()));
            rest = after_start;
            continue;
        };
        let name = &after_start[..end];
        if is_emoji_name(name) {
            out.push(EmojiTextToken::Emoji(name.to_owned()));
            rest = &after_start[end + 1..];
        } else {
            out.push(EmojiTextToken::Text(":".to_owned()));
            rest = after_start;
        }
    }
    if !rest.is_empty() {
        out.push(EmojiTextToken::Text(rest.to_owned()));
    }
    merge_text_tokens(out)
}

pub fn emoji_names_in_text(text: &str) -> Vec<String> {
    emoji_text_tokens(text)
        .into_iter()
        .filter_map(|token| match token {
            EmojiTextToken::Emoji(name) => Some(name),
            EmojiTextToken::Text(_) => None,
        })
        .collect()
}

fn custom_emoji_url<'a>(emojis: &'a HashMap<String, Emoji>, name: &str) -> Option<&'a str> {
    let mut current = name;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(current) {
            return None;
        }
        let emoji = emojis.get(current)?;
        if let Some(alias) = emoji.value.strip_prefix("alias:") {
            current = alias;
            continue;
        }
        return is_browser_url(&emoji.value).then_some(emoji.value.as_str());
    }
}

fn is_emoji_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
}

fn merge_text_tokens(tokens: Vec<EmojiTextToken>) -> Vec<EmojiTextToken> {
    let mut merged = Vec::new();
    for token in tokens {
        match (merged.last_mut(), token) {
            (Some(EmojiTextToken::Text(existing)), EmojiTextToken::Text(next)) => {
                existing.push_str(&next);
            }
            (_, token) => merged.push(token),
        }
    }
    merged
}

pub fn reaction_has_user(reaction: &Reaction, user: &str) -> bool {
    !user.is_empty() && reaction.users.iter().any(|u| u == user)
}

fn apply_message_reaction(msg: &mut SlackMessage, user: &str, name: &str, added: bool) -> bool {
    if name.is_empty() {
        return false;
    }

    if added {
        if let Some(reaction) = msg.reactions.iter_mut().find(|r| r.name == name) {
            if reaction_has_user(reaction, user) {
                return false;
            }
            if !user.is_empty() {
                reaction.users.push(user.to_owned());
            }
            reaction.count = reaction.count.saturating_add(1).max(1);
            return true;
        }
        msg.reactions.push(Reaction {
            name: name.to_owned(),
            users: if user.is_empty() {
                Vec::new()
            } else {
                vec![user.to_owned()]
            },
            count: 1,
            ..Default::default()
        });
        return true;
    }

    let Some(i) = msg.reactions.iter().position(|r| r.name == name) else {
        return false;
    };
    let reaction = &mut msg.reactions[i];
    let before_users = reaction.users.len();
    reaction.users.retain(|u| u != user);
    let removed_known_user = before_users != reaction.users.len();
    if removed_known_user || reaction.users.is_empty() {
        reaction.count = reaction.count.saturating_sub(1);
    }
    if reaction.count == 0 {
        msg.reactions.remove(i);
    }
    true
}

fn merge_message(existing: &mut SlackMessage, update: SlackMessage) {
    existing.user = update.user.or_else(|| existing.user.take());
    existing.bot_id = update.bot_id.or_else(|| existing.bot_id.take());
    existing.username = update.username.or_else(|| existing.username.take());
    existing.bot_profile = update.bot_profile.or_else(|| existing.bot_profile.take());
    existing.icons = update.icons.or_else(|| existing.icons.take());
    existing.kind = update.kind.or_else(|| existing.kind.take());
    existing.subtype = update.subtype.or_else(|| existing.subtype.take());
    existing.client_msg_id = update
        .client_msg_id
        .or_else(|| existing.client_msg_id.take());
    existing.text = update.text.or_else(|| existing.text.take());
    existing.team = update.team.or_else(|| existing.team.take());
    existing.channel = update.channel.or_else(|| existing.channel.take());
    existing.thread_ts = update.thread_ts.or_else(|| existing.thread_ts.take());
    existing.parent_user_id = update
        .parent_user_id
        .or_else(|| existing.parent_user_id.take());
    existing.reply_count = update.reply_count.or(existing.reply_count);
    existing.reply_users_count = update.reply_users_count.or(existing.reply_users_count);
    existing.latest_reply = update.latest_reply.or_else(|| existing.latest_reply.take());
    if !update.reply_users.is_empty() {
        existing.reply_users = update.reply_users;
    }
    if !update.reactions.is_empty() {
        existing.reactions = update.reactions;
    }
    if !update.blocks.is_empty() {
        existing.blocks = update.blocks;
    }
    if !update.files.is_empty() {
        existing.files = update.files;
    }
    if !update.attachments.is_empty() {
        existing.attachments = update.attachments;
    }
    existing.edited = update.edited.or_else(|| existing.edited.take());
    existing.message = update.message.or_else(|| existing.message.take());
    existing.extra.extend(update.extra);
}

pub fn ts_key(ts: &str) -> (u64, u64) {
    let mut parts = ts.splitn(2, '.');
    let secs = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let seq = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (secs, seq)
}

pub fn cmp_ts(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => ts_key(a).cmp(&ts_key(b)),
        (None, None) => std::cmp::Ordering::Equal,
        (None, _) => std::cmp::Ordering::Less,
        (_, None) => std::cmp::Ordering::Greater,
    }
}

pub fn format_ts_hm(ts: &str) -> String {
    use chrono::{Local, TimeZone};
    let (secs, _) = ts_key(ts);
    match Local.timestamp_opt(secs as i64, 0).single() {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => secs.to_string(),
    }
}

pub fn date_key_for_ts(ts: &str) -> Option<String> {
    use chrono::{Datelike, Local, TimeZone};
    let (secs, _) = ts_key(ts);
    let date = Local.timestamp_opt(secs as i64, 0).single()?.date_naive();
    Some(format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month(),
        date.day()
    ))
}

pub fn format_ts_date_label(ts: &str) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let (secs, _) = ts_key(ts);
    let Some(date_time) = Local.timestamp_opt(secs as i64, 0).single() else {
        return ts.to_owned();
    };
    let date = date_time.date_naive();
    let today = Local::now().date_naive();
    if date == today {
        return "Today".to_owned();
    }
    if today.signed_duration_since(date).num_days() == 1 {
        return "Yesterday".to_owned();
    }
    format!(
        "{}, {} {}",
        date.format("%A"),
        date.format("%B"),
        ordinal_day(date.day())
    )
}

fn ordinal_day(day: u32) -> String {
    let suffix = match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    format!("{day}{suffix}")
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

pub fn scroll_ratio_for_ts(messages: &[SlackMessage], ts: &str) -> Option<f32> {
    let index = messages.iter().position(|m| m.ts.as_deref() == Some(ts))?;
    let last = messages.len() - 1;
    if last == 0 {
        Some(0.0)
    } else {
        Some(index as f32 / last as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::models::{
        BootData, BootSelf, BotProfile, Emoji, MessageIcons, Reaction, UserProfile,
    };

    fn msg(ts: &str, text: &str) -> SlackMessage {
        SlackMessage {
            ts: Some(ts.to_owned()),
            text: Some(text.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn ts_key_orders_numerically_not_lexically() {
        assert!(ts_key("1783372360.000009") < ts_key("1783372360.000010"));
        assert!(ts_key("999.1") < ts_key("1000.0"));
    }

    #[test]
    fn ordinal_day_suffixes_match_slack_date_labels() {
        assert_eq!(ordinal_day(1), "1st");
        assert_eq!(ordinal_day(2), "2nd");
        assert_eq!(ordinal_day(3), "3rd");
        assert_eq!(ordinal_day(4), "4th");
        assert_eq!(ordinal_day(11), "11th");
        assert_eq!(ordinal_day(22), "22nd");
    }

    #[test]
    fn upsert_keeps_ascending_order_and_dedupes() {
        let mut cm = ChannelMessages::default();
        assert!(cm.upsert(msg("1783372360.741769", "b")));
        assert!(cm.upsert(msg("1783372350.000000", "a")));
        assert!(cm.upsert(msg("1783372370.000000", "c")));
        assert!(!cm.upsert(msg("1783372360.741769", "b-edited")));

        let texts: Vec<_> = cm
            .messages
            .iter()
            .map(|m| m.text.clone().unwrap())
            .collect();
        assert_eq!(texts, vec!["a", "b-edited", "c"]);
    }

    #[test]
    fn remove_by_ts() {
        let mut cm = ChannelMessages::default();
        cm.upsert(msg("100.0", "x"));
        cm.upsert(msg("200.0", "y"));
        assert!(cm.remove("100.0"));
        assert!(!cm.remove("100.0"));
        assert_eq!(cm.messages.len(), 1);
        assert_eq!(cm.messages[0].text.as_deref(), Some("y"));
    }

    #[test]
    fn confirm_reconciles_pending_by_client_msg_id() {
        let mut cm = ChannelMessages::default();
        let mut pending = msg("9999999999.000000", "hi");
        pending.client_msg_id = Some("cid-1".to_owned());
        cm.upsert(pending);
        cm.pending.push("9999999999.000000".to_owned());
        assert!(cm.is_pending("9999999999.000000"));

        let mut confirmed = msg("1783372400.111111", "hi");
        confirmed.client_msg_id = Some("cid-1".to_owned());
        assert!(cm.confirm("cid-1", confirmed));

        assert_eq!(cm.messages.len(), 1);
        assert_eq!(cm.messages[0].ts.as_deref(), Some("1783372400.111111"));
        assert!(!cm.is_pending("1783372400.111111"));
    }

    #[test]
    fn matching_pending_confirm_skips_ambiguous_duplicates() {
        let mut cm = ChannelMessages::default();
        cm.upsert(msg("9999999999.000001", "hi"));
        cm.upsert(msg("9999999999.000002", "hi"));
        cm.pending.push("9999999999.000001".to_owned());
        cm.pending.push("9999999999.000002".to_owned());

        assert!(!cm.confirm_matching_pending(None, Some("hi"), msg("1783372400.111111", "hi")));
        assert_eq!(cm.messages.len(), 2);
        assert!(cm.is_pending("9999999999.000001"));
        assert!(cm.is_pending("9999999999.000002"));
    }

    #[test]
    fn display_name_fallback_chain() {
        let u = User {
            id: "U1".into(),
            name: Some("uname".into()),
            real_name: Some("Real Name".into()),
            profile: Some(UserProfile {
                display_name: Some("Display".into()),
                real_name: Some("Profile Real".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(display_name(Some(&u), "U1"), "Display");

        let u2 = User {
            id: "U2".into(),
            profile: Some(UserProfile {
                display_name: Some("  ".into()),
                real_name: Some("Profile Real".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(display_name(Some(&u2), "U2"), "Profile Real");

        let u3 = User {
            id: "U3".into(),
            name: Some("uname".into()),
            ..Default::default()
        };
        assert_eq!(display_name(Some(&u3), "U3"), "uname");

        assert_eq!(display_name(None, "U4"), "U4");
    }

    #[test]
    fn message_author_prefers_bot_username_and_profile_over_raw_ids() {
        let ws = Workspace::from_session(&crate::config::WorkspaceSession {
            team_id: "T1".into(),
            enterprise_id: None,
            user_id: "U_SELF".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            token: "xoxc-test".into(),
        });
        let msg = SlackMessage {
            user: Some("U_APP".into()),
            bot_id: Some("B_FAKE".into()),
            username: Some("mattsob".into()),
            bot_profile: Some(BotProfile {
                name: Some("app fallback".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(message_author_name(&ws, &msg), "mattsob");

        let profile_only = SlackMessage {
            bot_id: Some("B_FAKE".into()),
            bot_profile: Some(BotProfile {
                name: Some("slimebot".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(message_author_name(&ws, &profile_only), "slimebot");
    }

    #[test]
    fn message_bot_avatar_prefers_profile_icon() {
        let msg = SlackMessage {
            bot_id: Some("B_FAKE".into()),
            bot_profile: Some(BotProfile {
                icons: Some(MessageIcons {
                    image_48: Some("https://example.test/bot-48.png".into()),
                    image_72: Some("https://example.test/bot-72.png".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            message_bot_avatar(&msg),
            Some((
                "bot-icon:B_FAKE:https://example.test/bot-48.png".into(),
                "https://example.test/bot-48.png".into()
            ))
        );
    }

    #[test]
    fn message_bot_avatar_key_includes_per_message_icon_url() {
        let mut first = SlackMessage {
            bot_id: Some("B_SAME".into()),
            icons: Some(MessageIcons {
                image_48: Some("https://example.test/first.png".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut second = first.clone();
        second.icons = Some(MessageIcons {
            image_48: Some("https://example.test/second.png".into()),
            ..Default::default()
        });

        let first_avatar = message_bot_avatar(&first).expect("first avatar");
        let second_avatar = message_bot_avatar(&second).expect("second avatar");

        assert_ne!(first_avatar.0, second_avatar.0);
        assert_eq!(first_avatar.1, "https://example.test/first.png");
        assert_eq!(second_avatar.1, "https://example.test/second.png");

        first.icons = second.icons.clone();
        assert_eq!(message_bot_avatar(&first), Some(second_avatar));
    }

    #[test]
    fn user_avatar_url_prefers_profile_image_48() {
        let user = User {
            id: "U1".into(),
            profile: Some(UserProfile {
                image_32: Some("https://example.test/32.png".into()),
                image_48: Some("https://example.test/48.png".into()),
                image_72: Some("https://example.test/72.png".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(user_avatar_url(&user), Some("https://example.test/48.png"));

        let fallback = User {
            id: "U2".into(),
            profile: Some(UserProfile {
                image_48: Some(" ".into()),
                image_32: Some("https://example.test/32.png".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            user_avatar_url(&fallback),
            Some("https://example.test/32.png")
        );

        let original_only = User {
            id: "U3".into(),
            profile: Some(UserProfile {
                image_original: Some("https://example.test/original.png".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            user_avatar_url(&original_only),
            Some("https://example.test/original.png")
        );

        let hash_only = User {
            id: "U4".into(),
            profile: Some(UserProfile {
                avatar_hash: Some("31dc9a4e9298".into()),
                team: Some("E1".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(user_avatar_url(&hash_only), None);
    }

    #[test]
    fn boot_self_user_provides_self_avatar_url() {
        let mut ws = Workspace {
            team_id: "T1".into(),
            name: "test".into(),
            url: "https://t".into(),
            self_user_id: "U_SESSION".into(),
            activity_unread_count: None,
            channels: BTreeMap::new(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        };

        ws.apply_boot(BootData {
            self_user: BootSelf {
                id: "U_SELF".into(),
                name: Some("rowan".into()),
                profile: Some(UserProfile {
                    image_48: Some("https://example.test/self.png".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        });

        assert_eq!(ws.self_user_id, "U_SELF");
        assert_eq!(
            ws.avatar_url("U_SELF"),
            Some("https://example.test/self.png".into())
        );
    }

    #[test]
    fn is_vip_channel_from_priority_and_metadata() {
        let mut ws = Workspace {
            team_id: "T".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            self_user_id: "U_SELF".into(),
            activity_unread_count: None,
            channels: BTreeMap::new(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: true,
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        };
        ws.priority_scores.insert("D_VIP".into(), 0.8);

        let vip_dm = Channel {
            id: "D_VIP".into(),
            is_im: true,
            user: Some("U_ALFIE".into()),
            ..Default::default()
        };
        assert!(is_vip_channel(&ws, &vip_dm));

        let plain_dm = Channel {
            id: "D_PLAIN".into(),
            is_im: true,
            user: Some("U_OTHER".into()),
            ..Default::default()
        };
        assert!(!is_vip_channel(&ws, &plain_dm));

        let mut named_vip = Channel {
            id: "C_NAMED".into(),
            is_channel: true,
            ..Default::default()
        };
        named_vip.extra.insert(
            "sidebar_section_name".into(),
            serde_json::json!("VIP unreads"),
        );
        assert!(is_vip_channel(&ws, &named_vip));
    }

    #[test]
    fn channel_label_variants() {
        let public = Channel {
            id: "C1".into(),
            name: Some("general".into()),
            is_channel: true,
            ..Default::default()
        };
        assert_eq!(channel_label(&public), "#general");

        let dm = Channel {
            id: "D1".into(),
            name: Some("alice".into()),
            is_im: true,
            ..Default::default()
        };
        assert_eq!(channel_label(&dm), "alice");

        let unnamed = Channel {
            id: "C9".into(),
            ..Default::default()
        };
        assert_eq!(channel_label(&unnamed), "C9");
    }

    #[test]
    fn message_text_fallbacks() {
        assert_eq!(message_text(&msg("1.0", "hello")), "hello");

        let file_only = SlackMessage {
            ts: Some("1.0".into()),
            files: vec![File {
                name: Some("mock.png".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(message_text(&file_only), "");

        let empty = SlackMessage {
            ts: Some("1.0".into()),
            text: Some("   ".into()),
            subtype: Some("channel_join".into()),
            ..Default::default()
        };
        assert_eq!(message_text(&empty), "[channel_join]");

        let bare = SlackMessage {
            ts: Some("1.0".into()),
            ..Default::default()
        };
        assert_eq!(message_text(&bare), "[no text]");
    }

    #[test]
    fn visible_message_unwraps_message_replied_envelope() {
        let envelope = SlackMessage {
            subtype: Some("message_replied".into()),
            channel: Some("C1".into()),
            reply_count: Some(2),
            message: Some(Box::new(SlackMessage {
                user: Some("U1".into()),
                ts: Some("1.0".into()),
                text: Some("actual".into()),
                ..Default::default()
            })),
            ..Default::default()
        };

        let visible = visible_message(envelope);
        assert_eq!(visible.user.as_deref(), Some("U1"));
        assert_eq!(visible.text.as_deref(), Some("actual"));
        assert_eq!(visible.channel.as_deref(), Some("C1"));
        assert_eq!(visible.reply_count, Some(2));
        assert_ne!(visible.subtype.as_deref(), Some("message_replied"));
    }

    #[test]
    fn channel_timeline_hides_thread_replies_but_keeps_roots_and_broadcasts() {
        let root = SlackMessage {
            ts: Some("100.0".into()),
            thread_ts: Some("100.0".into()),
            reply_count: Some(3),
            ..Default::default()
        };
        assert!(is_channel_timeline_visible(&root));

        let plain = SlackMessage {
            ts: Some("101.0".into()),
            ..Default::default()
        };
        assert!(is_channel_timeline_visible(&plain));

        let reply = SlackMessage {
            ts: Some("102.0".into()),
            thread_ts: Some("100.0".into()),
            ..Default::default()
        };
        assert!(!is_channel_timeline_visible(&reply));

        let broadcast = SlackMessage {
            ts: Some("103.0".into()),
            thread_ts: Some("100.0".into()),
            subtype: Some("thread_broadcast".into()),
            ..Default::default()
        };
        assert!(is_channel_timeline_visible(&broadcast));

        let replied_envelope = SlackMessage {
            ts: Some("104.0".into()),
            subtype: Some("message_replied".into()),
            ..Default::default()
        };
        assert!(!is_channel_timeline_visible(&replied_envelope));
    }

    #[test]
    fn file_summary_prefers_title_type_and_size() {
        let file = File {
            id: Some("F1".into()),
            name: Some("report.pdf".into()),
            title: Some("Quarterly report".into()),
            pretty_type: Some("PDF".into()),
            size: Some(1_572_864),
            ..Default::default()
        };

        assert_eq!(file_title(&file), "Quarterly report");
        assert_eq!(file_summary(&file), "PDF - 1.5 MB");
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(2048), "2.0 KB");
    }

    #[test]
    fn file_download_name_sanitizes_paths() {
        let file = File {
            name: Some("../bad/name?.png".into()),
            title: Some("ignored".into()),
            ..Default::default()
        };
        assert_eq!(file_download_name(&file), "_bad_name_.png");

        let fallback = File::default();
        assert_eq!(file_download_name(&fallback), "download");
    }

    #[test]
    fn file_preview_uses_largest_known_thumb_and_stable_key() {
        let file = File {
            id: Some("F123".into()),
            thumb_64: Some("https://files/thumb-64.png".into()),
            thumb_160: Some("https://files/thumb-160.png".into()),
            thumb_360: Some("https://files/thumb-360.png".into()),
            ..Default::default()
        };

        assert_eq!(file_preview_key(&file).as_deref(), Some("F123"));
        assert_eq!(file_preview_url(&file), Some("https://files/thumb-360.png"));

        let without_id = File {
            thumb_80: Some("https://files/thumb-80.png".into()),
            ..Default::default()
        };
        assert_eq!(
            file_preview_key(&without_id).as_deref(),
            Some("https://files/thumb-80.png")
        );
    }

    #[test]
    fn emoji_text_tokens_extracts_shortcodes_and_display_text_keeps_custom() {
        assert_eq!(
            emoji_text_tokens("ship it :wave: :party-hack:"),
            vec![
                EmojiTextToken::Text("ship it ".into()),
                EmojiTextToken::Emoji("wave".into()),
                EmojiTextToken::Text(" ".into()),
                EmojiTextToken::Emoji("party-hack".into()),
            ]
        );
        assert_eq!(
            emoji_text_to_display("ship it :wave: :party-hack:"),
            "ship it 👋 :party-hack:"
        );
    }

    #[test]
    fn custom_emoji_url_resolves_aliases() {
        let mut ws = Workspace::from_session(&crate::config::WorkspaceSession {
            team_id: "T1".into(),
            enterprise_id: None,
            user_id: "U_SELF".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            token: "xoxc-test".into(),
        });
        ws.apply_emojis(vec![
            Emoji {
                name: "party-hack".into(),
                value: "https://emoji.test/party.png".into(),
                ..Default::default()
            },
            Emoji {
                name: "party-alias".into(),
                value: "alias:party-hack".into(),
                ..Default::default()
            },
        ]);

        assert_eq!(
            ws.custom_emoji_url("party-alias"),
            Some("https://emoji.test/party.png")
        );
    }

    #[test]
    fn reaction_summary_format() {
        let r = Reaction {
            name: "thumbsup".into(),
            count: 3,
            ..Default::default()
        };
        assert_eq!(reaction_summary(&r), "👍 3");

        let custom = Reaction {
            name: "hackclub_bug".into(),
            count: 1,
            ..Default::default()
        };
        assert_eq!(reaction_summary(&custom), ":hackclub_bug: 1");
    }

    #[test]
    fn applies_reaction_add_remove_without_double_counting() {
        let mut cm = ChannelMessages::default();
        cm.upsert(msg("1.0", "hello"));

        assert!(cm.apply_reaction("1.0", "U1", "thumbsup", true));
        assert_eq!(cm.messages[0].reactions[0].count, 1);
        assert_eq!(cm.messages[0].reactions[0].users, vec!["U1"]);

        assert!(!cm.apply_reaction("1.0", "U1", "thumbsup", true));
        assert_eq!(cm.messages[0].reactions[0].count, 1);

        assert!(cm.apply_reaction("1.0", "U2", "thumbsup", true));
        assert_eq!(cm.messages[0].reactions[0].count, 2);

        assert!(cm.apply_reaction("1.0", "U1", "thumbsup", false));
        assert_eq!(cm.messages[0].reactions[0].count, 1);
        assert_eq!(cm.messages[0].reactions[0].users, vec!["U2"]);

        assert!(cm.apply_reaction("1.0", "U2", "thumbsup", false));
        assert!(cm.messages[0].reactions.is_empty());
    }

    #[test]
    fn prune_typing_drops_stale() {
        let mut ws = Workspace {
            team_id: "T1".into(),
            name: "test".into(),
            url: "https://t".into(),
            self_user_id: "USELF".into(),
            activity_unread_count: None,
            channels: BTreeMap::new(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        };
        let now = Instant::now();
        ws.set_typing("C1", "U1".into(), now - Duration::from_secs(10));
        ws.set_typing("C1", "U2".into(), now);
        assert!(ws.prune_typing(now, Duration::from_secs(4)));
        assert_eq!(ws.typing_names("C1"), vec![ws.display_name("U2")]);
    }

    #[test]
    fn is_browser_url_accepts_only_http_and_https() {
        assert!(is_browser_url("https://slack.com/archives/C1/p1"));
        assert!(is_browser_url("http://example.com"));
        assert!(!is_browser_url("file:///etc/passwd"));
        assert!(!is_browser_url("javascript:alert(1)"));
        assert!(!is_browser_url(""));
        assert!(!is_browser_url("ftp://example.com"));
    }

    #[test]
    fn scroll_ratio_for_ts_at_start_is_zero() {
        let messages = vec![msg("1.0", "a"), msg("2.0", "b"), msg("3.0", "c")];
        assert_eq!(scroll_ratio_for_ts(&messages, "1.0"), Some(0.0));
    }

    #[test]
    fn scroll_ratio_for_ts_at_end_is_one() {
        let messages = vec![msg("1.0", "a"), msg("2.0", "b"), msg("3.0", "c")];
        assert_eq!(scroll_ratio_for_ts(&messages, "3.0"), Some(1.0));
    }

    #[test]
    fn scroll_ratio_for_ts_middle_is_between() {
        let messages = vec![msg("1.0", "a"), msg("2.0", "b"), msg("3.0", "c")];
        assert_eq!(scroll_ratio_for_ts(&messages, "2.0"), Some(0.5));
    }

    #[test]
    fn scroll_ratio_for_ts_missing_is_none() {
        let messages = vec![msg("1.0", "a"), msg("2.0", "b")];
        assert_eq!(scroll_ratio_for_ts(&messages, "9.0"), None);
    }

    #[test]
    fn scroll_ratio_for_ts_single_message_is_zero() {
        let messages = vec![msg("1.0", "a")];
        assert_eq!(scroll_ratio_for_ts(&messages, "1.0"), Some(0.0));
    }

    #[test]
    fn scroll_ratio_for_ts_empty_is_none() {
        let messages: Vec<SlackMessage> = Vec::new();
        assert_eq!(scroll_ratio_for_ts(&messages, "1.0"), None);
    }
}
