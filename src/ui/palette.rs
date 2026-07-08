use std::collections::HashMap;

use iced::widget::{
    Column, Space, button, column, container, image, mouse_area, row, scrollable, stack, svg, text,
    text_input,
};
use iced::{Alignment, ContentFit, Element, Fill, Length, font};

use super::{icons, theme};
use crate::app::{FilePreview, Message, PaletteEntry, PaletteState, PaletteTarget};
use crate::slack::models::UserId;
use crate::state::Workspace;

type AvatarPreviews = HashMap<UserId, FilePreview>;

const CARD_WIDTH: f32 = 560.0;

pub const INPUT_ID: &str = "palette-input";

pub fn modal<'a>(
    base: Element<'a, Message>,
    ws: &'a Workspace,
    state: &'a PaletteState,
    avatars: &'a AvatarPreviews,
) -> Element<'a, Message> {
    let scrim = mouse_area(
        container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(theme::overlay_dim),
    )
    .on_press(Message::PaletteClosed);

    let centered = container(card(ws, state, avatars))
        .center_x(Fill)
        .align_top(Fill)
        .padding(theme::SPACE_LG * 5.0);

    stack![base, scrim, centered].into()
}

fn card<'a>(
    ws: &'a Workspace,
    state: &'a PaletteState,
    avatars: &'a AvatarPreviews,
) -> Element<'a, Message> {
    let input = text_input("Jump to channel or person…", &state.query)
        .id(INPUT_ID)
        .on_input(Message::PaletteQueryChanged)
        .on_submit(Message::PaletteSubmitted)
        .style(theme::input)
        .size(theme::TEXT_LG)
        .padding(theme::SPACE_MD)
        .width(Fill);

    let body = column![
        container(input).padding(theme::SPACE_SM),
        theme::divider(),
        results(ws, state, avatars),
    ]
    .width(Fill);

    container(body)
        .width(Length::Fixed(CARD_WIDTH))
        .padding(theme::SPACE_SM)
        .style(theme::panel)
        .into()
}

fn results<'a>(
    ws: &'a Workspace,
    state: &'a PaletteState,
    avatars: &'a AvatarPreviews,
) -> Element<'a, Message> {
    if state.entries.is_empty() {
        let label = if state.query.trim().is_empty() {
            "No recent channels yet."
        } else {
            "No matches."
        };
        return container(text(label).size(theme::TEXT_MD).color(theme::MUTED))
            .padding(theme::SPACE_LG)
            .into();
    }

    let mut list = Column::new().spacing(theme::SPACE_XS);
    if state.query.trim().is_empty() {
        list = list.push(section_header("Recent"));
    }
    for (i, entry) in state.entries.iter().enumerate() {
        list = list.push(entry_row(ws, avatars, entry, i, i == state.selected));
    }

    scrollable(list)
        .style(theme::scrollbar)
        .height(Length::Shrink)
        .width(Fill)
        .into()
}

fn section_header<'a>(title: &str) -> Element<'a, Message> {
    container(
        text(title.to_ascii_uppercase())
            .size(theme::TEXT_SM)
            .color(theme::TEXT_4)
            .font(iced::Font {
                weight: font::Weight::Semibold,
                ..iced::Font::default()
            }),
    )
    .padding([theme::SPACE_SM, theme::SPACE_MD])
    .into()
}

fn entry_row<'a>(
    ws: &'a Workspace,
    avatars: &'a AvatarPreviews,
    entry: &'a PaletteEntry,
    index: usize,
    selected: bool,
) -> Element<'a, Message> {
    let label = text(entry.label.clone())
        .size(theme::TEXT_MD)
        .color(theme::TEXT_1)
        .font(iced::Font {
            weight: font::Weight::Semibold,
            ..iced::Font::default()
        });

    let mut content = row![icon(ws, avatars, entry), label]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center);

    if !entry.sublabel.is_empty() {
        content = content.push(Space::new().width(Fill));
        content = content.push(
            text(entry.sublabel.clone())
                .size(theme::TEXT_SM)
                .color(theme::MUTED),
        );
    }

    button(content)
        .width(Fill)
        .padding([theme::SPACE_XS, theme::SPACE_MD])
        .style(theme::channel_row(selected))
        .on_press(Message::PaletteEntryPressed(index))
        .into()
}

fn icon<'a>(
    ws: &Workspace,
    avatars: &AvatarPreviews,
    entry: &PaletteEntry,
) -> Element<'a, Message> {
    match &entry.target {
        PaletteTarget::User { user, .. } => icon_slot(user_avatar(avatars, user, &entry.label)),
        PaletteTarget::Channel(id) => {
            let channel = ws.channels.get(id);
            let is_mpim = channel.map(|c| c.is_mpim).unwrap_or(false);
            if is_mpim {
                return icon_slot(group_chip());
            }
            let private = channel.map(|c| c.is_private || c.is_group).unwrap_or(false);
            let handle = if private { icons::lock() } else { icons::tag() };
            icon_slot(
                svg(handle)
                    .width(Length::Fixed(theme::SIDEBAR_ICON))
                    .height(Length::Fixed(theme::SIDEBAR_ICON))
                    .style(theme::sidebar_icon(theme::TEXT_3))
                    .into(),
            )
        }
    }
}

fn icon_slot<'a>(inner: Element<'a, Message>) -> Element<'a, Message> {
    container(inner)
        .width(Length::Fixed(theme::SIDEBAR_ICON_SLOT))
        .center_x(Length::Fixed(theme::SIDEBAR_ICON_SLOT))
        .center_y(Length::Fixed(theme::SIDEBAR_AVATAR))
        .into()
}

fn user_avatar<'a>(avatars: &AvatarPreviews, user: &str, label: &str) -> Element<'a, Message> {
    let size = Length::Fixed(theme::SIDEBAR_AVATAR);
    if let Some(FilePreview::Loaded(handle)) = avatars.get(user) {
        return image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(ContentFit::Cover)
            .border_radius(theme::SIDEBAR_AVATAR / 2.0)
            .into();
    }
    let initial = label
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_owned());
    container(text(initial).size(theme::TEXT_SM).font(iced::Font {
        weight: font::Weight::Bold,
        ..iced::Font::default()
    }))
    .width(size)
    .height(size)
    .center_x(size)
    .center_y(size)
    .style(theme::avatar_placeholder)
    .into()
}

fn group_chip<'a>() -> Element<'a, Message> {
    container(
        text("#")
            .size(theme::TEXT_SM)
            .color(theme::accent_bright())
            .font(iced::Font {
                weight: font::Weight::Bold,
                ..iced::Font::default()
            }),
    )
    .width(Length::Fixed(theme::SIDEBAR_AVATAR))
    .height(Length::Fixed(theme::SIDEBAR_AVATAR))
    .center_x(Length::Fixed(theme::SIDEBAR_AVATAR))
    .center_y(Length::Fixed(theme::SIDEBAR_AVATAR))
    .style(theme::avatar_placeholder)
    .into()
}
