use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Element, Fill, Font, Length};
use std::collections::HashMap;

use super::{message, theme};
use crate::app::{FilePreview, Message};
use crate::slack::models::Message as SlackMessage;
use crate::state::{ChannelMessages, Workspace};

pub fn view<'a>(
    ws: &Workspace,
    channel_id: &str,
    root: Option<&SlackMessage>,
    replies: Option<&ChannelMessages>,
    value: &str,
    file_previews: &HashMap<String, FilePreview>,
    avatar_previews: &HashMap<String, FilePreview>,
    editing: Option<(&str, &str)>,
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
                col = col.push(message::row(
                    ws,
                    channel_id,
                    msg,
                    pending,
                    file_previews,
                    avatar_previews,
                    edit,
                ));
            }
            scrollable(col).style(theme::scrollbar).height(Fill).into()
        }
        _ => {
            let mut col = Column::new().spacing(theme::SPACE_XS);
            if let Some(root) = root {
                let edit = editing
                    .filter(|(ts, _)| Some(*ts) == root.ts.as_deref())
                    .map(|(_, value)| value);
                col = col.push(message::row(
                    ws,
                    channel_id,
                    root,
                    false,
                    file_previews,
                    avatar_previews,
                    edit,
                ));
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

    let input = text_input("Reply in thread", value)
        .on_input(Message::ThreadComposerChanged)
        .on_submit(Message::ThreadSendPressed)
        .style(theme::input)
        .padding(theme::SPACE_SM)
        .width(Fill);

    container(column![
        container(header).padding(theme::SPACE_MD),
        theme::divider(),
        container(list).height(Fill),
        container(input).padding(theme::SPACE_MD),
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
