use iced::widget::text_editor::Content;
use iced::widget::{Column, Id, button, column, container, mouse_area, row, scrollable, text};
use iced::{Element, Fill, Font, Length};
use std::collections::HashMap;
use std::time::Duration;

use super::{composer, message, theme};
use crate::app::{
    ComposerAttachment, ComposerTarget, FilePreview, Message, PendingFileMessage,
    ProfileHoverState, TextSelection, TextSelectionSurface,
};
use crate::slack::models::Message as SlackMessage;
use crate::state::{ChannelMessages, Workspace};

pub fn scrollable_id(channel_id: &str, root_ts: &str) -> Id {
    Id::from(format!("thread-messages:{channel_id}:{root_ts}"))
}

pub fn view<'a>(
    ws: &'a Workspace,
    channel_id: &str,
    root_ts: &str,
    root: Option<&'a SlackMessage>,
    replies: Option<&'a ChannelMessages>,
    content: &'a Content,
    attachments: &'a [ComposerAttachment],
    file_previews: &'a HashMap<String, FilePreview>,
    avatar_previews: &'a HashMap<String, FilePreview>,
    emoji_previews: &'a HashMap<String, FilePreview>,
    emoji_animation_elapsed: Duration,
    editing: Option<(&str, &str)>,
    hovered_ts: Option<&str>,
    unread_marker_ts: Option<&str>,
    text_selection: Option<&TextSelection>,
    pending_file_messages: &'a [PendingFileMessage],
    width: Length,
    profile_hover: Option<&'a ProfileHoverState>,
) -> Element<'a, Message> {
    let header = row![
        text("Thread")
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(Font {
                weight: iced::font::Weight::Bold,
                ..Font::default()
            })
            .width(Fill),
        button(text("Close").size(theme::TEXT_SM))
            .style(theme::secondary_button)
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .on_press(Message::ThreadClosed),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(theme::SPACE_SM);

    let list: Element<'a, Message> = match replies {
        Some(cm) if !cm.messages.is_empty() => {
            let mut col = Column::new().spacing(theme::SPACE_XS);
            let surface = TextSelectionSurface::Thread {
                channel: channel_id.to_owned(),
                root_ts: root_ts.to_owned(),
            };
            for (message_index, msg) in cm.messages.iter().enumerate() {
                if msg.ts.as_deref().is_some() && msg.ts.as_deref() == unread_marker_ts {
                    col = col.push(unread_divider());
                }
                let pending = msg
                    .ts
                    .as_deref()
                    .map(|ts| cm.is_pending(ts))
                    .unwrap_or(false);
                let edit = editing
                    .filter(|(ts, _)| Some(*ts) == msg.ts.as_deref())
                    .map(|(_, value)| value);
                let hovered = msg.ts.as_deref().is_some() && msg.ts.as_deref() == hovered_ts;
                let row = message::row(
                    ws,
                    channel_id,
                    msg,
                    pending,
                    false,
                    true,
                    hovered,
                    file_previews,
                    avatar_previews,
                    emoji_previews,
                    emoji_animation_elapsed,
                    edit,
                    surface.clone(),
                    message_index,
                    text_selection,
                    pending_file_messages
                        .iter()
                        .find(|pending| {
                            pending.team == ws.team_id
                                && pending.channel == channel_id
                                && pending.thread_ts.as_deref() == Some(root_ts)
                                && msg.client_msg_id.as_deref()
                                    == Some(pending.client_msg_id.as_str())
                        })
                        .map(|pending| pending.attachments.as_slice()),
                    profile_hover,
                );
                let row: Element<'a, Message> = match msg.ts.clone() {
                    Some(ts) => mouse_area(row)
                        .on_enter(Message::MessageHovered {
                            in_thread: true,
                            ts,
                        })
                        .on_exit(Message::MessageUnhovered)
                        .into(),
                    None => row,
                };
                col = col.push(row);
            }
            scrollable(col)
                .id(scrollable_id(channel_id, root_ts))
                .style(theme::scrollbar)
                .height(Fill)
                .into()
        }
        _ => {
            let mut col = Column::new().spacing(theme::SPACE_XS);
            if let Some(root) = root {
                let edit = editing
                    .filter(|(ts, _)| Some(*ts) == root.ts.as_deref())
                    .map(|(_, value)| value);
                let hovered = root.ts.as_deref().is_some() && root.ts.as_deref() == hovered_ts;
                let root_row = message::row(
                    ws,
                    channel_id,
                    root,
                    false,
                    false,
                    true,
                    hovered,
                    file_previews,
                    avatar_previews,
                    emoji_previews,
                    emoji_animation_elapsed,
                    edit,
                    TextSelectionSurface::Thread {
                        channel: channel_id.to_owned(),
                        root_ts: root_ts.to_owned(),
                    },
                    0,
                    text_selection,
                    None,
                    profile_hover,
                );
                let root_row: Element<'a, Message> = match root.ts.clone() {
                    Some(ts) => mouse_area(root_row)
                        .on_enter(Message::MessageHovered {
                            in_thread: true,
                            ts,
                        })
                        .on_exit(Message::MessageUnhovered)
                        .into(),
                    None => root_row,
                };
                col = col.push(root_row);
            }
            let status = if replies.map(|cm| cm.loaded).unwrap_or(false) {
                "No replies yet."
            } else {
                "Loading thread..."
            };
            col = col.push(
                container(text(status).size(theme::TEXT_MD).color(theme::MUTED))
                    .padding(theme::SPACE_MD),
            );
            scrollable(col)
                .id(scrollable_id(channel_id, root_ts))
                .style(theme::scrollbar)
                .height(Fill)
                .into()
        }
    };

    let input = composer::thread_view(content, attachments, ComposerTarget::Thread);

    container(column![
        container(header).padding([theme::SPACE_SM, theme::SPACE_MD]),
        theme::divider(),
        container(list).height(Fill),
        input,
    ])
    .width(width)
    .height(Fill)
    .style(theme::panel)
    .into()
}

fn unread_divider<'a>() -> Element<'a, Message> {
    let line = container(
        iced::widget::Space::new()
            .width(Fill)
            .height(Length::Fixed(theme::UNREAD_DIVIDER_THICKNESS)),
    )
    .width(Fill)
    .style(theme::unread_divider_line);

    row![
        line,
        container(text("NEW").size(10.0).font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::default()
        }))
        .padding([1.0, theme::SPACE_SM])
        .style(theme::unread_divider_pill),
    ]
    .spacing(theme::SPACE_XS)
    .align_y(iced::Alignment::Center)
    .padding([theme::SPACE_XS, theme::SPACE_SM])
    .into()
}

pub fn root_message<'a>(
    ws: &'a Workspace,
    channel_id: &str,
    root_ts: &str,
) -> Option<&'a SlackMessage> {
    ws.messages
        .get(channel_id)?
        .messages
        .iter()
        .find(|msg| msg.ts.as_deref() == Some(root_ts))
}
