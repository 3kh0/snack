use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use crate::slack::models::{
    Channel, ChannelId, File, Message as SlackMessage, MessageTs, Reaction, TeamId, User, UserId,
};
use crate::slack::realtime::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    Loading,
    Main,
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
        let temp_ts = self.messages.iter().find_map(|m| {
            let ts = m.ts.as_deref()?;
            if !self.pending.iter().any(|p| p == ts) {
                return None;
            }
            if m.user.as_deref() == user && m.text.as_deref() == text {
                Some(ts.to_owned())
            } else {
                None
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
    pub channels: BTreeMap<ChannelId, Channel>,
    pub users: HashMap<UserId, User>,
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
            channels: BTreeMap::new(),
            users: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        }
    }

    pub fn apply_boot(&mut self, boot: crate::slack::models::BootData) {
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
        for channel in boot.all_channels() {
            self.apply_channel_read_state(&channel);
            self.channels.insert(channel.id.clone(), channel);
        }
        for user in boot.users {
            self.users.insert(user.id.clone(), user);
        }
    }

    pub fn apply_counts(&mut self, counts: crate::slack::models::CountsPage) {
        for channel in counts.all_channels() {
            self.apply_channel_read_state(&channel);
            if let Some(existing) = self.channels.get_mut(&channel.id) {
                existing.unread_count = channel.unread_count.or(existing.unread_count);
                existing.unread_count_display = channel
                    .unread_count_display
                    .or(existing.unread_count_display);
                existing.last_read = channel.last_read.or_else(|| existing.last_read.take());
            } else {
                self.channels.insert(channel.id.clone(), channel);
            }
        }
    }

    pub fn display_name(&self, user_id: &str) -> String {
        display_name(self.users.get(user_id), user_id)
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

    fn apply_channel_read_state(&mut self, channel: &Channel) {
        let cm = self.messages.entry(channel.id.clone()).or_default();
        if let Some(last_read) = &channel.last_read {
            cm.last_read = Some(last_read.clone());
        }
        if let Some(unread) = channel.unread_count.or(channel.unread_count_display) {
            cm.unread_count = unread;
        }
    }
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
    format!(":{}: {}", reaction.name, reaction.count.max(1))
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
    existing.edited = update.edited.or_else(|| existing.edited.take());
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
    use crate::slack::models::{Reaction, UserProfile};

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
    fn reaction_summary_format() {
        let r = Reaction {
            name: "thumbsup".into(),
            count: 3,
            ..Default::default()
        };
        assert_eq!(reaction_summary(&r), ":thumbsup: 3");
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
            channels: BTreeMap::new(),
            users: HashMap::new(),
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
