use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::models::{ChannelId, Message, MessageTs, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub enum RtEvent {
    Message(Message),
    MessageChanged {
        channel: ChannelId,
        message: Message,
    },
    MessageDeleted {
        channel: ChannelId,
        deleted_ts: MessageTs,
    },
    UserTyping {
        channel: ChannelId,
        user: UserId,
    },
    PresenceChange {
        user: UserId,
        presence: String,
    },
    Unknown(RawEvent),
}
