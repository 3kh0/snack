use iced::widget::{Column, Row, button, column, container, row, scrollable, text};
use iced::{Element, Fill, Font};

use super::{blocks, theme};
use crate::app::{Message, SearchHit, SearchState};
use crate::state::{self, Workspace};

pub fn view<'a>(ws: &Workspace, state: &SearchState) -> Element<'a, Message> {
    let summary = if state.loading {
        "Searching…".to_owned()
    } else {
        format!(
            "{} result{} for \"{}\"",
            state.total,
            if state.total == 1 { "" } else { "s" },
            state.query
        )
    };

    let header = row![
        column![
            text("Search")
                .size(theme::TEXT_LG)
                .color(theme::TEXT_1)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Font::default()
                }),
            text(summary).size(theme::TEXT_SM).color(theme::TEXT_4),
        ]
        .spacing(theme::SPACE_XS)
        .width(Fill),
        button(text("Close").size(theme::TEXT_SM))
            .style(theme::secondary_button)
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .on_press(Message::SearchCleared),
    ]
    .spacing(theme::SPACE_MD);

    let body: Element<'a, Message> = if state.hits.is_empty() {
        let label = if state.loading {
            "Searching…"
        } else {
            "No messages found."
        };
        container(text(label).size(theme::TEXT_MD).color(theme::MUTED))
            .padding(theme::SPACE_LG)
            .into()
    } else {
        let mut list = Column::new().spacing(theme::SPACE_XS);
        for hit in &state.hits {
            list = list.push(hit_row(ws, hit));
        }
        scrollable(list).style(theme::scrollbar).height(Fill).into()
    };

    let footer = pagination(state);

    column![
        container(header).padding(theme::SPACE_MD),
        theme::divider(),
        container(body).height(Fill),
        footer,
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

fn hit_row<'a>(ws: &Workspace, hit: &SearchHit) -> Element<'a, Message> {
    let msg = &hit.message;
    let author = author_name(ws, hit);
    let time = msg
        .ts
        .as_deref()
        .map(state::format_ts_hm)
        .unwrap_or_default();

    let meta = Row::new()
        .spacing(theme::SPACE_SM)
        .push(
            text(hit.channel_label.clone())
                .size(theme::TEXT_SM)
                .color(theme::accent()),
        )
        .push(text(author).size(theme::TEXT_SM).font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::default()
        }))
        .push(text(time).size(theme::TEXT_SM).color(theme::MUTED));

    let snippet = blocks::notification_text(ws, msg);
    let snippet = if snippet.trim().is_empty() {
        state::message_text(msg)
    } else {
        snippet
    };

    let content = column![meta, text(snippet).size(theme::TEXT_MD)].spacing(theme::SPACE_XS);

    let target_ts = msg.ts.clone().unwrap_or_default();
    button(content)
        .width(Fill)
        .padding([theme::SPACE_XS, theme::SPACE_MD])
        .style(theme::channel_row(false))
        .on_press(Message::SearchResultSelected {
            channel: hit.channel.clone(),
            ts: target_ts,
            thread_ts: msg.thread_ts.clone(),
        })
        .into()
}

fn pagination<'a>(state: &SearchState) -> Element<'a, Message> {
    if state.page_count <= 1 {
        return container(text("")).into();
    }
    let mut controls = Row::new().spacing(theme::SPACE_MD);
    if state.page > 1 {
        controls = controls.push(
            button(text("‹ Prev").size(theme::TEXT_SM))
                .style(theme::link_button)
                .on_press(Message::SearchPageRequested(state.page - 1)),
        );
    }
    controls = controls.push(
        text(format!("Page {} of {}", state.page, state.page_count))
            .size(theme::TEXT_SM)
            .color(theme::MUTED),
    );
    if state.page < state.page_count {
        controls = controls.push(
            button(text("Next ›").size(theme::TEXT_SM))
                .style(theme::link_button)
                .on_press(Message::SearchPageRequested(state.page + 1)),
        );
    }
    container(controls).padding(theme::SPACE_MD).into()
}

fn author_name(ws: &Workspace, hit: &SearchHit) -> String {
    if let Some(user) = hit.message.user.as_deref() {
        return ws.display_name(user);
    }
    if let Some(username) = hit
        .message
        .extra
        .get("username")
        .and_then(serde_json::Value::as_str)
    {
        return username.to_owned();
    }
    hit.message
        .bot_id
        .clone()
        .unwrap_or_else(|| "unknown".to_owned())
}
