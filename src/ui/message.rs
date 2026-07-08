use std::collections::HashMap;
use std::time::Duration;

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{Column, Row, button, container, image, text, text_input};
use iced::{Alignment, ContentFit, Element, Fill, Font, Length};

use super::{blocks, selectable, theme};
use crate::app::{FilePreview, Message};
use crate::slack::models::Message as SlackMessage;
use crate::state::{self, Workspace};

pub fn row<'a>(
    ws: &Workspace,
    channel_id: &str,
    msg: &SlackMessage,
    pending: bool,
    in_thread: bool,
    hovered: bool,
    file_previews: &HashMap<String, FilePreview>,
    avatar_previews: &HashMap<String, FilePreview>,
    emoji_previews: &HashMap<String, FilePreview>,
    emoji_animation_elapsed: Duration,
    edit_text: Option<&str>,
) -> Element<'a, Message> {
    let author = msg
        .user
        .as_deref()
        .map(|u| ws.display_name(u))
        .unwrap_or_else(|| msg.bot_id.clone().unwrap_or_else(|| "unknown".to_owned()));

    let time = msg
        .ts
        .as_deref()
        .map(state::format_ts_hm)
        .unwrap_or_default();

    let avatar = avatar(
        msg.user.as_deref(),
        ws,
        avatar_previews,
        author.chars().next(),
    );

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
        )
        .push(text(time).size(theme::TEXT_SM).color(theme::TEXT_5));

    if msg.edited.is_some() {
        header = header.push(text("(edited)").size(theme::TEXT_SM).color(theme::MUTED));
    }
    if pending {
        header = header.push(text("sending…").size(theme::TEXT_SM).color(theme::MUTED));
    }

    let block_lines = blocks::render_lines(ws, msg);
    let copy_text = if block_lines.is_empty() {
        state::message_text(msg)
    } else {
        block_lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };

    let editable = !pending
        && msg.bot_id.is_none()
        && msg.ts.is_some()
        && msg.user.as_deref() == Some(ws.self_user_id.as_str());

    // Slack-style toolbar overlaid on the message, revealed only on hover.
    let action_bar: Option<Element<'a, Message>> = if hovered && edit_text.is_none() {
        let mut actions = Row::new();
        let mut has_actions = false;
        if !copy_text.is_empty() {
            actions = actions.push(action_item("Copy", Message::CopyMessage(copy_text.clone())));
            has_actions = true;
        }
        if editable {
            if let Some(ts) = msg.ts.clone() {
                actions = actions
                    .push(action_item(
                        "Edit",
                        Message::EditPressed {
                            channel: channel_id.to_owned(),
                            ts: ts.clone(),
                        },
                    ))
                    .push(action_item(
                        "Delete",
                        Message::DeletePressed {
                            channel: channel_id.to_owned(),
                            ts,
                        },
                    ));
                has_actions = true;
            }
        }
        has_actions.then(|| {
            container(actions.padding(2))
                .padding(2)
                .style(theme::action_bar)
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
        return container(
            Row::new()
                .spacing(theme::SPACE_SM)
                .align_y(Alignment::Start)
                .push(avatar)
                .push(container(content).width(Fill)),
        )
        .padding([theme::SPACE_XS, theme::SPACE_MD])
        .into();
    }

    let mut col = Column::new().spacing(theme::SPACE_XS).push(header);
    let body_lines = body_lines(msg, block_lines);
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
                segments.push(selectable::Segment {
                    text: state::emoji_text_to_display(&line.text),
                    mono: line.mono,
                    color: None,
                });
            }
            col = col.push(selectable::SelectableText::new(
                &segments,
                theme::TEXT_MD,
                theme::TEXT_2,
                theme::selection(),
            ));
        }
    }

    for file in &msg.files {
        col = col.push(file_row(file, file_previews));
    }

    for att in &msg.attachments {
        col = col.push(attachment_row(att, file_previews));
    }

    let thread_ts = match (msg.thread_ts.as_deref(), msg.ts.as_deref()) {
        (Some(root), Some(ts)) if root != ts => Some(root.to_owned()),
        (_, Some(ts)) => Some(ts.to_owned()),
        _ => None,
    };
    // Inside the thread panel there is no reply-to-reply, so hide the link.
    if let Some(ts) = thread_ts.filter(|_| !in_thread) {
        let reply_label = msg
            .reply_count
            .filter(|c| *c > 0)
            .map(|count| format!("{count} repl{}", if count == 1 { "y" } else { "ies" }))
            .unwrap_or_else(|| "Reply".to_owned());
        col = col.push(
            button(text(reply_label).size(theme::TEXT_SM))
                .padding([2, 0])
                .style(theme::link_button)
                .on_press(Message::ThreadOpened {
                    channel: channel_id.to_owned(),
                    ts,
                }),
        );
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

    let body = container(
        Row::new()
            .spacing(theme::SPACE_SM)
            .align_y(Alignment::Start)
            .push(avatar)
            .push(container(col).width(Fill)),
    )
    .padding([theme::SPACE_XS, theme::SPACE_MD])
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

fn body_lines(msg: &SlackMessage, block_lines: Vec<blocks::RenderLine>) -> Vec<blocks::RenderLine> {
    if !block_lines.is_empty() {
        return block_lines;
    }
    let body = state::message_text(msg);
    if body.is_empty() {
        Vec::new()
    } else {
        vec![blocks::RenderLine {
            text: body,
            mono: false,
        }]
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

fn emoji_body<'a>(
    lines: &[blocks::RenderLine],
    ws: &Workspace,
    emoji_previews: &HashMap<String, FilePreview>,
    elapsed: Duration,
) -> Element<'a, Message> {
    let mut col = Column::new().spacing(theme::SPACE_XS);
    for line in lines {
        let mut row = Row::new().spacing(2).align_y(Alignment::Center).width(Fill);
        for token in state::emoji_text_tokens(&line.text) {
            match token {
                state::EmojiTextToken::Text(value) if !value.is_empty() => {
                    let mut styled = text(value).size(theme::TEXT_MD).color(theme::TEXT_2);
                    if line.mono {
                        styled = styled.font(Font::MONOSPACE);
                    }
                    row = row.push(styled);
                }
                state::EmojiTextToken::Text(_) => {}
                state::EmojiTextToken::Emoji(name) => {
                    row = row.push(emoji_inline(
                        ws,
                        &name,
                        emoji_previews,
                        elapsed,
                        theme::TEXT_MD,
                    ));
                }
            }
        }
        col = col.push(row.wrap().vertical_spacing(2));
    }
    col.into()
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
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .style(theme::action_button)
        .on_press(on_press)
        .into()
}

pub fn empty_placeholder<'a>() -> Element<'a, Message> {
    container(
        text("No messages yet.")
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
    user_id: Option<&str>,
    ws: &Workspace,
    avatar_previews: &HashMap<String, FilePreview>,
    fallback: Option<char>,
) -> Element<'a, Message> {
    const SIZE: f32 = 32.0;
    const RADIUS: f32 = 7.0;
    if let Some(user_id) = user_id {
        if ws.avatar_url(user_id).is_some() {
            if let Some(FilePreview::Loaded(handle)) = avatar_previews.get(user_id) {
                return image::Image::new(handle.clone())
                    .width(Length::Fixed(SIZE))
                    .height(Length::Fixed(SIZE))
                    .content_fit(ContentFit::Cover)
                    .border_radius(RADIUS)
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
    .width(Length::Fixed(SIZE))
    .height(Length::Fixed(SIZE))
    .center_x(Length::Fixed(SIZE))
    .center_y(Length::Fixed(SIZE))
    .style(theme::avatar_placeholder)
    .into()
}

fn file_row<'a>(
    file: &crate::slack::models::File,
    file_previews: &HashMap<String, FilePreview>,
) -> Element<'a, Message> {
    let title = state::file_title(file);
    let summary = state::file_summary(file);
    let mut content = Column::new()
        .spacing(theme::SPACE_XS)
        .push(text(title).size(theme::TEXT_MD).font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::default()
        }))
        .push(text(summary).size(theme::TEXT_SM).color(theme::MUTED));

    if let Some(preview) = state::file_preview_key(file).and_then(|key| file_previews.get(&key)) {
        match preview {
            FilePreview::Loaded(handle) => {
                content = content.push(
                    image::Image::new(handle.clone())
                        .width(Length::Fixed(220.0))
                        .height(Length::Fixed(140.0))
                        .content_fit(ContentFit::Contain)
                        .border_radius(6.0),
                );
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
                    content = content.push(
                        image::Image::new(handle.clone())
                            .width(Length::Fixed(220.0))
                            .height(Length::Fixed(140.0))
                            .content_fit(ContentFit::Contain)
                            .border_radius(6.0),
                    );
                }
            }
        }
    }

    if let Some(url) = file.url_private.clone() {
        content = content.push(
            button(text("Download").size(theme::TEXT_SM))
                .padding([2, 0])
                .style(theme::link_button)
                .on_press(Message::FileDownloadPressed {
                    url,
                    filename: state::file_download_name(file),
                }),
        );
    }

    container(content)
        .padding(theme::SPACE_SM)
        .style(theme::file_attachment)
        .into()
}
