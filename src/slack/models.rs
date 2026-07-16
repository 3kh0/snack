use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type TeamId = String;
pub type ChannelId = String;
pub type UserId = String;
pub type MessageTs = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(flatten)]
    pub body: T,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseMetadata {
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryPage {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub unchanged_messages: Vec<MessageTs>,
    #[serde(default)]
    pub latest_updates: BTreeMap<MessageTs, String>,
    #[serde(default)]
    pub pin_count: Option<u32>,
    #[serde(default)]
    pub response_metadata: Option<ResponseMetadata>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CountsPage {
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub ims: Vec<Channel>,
    #[serde(default)]
    pub groups: Vec<Channel>,
    #[serde(default)]
    pub mpims: Vec<Channel>,
    #[serde(default)]
    pub activity_v2: Option<BTreeMap<String, u32>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SidebarDmsPage {
    #[serde(default)]
    pub ims: Vec<Channel>,
    #[serde(default)]
    pub mpdms: Vec<Channel>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl SidebarDmsPage {
    pub fn all_channels(&self) -> Vec<Channel> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for channel in self.ims.iter().chain(&self.mpdms) {
            if seen.insert(channel.id.clone()) {
                out.push(channel.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientDmsPage {
    #[serde(default)]
    pub dms: Vec<DmEntry>,
    #[serde(default)]
    pub response_metadata: Option<ResponseMetadata>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DmEntry {
    pub id: ChannelId,
    #[serde(default)]
    pub latest: Option<MessageTs>,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub channel: Option<Channel>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CountsPage {
    pub fn activity_unread_count(&self) -> Option<u32> {
        self.activity_v2
            .as_ref()
            .map(|counts| counts.values().copied().fold(0, u32::saturating_add))
    }

    pub fn all_channels(&self) -> Vec<Channel> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for channel in self
            .channels
            .iter()
            .chain(&self.groups)
            .chain(&self.ims)
            .chain(&self.mpims)
        {
            if seen.insert(channel.id.clone()) {
                out.push(channel.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub user: Option<UserId>,
    #[serde(default)]
    pub bot_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub bot_profile: Option<BotProfile>,
    #[serde(default)]
    pub icons: Option<MessageIcons>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub ts: Option<MessageTs>,
    #[serde(default)]
    pub client_msg_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub team: Option<TeamId>,
    #[serde(default)]
    pub channel: Option<ChannelId>,
    #[serde(default)]
    pub thread_ts: Option<MessageTs>,
    #[serde(default)]
    pub parent_user_id: Option<UserId>,
    #[serde(default)]
    pub reply_count: Option<u32>,
    #[serde(default)]
    pub reply_users_count: Option<u32>,
    #[serde(default)]
    pub latest_reply: Option<MessageTs>,
    #[serde(default)]
    pub reply_users: Vec<UserId>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub blocks: Vec<Value>,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub edited: Option<Value>,
    #[serde(default)]
    pub message: Option<Box<Message>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BotProfile {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub user_id: Option<UserId>,
    #[serde(default)]
    pub icons: Option<MessageIcons>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageIcons {
    #[serde(default)]
    pub image_36: Option<String>,
    #[serde(default)]
    pub image_48: Option<String>,
    #[serde(default)]
    pub image_64: Option<String>,
    #[serde(default)]
    pub image_72: Option<String>,
    #[serde(default)]
    pub image_192: Option<String>,
    #[serde(default)]
    pub image_512: Option<String>,
    #[serde(default)]
    pub image_original: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub service_icon: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_link: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_link: Option<String>,
    #[serde(default)]
    pub pretext: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub footer: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub thumb_url: Option<String>,
    #[serde(default)]
    pub from_url: Option<String>,
    #[serde(default)]
    pub original_url: Option<String>,
    #[serde(default)]
    pub fields: Vec<AttachmentField>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttachmentField {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub short: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SentMessage {
    pub channel: ChannelId,
    pub ts: MessageTs,
    pub message: Message,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reaction {
    pub name: String,
    #[serde(default)]
    pub users: Vec<UserId>,
    #[serde(default)]
    pub count: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct File {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub mimetype: Option<String>,
    #[serde(default)]
    pub filetype: Option<String>,
    #[serde(default)]
    pub pretty_type: Option<String>,
    #[serde(default)]
    pub url_private: Option<String>,
    #[serde(default)]
    pub thumb_64: Option<String>,
    #[serde(default)]
    pub thumb_80: Option<String>,
    #[serde(default)]
    pub thumb_160: Option<String>,
    #[serde(default)]
    pub thumb_360: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub is_external: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub real_name: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub profile: Option<UserProfile>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserProfile {
    #[serde(default)]
    pub real_name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub image_24: Option<String>,
    #[serde(default)]
    pub image_32: Option<String>,
    #[serde(default)]
    pub image_48: Option<String>,
    #[serde(default)]
    pub image_72: Option<String>,
    #[serde(default)]
    pub image_192: Option<String>,
    #[serde(default)]
    pub image_512: Option<String>,
    #[serde(default)]
    pub image_original: Option<String>,
    #[serde(default)]
    pub avatar_hash: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub status_text: Option<String>,
    #[serde(default)]
    pub status_emoji: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_channel: bool,
    #[serde(default)]
    pub is_group: bool,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_mpim: bool,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub is_starred: bool,
    #[serde(default, deserialize_with = "deserialize_optional_u64")]
    pub updated: Option<u64>,
    #[serde(default)]
    pub user: Option<UserId>,
    #[serde(default)]
    pub unread_count: Option<u32>,
    #[serde(default)]
    pub unread_count_display: Option<u32>,
    #[serde(default)]
    pub mention_count: Option<u32>,
    #[serde(default)]
    pub has_unreads: bool,
    #[serde(default)]
    pub last_read: Option<MessageTs>,
    #[serde(default)]
    pub previous_names: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A huddle/call "room", as delivered on `sh_room_*` realtime frames and the
/// `huddle_thread` system message. Only the fields snack uses for awareness +
/// join-handoff are named; the rest are preserved in `extra`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    #[serde(default)]
    pub call_family: Option<String>,
    #[serde(default)]
    pub channels: Vec<ChannelId>,
    #[serde(default)]
    pub created_by: Option<UserId>,
    #[serde(default)]
    pub date_start: Option<i64>,
    #[serde(default)]
    pub date_end: Option<i64>,
    #[serde(default)]
    pub has_ended: bool,
    #[serde(default)]
    pub huddle_link: Option<String>,
    #[serde(default)]
    pub participants: Vec<UserId>,
    #[serde(default)]
    pub participant_history: Vec<UserId>,
    #[serde(default)]
    pub media_backend_type: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Room {
    /// The channel this huddle lives in (huddles are single-channel in practice).
    pub fn channel(&self) -> Option<&ChannelId> {
        self.channels.first()
    }

    /// A huddle is "active" while it has not ended.
    pub fn is_active(&self) -> bool {
        !self.has_ended
    }
}

fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(string)) => string
            .split_once('.')
            .map(|(seconds, _)| seconds)
            .unwrap_or(&string)
            .parse()
            .ok(),
        _ => None,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Team {
    pub id: TeamId,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub enterprise_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BootSelf {
    pub id: UserId,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub profile: Option<UserProfile>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BootData {
    #[serde(rename = "self", default)]
    pub self_user: BootSelf,
    #[serde(default)]
    pub team: Option<Team>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub ims: Vec<Channel>,
    #[serde(default)]
    pub groups: Vec<Channel>,
    #[serde(default)]
    pub mpims: Vec<Channel>,
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub starred: Vec<ChannelId>,
    #[serde(default)]
    pub channels_priority: BTreeMap<ChannelId, f64>,
    #[serde(default)]
    pub prefs: BootPrefs,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl BootData {
    pub fn all_channels(&self) -> Vec<Channel> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for channel in self
            .channels
            .iter()
            .chain(&self.groups)
            .chain(&self.ims)
            .chain(&self.mpims)
        {
            if seen.insert(channel.id.clone()) {
                out.push(channel.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BootPrefs {
    #[serde(default)]
    pub sidebar_behavior: Option<String>,
    #[serde(default)]
    pub priority_sidebar_section: bool,
    #[serde(default)]
    pub channel_sections: Option<String>,
    #[serde(default)]
    pub team_channel_sections: Option<String>,
    #[serde(default)]
    pub hidden_user_group_sections: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchMessagesPage {
    #[serde(default)]
    pub items: Vec<SearchItem>,
    #[serde(default)]
    pub pagination: Option<SearchPagination>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchItem {
    #[serde(default)]
    pub iid: Option<String>,
    #[serde(default)]
    pub team: Option<TeamId>,
    #[serde(default)]
    pub channel: Option<Channel>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchPagination {
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_count: Option<u32>,
    #[serde(default)]
    pub per_page: Option<u32>,
    #[serde(default)]
    pub total_count: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchInlinePage {
    #[serde(default)]
    pub items: Vec<SearchInlineItem>,
    #[serde(default)]
    pub pagination: Option<SearchPagination>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchInlineItem {
    #[serde(default)]
    pub channel_id: Option<ChannelId>,
    #[serde(default)]
    pub iid: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub ts: Option<MessageTs>,
    #[serde(default)]
    pub user: Option<UserId>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenedConversation {
    pub channel: OpenedChannel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenedChannel {
    pub id: ChannelId,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeResults<T> {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub results: Vec<T>,
    #[serde(default)]
    pub failed_ids: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Emoji {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub updated: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityFeedPage {
    #[serde(default)]
    pub items: Vec<ActivityItem>,
    #[serde(default)]
    pub response_metadata: Option<ResponseMetadata>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityItem {
    #[serde(default)]
    pub is_unread: bool,
    #[serde(default)]
    pub feed_ts: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub item: ActivityEntry,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityEntry {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub message: Option<ActivityMessageRef>,
    #[serde(default)]
    pub reaction: Option<ActivityReaction>,
    #[serde(default)]
    pub bundle_info: Option<ActivityBundleInfo>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityMessageRef {
    #[serde(default)]
    pub ts: Option<MessageTs>,
    #[serde(default)]
    pub channel: Option<ChannelId>,
    #[serde(default)]
    pub thread_ts: Option<MessageTs>,
    #[serde(default)]
    pub author_user_id: Option<UserId>,
    #[serde(default)]
    pub is_broadcast: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityReaction {
    #[serde(default)]
    pub user: Option<UserId>,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityBundleInfo {
    #[serde(default)]
    pub payload: Option<ActivityBundlePayload>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityBundlePayload {
    #[serde(default)]
    pub thread_entry: Option<ActivityThreadEntry>,
    #[serde(default)]
    pub dm_entry: Option<ActivityDmEntry>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityDmEntry {
    #[serde(default)]
    pub latest_message: Option<ActivityMessageRef>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivityThreadEntry {
    #[serde(default)]
    pub channel_id: Option<ChannelId>,
    #[serde(default)]
    pub thread_ts: Option<MessageTs>,
    #[serde(default)]
    pub latest_ts: Option<MessageTs>,
    #[serde(default)]
    pub unread_msg_count: u32,
    #[serde(default)]
    pub min_unread_ts: Option<MessageTs>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessagesListPage {
    #[serde(default)]
    pub messages_data: BTreeMap<ChannelId, MessagesListChannel>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessagesListChannel {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ActivityItem {
    pub fn channel(&self) -> Option<&str> {
        if let Some(entry) = self.thread_entry() {
            return entry.channel_id.as_deref();
        }
        if let Some(dm) = self.dm_message() {
            return dm.channel.as_deref();
        }
        self.item
            .message
            .as_ref()
            .and_then(|m| m.channel.as_deref())
    }

    pub fn ts(&self) -> Option<&str> {
        if let Some(entry) = self.thread_entry() {
            return entry.thread_ts.as_deref();
        }
        if let Some(dm) = self.dm_message() {
            return dm.ts.as_deref();
        }
        self.item.message.as_ref().and_then(|m| m.ts.as_deref())
    }

    pub fn thread_ts(&self) -> Option<&str> {
        if let Some(entry) = self.thread_entry() {
            return entry.thread_ts.as_deref();
        }
        self.item
            .message
            .as_ref()
            .and_then(|m| m.thread_ts.as_deref())
    }

    pub fn author(&self) -> Option<&str> {
        if let Some(reaction) = &self.item.reaction {
            return reaction.user.as_deref();
        }
        self.item
            .message
            .as_ref()
            .and_then(|m| m.author_user_id.as_deref())
    }

    pub fn latest_ts(&self) -> Option<&str> {
        self.thread_entry().and_then(|e| e.latest_ts.as_deref())
    }

    pub fn preview_ts(&self) -> Option<&str> {
        self.latest_ts().or_else(|| self.ts())
    }

    pub fn request_ts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for ts in [self.ts(), self.latest_ts()].into_iter().flatten() {
            let ts = ts.to_owned();
            if !out.contains(&ts) {
                out.push(ts);
            }
        }
        out
    }

    fn thread_entry(&self) -> Option<&ActivityThreadEntry> {
        self.item
            .bundle_info
            .as_ref()?
            .payload
            .as_ref()?
            .thread_entry
            .as_ref()
    }

    fn dm_message(&self) -> Option<&ActivityMessageRef> {
        self.item
            .bundle_info
            .as_ref()?
            .payload
            .as_ref()?
            .dm_entry
            .as_ref()?
            .latest_message
            .as_ref()
    }

    pub fn identity(&self) -> String {
        let channel = self.channel().unwrap_or("");
        if self.thread_entry().is_some()
            || matches!(self.item.kind.as_str(), "thread_v2" | "thread_reply")
        {
            let thread_ts = self.thread_ts().unwrap_or("");
            return format!("thread:{channel}:{thread_ts}");
        }
        let kind = self.item.kind.as_str();
        if matches!(kind, "dm" | "bot_dm_bundle") {
            return format!("dm:{channel}");
        }
        let ts = self.ts().unwrap_or("");
        format!("{kind}:{channel}:{ts}")
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    #[test]
    fn activity_feed_decodes_mixed_item_shapes() {
        let page: ActivityFeedPage = serde_json::from_str(
            r#"{
                "items": [
                    {"is_unread":true,"feed_ts":"1783828299.522099","key":"thread_v2-C0B1-1",
                     "item":{"type":"thread_v2","bundle_info":{"payload":{"thread_entry":{
                        "channel_id":"C0B1","thread_ts":"1783823177.936649","latest_ts":"1783828299.522099","unread_msg_count":1}}}}},
                    {"is_unread":false,"feed_ts":"1783827397.000000","key":"reaction-1",
                     "item":{"type":"message_reaction","message":{"ts":"1783716780.838909","channel":"C05S"},
                             "reaction":{"user":"U08R","name":"yay"}}},
                    {"is_unread":false,"feed_ts":"1783825812.401009","key":"at_user-1",
                     "item":{"type":"at_user","message":{"ts":"1783825812.401009","channel":"C08G",
                             "thread_ts":"1783822728.610059","author_user_id":"U07U"}}}
                ]
            }"#,
        )
        .expect("decode activity feed");

        assert_eq!(page.items.len(), 3);

        let thread = &page.items[0];
        assert_eq!(thread.item.kind, "thread_v2");
        assert_eq!(thread.channel(), Some("C0B1"));
        assert_eq!(thread.ts(), Some("1783823177.936649"));
        assert!(thread.is_unread);

        let reaction = &page.items[1];
        assert_eq!(reaction.channel(), Some("C05S"));
        assert_eq!(reaction.ts(), Some("1783716780.838909"));
        assert_eq!(reaction.author(), Some("U08R"));
        assert_eq!(reaction.item.reaction.as_ref().unwrap().name, "yay");

        let mention = &page.items[2];
        assert_eq!(mention.channel(), Some("C08G"));
        assert_eq!(mention.thread_ts(), Some("1783822728.610059"));
        assert_eq!(mention.author(), Some("U07U"));
    }

    #[test]
    fn deserialize_history_response() {
        let page: HistoryPage = serde_json::from_str(
            r#"{
                "ok": true,
                "messages": [
                    {"type":"message","user":"U01234567","text":"fixture message one","ts":"1783372360.741769"},
                    {"type":"message","user":"U07654321","text":"two","ts":"1783372400.100200",
                     "reactions":[{"name":"wave","users":["U01234567"],"count":1}]},
                    {"type":"message","subtype":"channel_join","user":"U01234567","ts":"1783372000.000001"}
                ],
                "has_more": true,
                "pin_count": 2,
                "response_metadata": {"next_cursor": "bmV4dF9jdXJzb3I="}
            }"#,
        )
        .unwrap();
        assert_eq!(page.messages.len(), 3);
        assert!(page.has_more);
        assert_eq!(page.pin_count, Some(2));
        assert_eq!(
            page.response_metadata
                .as_ref()
                .unwrap()
                .next_cursor
                .as_deref(),
            Some("bmV4dF9jdXJzb3I=")
        );
        assert_eq!(
            page.messages[0].text.as_deref(),
            Some("fixture message one")
        );
        assert_eq!(page.messages[1].reactions[0].name, "wave");
        assert_eq!(page.messages[2].subtype.as_deref(), Some("channel_join"));
    }

    #[test]
    fn deserialize_edge_channels_info() {
        let page: EdgeResults<Channel> = serde_json::from_str(
            r#"{"ok":true,"results":[
                {"id":"C0159TSJVH8","name":"general","is_channel":true,"updated":1783372000},
                {"id":"C09876543","name":"random","is_channel":true,"updated":1783371800}
            ],"failed_ids":[]}"#,
        )
        .unwrap();
        assert!(page.ok);
        assert_eq!(page.results.len(), 2);
        assert_eq!(page.results[0].name.as_deref(), Some("general"));
        assert!(page.failed_ids.is_empty());
    }

    #[test]
    fn deserialize_sidebar_dms_response() {
        let page: SidebarDmsPage = serde_json::from_str(
            r#"{
                "ok": true,
                "ims": [{"id":"D1","name":"alice","is_im":true,"user":"U1"}],
                "mpdms": [{"id":"G1","name":"alice--bob","is_mpim":true,"is_group":true}]
            }"#,
        )
        .unwrap();

        assert_eq!(page.all_channels().len(), 2);
        assert!(page.all_channels()[0].is_im);
        assert!(page.all_channels()[1].is_mpim);
    }

    #[test]
    fn deserialize_boot_sidebar_preferences() {
        let boot: BootData = serde_json::from_str(
            r#"{
                "self": {"id":"U_SELF"},
                "starred": ["C_STAR", "C_OTHER"],
                "channels_priority": {"C_VIP": 0.9, "D_DM": 0.25},
                "prefs": {
                    "sidebar_behavior": "hide_read_channels_unless_starred",
                    "priority_sidebar_section": true,
                    "channel_sections": "{\"priority\":{\"sort\":\"recent\"}}"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(boot.starred, ["C_STAR", "C_OTHER"]);
        assert_eq!(boot.channels_priority.get("C_VIP"), Some(&0.9));
        assert!(boot.prefs.priority_sidebar_section);
        assert_eq!(
            boot.prefs.sidebar_behavior.as_deref(),
            Some("hide_read_channels_unless_starred")
        );
    }

    #[test]
    fn deserialize_counts_unread_string_updated() {
        let page: CountsPage = serde_json::from_str(
            r#"{
                "ok": true,
                "activity_v2": {
                    "at_user": 6,
                    "dm": 2,
                    "thread_v2": 14,
                    "message_reaction": 0
                },
                "ims": [
                    {
                        "id": "D083XVDGBJ8",
                        "is_im": true,
                        "updated": "1778339234.000100",
                        "mention_count": 1,
                        "has_unreads": true
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(page.ims[0].updated, Some(1_778_339_234));
        assert!(page.ims[0].has_unreads);
        assert_eq!(page.ims[0].mention_count, Some(1));
        assert_eq!(page.activity_unread_count(), Some(22));
    }

    #[test]
    fn deserialize_edge_users_list() {
        let page: EdgeResults<User> = serde_json::from_str(
            r#"{"ok":true,"results":[{
                "id":"U01234567","name":"alice","real_name":"Alice Anderson",
                "profile":{"display_name":"alice","real_name":"Alice Anderson"}
            }],"failed_ids":["U_MISSING"]}"#,
        )
        .unwrap();
        assert!(page.ok);
        assert_eq!(page.results.len(), 1);
        assert_eq!(
            page.results[0]
                .profile
                .as_ref()
                .unwrap()
                .display_name
                .as_deref(),
            Some("alice")
        );
        assert_eq!(page.failed_ids, vec!["U_MISSING".to_owned()]);
    }

    #[test]
    fn deserialize_users_search_results() {
        let page: EdgeResults<User> = serde_json::from_str(
            r#"{"ok":true,"results":[
                {"id":"U0ALICE","name":"alice","deleted":false,"color":"9f69e7",
                 "real_name":"Alice A","tz":"America/New_York","is_admin":true,
                 "is_bot":false,"is_app_user":false,"updated":1783000000,
                 "enterprise_user":{"id":"W1"},"enterprise_id":"E09V59WQY1E",
                 "profile":{"display_name":"alice","real_name":"Alice A"}},
                {"id":"U0BOT","name":"helperbot","is_bot":true}
            ]}"#,
        )
        .unwrap();
        assert!(page.ok);
        assert_eq!(page.results.len(), 2);
        assert_eq!(page.results[0].id, "U0ALICE");
        assert_eq!(
            page.results[0]
                .profile
                .as_ref()
                .unwrap()
                .display_name
                .as_deref(),
            Some("alice")
        );
        assert!(page.results[1].is_bot);
    }

    #[test]
    fn deserialize_conversations_open() {
        let opened: OpenedConversation = serde_json::from_str(
            r#"{"ok":true,"no_op":false,"already_open":true,"channel":{"id":"D0ALICE"}}"#,
        )
        .unwrap();
        assert_eq!(opened.channel.id, "D0ALICE");
    }

    #[test]
    fn deserialize_search_messages_page() {
        let page: SearchMessagesPage = serde_json::from_str(
            r#"{
                "ok": true,
                "module": "messages",
                "query": "hello",
                "items": [
                    {
                        "iid": "21c94942-e73c-4cb6-a605-44994d411930",
                        "team": "T0266FRGM",
                        "channel": {"id":"C07TM4C0AQ5","name":"help","is_channel":true,"is_private":false},
                        "messages": [{
                            "type":"message",
                            "text":"Hello, can anyone help me?",
                            "ts":"1783431840.570159",
                            "thread_ts":"1783431840.570159",
                            "user":"U0BF7PL6RQF",
                            "username":"lolfero095",
                            "reply_count":5,
                            "reactions":[{"name":"white_check_mark","count":1,"users":["U0ASE1R05FW"]}],
                            "permalink":"https://hackclub.slack.com/archives/C07TM4C0AQ5/p1783431840570159"
                        }]
                    }
                ],
                "pagination": {"first":1,"last":5,"page":1,"page_count":23551,"per_page":5,"total_count":117752}
            }"#,
        )
        .unwrap();

        assert_eq!(page.module.as_deref(), Some("messages"));
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.channel.as_ref().unwrap().name.as_deref(), Some("help"));
        let msg = &item.messages[0];
        assert_eq!(msg.text.as_deref(), Some("Hello, can anyone help me?"));
        assert_eq!(msg.ts.as_deref(), Some("1783431840.570159"));
        assert_eq!(msg.reply_count, Some(5));
        assert_eq!(msg.reactions[0].name, "white_check_mark");
        assert_eq!(
            msg.extra["permalink"].as_str(),
            Some("https://hackclub.slack.com/archives/C07TM4C0AQ5/p1783431840570159")
        );
        let pagination = page.pagination.unwrap();
        assert_eq!(pagination.page, Some(1));
        assert_eq!(pagination.page_count, Some(23551));
        assert_eq!(pagination.total_count, Some(117752));
    }

    #[test]
    fn deserialize_search_inline_page() {
        let page: SearchInlinePage = serde_json::from_str(
            r#"{
                "items": [
                    {
                        "channel_id": "C0BBMA16677",
                        "iid": "fac7c614-5c33-4702-91bd-06122777ee4f",
                        "permalink": "https://hackclub.enterprise.slack.com/archives/C0BBMA16677/p1783308502631879",
                        "ts": "1783308502.631879",
                        "user": "U0AEY1PUMPX"
                    }
                ],
                "ok": true,
                "pagination": { "first": 1, "last": 2, "page": 1, "page_count": 1, "per_page": 20, "total_count": 2 },
                "query": "deploy"
            }"#,
        )
        .unwrap();

        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.channel_id.as_deref(), Some("C0BBMA16677"));
        assert_eq!(item.ts.as_deref(), Some("1783308502.631879"));
        assert_eq!(item.user.as_deref(), Some("U0AEY1PUMPX"));
        assert_eq!(
            item.permalink.as_deref(),
            Some("https://hackclub.enterprise.slack.com/archives/C0BBMA16677/p1783308502631879")
        );

        let pagination = page.pagination.unwrap();
        assert_eq!(pagination.total_count, Some(2));
        assert!(pagination.extra.contains_key("first"));
    }

    #[test]
    fn deserialize_message_attachments() {
        let page: HistoryPage = serde_json::from_str(
            r#"{"ok":true,"messages":[{
                "type":"message",
                "ts":"1783328401.000100",
                "text":"check this",
                "attachments":[{
                    "id":1,
                    "service_name":"the Guardian",
                    "service_icon":"https://www.theguardian.com/favicon.ico",
                    "title":"Some headline",
                    "title_link":"https://www.theguardian.com/football/x",
                    "text":"A short description of the article.",
                    "image_url":"https://i.guim.co.uk/img/media/x/master/2961.jpg",
                    "from_url":"https://www.theguardian.com/football/x",
                    "fields":[{"title":"Score","value":"1-0","short":true}]
                }]
            }]}"#,
        )
        .unwrap();
        let att = &page.messages[0].attachments[0];
        assert_eq!(att.service_name.as_deref(), Some("the Guardian"));
        assert_eq!(att.title.as_deref(), Some("Some headline"));
        assert_eq!(
            att.title_link.as_deref(),
            Some("https://www.theguardian.com/football/x")
        );
        assert_eq!(
            att.image_url.as_deref().unwrap().ends_with("2961.jpg"),
            true
        );
        assert_eq!(att.fields[0].title.as_deref(), Some("Score"));
        assert_eq!(att.fields[0].value.as_deref(), Some("1-0"));
        assert!(att.fields[0].short);
    }

    #[test]
    fn deserialize_message_files() {
        let page: HistoryPage = serde_json::from_str(
            r#"{"ok":true,"messages":[{
                "type":"message",
                "ts":"1783372600.000100",
                "files":[{
                    "id":"F123",
                    "name":"design.png",
                    "title":"Design mock",
                    "mimetype":"image/png",
                    "filetype":"png",
                    "pretty_type":"PNG",
                    "url_private":"https://files.slack.com/files-pri/T-F/design.png",
                    "thumb_160":"https://files.slack.com/files-tmb/T-F/design_160.png",
                    "size":2048,
                    "mode":"hosted"
                }]
            }]}"#,
        )
        .unwrap();
        let file = &page.messages[0].files[0];
        assert_eq!(file.id.as_deref(), Some("F123"));
        assert_eq!(file.title.as_deref(), Some("Design mock"));
        assert_eq!(file.pretty_type.as_deref(), Some("PNG"));
        assert_eq!(file.size, Some(2048));
        assert_eq!(file.extra["mode"].as_str(), Some("hosted"));
    }

    #[test]
    fn deserialize_message_replied_envelope() {
        let page: HistoryPage = serde_json::from_str(
            r#"{"ok":true,"messages":[{
                "type":"message",
                "subtype":"message_replied",
                "channel":"C1",
                "ts":"1783372700.000100",
                "message":{
                    "type":"message",
                    "user":"U1",
                    "text":"actual root",
                    "ts":"1783372600.000100",
                    "reply_count":1
                }
            }]}"#,
        )
        .unwrap();
        let envelope = &page.messages[0];
        let nested = envelope.message.as_ref().unwrap();
        assert_eq!(envelope.subtype.as_deref(), Some("message_replied"));
        assert_eq!(nested.user.as_deref(), Some("U1"));
        assert_eq!(nested.text.as_deref(), Some("actual root"));
    }

    #[test]
    fn deserialize_bot_profile_message() {
        let page: HistoryPage = serde_json::from_str(
            r#"{"ok":true,"messages":[{
                "type":"message",
                "bot_id":"B123",
                "username":"helper",
                "text":"hello",
                "ts":"1783372600.000100",
                "bot_profile":{
                    "id":"B123",
                    "name":"Helper Bot",
                    "user_id":"U_APP",
                    "icons":{
                        "image_36":"https://example.test/36.png",
                        "image_48":"https://example.test/48.png",
                        "image_72":"https://example.test/72.png"
                    }
                }
            }]}"#,
        )
        .unwrap();
        let msg = &page.messages[0];
        assert_eq!(msg.username.as_deref(), Some("helper"));
        let profile = msg.bot_profile.as_ref().unwrap();
        assert_eq!(profile.name.as_deref(), Some("Helper Bot"));
        assert_eq!(
            profile.icons.as_ref().unwrap().image_48.as_deref(),
            Some("https://example.test/48.png")
        );
    }
}
