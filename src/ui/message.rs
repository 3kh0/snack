use iced::widget::{Column, Row, button, container, text};
use iced::{Element, Font};

use super::theme;
use crate::app::Message;
use crate::slack::models::Message as SlackMessage;
use crate::state::{self, Workspace};

pub fn row<'a>(
    ws: &Workspace,
    channel_id: &str,
    msg: &SlackMessage,
    pending: bool,
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

    let mut col = Column::new()
        .spacing(theme::SPACE_XS)
        .push(header)
        .push(text(state::message_text(msg)).size(theme::TEXT_MD));

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
