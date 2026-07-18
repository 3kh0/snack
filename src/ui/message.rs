use std::collections::HashMap;
use std::f32::consts::TAU;
use std::time::Duration;

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{Column, Row, button, container, image, stack, svg, text, text_input};
use iced::{Alignment, Color, ContentFit, Element, Fill, Font, Length, Point};
use unicode_segmentation::UnicodeSegmentation;

use super::{blocks, composer, icons, selectable, theme};
use crate::app::{ComposerAttachment, FilePreview, Message, TextSelection, TextSelectionSurface};
use crate::slack::models::Message as SlackMessage;
use crate::state::{self, Workspace};

pub fn row<'a>(
    ws: &Workspace,
    channel_id: &str,
    msg: &SlackMessage,
    pending: bool,
    compact: bool,
    in_thread: bool,
    hovered: bool,
    file_previews: &HashMap<String, FilePreview>,
    avatar_previews: &HashMap<String, FilePreview>,
    emoji_previews: &HashMap<String, FilePreview>,
    emoji_animation_elapsed: Duration,
    edit_text: Option<&str>,
    selection_surface: TextSelectionSurface,
    message_index: usize,
    text_selection: Option<&TextSelection>,
    pending_attachments: Option<&'a [ComposerAttachment]>,
) -> Element<'a, Message> {
    let author = ws.message_author_name(msg);

    let time = msg
        .ts
        .as_deref()
        .map(state::format_ts_hm)
        .unwrap_or_default();

    let mut header = Row::new()
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center)
        .push(
            text(author.clone())
                .size(theme::TEXT_MD)
                .color(theme::TEXT_1)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Font::default()
                }),
        );

    if is_app_message(msg) {
        header = header.push(
            container(text("APP").size(10.0).font(Font {
                weight: iced::font::Weight::Semibold,
                ..Font::default()
            }))
            .padding([1.0, 4.0])
            .style(theme::app_badge),
        );
    }

    header = header.push(text(time).size(theme::TEXT_SM).color(theme::TEXT_5));

    if msg.edited.is_some() {
        header = header.push(text("(edited)").size(theme::TEXT_SM).color(theme::MUTED));
    }
    if pending {
        header = header.push(sending_clock(emoji_animation_elapsed));
    }

    let block_lines = blocks::render_lines(ws, msg);
    let copy_text = selectable_copy_text_from_lines(ws, msg, &block_lines);

    let editable = !pending
        && msg.bot_id.is_none()
        && msg.ts.is_some()
        && msg.user.as_deref() == Some(ws.self_user_id.as_str());

    let thread_ts = thread_target_ts(msg);
    let can_reply = !in_thread && thread_ts.is_some();

    let action_bar: Option<Element<'a, Message>> = if hovered && edit_text.is_none() {
        let can_copy = !copy_text.is_empty();
        let edit_ts = editable.then(|| msg.ts.clone()).flatten();
        (can_reply || can_copy || edit_ts.is_some()).then(|| {
            let copy_text = copy_text.clone();
            let channel_id = channel_id.to_owned();
            let reply_ts = thread_ts.clone().filter(|_| can_reply);
            super::motion::micro_reveal(true, move |anim, at| {
                let progress = super::motion::t(anim, at);
                let mut actions = Row::new();
                if let Some(ts) = reply_ts.clone() {
                    actions = actions.push(action_item(
                        "Reply",
                        Message::ThreadOpened {
                            channel: channel_id.clone(),
                            ts,
                            unread_range: None,
                        },
                    ));
                }
                if can_copy {
                    actions =
                        actions.push(action_item("Copy", Message::CopyMessage(copy_text.clone())));
                }
                if let Some(ts) = edit_ts.clone() {
                    actions = actions
                        .push(action_item(
                            "Edit",
                            Message::EditPressed {
                                channel: channel_id.clone(),
                                ts: ts.clone(),
                            },
                        ))
                        .push(action_item(
                            "Delete",
                            Message::DeletePressed {
                                channel: channel_id.clone(),
                                ts,
                            },
                        ));
                }
                let bar = container(actions.padding(2))
                    .padding(2)
                    .style(theme::action_bar);
                super::motion::slide_y(bar.into(), progress, -4.0)
            })
            .into()
        })
    } else {
        None
    };

    if let Some(value) = edit_text {
        let input = text_input("Edit message", value)
            .on_input(Message::EditComposerChanged)
            .on_submit(Message::EditSubmit)
            .style(theme::input)
            .size(theme::TEXT_MD)
            .padding(theme::SPACE_SM)
            .width(Length::Fixed(360.0));
        let actions = Row::new()
            .spacing(theme::SPACE_SM)
            .push(
                button(text("Save").size(theme::TEXT_SM))
                    .padding([2, 0])
                    .style(theme::link_button)
                    .on_press(Message::EditSubmit),
            )
            .push(
                button(text("Cancel").size(theme::TEXT_SM))
                    .padding([2, 0])
                    .style(theme::link_button)
                    .on_press(Message::EditCancelled),
            );
        let content = Column::new()
            .spacing(theme::SPACE_XS)
            .push(header)
            .push(input)
            .push(actions);
        let (avatar_key, avatar_url) = ws.message_avatar(msg);
        let avatar = avatar(
            avatar_key.as_deref(),
            avatar_url.as_deref(),
            avatar_previews,
            author.chars().next(),
        );
        return container(
            Row::new()
                .spacing(theme::SPACE_SM)
                .align_y(Alignment::Start)
                .push(avatar)
                .push(container(content).width(Fill)),
        )
        .padding([theme::SPACE_XS / 2.0, theme::SPACE_SM])
        .into();
    }

    let mut col = Column::new().spacing(2.0);
    if !compact {
        col = col.push(header);
    }
    let body_lines = body_lines(ws, msg, block_lines);
    if !body_lines.is_empty() {
        if body_lines
            .iter()
            .any(|line| line_has_custom_emoji(ws, &line.text))
        {
            col = col.push(emoji_body(
                &body_lines,
                ws,
                emoji_previews,
                emoji_animation_elapsed,
            ));
        } else {
            // Body renders through the selectable widget so text can be dragged
            // over and copied (iced's plain `text` cannot be selected).
            let mut segments = Vec::new();
            for (i, line) in body_lines.into_iter().enumerate() {
                if i > 0 {
                    segments.push(selectable::Segment::plain("\n"));
                }
                segments.extend(selectable_segments(&line));
            }
            let selected_range = msg.ts.as_deref().and_then(|ts| {
                selection_range_for_message(
                    text_selection,
                    &selection_surface,
                    ts,
                    message_index,
                    selectable_span_count(&segments),
                )
            });
            let selection_active = text_selection.is_some_and(|selection| {
                selection.dragging && selection.anchor.surface == selection_surface
            });
            let mut body = selectable::SelectableText::new(
                &segments,
                theme::TEXT_MD,
                theme::TEXT_2,
                theme::selection(),
            )
            .selection(selected_range);
            if let Some(ts) = msg.ts.clone() {
                body = body.context(
                    selection_surface.clone(),
                    ts,
                    message_index,
                    selection_active,
                );
            }
            col = col.push(body);
        }
    }

    for file in &msg.files {
        col = col.push(file_row(file, file_previews, hovered));
    }

    for att in &msg.attachments {
        col = col.push(attachment_row(att, file_previews));
    }

    if let Some(attachments) = pending_attachments {
        col = col.push(composer::pending_attachment_strip(attachments));
    }

    if let (Some(ts), Some(count)) = (thread_ts.filter(|_| !in_thread), msg.reply_count) {
        if count > 0 {
            col = col.push(thread_summary(
                ws,
                msg,
                avatar_previews,
                channel_id,
                &ts,
                count,
            ));
        }
    }

    if !msg.reactions.is_empty() {
        let mut chips = Row::new().spacing(theme::SPACE_XS);
        for r in &msg.reactions {
            let active = state::reaction_has_user(r, &ws.self_user_id);
            let chip: Element<'a, Message> = if let Some(ts) = msg.ts.clone() {
                button(reaction_content(
                    ws,
                    r,
                    emoji_previews,
                    emoji_animation_elapsed,
                ))
                .padding([2, 6])
                .style(theme::reaction_button(active))
                .on_press(Message::ReactionPressed {
                    channel: channel_id.to_owned(),
                    ts,
                    name: r.name.clone(),
                })
                .into()
            } else {
                container(reaction_content(
                    ws,
                    r,
                    emoji_previews,
                    emoji_animation_elapsed,
                ))
                .padding([2, 6])
                .style(theme::reaction_chip)
                .into()
            };
            chips = chips.push(chip);
        }
        col = col.push(chips);
    }

    let content: Element<'a, Message> = if compact && pending {
        Row::new()
            .spacing(theme::SPACE_XS)
            .align_y(Alignment::Start)
            .push(sending_clock(emoji_animation_elapsed))
            .push(container(col).width(Fill))
            .into()
    } else {
        container(col).width(Fill).into()
    };
    let body = container(
        Row::new()
            .spacing(theme::SPACE_SM)
            .align_y(Alignment::Start)
            .push(if compact {
                avatar_spacer()
            } else {
                let (avatar_key, avatar_url) = ws.message_avatar(msg);
                avatar(
                    avatar_key.as_deref(),
                    avatar_url.as_deref(),
                    avatar_previews,
                    author.chars().next(),
                )
            })
            .push(content),
    )
    .padding([if compact { 1.0 } else { theme::SPACE_XS }, theme::SPACE_SM])
    .width(Fill);

    match action_bar {
        Some(bar) => iced::widget::stack![
            body,
            container(bar)
                .width(Fill)
                .align_right(Fill)
                .padding([0.0, theme::SPACE_MD]),
        ]
        .into(),
        None => body.into(),
    }
}

fn is_app_message(msg: &SlackMessage) -> bool {
    msg.subtype.as_deref() == Some("bot_message")
}

fn thread_target_ts(msg: &SlackMessage) -> Option<String> {
    match (msg.thread_ts.as_deref(), msg.ts.as_deref()) {
        (Some(root), Some(ts)) if root != ts => Some(root.to_owned()),
        (_, Some(ts)) => Some(ts.to_owned()),
        _ => None,
    }
}

fn thread_summary<'a>(
    ws: &Workspace,
    msg: &SlackMessage,
    avatar_previews: &HashMap<String, FilePreview>,
    channel_id: &str,
    ts: &str,
    count: u32,
) -> Element<'a, Message> {
    let mut content = Row::new()
        .spacing(theme::SPACE_XS)
        .align_y(Alignment::Center);

    let users = thread_participants(msg);
    if !users.is_empty() {
        let mut avatars = Row::new().spacing(2).align_y(Alignment::Center);
        for user in users {
            avatars = avatars.push(thread_participant_avatar(ws, avatar_previews, user));
        }
        content = content.push(avatars);
    }

    content = content.push(
        text(format!(
            "{count} repl{}",
            if count == 1 { "y" } else { "ies" }
        ))
        .size(theme::TEXT_SM),
    );

    button(content)
        .padding([2, 0])
        .style(theme::link_button)
        .on_press(Message::ThreadOpened {
            channel: channel_id.to_owned(),
            ts: ts.to_owned(),
            unread_range: None,
        })
        .into()
}

fn thread_participants(msg: &SlackMessage) -> Vec<&str> {
    let mut users = Vec::new();
    for user in &msg.reply_users {
        if users.len() >= 5 {
            break;
        }
        if !users.contains(&user.as_str()) {
            users.push(user.as_str());
        }
    }
    users
}

fn thread_participant_avatar<'a>(
    ws: &Workspace,
    avatar_previews: &HashMap<String, FilePreview>,
    user: &str,
) -> Element<'a, Message> {
    let fallback = ws.display_name(user).chars().next();
    avatar_with_size(
        Some(user),
        ws.avatar_url(user).as_deref(),
        avatar_previews,
        fallback,
        20.0,
        5.0,
    )
}

fn avatar_spacer<'a>() -> Element<'a, Message> {
    iced::widget::Space::new()
        .width(Length::Fixed(theme::MSG_AVATAR))
        .into()
}

fn sending_clock<'a>(elapsed: Duration) -> Element<'a, Message> {
    let hour_end = clock_hand_end(elapsed, 2.6, 4.6);
    let minute_end = clock_hand_end(elapsed, 1.3, 5.7);
    let muted = svg_color(theme::MUTED);
    let hand = svg_color(theme::TEXT_4);
    let data = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none">
<circle cx="7" cy="7" r="5.8" stroke="{muted}" stroke-width="1.25"/>
<path d="M7 7 L{:.3} {:.3}" stroke="{hand}" stroke-width="1.8" stroke-linecap="round"/>
<path d="M7 7 L{:.3} {:.3}" stroke="{hand}" stroke-width="2.4" stroke-linecap="round"/>
<circle cx="7" cy="7" r="1.15" fill="{hand}"/>
</svg>"##,
        hour_end.x, hour_end.y, minute_end.x, minute_end.y
    );
    container(
        svg(svg::Handle::from_memory(data.into_bytes()))
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0)),
    )
    .into()
}

fn clock_hand_end(elapsed: Duration, period_secs: f32, length: f32) -> Point {
    let angle = elapsed.as_secs_f32() / period_secs * TAU - TAU / 4.0;
    Point::new(7.0 + angle.cos() * length, 7.0 + angle.sin() * length)
}

fn svg_color(color: Color) -> String {
    let red = (color.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let green = (color.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let blue = (color.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{red:02x}{green:02x}{blue:02x}")
}

fn body_lines(
    ws: &Workspace,
    msg: &SlackMessage,
    block_lines: Vec<blocks::RenderLine>,
) -> Vec<blocks::RenderLine> {
    if !block_lines.is_empty() {
        return block_lines;
    }
    let body = state::message_text(msg);
    if body.is_empty() {
        Vec::new()
    } else {
        blocks::mrkdwn_lines(ws, &body)
    }
}

pub fn selectable_copy_text(ws: &Workspace, msg: &SlackMessage) -> String {
    let block_lines = blocks::render_lines(ws, msg);
    selectable_copy_text_from_lines(ws, msg, &block_lines)
}

fn selectable_copy_text_from_lines(
    ws: &Workspace,
    msg: &SlackMessage,
    block_lines: &[blocks::RenderLine],
) -> String {
    body_lines(ws, msg, block_lines.to_vec())
        .into_iter()
        .map(|line| {
            if line.segments.is_empty() {
                state::emoji_text_to_display(&line.text)
            } else {
                line.segments
                    .iter()
                    .map(|segment| state::emoji_text_to_display(&segment.text))
                    .collect()
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn selectable_span_count(segments: &[selectable::Segment]) -> usize {
    segments
        .iter()
        .map(|segment| segment.text.graphemes(true).count())
        .sum()
}

fn selection_range_for_message(
    selection: Option<&TextSelection>,
    surface: &TextSelectionSurface,
    ts: &str,
    index: usize,
    len: usize,
) -> Option<(usize, usize)> {
    let selection = selection?;
    if len == 0
        || &selection.anchor.surface != surface
        || &selection.focus.surface != surface
        || (selection.anchor.message_ts != ts && selection.focus.message_ts != ts)
            && (index
                < selection
                    .anchor
                    .message_index
                    .min(selection.focus.message_index)
                || index
                    > selection
                        .anchor
                        .message_index
                        .max(selection.focus.message_index))
    {
        return None;
    }

    let anchor = &selection.anchor;
    let focus = &selection.focus;
    let start_index = anchor.message_index.min(focus.message_index);
    let end_index = anchor.message_index.max(focus.message_index);
    if index < start_index || index > end_index {
        return None;
    }

    let anchor_offset = anchor.offset.min(len - 1);
    let focus_offset = focus.offset.min(len - 1);
    let forward = anchor.message_index < focus.message_index
        || (anchor.message_index == focus.message_index && anchor.offset <= focus.offset);

    if anchor.message_index == focus.message_index {
        return Some((
            anchor_offset.min(focus_offset),
            anchor_offset.max(focus_offset),
        ));
    }
    if index != anchor.message_index && index != focus.message_index {
        return Some((0, len - 1));
    }
    if forward {
        if index == anchor.message_index {
            Some((anchor_offset, len - 1))
        } else {
            Some((0, focus_offset))
        }
    } else if index == focus.message_index {
        Some((focus_offset, len - 1))
    } else {
        Some((0, anchor_offset))
    }
}

fn line_has_custom_emoji(ws: &Workspace, line: &str) -> bool {
    state::emoji_text_tokens(line).into_iter().any(|token| {
        matches!(
            token,
            state::EmojiTextToken::Emoji(name) if ws.custom_emoji_url(&name).is_some()
        )
    })
}

fn selectable_segments(line: &blocks::RenderLine) -> Vec<selectable::Segment> {
    if line.segments.is_empty() {
        return vec![selectable::Segment {
            text: state::emoji_text_to_display(&line.text),
            channel: None,
            mono: line.mono,
            color: None,
            background: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }];
    }
    line.segments
        .iter()
        .map(|segment| {
            let style = &segment.style;
            selectable::Segment {
                text: state::emoji_text_to_display(&segment.text),
                channel: segment.channel.clone(),
                mono: line.mono || style.code,
                color: segment_fg(style),
                background: segment_bg(style),
                bold: style.bold,
                italic: style.italic,
                underline: style.underline,
                strikethrough: style.strike,
            }
        })
        .collect()
}

fn emoji_body<'a>(
    lines: &[blocks::RenderLine],
    ws: &Workspace,
    emoji_previews: &HashMap<String, FilePreview>,
    elapsed: Duration,
) -> Element<'a, Message> {
    let mut col = Column::new().spacing(theme::SPACE_XS);
    for line in lines {
        for soft_line in soft_wrap_lines(line) {
            let mut row = Row::new().spacing(0).align_y(Alignment::End).width(Fill);
            for (text_value, mono, style, channel) in &soft_line.parts {
                match text_value {
                    SoftPart::Text(value) if !value.is_empty() => {
                        for run in text_runs(value) {
                            row = row.push(text_run(run, *mono, style, channel.as_deref()));
                        }
                    }
                    SoftPart::Text(_) => {}
                    SoftPart::Emoji(name) => {
                        row = row.push(emoji_inline(
                            ws,
                            name,
                            emoji_previews,
                            elapsed,
                            theme::TEXT_MD,
                        ));
                    }
                }
            }
            col = col.push(row.wrap().vertical_spacing(2));
        }
    }
    col.into()
}

enum SoftPart {
    Text(String),
    Emoji(String),
}

struct SoftLine {
    parts: Vec<(SoftPart, bool, blocks::SegmentStyle, Option<String>)>,
}

fn soft_wrap_lines(line: &blocks::RenderLine) -> Vec<SoftLine> {
    let mut rows = vec![SoftLine { parts: Vec::new() }];
    for segment in line_segments(line) {
        let mono = line.mono || segment.style.code;
        for token in state::emoji_text_tokens(&segment.text) {
            match token {
                state::EmojiTextToken::Text(value) => {
                    let mut parts = value.split('\n').peekable();
                    while let Some(part) = parts.next() {
                        if !part.is_empty() {
                            rows.last_mut().unwrap().parts.push((
                                SoftPart::Text(part.to_owned()),
                                mono,
                                segment.style.clone(),
                                segment.channel.clone(),
                            ));
                        }
                        if parts.peek().is_some() {
                            rows.push(SoftLine { parts: Vec::new() });
                        }
                    }
                }
                state::EmojiTextToken::Emoji(name) => {
                    rows.last_mut().unwrap().parts.push((
                        SoftPart::Emoji(name),
                        mono,
                        segment.style.clone(),
                        segment.channel.clone(),
                    ));
                }
            }
        }
    }
    rows.retain(|row| !row.parts.is_empty());
    if rows.is_empty() {
        rows.push(SoftLine { parts: Vec::new() });
    }
    rows
}

fn segment_fg(style: &blocks::SegmentStyle) -> Option<iced::Color> {
    if style.broadcast {
        Some(theme::BROADCAST_FG)
    } else if style.mention {
        Some(theme::MENTION_FG)
    } else if style.link {
        Some(theme::accent_bright())
    } else {
        None
    }
}

fn segment_bg(style: &blocks::SegmentStyle) -> Option<iced::Color> {
    if style.broadcast {
        Some(theme::BROADCAST_BG)
    } else if style.mention {
        Some(theme::MENTION_BG)
    } else {
        None
    }
}

fn line_segments(line: &blocks::RenderLine) -> Vec<blocks::RenderSegment> {
    if line.segments.is_empty() {
        vec![blocks::RenderSegment {
            text: line.text.clone(),
            style: blocks::SegmentStyle::default(),
            channel: None,
        }]
    } else {
        line.segments.clone()
    }
}

fn text_runs(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = String::new();
    for c in value.chars() {
        run.push(c);
        if c.is_whitespace() {
            out.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        out.push(run);
    }
    out
}

fn text_run<'a>(
    value: String,
    mono: bool,
    style: &blocks::SegmentStyle,
    channel: Option<&str>,
) -> Element<'a, Message> {
    let mut font = if mono { Font::MONOSPACE } else { Font::DEFAULT };
    if style.bold {
        font.weight = iced::font::Weight::Bold;
    }
    if style.italic {
        font.style = iced::font::Style::Italic;
    }
    let styled = text(value)
        .size(theme::TEXT_MD)
        .font(font)
        .color(segment_fg(style).unwrap_or(theme::TEXT_2));
    match (segment_bg(style), channel) {
        (Some(_), Some(channel)) => button(styled)
            .padding([0.0, 3.0])
            .style(theme::inline_mention_button(style.broadcast))
            .on_press(Message::ChannelSelected(channel.to_owned()))
            .into(),
        (Some(_), None) => container(styled)
            .padding([0.0, 3.0])
            .style(theme::inline_mention(style.broadcast))
            .into(),
        (None, _) => styled.into(),
    }
}

fn reaction_content<'a>(
    ws: &Workspace,
    reaction: &crate::slack::models::Reaction,
    emoji_previews: &HashMap<String, FilePreview>,
    elapsed: Duration,
) -> Element<'a, Message> {
    Row::new()
        .spacing(theme::SPACE_XS)
        .align_y(Alignment::Center)
        .push(emoji_inline(
            ws,
            &reaction.name,
            emoji_previews,
            elapsed,
            theme::TEXT_SM,
        ))
        .push(text(reaction.count.max(1).to_string()).size(theme::TEXT_SM))
        .into()
}

pub fn inline_line<'a>(
    ws: &Workspace,
    line: &str,
    emoji_previews: &HashMap<String, FilePreview>,
    elapsed: Duration,
    size: f32,
    color: Color,
) -> Element<'a, Message> {
    let mut row = Row::new().spacing(0).align_y(Alignment::Center);
    for token in state::emoji_text_tokens(line) {
        match token {
            state::EmojiTextToken::Text(value) if !value.is_empty() => {
                row = row.push(
                    text(value)
                        .size(size)
                        .color(color)
                        .wrapping(text::Wrapping::None),
                );
            }
            state::EmojiTextToken::Text(_) => {}
            state::EmojiTextToken::Emoji(name) => {
                row = row.push(emoji_inline(ws, &name, emoji_previews, elapsed, size));
            }
        }
    }
    row.into()
}

fn emoji_inline<'a>(
    ws: &Workspace,
    name: &str,
    emoji_previews: &HashMap<String, FilePreview>,
    elapsed: Duration,
    size: f32,
) -> Element<'a, Message> {
    if ws.custom_emoji_url(name).is_some() {
        let key = state::emoji_preview_key(&ws.team_id, name);
        match emoji_previews.get(&key) {
            Some(FilePreview::Loaded(handle)) => {
                return emoji_image(handle.clone(), size);
            }
            Some(FilePreview::Animated {
                frames,
                delays,
                total,
            }) => {
                if let Some(handle) = animated_frame(frames, delays, *total, elapsed) {
                    return emoji_image(handle, size);
                }
            }
            _ => {}
        }
    }
    text(state::emoji_glyph(name))
        .size(size)
        .color(theme::TEXT_2)
        .into()
}

fn emoji_image<'a>(handle: ImageHandle, size: f32) -> Element<'a, Message> {
    image::Image::new(handle)
        .width(Length::Fixed(size + 2.0))
        .height(Length::Fixed(size + 2.0))
        .content_fit(ContentFit::Contain)
        .into()
}

fn animated_frame(
    frames: &[ImageHandle],
    delays: &[Duration],
    total: Duration,
    elapsed: Duration,
) -> Option<ImageHandle> {
    if frames.is_empty() || total.is_zero() {
        return None;
    }
    let elapsed_ms = elapsed.as_millis() % total.as_millis().max(1);
    let mut cursor = 0u128;
    for (index, delay) in delays.iter().enumerate() {
        cursor += delay.as_millis().max(1);
        if elapsed_ms < cursor {
            return frames.get(index).cloned();
        }
    }
    frames.last().cloned()
}

fn action_item<'a>(label: &'a str, on_press: Message) -> Element<'a, Message> {
    button(text(label).size(theme::TEXT_SM))
        .padding([2.0, theme::SPACE_SM])
        .style(theme::action_button)
        .on_press(on_press)
        .into()
}

pub fn empty_placeholder<'a>(label: &str) -> Element<'a, Message> {
    container(
        text(label.to_owned())
            .size(theme::TEXT_MD)
            .color(theme::MUTED),
    )
    .padding(theme::SPACE_LG)
    .into()
}

fn attachment_row<'a>(
    att: &crate::slack::models::Attachment,
    file_previews: &HashMap<String, FilePreview>,
) -> Element<'a, Message> {
    let mut content = Column::new().spacing(theme::SPACE_XS);

    if let Some(service) = non_empty(att.service_name.as_deref()) {
        content = content.push(
            text(service.to_owned())
                .size(theme::TEXT_SM)
                .color(theme::MUTED),
        );
    }
    if let Some(author) = non_empty(att.author_name.as_deref()) {
        content = content.push(
            text(author.to_owned())
                .size(theme::TEXT_SM)
                .color(theme::MUTED),
        );
    }
    if let Some(title) = non_empty(att.title.as_deref()) {
        let styled = text(title.to_owned()).size(theme::TEXT_MD).font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::default()
        });
        let widget: Element<'a, Message> = match non_empty(att.title_link.as_deref()) {
            Some(link) => button(styled.color(theme::accent()))
                .padding(0)
                .style(theme::link_button)
                .on_press(Message::OpenUrl(link.to_owned()))
                .into(),
            None => styled.into(),
        };
        content = content.push(widget);
    }
    if let Some(body) = non_empty(att.text.as_deref()) {
        content = content.push(text(body.to_owned()).size(theme::TEXT_SM));
    }
    for field in &att.fields {
        let title = non_empty(field.title.as_deref());
        let value = non_empty(field.value.as_deref());
        let line = match (title, value) {
            (Some(t), Some(v)) => format!("{t}: {v}"),
            (Some(t), None) => t.to_owned(),
            (None, Some(v)) => v.to_owned(),
            (None, None) => continue,
        };
        content = content.push(text(line).size(theme::TEXT_SM));
    }

    if let Some(preview) = state::attachment_preview_url(att).and_then(|url| file_previews.get(url))
    {
        if let FilePreview::Loaded(handle) = preview {
            content = content.push(
                image::Image::new(handle.clone())
                    .width(Length::Fixed(260.0))
                    .height(Length::Fixed(160.0))
                    .content_fit(ContentFit::Contain)
                    .border_radius(6.0),
            );
        }
    }

    container(content)
        .padding(theme::SPACE_SM)
        .style(theme::file_attachment)
        .into()
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn avatar<'a>(
    key: Option<&str>,
    url: Option<&str>,
    avatar_previews: &HashMap<String, FilePreview>,
    fallback: Option<char>,
) -> Element<'a, Message> {
    avatar_with_size(
        key,
        url,
        avatar_previews,
        fallback,
        theme::MSG_AVATAR,
        theme::MSG_AVATAR_RADIUS,
    )
}

pub fn avatar_with_size<'a>(
    key: Option<&str>,
    url: Option<&str>,
    avatar_previews: &HashMap<String, FilePreview>,
    fallback: Option<char>,
    size: f32,
    radius: f32,
) -> Element<'a, Message> {
    if let Some(key) = key {
        if url.is_some() {
            if let Some(FilePreview::Loaded(handle)) = avatar_previews.get(key) {
                return image::Image::new(handle.clone())
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size))
                    .content_fit(ContentFit::Cover)
                    .border_radius(radius)
                    .into();
            }
        }
    }

    let initial = fallback
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_owned());
    container(text(initial).size(theme::TEXT_SM).font(Font {
        weight: iced::font::Weight::Bold,
        ..Font::default()
    }))
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .center_x(Length::Fixed(size))
    .center_y(Length::Fixed(size))
    .style(theme::avatar_placeholder)
    .into()
}

fn file_row<'a>(
    file: &crate::slack::models::File,
    file_previews: &HashMap<String, FilePreview>,
    hovered: bool,
) -> Element<'a, Message> {
    let title = file
        .name
        .as_deref()
        .and_then(|name| non_empty(Some(name)))
        .map(str::to_owned)
        .unwrap_or_else(|| state::file_title(file));
    let (preview_width, preview_height) = file_preview_dimensions(file);
    let download = file
        .url_private
        .clone()
        .map(|url| Message::FileDownloadPressed {
            url,
            filename: state::file_download_name(file),
        });
    let mut content = Column::new()
        .spacing(theme::SPACE_XS)
        .push(text(title).size(theme::TEXT_SM).color(theme::TEXT_3));

    if let Some(preview) = state::file_preview_key(file).and_then(|key| file_previews.get(&key)) {
        match preview {
            FilePreview::Loaded(handle) => {
                let preview = image::Image::new(handle.clone())
                    .width(Length::Fixed(preview_width))
                    .height(Length::Fixed(preview_height))
                    .content_fit(ContentFit::Contain)
                    .border_radius(6.0)
                    .into();
                content = content.push(file_preview(
                    preview,
                    download.clone(),
                    hovered,
                    preview_width,
                    preview_height,
                ));
            }
            FilePreview::Loading => {
                content = content.push(
                    text("Loading preview...")
                        .size(theme::TEXT_SM)
                        .color(theme::MUTED),
                );
            }
            FilePreview::Failed => {
                content = content.push(
                    text("Preview unavailable")
                        .size(theme::TEXT_SM)
                        .color(theme::MUTED),
                );
            }
            FilePreview::Animated { frames, .. } => {
                if let Some(handle) = frames.first() {
                    let preview = image::Image::new(handle.clone())
                        .width(Length::Fixed(preview_width))
                        .height(Length::Fixed(preview_height))
                        .content_fit(ContentFit::Contain)
                        .border_radius(6.0)
                        .into();
                    content = content.push(file_preview(
                        preview,
                        download.clone(),
                        hovered,
                        preview_width,
                        preview_height,
                    ));
                }
            }
        }
    }

    content.into()
}

fn file_preview<'a>(
    preview: Element<'a, Message>,
    download: Option<Message>,
    hovered: bool,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    let Some(download) = download.filter(|_| hovered) else {
        return preview;
    };

    let icon = svg(icons::download())
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .style(theme::sidebar_icon(theme::TEXT_1));
    let action = container(
        button(icon)
            .padding(7.0)
            .style(theme::action_button)
            .on_press(download),
    )
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .padding(8.0)
    .align_right(Length::Fill)
    .align_bottom(Length::Fill);

    stack![preview, action].into()
}

fn file_preview_dimensions(file: &crate::slack::models::File) -> (f32, f32) {
    const MAX: f32 = 320.0;
    const FALLBACK: (f32, f32) = (260.0, 160.0);
    let dimension = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| file.extra.get(*key)?.as_f64())
            .filter(|value| *value > 0.0)
    };
    let Some(width) = dimension(&["original_w", "thumb_360_w"]) else {
        return FALLBACK;
    };
    let Some(height) = dimension(&["original_h", "thumb_360_h"]) else {
        return FALLBACK;
    };
    let scale = (MAX / width.max(height) as f32).min(1.0);
    (width as f32 * scale, height as f32 * scale)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::file_preview_dimensions;
    use crate::slack::models::File;

    #[test]
    fn square_file_preview_uses_square_viewer() {
        let mut file = File::default();
        file.extra.insert("original_w".into(), json!(1024));
        file.extra.insert("original_h".into(), json!(1024));
        assert_eq!(file_preview_dimensions(&file), (320.0, 320.0));
    }
}
