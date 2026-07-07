use std::collections::HashMap;

use iced::widget::{Column, Row, button, container, image, text, text_input};
use iced::{ContentFit, Element, Font, Length};

use super::{blocks, theme};
use crate::app::{FilePreview, Message};
use crate::slack::models::Message as SlackMessage;
use crate::state::{self, Workspace};

pub fn row<'a>(
    ws: &Workspace,
    channel_id: &str,
    msg: &SlackMessage,
    pending: bool,
    file_previews: &HashMap<String, FilePreview>,
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

    let mut header = Row::new()
        .spacing(theme::SPACE_SM)
        .push(text(author).size(theme::TEXT_MD).font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::default()
        }))
        .push(text(time).size(theme::TEXT_SM).color(theme::MUTED));

    if msg.edited.is_some() {
        header = header.push(text("(edited)").size(theme::TEXT_SM).color(theme::MUTED));
    }
    if pending {
        header = header.push(text("sending…").size(theme::TEXT_SM).color(theme::MUTED));
    }

    let editable = !pending
        && msg.bot_id.is_none()
        && msg.ts.is_some()
        && msg.user.as_deref() == Some(ws.self_user_id.as_str());
    if editable && edit_text.is_none() {
        if let Some(ts) = msg.ts.clone() {
            let del_ts = ts.clone();
            header = header
                .push(
                    button(text("Edit").size(theme::TEXT_SM))
                        .padding([2, 0])
                        .style(theme::link_button)
                        .on_press(Message::EditPressed {
                            channel: channel_id.to_owned(),
                            ts,
                        }),
                )
                .push(
                    button(text("Delete").size(theme::TEXT_SM))
                        .padding([2, 0])
                        .style(theme::link_button)
                        .on_press(Message::DeletePressed {
                            channel: channel_id.to_owned(),
                            ts: del_ts,
                        }),
                );
        }
    }

    if let Some(value) = edit_text {
        let input = text_input("Edit message", value)
            .on_input(Message::EditComposerChanged)
            .on_submit(Message::EditSubmit)
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
        return container(
            Column::new()
                .spacing(theme::SPACE_XS)
                .push(header)
                .push(input)
                .push(actions),
        )
        .padding([theme::SPACE_XS, theme::SPACE_MD])
        .into();
    }

    let mut col = Column::new().spacing(theme::SPACE_XS).push(header);
    let block_lines = blocks::render_lines(ws, msg);
    if block_lines.is_empty() {
        let body = state::message_text(msg);
        if !body.is_empty() {
            col = col.push(text(body).size(theme::TEXT_MD));
        }
    } else {
        for line in block_lines {
            let widget = text(line.text).size(theme::TEXT_MD);
            let widget = if line.mono {
                widget.font(Font::MONOSPACE)
            } else {
                widget
            };
            col = col.push(widget);
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
    if let Some(ts) = thread_ts {
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
            let label = state::reaction_summary(r);
            let active = state::reaction_has_user(r, &ws.self_user_id);
            let chip: Element<'a, Message> = if let Some(ts) = msg.ts.clone() {
                button(text(label).size(theme::TEXT_SM))
                    .padding([2, 6])
                    .style(theme::reaction_button(active))
                    .on_press(Message::ReactionPressed {
                        channel: channel_id.to_owned(),
                        ts,
                        name: r.name.clone(),
                    })
                    .into()
            } else {
                container(text(label).size(theme::TEXT_SM))
                    .padding([2, 6])
                    .style(theme::reaction_chip)
                    .into()
            };
            chips = chips.push(chip);
        }
        col = col.push(chips);
    }

    container(col)
        .padding([theme::SPACE_XS, theme::SPACE_MD])
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
        let styled = if att.title_link.is_some() {
            styled.color(theme::SIDEBAR_ACTIVE_BG)
        } else {
            styled
        };
        content = content.push(styled);
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
