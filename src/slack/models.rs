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
pub struct Message {
    #[serde(default)]
    pub user: Option<UserId>,
    #[serde(default)]
    pub bot_id: Option<String>,
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
    pub files: Vec<Value>,
    #[serde(default)]
    pub edited: Option<Value>,
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
    pub is_archived: bool,
    #[serde(default)]
    pub updated: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
