use crate::config::WorkspaceSession;

use super::Error;
use super::client::{PreparedRequest, SlackClient};
use super::models::{
    BootData, Channel, ChannelId, CountsPage, EdgeResults, Emoji, HistoryPage, MessageTs,
    SearchInlinePage, SearchMessagesPage, SentMessage, SidebarDmsPage, User,
};
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

pub fn conversations_info(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "conversations.info",
        vec![
            ("channel", channel),
            ("include_num_members", "false".to_owned()),
        ],
    )
}

pub fn client_counts(client: &SlackClient, workspace: &WorkspaceSession) -> PreparedRequest {
    client.rest_form(workspace, "client.counts", Vec::new())
}

pub fn sidebar_dms(client: &SlackClient, workspace: &WorkspaceSession) -> PreparedRequest {
    client.rest_form(workspace, "sidebar.dms", Vec::new())
}

pub fn users_set_presence(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    presence: String,
) -> PreparedRequest {
    client.rest_form(workspace, "users.setPresence", vec![("presence", presence)])
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

#[derive(Debug, Clone)]
pub struct SearchArgs {
    pub query: String,
    pub count: u32,
    pub page: u32,
}

impl SearchArgs {
    pub fn new(query: impl Into<String>) -> Self {
        SearchArgs {
            query: query.into(),
            count: 20,
            page: 1,
        }
    }
}

pub fn search_messages(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    args: SearchArgs,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "search.modules.messages",
        vec![
            ("module", "messages".to_owned()),
            ("query", args.query),
            ("count", args.count.to_string()),
            ("page", args.page.to_string()),
            ("sort", "timestamp".to_owned()),
            ("sort_dir", "desc".to_owned()),
            ("highlight", "true".to_owned()),
            ("extracts", "true".to_owned()),
            ("extra_message_data", "true".to_owned()),
            ("client_req_id", uuid::Uuid::new_v4().to_string()),
            ("search_session_id", uuid::Uuid::new_v4().to_string()),
        ],
    )
}

#[derive(Debug, Clone)]
pub struct SearchInlineArgs {
    pub query: String,
    pub channel: ChannelId,
    pub count: u32,
    pub page: u32,
}

impl SearchInlineArgs {
    pub fn new(query: impl Into<String>, channel: ChannelId) -> Self {
        SearchInlineArgs {
            query: query.into(),
            channel,
            count: 20,
            page: 1,
        }
    }
}

pub fn search_inline(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    args: SearchInlineArgs,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "search.inline",
        vec![
            ("query", args.query),
            ("channel", args.channel),
            ("count", args.count.to_string()),
            ("page", args.page.to_string()),
            ("sort", "timestamp".to_owned()),
            ("sort_dir", "desc".to_owned()),
            ("highlight", "true".to_owned()),
            ("extracts", "true".to_owned()),
            ("client_req_id", uuid::Uuid::new_v4().to_string()),
            ("search_session_id", uuid::Uuid::new_v4().to_string()),
        ],
    )
}

pub fn chat_update(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
    text: String,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "chat.update",
        vec![("channel", channel), ("ts", ts), ("text", text)],
    )
}

pub fn chat_delete(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
) -> PreparedRequest {
    client.rest_form(
        workspace,
        "chat.delete",
        vec![("channel", channel), ("ts", ts)],
    )
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

pub async fn fetch_replies(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
    cursor: Option<String>,
) -> Result<HistoryPage, Error> {
    let value = transport
        .execute(conversations_replies(
            client, workspace, channel, ts, cursor,
        ))
        .await?;
    decode(value, "conversations.replies")
}

pub async fn mark_channel(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
) -> Result<(), Error> {
    transport
        .execute(conversations_mark(client, workspace, channel, ts))
        .await?;
    Ok(())
}

pub async fn fetch_counts(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
) -> Result<CountsPage, Error> {
    let value = transport.execute(client_counts(client, workspace)).await?;
    decode(value, "client.counts")
}

pub async fn fetch_sidebar_dms(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
) -> Result<SidebarDmsPage, Error> {
    let value = transport.execute(sidebar_dms(client, workspace)).await?;
    decode(value, "sidebar.dms")
}

pub async fn set_presence(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    presence: String,
) -> Result<(), Error> {
    transport
        .execute(users_set_presence(client, workspace, presence))
        .await?;
    Ok(())
}

pub async fn fetch_users_info(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    user_ids: Vec<String>,
) -> Result<Vec<User>, Error> {
    let request = super::edge::users_info(client, workspace, &user_ids)
        .map_err(|e| Error::Transport(format!("build users/info: {e}")))?;
    let value = transport.execute(request).await?;
    let page: EdgeResults<User> = decode(value, "users/info")?;
    Ok(page.results)
}

pub async fn fetch_channels_info(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel_ids: Vec<ChannelId>,
) -> Result<Vec<Channel>, Error> {
    let request = super::edge::channels_info(client, workspace, &channel_ids)
        .map_err(|e| Error::Transport(format!("build channels/info: {e}")))?;
    let mut channels = match transport.execute(request).await {
        Ok(value) => {
            let page: EdgeResults<Channel> = decode(value, "channels/info")?;
            page.results
        }
        Err(e) => {
            tracing::debug!(error = %e, "edge channels/info failed; falling back to conversations.info");
            Vec::new()
        }
    };

    let found = channels
        .iter()
        .map(|channel| channel.id.clone())
        .collect::<std::collections::HashSet<_>>();
    for channel_id in channel_ids
        .into_iter()
        .filter(|channel_id| !found.contains(channel_id))
    {
        match fetch_conversation_info(transport, client, workspace, channel_id.clone()).await {
            Ok(channel) => channels.push(channel),
            Err(e) => {
                tracing::debug!(channel = %channel_id, error = %e, "conversations.info fallback failed")
            }
        }
    }

    Ok(channels)
}

pub async fn fetch_emojis_info(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    names: Vec<String>,
) -> Result<Vec<Emoji>, Error> {
    let request = super::edge::emojis_info(client, workspace, &names)
        .map_err(|e| Error::Transport(format!("build emojis/info: {e}")))?;
    let value = transport.execute(request).await?;
    let page: EdgeResults<Emoji> = decode(value, "emojis/info")?;
    Ok(page.results)
}

async fn fetch_conversation_info(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel_id: ChannelId,
) -> Result<Channel, Error> {
    #[derive(serde::Deserialize)]
    struct ConversationInfoPage {
        channel: Channel,
    }

    let value = transport
        .execute(conversations_info(client, workspace, channel_id))
        .await?;
    let page: ConversationInfoPage = decode(value, "conversations.info")?;
    Ok(page.channel)
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

pub async fn fetch_search_messages(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    args: SearchArgs,
) -> Result<SearchMessagesPage, Error> {
    let value = transport
        .execute(search_messages(client, workspace, args))
        .await?;
    decode(value, "search.modules.messages")
}

pub async fn fetch_search_inline(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    args: SearchInlineArgs,
) -> Result<SearchInlinePage, Error> {
    let value = transport
        .execute(search_inline(client, workspace, args))
        .await?;
    decode(value, "search.inline")
}

pub async fn edit_message(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
    text: String,
) -> Result<SentMessage, Error> {
    let value = transport
        .execute(chat_update(client, workspace, channel, ts, text))
        .await?;
    decode(value, "chat.update")
}

pub async fn delete_message(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    ts: MessageTs,
) -> Result<(), Error> {
    transport
        .execute(chat_delete(client, workspace, channel, ts))
        .await?;
    Ok(())
}

pub async fn add_reaction(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    timestamp: MessageTs,
    name: String,
) -> Result<(), Error> {
    transport
        .execute(reactions_add(client, workspace, channel, timestamp, name))
        .await?;
    Ok(())
}

pub async fn remove_reaction(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
    timestamp: MessageTs,
    name: String,
) -> Result<(), Error> {
    transport
        .execute(reactions_remove(
            client, workspace, channel, timestamp, name,
        ))
        .await?;
    Ok(())
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
    fn replies_request_includes_thread_target() {
        let request = conversations_replies(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "1783372360.741769".into(),
            None,
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/conversations.replies?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("ts".into(), "1783372360.741769".into())));
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

    #[test]
    fn send_thread_reply_includes_thread_ts() {
        let request = chat_post_message(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "reply from snack".into(),
            Some("1783372360.741769".into()),
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/chat.postMessage?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("text".into(), "reply from snack".into())));
        assert!(fields.contains(&("thread_ts".into(), "1783372360.741769".into())));
    }

    #[test]
    fn search_request_targets_messages_module_with_query_and_page() {
        let request = search_messages(
            &SlackClient::default(),
            &workspace(),
            SearchArgs {
                query: "deploy failed".into(),
                count: 20,
                page: 2,
            },
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/search.modules.messages?"));
        assert!(fields.contains(&("module".into(), "messages".into())));
        assert!(fields.contains(&("query".into(), "deploy failed".into())));
        assert!(fields.contains(&("count".into(), "20".into())));
        assert!(fields.contains(&("page".into(), "2".into())));
        assert!(fields.iter().any(|(k, _)| k == "client_req_id"));
        assert!(fields.iter().any(|(k, _)| k == "search_session_id"));
    }

    #[test]
    fn search_inline_request_targets_channel_with_query() {
        let request = search_inline(
            &SlackClient::default(),
            &workspace(),
            SearchInlineArgs {
                query: "deploy".into(),
                channel: "C0BBMA16677".into(),
                count: 20,
                page: 1,
            },
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/search.inline?"));
        assert!(fields.contains(&("query".into(), "deploy".into())));
        assert!(fields.contains(&("channel".into(), "C0BBMA16677".into())));
        assert!(fields.contains(&("count".into(), "20".into())));
        assert!(fields.iter().any(|(k, _)| k == "client_req_id"));
        assert!(fields.iter().any(|(k, _)| k == "search_session_id"));
    }

    #[test]
    fn update_request_includes_channel_ts_and_text() {
        let request = chat_update(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "1783372360.741769".into(),
            "edited body".into(),
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/chat.update?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("ts".into(), "1783372360.741769".into())));
        assert!(fields.contains(&("text".into(), "edited body".into())));
    }

    #[test]
    fn delete_request_includes_channel_and_ts() {
        let request = chat_delete(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "1783372360.741769".into(),
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/chat.delete?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("ts".into(), "1783372360.741769".into())));
    }

    #[test]
    fn mark_request_includes_channel_and_ts() {
        let request = conversations_mark(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "1783372400.111111".into(),
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/conversations.mark?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("ts".into(), "1783372400.111111".into())));
    }

    #[test]
    fn conversations_info_request_targets_channel() {
        let request = conversations_info(&SlackClient::default(), &workspace(), "C123".into());
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/conversations.info?"));
        assert!(fields.contains(&("channel".into(), "C123".into())));
        assert!(fields.contains(&("include_num_members".into(), "false".into())));
    }

    #[test]
    fn reaction_request_includes_message_target() {
        let request = reactions_add(
            &SlackClient::default(),
            &workspace(),
            "C0159TSJVH8".into(),
            "1783372360.741769".into(),
            "thumbsup".into(),
        );
        let fields = form_fields(&request);

        assert!(request.url.contains("/api/reactions.add?"));
        assert!(fields.contains(&("channel".into(), "C0159TSJVH8".into())));
        assert!(fields.contains(&("timestamp".into(), "1783372360.741769".into())));
        assert!(fields.contains(&("name".into(), "thumbsup".into())));
    }

    #[test]
    fn counts_request_targets_client_counts() {
        let request = client_counts(&SlackClient::default(), &workspace());
        assert!(request.url.contains("/api/client.counts?"));
        assert!(form_fields(&request).contains(&("token".into(), "xoxc-test-token".into())));
    }

    #[test]
    fn sidebar_dms_request_targets_sidebar_dms() {
        let request = sidebar_dms(&SlackClient::default(), &workspace());
        assert!(request.url.contains("/api/sidebar.dms?"));
        assert!(form_fields(&request).contains(&("token".into(), "xoxc-test-token".into())));
    }

    #[test]
    fn set_presence_request_includes_presence_value() {
        let request = users_set_presence(&SlackClient::default(), &workspace(), "away".into());
        let fields = form_fields(&request);
        assert!(request.url.contains("/api/users.setPresence?"));
        assert!(fields.contains(&("presence".into(), "away".into())));
        assert!(fields.contains(&("token".into(), "xoxc-test-token".into())));
    }
}
