use serde_json::Value;

use crate::state::{self, Workspace};

pub fn message_lines(ws: &Workspace, msg: &crate::slack::models::Message) -> Vec<String> {
    lines(ws, &msg.blocks)
}

pub fn notification_text(ws: &Workspace, msg: &crate::slack::models::Message) -> String {
    let block_text = message_lines(ws, msg).join(" ");
    if !block_text.trim().is_empty() {
        return block_text;
    }
    state::message_text(msg)
}

pub fn lines(ws: &Workspace, blocks: &[Value]) -> Vec<String> {
    blocks
        .iter()
        .flat_map(|block| block_lines(ws, block))
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

fn block_lines(ws: &Workspace, block: &Value) -> Vec<String> {
    match value_type(block) {
        Some("rich_text") => block
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|element| rich_element_lines(ws, element))
            .collect(),
        Some("section") => {
            let mut out = Vec::new();
            if let Some(text) = block.get("text").and_then(text_object) {
                out.push(text);
            }
            if let Some(fields) = block.get("fields").and_then(Value::as_array) {
                out.extend(fields.iter().filter_map(text_object));
            }
            out
        }
        Some("context") => block
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(text_object)
            .collect(),
        Some("header") => block
            .get("text")
            .and_then(text_object)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn rich_element_lines(ws: &Workspace, element: &Value) -> Vec<String> {
    match value_type(element) {
        Some("rich_text_section") => vec![rich_inline_text(ws, element)],
        Some("rich_text_list") => element
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|item| format!("- {}", rich_inline_text(ws, item)))
            .collect(),
        Some("rich_text_preformatted") => vec![rich_inline_text(ws, element)],
        Some("rich_text_quote") => vec![format!("> {}", rich_inline_text(ws, element))],
        _ => Vec::new(),
    }
}

fn rich_inline_text(ws: &Workspace, element: &Value) -> String {
    element
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|child| rich_leaf_text(ws, child))
        .collect::<String>()
}

fn rich_leaf_text(ws: &Workspace, leaf: &Value) -> String {
    match value_type(leaf) {
        Some("text") => leaf
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        Some("emoji") => leaf
            .get("name")
            .and_then(Value::as_str)
            .map(|name| format!(":{name}:"))
            .unwrap_or_default(),
        Some("user") => leaf
            .get("user_id")
            .or_else(|| leaf.get("user"))
            .and_then(Value::as_str)
            .map(|user| format!("@{}", ws.display_name(user)))
            .unwrap_or_default(),
        Some("channel") => leaf
            .get("channel_id")
            .or_else(|| leaf.get("channel"))
            .and_then(Value::as_str)
            .map(|channel| {
                ws.channels
                    .get(channel)
                    .map(state::channel_label)
                    .unwrap_or_else(|| format!("#{channel}"))
            })
            .unwrap_or_default(),
        Some("link") => leaf
            .get("text")
            .or_else(|| leaf.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        Some("broadcast") => leaf
            .get("range")
            .and_then(Value::as_str)
            .map(|range| format!("@{range}"))
            .unwrap_or_default(),
        Some("usergroup") => leaf
            .get("usergroup_id")
            .or_else(|| leaf.get("usergroup"))
            .and_then(Value::as_str)
            .map(|group| format!("@{group}"))
            .unwrap_or_default(),
        Some("date") => leaf
            .get("fallback")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    }
}

fn text_object(value: &Value) -> Option<String> {
    value
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn value_type(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use serde_json::json;

    use super::*;
    use crate::slack::models::{Channel, User, UserProfile};
    use crate::state::{RealtimeStatus, Workspace};

    fn ws() -> Workspace {
        Workspace {
            team_id: "T1".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            self_user_id: "U_SELF".into(),
            channels: BTreeMap::from([(
                "C1".into(),
                Channel {
                    id: "C1".into(),
                    name: Some("general".into()),
                    is_channel: true,
                    ..Default::default()
                },
            )]),
            users: HashMap::from([(
                "U1".into(),
                User {
                    id: "U1".into(),
                    profile: Some(UserProfile {
                        display_name: Some("alice".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )]),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        }
    }

    #[test]
    fn renders_section_text_and_fields() {
        let blocks = vec![json!({
            "type": "section",
            "text": {"type": "mrkdwn", "text": "*Deploy* finished"},
            "fields": [
                {"type": "plain_text", "text": "Status: green"},
                {"type": "mrkdwn", "text": "Owner: ops"}
            ]
        })];

        assert_eq!(
            lines(&ws(), &blocks),
            vec!["*Deploy* finished", "Status: green", "Owner: ops"]
        );
    }

    #[test]
    fn renders_rich_text_mentions_channels_and_links() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_section",
                "elements": [
                    {"type": "text", "text": "Hi "},
                    {"type": "user", "user_id": "U1"},
                    {"type": "text", "text": " in "},
                    {"type": "channel", "channel_id": "C1"},
                    {"type": "text", "text": " "},
                    {"type": "emoji", "name": "wave"},
                    {"type": "text", "text": " "},
                    {"type": "link", "url": "https://example.com", "text": "example"}
                ]
            }]
        })];

        assert_eq!(
            lines(&ws(), &blocks),
            vec!["Hi @alice in #general :wave: example"]
        );
    }
}
