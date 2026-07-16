use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::models::{ActivityItem, ChannelId, Message, MessageTs, Room, UserId};

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
    ReactionAdded {
        channel: ChannelId,
        ts: MessageTs,
        user: UserId,
        reaction: String,
    },
    ReactionRemoved {
        channel: ChannelId,
        ts: MessageTs,
        user: UserId,
        reaction: String,
    },
    ActivityUpdated(ActivityItem),
    /// A user joined a huddle (`sh_room_join`).
    RoomJoin {
        room: Room,
        user: UserId,
    },
    /// A user left a huddle (`sh_room_leave`).
    RoomLeave {
        room: Room,
        user: UserId,
    },
    /// A huddle's state changed, including ending (`sh_room_update`).
    RoomUpdate {
        room: Room,
    },
    ChannelMarked {
        channel: ChannelId,
        ts: MessageTs,
        unread_count: Option<u32>,
        mention_count: Option<u32>,
    },
    Unknown(RawEvent),
}
