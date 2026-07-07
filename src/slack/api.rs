use crate::config::WorkspaceSession;

use super::Error;
use super::client::{PreparedRequest, SlackClient};
use super::models::{BootData, ChannelId, HistoryPage, MessageTs, SentMessage};
use super::transport::Transport;
pub fn user_boot(client: &SlackClient, workspace: &WorkspaceSession) -> PreparedRequest {
    client.rest_form(
        workspace,
        "client.userBoot",
        vec![("include_min_version_bump_check", "1".to_owned())],
    )
}

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
    reaction(client, workspace, "reactions.add", channel, timestamp, name)
}

pub fn reactions_remove(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    timestamp: MessageTs,
    name: String,
) -> PreparedRequest {
    reaction(
        client,
        workspace,
        "reactions.remove",
        channel,
        timestamp,
        name,
    )
}

fn reaction(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    endpoint: &str,
    channel: ChannelId,
    timestamp: MessageTs,
    name: String,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        endpoint,
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

fn decode<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    what: &str,
) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| Error::Transport(format!("decode {what}: {e}")))
}


pub async fn fetch_user_boot(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
) -> Result<BootData, Error> {
    let value = transport.execute(user_boot(client, workspace)).await?;
    decode(value, "client.userBoot")
}

pub async fn fetch_history(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    args: HistoryArgs,
) -> Result<HistoryPage, Error> {
    let value = transport
        .execute(conversations_history(client, workspace, args))
        .await?;
    decode(value, "conversations.history")
}

pub async fn send_message(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    text: String,
    thread_ts: Option<MessageTs>,
) -> Result<SentMessage, Error> {
    let value = transport
        .execute(chat_post_message(
            client, workspace, channel, text, thread_ts,
        ))
        .await?;
    decode(value, "chat.postMessage")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkspaceSession;

    use super::super::client::RequestBody;

    fn workspace() -> WorkspaceSession {
        WorkspaceSession {
            team_id: "E09V59WQY1E".into(),
            enterprise_id: Some("E09V59WQY1E".into()),
            user_id: "U080A3QP42C".into(),
            name: "Hack Club".into(),
            url: "https://hackclub.enterprise.slack.com".into(),
            token: "xoxc-test-token".into(),
        }
    }

    fn form_fields(req: &PreparedRequest) -> &Vec<(String, String)> {
        match &req.body {
            RequestBody::Form(fields) => fields,
            other => panic!("expected form body, got {other:?}"),
        }
    }

    #[test]
    fn history_request_targets_enterprise_host_with_channel_and_limit() {
        let request = conversations_history(
            &SlackClient::default(),
            &workspace(),
            HistoryArgs {
                channel: "C0159TSJVH8".into(),
                limit: Some(50),
                ..Default::default()
            },
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/conversations.history?"));
        assert!(
            request
                .url
                .starts_with("https://hackclub.enterprise.slack.com")
        );
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("limit".into(), "50".into())));
        assert!(fields.contains(&("token".into(), "xoxc-test-token".into())));
        assert!(!request.redacted_debug().contains("xoxc-test-token"));
    }

    #[test]
    fn send_request_includes_channel_and_text() {
        let request = chat_post_message(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "hello from snack".into(),
            None,
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/chat.postMessage?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("text".into(), "hello from snack".into())));
    }
}
