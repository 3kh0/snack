use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::WorkspaceSession;

use super::client::{PreparedRequest, SlackClient};
use super::models::ChannelId;

#[derive(Debug, Clone, Serialize)]
struct UpdatedIds<'a> {
    updated_ids: BTreeMap<&'a str, u64>,
}

pub fn channels_info(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channels: &[ChannelId],
) -> serde_json::Result<PreparedRequest> {
    let updated_ids = channels
        .iter()
        .map(|id| (id.as_str(), 0))
        .collect::<BTreeMap<_, _>>();

    client.edge_json(
        workspace,
        "channels/info",
        serde_json::json!({
            "check_membership": true,
            "updated_ids": updated_ids,
        }),
    )
}

pub fn users_info(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    user_ids: &[String],
) -> serde_json::Result<PreparedRequest> {
    client.edge_json(workspace, "users/info", ids_payload(user_ids))
}

pub fn emojis_info(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    names: &[String],
) -> serde_json::Result<PreparedRequest> {
    client.edge_json(workspace, "emojis/info", ids_payload(names))
}

pub fn users_search(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    query: &str,
    count: u32,
) -> serde_json::Result<PreparedRequest> {
    client.edge_json(
        workspace,
        "users/search",
        serde_json::json!({
            "query": query,
            "count": count,
            "fuzz": 1,
            "include_profile_only_users": true,
            "enable_workspace_ranking": true,
            "search_email": true,
        }),
    )
}

fn ids_payload(ids: &[String]) -> UpdatedIds<'_> {
    UpdatedIds {
        updated_ids: ids.iter().map(|id| (id.as_str(), 0)).collect(),
    }
}
