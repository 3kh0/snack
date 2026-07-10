use serde_json::Value;

use crate::state::{self, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderLine {
    pub text: String,
    pub mono: bool,
    pub segments: Vec<RenderSegment>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderSegment {
    pub text: String,
    pub style: SegmentStyle,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegmentStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub code: bool,
    pub link: bool,
    pub mention: bool,
    pub broadcast: bool,
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

pub fn mrkdwn_lines(ws: &Workspace, text: &str) -> Vec<RenderLine> {
    text.lines()
        .map(|line| RenderLine::from_segments(mrkdwn_segments(ws, line), false))
        .collect()
}

fn render_blocks(ws: &Workspace, blocks: &[Value]) -> Vec<RenderLine> {
    blocks
        .iter()
        .flat_map(|block| block_lines(ws, block))
        .map(|mut line| {
            if !line.mono {
                line.text = line.text.trim().to_owned();
                line.segments = trim_segments(line.segments);
            }
            line
        })
        .filter(|line| !line.text.trim().is_empty())
        .collect()
}

fn plain(text: String) -> RenderLine {
    RenderLine::from_segments(vec![RenderSegment::plain(text)], false)
}

impl RenderSegment {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: SegmentStyle::default(),
            channel: None,
        }
    }

    fn styled(text: impl Into<String>, style: SegmentStyle) -> Self {
        Self::styled_channel(text, style, None)
    }

    fn styled_channel(
        text: impl Into<String>,
        style: SegmentStyle,
        channel: Option<String>,
    ) -> Self {
        Self {
            text: text.into(),
            style,
            channel,
        }
    }
}

impl RenderLine {
    fn from_segments(segments: Vec<RenderSegment>, mono: bool) -> Self {
        let text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        Self {
            text,
            mono,
            segments,
        }
    }
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
        Some("rich_text_section") => split_segments_on_newlines(&rich_inline_segments(ws, element))
            .into_iter()
            .map(|segments| RenderLine::from_segments(segments, false))
            .collect(),
        Some("rich_text_list") => element
            .get("elements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|item| {
                let mut segments = vec![RenderSegment::plain("- ")];
                segments.extend(rich_inline_segments(ws, item));
                split_segments_on_newlines(&segments)
                    .into_iter()
                    .map(|segments| RenderLine::from_segments(segments, false))
            })
            .collect(),
        Some("rich_text_preformatted") => rich_inline_segments(ws, element)
            .into_iter()
            .map(|mut segment| {
                segment.style.code = true;
                segment
            })
            .collect::<Vec<_>>()
            .split(|segment| segment.text == "\n")
            .flat_map(split_segments_on_newlines)
            .map(|segments| RenderLine::from_segments(segments, true))
            .collect(),
        Some("rich_text_quote") => {
            let mut segments = vec![RenderSegment::plain("> ")];
            segments.extend(rich_inline_segments(ws, element));
            split_segments_on_newlines(&segments)
                .into_iter()
                .map(|segments| RenderLine::from_segments(segments, false))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn rich_inline_segments(ws: &Workspace, element: &Value) -> Vec<RenderSegment> {
    element
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|child| rich_leaf_segments(ws, child))
        .collect()
}

fn rich_leaf_segments(ws: &Workspace, leaf: &Value) -> Vec<RenderSegment> {
    let style = segment_style(leaf.get("style"));
    match value_type(leaf) {
        Some("text") => {
            let raw = leaf.get("text").and_then(Value::as_str).unwrap_or_default();
            vec![RenderSegment::styled(raw.to_owned(), style)]
        }
        Some("emoji") => leaf
            .get("name")
            .and_then(Value::as_str)
            .map(state::emoji_glyph)
            .map(|text| RenderSegment::styled(text, style))
            .into_iter()
            .collect(),
        Some("user") => leaf
            .get("user_id")
            .or_else(|| leaf.get("user"))
            .and_then(Value::as_str)
            .map(|user| {
                let mut style = style.clone();
                style.mention = true;
                RenderSegment::styled(format!("@{}", ws.display_name(user)), style)
            })
            .into_iter()
            .collect(),
        Some("channel") => leaf
            .get("channel_id")
            .or_else(|| leaf.get("channel"))
            .and_then(Value::as_str)
            .map(|channel| {
                let label = ws
                    .channels
                    .get(channel)
                    .map(state::channel_label)
                    .unwrap_or_else(|| format!("#{channel}"));
                let mut style = style.clone();
                style.mention = true;
                RenderSegment::styled_channel(label, style, Some(channel.to_owned()))
            })
            .into_iter()
            .collect(),
        Some("link") | Some("message_mention") => {
            let text = leaf
                .get("text")
                .or_else(|| leaf.get("url"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut style = style;
            style.link = true;
            vec![RenderSegment::styled(text.to_owned(), style)]
        }
        Some("broadcast") => leaf
            .get("range")
            .and_then(Value::as_str)
            .map(|range| {
                let mut style = style.clone();
                style.mention = true;
                style.broadcast = true;
                RenderSegment::styled(format!("@{range}"), style)
            })
            .into_iter()
            .collect(),
        Some("usergroup") => leaf
            .get("usergroup_id")
            .or_else(|| leaf.get("usergroup"))
            .and_then(Value::as_str)
            .map(|group| {
                let mut style = style.clone();
                style.mention = true;
                RenderSegment::styled(format!("@{group}"), style)
            })
            .into_iter()
            .collect(),
        Some("date") => leaf
            .get("fallback")
            .and_then(Value::as_str)
            .map(|fallback| RenderSegment::styled(fallback.to_owned(), style))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn split_segments_on_newlines(segments: &[RenderSegment]) -> Vec<Vec<RenderSegment>> {
    let mut lines = vec![Vec::new()];
    for segment in segments {
        let mut parts = segment.text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                let mut segment = segment.clone();
                segment.text = part.to_owned();
                lines.last_mut().unwrap().push(segment);
            }
            if parts.peek().is_some() {
                lines.push(Vec::new());
            }
        }
    }
    lines
}

fn trim_segments(segments: Vec<RenderSegment>) -> Vec<RenderSegment> {
    let mut start = 0;
    let mut end = segments.len();
    while start < end && segments[start].text.trim().is_empty() {
        start += 1;
    }
    while end > start && segments[end - 1].text.trim().is_empty() {
        end -= 1;
    }
    let mut out = segments[start..end].to_vec();
    if let Some(first) = out.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = out.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    out
}

fn segment_style(style: Option<&Value>) -> SegmentStyle {
    let Some(style) = style else {
        return SegmentStyle::default();
    };
    let flag = |key: &str| style.get(key).and_then(Value::as_bool).unwrap_or(false);
    SegmentStyle {
        bold: flag("bold"),
        italic: flag("italic"),
        strike: flag("strike"),
        underline: flag("underline"),
        code: flag("code"),
        link: false,
        mention: false,
        broadcast: false,
    }
}

fn mrkdwn_segments(ws: &Workspace, text: &str) -> Vec<RenderSegment> {
    let mut out = Vec::new();
    let mut style = SegmentStyle::default();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some((token, len)) = rest
            .strip_prefix('<')
            .and_then(|_| parse_slack_ref(ws, rest))
        {
            out.push(RenderSegment::styled_channel(token.0, token.1, token.2));
            i += len;
            continue;
        }
        if let Some((next_style, len)) = toggle_style(rest, &style) {
            style = next_style;
            i += len;
            continue;
        }

        let next = rest
            .char_indices()
            .skip(1)
            .find(|(_, ch)| matches!(ch, '<' | '*' | '_' | '~' | '`'))
            .map(|(index, _)| index)
            .unwrap_or(rest.len());
        out.push(RenderSegment::styled(
            rest[..next].to_owned(),
            style.clone(),
        ));
        i += next;
    }
    merge_segments(out)
}

fn parse_slack_ref(
    ws: &Workspace,
    text: &str,
) -> Option<((String, SegmentStyle, Option<String>), usize)> {
    let end = text.find('>')?;
    let raw = &text[1..end];
    let (label, mut style, channel_target) = if let Some(user) = raw.strip_prefix('@') {
        (
            format!(
                "@{}",
                ws.display_name(user.split('|').next().unwrap_or(user))
            ),
            SegmentStyle {
                mention: true,
                ..SegmentStyle::default()
            },
            None,
        )
    } else if let Some(channel) = raw.strip_prefix('#') {
        let (id, fallback_label) = channel.split_once('|').unwrap_or((channel, channel));
        (
            ws.channels
                .get(id)
                .map(state::channel_label)
                .unwrap_or_else(|| format!("#{}", fallback_label.trim_start_matches('#'))),
            SegmentStyle {
                mention: true,
                ..SegmentStyle::default()
            },
            Some(id.to_owned()),
        )
    } else if let Some(broadcast) = raw.strip_prefix('!') {
        (
            format!("@{}", broadcast.split('|').next().unwrap_or(broadcast)),
            SegmentStyle {
                mention: true,
                broadcast: true,
                ..SegmentStyle::default()
            },
            None,
        )
    } else {
        let (url, label) = raw.split_once('|').unwrap_or((raw, raw));
        (
            label.to_owned(),
            SegmentStyle {
                link: state::is_browser_url(url),
                ..SegmentStyle::default()
            },
            None,
        )
    };
    if !style.link && state::is_browser_url(&label) {
        style.link = true;
    }
    Some(((label, style, channel_target), end + 1))
}

/// Returns every channel referenced by a Slack message's rich text or fallback
/// mrkdwn. The update layer uses this to fill channel names not included in the
/// initial sidebar payload.
pub fn mentioned_channel_ids(msg: &crate::slack::models::Message) -> Vec<String> {
    let mut ids = Vec::new();
    for block in &msg.blocks {
        collect_channel_ids(block, &mut ids);
    }
    if let Some(text) = &msg.text {
        collect_mrkdwn_channel_ids(text, &mut ids);
    }
    ids
}

fn collect_channel_ids(value: &Value, ids: &mut Vec<String>) {
    if value_type(value) == Some("channel") {
        if let Some(id) = value
            .get("channel_id")
            .or_else(|| value.get("channel"))
            .and_then(Value::as_str)
        {
            push_channel_id(ids, id);
        }
    }
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if key == "text" {
                    if let Some(text) = value.as_str() {
                        collect_mrkdwn_channel_ids(text, ids);
                    }
                }
                collect_channel_ids(value, ids);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_channel_ids(value, ids);
            }
        }
        _ => {}
    }
}

fn collect_mrkdwn_channel_ids(text: &str, ids: &mut Vec<String>) {
    let mut rest = text;
    while let Some(start) = rest.find("<#") {
        let Some(end) = rest[start + 2..].find('>') else {
            break;
        };
        let raw = &rest[start + 2..start + 2 + end];
        let id = raw.split_once('|').map_or(raw, |(id, _)| id);
        push_channel_id(ids, id);
        rest = &rest[start + 3 + end..];
    }
}

fn push_channel_id(ids: &mut Vec<String>, id: &str) {
    if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
        ids.push(id.to_owned());
    }
}

fn toggle_style(text: &str, current: &SegmentStyle) -> Option<(SegmentStyle, usize)> {
    let (delimiter, set): (char, fn(&mut SegmentStyle, bool)) =
        if text.starts_with('*') && (current.bold || text[1..].contains('*')) {
            ('*', |style, value| style.bold = value)
        } else if text.starts_with('_') && (current.italic || text[1..].contains('_')) {
            ('_', |style, value| style.italic = value)
        } else if text.starts_with('~') && (current.strike || text[1..].contains('~')) {
            ('~', |style, value| style.strike = value)
        } else if text.starts_with('`') && (current.code || text[1..].contains('`')) {
            ('`', |style, value| style.code = value)
        } else {
            return None;
        };
    let mut next = current.clone();
    match delimiter {
        '*' => set(&mut next, !current.bold),
        '_' => set(&mut next, !current.italic),
        '~' => set(&mut next, !current.strike),
        '`' => set(&mut next, !current.code),
        _ => {}
    }
    Some((next, delimiter.len_utf8()))
}

fn merge_segments(segments: Vec<RenderSegment>) -> Vec<RenderSegment> {
    let mut merged: Vec<RenderSegment> = Vec::new();
    for segment in segments
        .into_iter()
        .filter(|segment| !segment.text.is_empty())
    {
        match merged.last_mut() {
            Some(existing)
                if existing.style == segment.style && existing.channel == segment.channel =>
            {
                existing.text.push_str(&segment.text)
            }
            _ => merged.push(segment),
        }
    }
    merged
}

fn text_object(value: &Value) -> Option<String> {
    value
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(state::emoji_text_to_display)
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
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
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
            custom_emoji: HashMap::new(),
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
            vec!["Hi @alice in #general 👋 example"]
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

        let rendered = render_blocks(&ws(), &blocks);
        assert_eq!(
            rendered
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["run cargo test now maybe", "let x = 1;"]
        );
        assert!(rendered[0].segments[1].style.code);
        assert!(rendered[0].segments[3].style.bold);
        assert!(rendered[0].segments[5].style.italic);
        assert!(rendered[0].segments[5].style.strike);
    }

    #[test]
    fn renders_fallback_mrkdwn_refs_and_styles_as_segments() {
        let rendered = mrkdwn_lines(
            &ws(),
            "hi <@U1> in <#C1|old-general> see <https://example.com|example> *bold* _em_ ~no~ `code` <!channel>",
        );

        assert_eq!(
            rendered[0]
                .segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "hi ", "@alice", " in ", "#general", " see ", "example", " ", "bold", " ", "em",
                " ", "no", " ", "code", " ", "@channel"
            ]
        );
        assert!(rendered[0].segments[1].style.mention);
        assert!(rendered[0].segments[3].style.mention);
        assert_eq!(rendered[0].segments[3].channel.as_deref(), Some("C1"));
        assert!(rendered[0].segments[5].style.link);
        assert!(rendered[0].segments[7].style.bold);
        assert!(rendered[0].segments[9].style.italic);
        assert!(rendered[0].segments[11].style.strike);
        assert!(rendered[0].segments[13].style.code);
        assert!(rendered[0].segments[15].style.mention);
        assert!(rendered[0].segments[15].style.broadcast);
    }

    #[test]
    fn finds_rich_text_and_mrkdwn_channel_mentions() {
        let msg = crate::slack::models::Message {
            text: Some("also <#C2|random> and <#C1>".into()),
            blocks: vec![json!({
                "type": "rich_text",
                "elements": [{
                    "type": "rich_text_section",
                    "elements": [{"type": "channel", "channel_id": "C1"}]
                }]
            })],
            ..Default::default()
        };

        assert_eq!(mentioned_channel_ids(&msg), vec!["C1", "C2"]);
    }

    #[test]
    fn channel_mention_keeps_target_id_and_prefers_workspace_name() {
        let rendered = mrkdwn_lines(&ws(), "ping <#C1|old-general> now");
        assert_eq!(
            rendered[0]
                .segments
                .iter()
                .map(|s| (s.text.as_str(), s.channel.as_deref(), s.style.mention))
                .collect::<Vec<_>>(),
            vec![
                ("ping ", None, false),
                ("#general", Some("C1"), true),
                (" now", None, false),
            ]
        );

        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_section",
                "elements": [
                    {"type": "text", "text": "see "},
                    {"type": "channel", "channel_id": "C1"}
                ]
            }]
        })];
        let rich = render_blocks(&ws(), &blocks);
        assert_eq!(rich[0].segments.len(), 2);
        assert_eq!(rich[0].segments[1].text, "#general");
        assert_eq!(rich[0].segments[1].channel.as_deref(), Some("C1"));
        assert!(rich[0].segments[1].style.mention);
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
            rendered
                .iter()
                .map(|line| (line.text.as_str(), line.mono))
                .collect::<Vec<_>>(),
            vec![
                ("prose", false),
                ("fn main() {}", true),
                ("    indented", true)
            ]
        );
        assert!(
            rendered[1]
                .segments
                .iter()
                .all(|segment| segment.style.code)
        );
    }
}
