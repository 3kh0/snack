use crate::config::WorkspaceSession;

use super::client::{PreparedRequest, SlackClient};
use super::models::{ChannelId, MessageTs};

#[derive(Debug, Clone, Default)]
pub struct HistoryArgs {
    pub channel: ChannelId,
    pub cursor: Option<String>,
    pub latest: Option<MessageTs>,
    pub oldest: Option<MessageTs>,
    pub limit: Option<u32>,
    pub inclusive: bool,
}

pub fn conversations_history(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    args: HistoryArgs,
) -> PreparedRequest {
    let mut fields = vec![("channel", args.channel)];
    push_opt(&mut fields, "cursor", args.cursor);
    push_opt(&mut fields, "latest", args.latest);
    push_opt(&mut fields, "oldest", args.oldest);
    push_opt(
        &mut fields,
        "limit",
        args.limit.map(|limit| limit.to_string()),
    );
    if args.inclusive {
        fields.push(("inclusive", "true".to_owned()));
    }

    client.rest_form(workspace, "conversations.history", fields)
}

pub fn conversations_replies(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
    cursor: Option<String>,
) -> PreparedRequest {
    let mut fields = vec![("channel", channel), ("ts", ts)];
    push_opt(&mut fields, "cursor", cursor);
    client.rest_form(workspace, "conversations.replies", fields)
}

pub fn conversations_mark(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "conversations.mark",
        vec![("channel", channel), ("ts", ts)],
    )
}

pub fn chat_post_message(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    text: String,
    thread_ts: Option<MessageTs>,
) -> PreparedRequest {
    let mut fields = vec![("channel", channel), ("text", text)];
    push_opt(&mut fields, "thread_ts", thread_ts);
    client.rest_form(workspace, "chat.postMessage", fields)
}

pub fn reactions_add(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    timestamp: MessageTs,
    name: String,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "reactions.add",
        vec![
            ("channel", channel),
            ("timestamp", timestamp),
            ("name", name),
        ],
    )
}

pub fn reactions_remove(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    timestamp: MessageTs,
    name: String,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "reactions.remove",
        vec![
            ("channel", channel),
            ("timestamp", timestamp),
            ("name", name),
        ],
    )
}

fn push_opt(fields: &mut Vec<(&str, String)>, key: &'static str, value: Option<String>) {
    if let Some(value) = value {
        fields.push((key, value));
    }
}
