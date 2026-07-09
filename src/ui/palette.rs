use std::collections::HashMap;

use iced::widget::{
    Column, Space, button, column, container, image, mouse_area, rich_text, row, scrollable, span,
    stack, svg, text, text_input,
};
use iced::{Alignment, ContentFit, Element, Fill, Length, font};

use super::{icons, theme};
use crate::app::{FilePreview, Message, PaletteEntry, PaletteState, PaletteTarget};
use crate::slack::models::UserId;
use crate::state::{Presence, Workspace};

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

    let layers = stack![scrim, centered].width(Fill).height(Fill);
    stack![base, layers].into()
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
    let query = state.query.trim();
    for (i, entry) in state.entries.iter().enumerate() {
        list = list.push(entry_row(ws, avatars, entry, query, i, i == state.selected));
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
    query: &str,
    index: usize,
    selected: bool,
) -> Element<'a, Message> {
    let label = highlighted(
        &entry.label,
        query,
        theme::TEXT_MD,
        theme::TEXT_1,
        font::Weight::Semibold,
    );

    let mut content = row![icon(ws, avatars, entry), label]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center);

    if !entry.sublabel.is_empty() {
        content = content.push(Space::new().width(Fill));
        content = content.push(highlighted(
            &entry.sublabel,
            query,
            theme::TEXT_SM,
            theme::MUTED,
            font::Weight::Normal,
        ));
    }

    button(content)
        .width(Fill)
        .padding([theme::SPACE_XS, theme::SPACE_MD])
        .style(theme::channel_row(selected))
        .on_press(Message::PaletteEntryPressed(index))
        .into()
}

fn highlighted<'a>(
    label: &str,
    query: &str,
    size: f32,
    color: iced::Color,
    base_weight: font::Weight,
) -> Element<'a, Message> {
    let base_font = iced::Font {
        weight: base_weight,
        ..iced::Font::default()
    };
    let bold_font = iced::Font {
        weight: font::Weight::Bold,
        ..iced::Font::default()
    };
    let mk = |s: &str, font: iced::Font| -> iced::advanced::text::Span<'a, Message, iced::Font> {
        span(s.to_owned()).font(font).color(color)
    };

    let start = if query.is_empty() || !label.is_ascii() || !query.is_ascii() {
        None
    } else {
        label.to_lowercase().find(&query.to_lowercase())
    };
    let spans = match start {
        Some(start) => {
            let end = start + query.len();
            vec![
                mk(&label[..start], base_font),
                mk(&label[start..end], bold_font),
                mk(&label[end..], base_font),
            ]
        }
        None => vec![mk(label, base_font)],
    };
    rich_text(spans).size(size).into()
}

fn icon<'a>(
    ws: &Workspace,
    avatars: &AvatarPreviews,
    entry: &PaletteEntry,
) -> Element<'a, Message> {
    match &entry.target {
        PaletteTarget::User { user, .. } => icon_slot(user_avatar(
            avatars,
            user,
            &entry.label,
            ws.presence.get(user).copied().unwrap_or(Presence::Unknown),
        )),
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

fn user_avatar<'a>(
    avatars: &AvatarPreviews,
    user: &str,
    label: &str,
    presence: Presence,
) -> Element<'a, Message> {
    let size = Length::Fixed(theme::SIDEBAR_AVATAR);
    let base: Element<'a, Message> = if let Some(FilePreview::Loaded(handle)) = avatars.get(user) {
        image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(ContentFit::Cover)
            .border_radius(theme::SIDEBAR_AVATAR_RADIUS)
            .into()
    } else {
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
    };

    stack![base, presence_badge(presence)].into()
}

fn presence_badge<'a>(presence: Presence) -> Element<'a, Message> {
    let style = if presence == Presence::Active {
        theme::presence_online
    } else {
        theme::presence_offline
    };
    let dot = container(Space::new())
        .width(Length::Fixed(theme::PRESENCE_DOT))
        .height(Length::Fixed(theme::PRESENCE_DOT))
        .style(style);
    container(dot)
        .width(Length::Fixed(theme::SIDEBAR_AVATAR))
        .height(Length::Fixed(theme::SIDEBAR_AVATAR))
        .align_right(Length::Fixed(theme::SIDEBAR_AVATAR))
        .align_bottom(Length::Fixed(theme::SIDEBAR_AVATAR))
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
    .style(theme::avatar_placeholder) // shares squarish radius with user avatars
    .into()
}
