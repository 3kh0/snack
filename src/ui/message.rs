use iced::widget::{Column, Row, container, text};
use iced::{Element, Font};

use super::theme;
use crate::app::Message;
use crate::slack::models::Message as SlackMessage;
use crate::state::{self, Workspace};

pub fn row<'a>(ws: &Workspace, msg: &SlackMessage, pending: bool) -> Element<'a, Message> {
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

    if let Some(count) = msg.reply_count.filter(|c| *c > 0) {
        col = col.push(
            text(format!(
                "{count} repl{}",
                if count == 1 { "y" } else { "ies" }
            ))
            .size(theme::TEXT_SM)
            .color(theme::SIDEBAR_ACTIVE_BG),
        );
    }

    if !msg.reactions.is_empty() {
        let mut chips = Row::new().spacing(theme::SPACE_XS);
        for r in &msg.reactions {
            chips = chips.push(
                container(text(state::reaction_summary(r)).size(theme::TEXT_SM))
                    .padding([2, 6])
                    .style(theme::reaction_chip),
            );
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
