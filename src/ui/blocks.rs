use serde_json::Value;

use crate::state::{self, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderLine {
    pub text: String,
    pub mono: bool,
}

pub fn render_lines(ws: &Workspace, msg: &crate::slack::models::Message) -> Vec<RenderLine> {
    render_blocks(ws, &msg.blocks)
}

pub fn notification_text(ws: &Workspace, msg: &crate::slack::models::Message) -> String {
    let block_text = lines(ws, &msg.blocks).join(" ");
    if !block_text.trim().is_empty() {
        return block_text;
    }
    state::message_text(msg)
}

pub fn lines(ws: &Workspace, blocks: &[Value]) -> Vec<String> {
    render_blocks(ws, blocks)
        .into_iter()
        .map(|line| line.text)
        .collect()
}

fn render_blocks(ws: &Workspace, blocks: &[Value]) -> Vec<RenderLine> {
    blocks
        .iter()
        .flat_map(|block| block_lines(ws, block))
        .map(|mut line| {
            if !line.mono {
                line.text = line.text.trim().to_owned();
            }
            line
        })
        .filter(|line| !line.text.trim().is_empty())
        .collect()
}

fn plain(text: String) -> RenderLine {
    RenderLine { text, mono: false }
}

fn block_lines(ws: &Workspace, block: &Value) -> Vec<RenderLine> {
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
                out.push(plain(text));
            }
            if let Some(fields) = block.get("fields").and_then(Value::as_array) {
                out.extend(fields.iter().filter_map(text_object).map(plain));
            }
            out
        }
        Some("context") => block
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(text_object)
            .map(plain)
            .collect(),
        Some("header") => block
            .get("text")
            .and_then(text_object)
            .map(plain)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn rich_element_lines(ws: &Workspace, element: &Value) -> Vec<RenderLine> {
    match value_type(element) {
        Some("rich_text_section") => vec![plain(rich_inline_text(ws, element))],
        Some("rich_text_list") => element
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|item| plain(format!("- {}", rich_inline_text(ws, item))))
            .collect(),
        Some("rich_text_preformatted") => rich_inline_text(ws, element)
            .lines()
            .map(|line| RenderLine {
                text: line.to_owned(),
                mono: true,
            })
            .collect(),
        Some("rich_text_quote") => vec![plain(format!("> {}", rich_inline_text(ws, element)))],
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
        Some("text") => {
            let raw = leaf.get("text").and_then(Value::as_str).unwrap_or_default();
            apply_mrkdwn_style(raw, leaf.get("style"))
        }
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

fn apply_mrkdwn_style(raw: &str, style: Option<&Value>) -> String {
    let Some(style) = style else {
        return raw.to_owned();
    };
    if raw.is_empty() {
        return raw.to_owned();
    }
    let flag = |key: &str| style.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mut out = raw.to_owned();
    if flag("code") {
        return format!("`{out}`");
    }
    if flag("bold") {
        out = format!("*{out}*");
    }
    if flag("italic") {
        out = format!("_{out}_");
    }
    if flag("strike") {
        out = format!("~{out}~");
    }
    out
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
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            priority_scores: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
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

    #[test]
    fn renders_inline_styles_and_code_block() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [
                {
                    "type": "rich_text_section",
                    "elements": [
                        {"type": "text", "text": "run "},
                        {"type": "text", "text": "cargo test", "style": {"code": true}},
                        {"type": "text", "text": " "},
                        {"type": "text", "text": "now", "style": {"bold": true}},
                        {"type": "text", "text": " "},
                        {"type": "text", "text": "maybe", "style": {"italic": true, "strike": true}}
                    ]
                },
                {
                    "type": "rich_text_preformatted",
                    "elements": [{"type": "text", "text": "let x = 1;"}]
                }
            ]
        })];

        assert_eq!(
            lines(&ws(), &blocks),
            vec!["run `cargo test` *now* ~_maybe_~", "let x = 1;"]
        );
    }

    #[test]
    fn render_lines_marks_preformatted_as_mono() {
        let msg = crate::slack::models::Message {
            blocks: vec![json!({
                "type": "rich_text",
                "elements": [
                    {
                        "type": "rich_text_section",
                        "elements": [{"type": "text", "text": "prose"}]
                    },
                    {
                        "type": "rich_text_preformatted",
                        "elements": [{"type": "text", "text": "fn main() {}\n    indented"}]
                    }
                ]
            })],
            ..Default::default()
        };

        let rendered = render_lines(&ws(), &msg);
        assert_eq!(
            rendered,
            vec![
                RenderLine {
                    text: "prose".into(),
                    mono: false
                },
                RenderLine {
                    text: "fn main() {}".into(),
                    mono: true
                },
                RenderLine {
                    text: "    indented".into(),
                    mono: true
                },
            ]
        );
    }
}
