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
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CountsPage {
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
    #[serde(default)]
    pub user: Option<UserId>,
    #[serde(default)]
    pub unread_count: Option<u32>,
    #[serde(default)]
    pub unread_count_display: Option<u32>,
    #[serde(default)]
    pub last_read: Option<MessageTs>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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

#[cfg(test)]
mod fixture_tests {
    use super::*;

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
}
