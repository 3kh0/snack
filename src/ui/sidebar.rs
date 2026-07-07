use iced::widget::{Column, button, column, container, scrollable, text};
use iced::{Element, Fill, Length};

use super::theme;
use crate::app::Message;
use crate::slack::models::Channel;
use crate::state::{self, Presence, Workspace};

fn grouped(ws: &Workspace) -> (Vec<&Channel>, Vec<&Channel>) {
    let mut rooms: Vec<&Channel> = Vec::new();
    let mut dms: Vec<&Channel> = Vec::new();
    for c in ws.channels.values() {
        if c.is_im || c.is_mpim {
            dms.push(c);
        } else {
            rooms.push(c);
        }
    }
    rooms.sort_by(|a, b| state::channel_label(a).cmp(&state::channel_label(b)));
    dms.sort_by(|a, b| state::channel_label(a).cmp(&state::channel_label(b)));
    (rooms, dms)
}

fn section_header<'a>(title: &str) -> Element<'a, Message> {
    container(
        text(title.to_owned())
            .size(theme::TEXT_SM)
            .color(theme::MUTED),
    )
    .padding([theme::SPACE_SM, theme::SPACE_MD])
    .into()
}

fn channel_button<'a>(ws: &Workspace, c: &Channel, active: bool) -> Element<'a, Message> {
    let mut label = sidebar_label(ws, c);
    if c.is_archived {
        label = format!("{label} (archived)");
    }
    button(text(label).size(theme::TEXT_MD))
        .width(Fill)
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .style(theme::channel_row(active))
        .on_press(Message::ChannelSelected(c.id.clone()))
        .into()
}

pub fn view<'a>(ws: &Workspace, active: Option<&str>) -> Element<'a, Message> {
    let (rooms, dms) = grouped(ws);

    let mut list = Column::new()
        .spacing(theme::SPACE_XS)
        .push(section_header("Channels"));
    for c in rooms {
        list = list.push(channel_button(ws, c, active == Some(c.id.as_str())));
    }
    list = list.push(section_header("Direct messages"));
    for c in dms {
        list = list.push(channel_button(ws, c, active == Some(c.id.as_str())));
    }

    let header = container(text(ws.name.clone()).size(theme::TEXT_LG).font(iced::Font {
        weight: iced::font::Weight::Bold,
        ..iced::Font::default()
    }))
    .padding(theme::SPACE_MD);

    let body = column![header, scrollable(list).height(Fill)]
        .width(Length::Fixed(theme::SIDEBAR_WIDTH))
        .height(Fill);

    container(body)
        .width(Length::Fixed(theme::SIDEBAR_WIDTH))
        .height(Fill)
        .style(theme::sidebar)
        .into()
}

fn sidebar_label(ws: &Workspace, c: &Channel) -> String {
    let mut label = state::channel_label(c);
    if c.is_im || c.is_mpim {
        label = format!("{} {label}", presence_marker(ws.presence_for_channel(c)));
    }
    if let Some(cm) = ws.messages.get(&c.id) {
        if cm.unread_count > 0 {
            label = format!("{label}  {}", cm.unread_count);
        }
    }
    label
}

fn presence_marker(presence: Presence) -> &'static str {
    match presence {
        Presence::Active => "●",
        Presence::Away => "○",
        Presence::Unknown => " ",
    }
}
