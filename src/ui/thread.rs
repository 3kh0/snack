use iced::widget::text_editor::Content;
use iced::widget::{Column, button, column, container, mouse_area, row, scrollable, text};
use iced::{Element, Fill, Font, Length};
use std::collections::HashMap;
use std::time::Duration;

use super::{composer, message, theme};
use crate::app::{ComposerTarget, FilePreview, Message};
use crate::slack::models::Message as SlackMessage;
use crate::state::{ChannelMessages, Workspace};

pub fn view<'a>(
    ws: &Workspace,
    channel_id: &str,
    root: Option<&SlackMessage>,
    replies: Option<&ChannelMessages>,
    content: &'a Content,
    file_previews: &HashMap<String, FilePreview>,
    avatar_previews: &HashMap<String, FilePreview>,
    emoji_previews: &HashMap<String, FilePreview>,
    emoji_animation_elapsed: Duration,
    editing: Option<(&str, &str)>,
    hovered_ts: Option<&str>,
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
    .spacing(theme::SPACE_MD);

    let list: Element<'a, Message> = match replies {
        Some(cm) if !cm.messages.is_empty() => {
            let mut col = Column::new().spacing(theme::SPACE_XS);
            for msg in &cm.messages {
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
            scrollable(col).style(theme::scrollbar).height(Fill).into()
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
            scrollable(col).style(theme::scrollbar).height(Fill).into()
        }
    };

    let input = composer::thread_view(content, ComposerTarget::Thread);

    container(column![
        container(header).padding(theme::SPACE_MD),
        theme::divider(),
        container(list).height(Fill),
        input,
    ])
    .width(Length::Fixed(theme::THREAD_WIDTH))
    .height(Fill)
    .style(theme::panel)
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
