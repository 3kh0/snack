use std::collections::HashMap;

use iced::widget::{Space, button, column, container, image, row, svg, text};
use iced::{Alignment, ContentFit, Element, Fill, Length, font};

use super::{icons, theme};
use crate::app::{FilePreview, Message};
use crate::slack::models::UserId;
use crate::state::{MainView, Presence, Workspace};

pub const RAIL_WIDTH: f32 = 40.0;

pub const ICON_SIZE: f32 = 28.0;
const NAV_ICON_SIZE: f32 = 18.0;
const AVATAR_RADIUS: f32 = 8.0;
const RAIL_PADDING: f32 = 6.0;
const MENU_WIDTH: f32 = 220.0;

type AvatarPreviews = HashMap<UserId, FilePreview>;

pub fn view<'a>(
    ws: &'a Workspace,
    avatars: &'a AvatarPreviews,
    view: MainView,
    activity_unread: usize,
) -> Element<'a, Message> {
    let home = nav_button(
        icons::home(),
        view == MainView::Home,
        0,
        Message::MainViewSelected(MainView::Home),
    );
    let notifications = nav_button(
        icons::bell(),
        view == MainView::Activity,
        activity_unread,
        Message::MainViewSelected(MainView::Activity),
    );

    let account = button(account_avatar(ws, avatars))
        .width(Length::Fixed(ICON_SIZE))
        .height(Length::Fixed(ICON_SIZE))
        .padding(0)
        .style(theme::rail_button)
        .on_press(Message::AccountMenuToggled);

    let body = column![home, notifications, Space::new().height(Fill), account]
        .spacing(theme::SPACE_SM)
        .align_x(Alignment::Center)
        .padding(RAIL_PADDING)
        .width(Length::Fill)
        .height(Length::Fill);

    container(body)
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fill)
        .style(theme::panel)
        .into()
}

fn nav_button<'a>(
    icon: svg::Handle,
    selected: bool,
    badge: usize,
    message: Message,
) -> Element<'a, Message> {
    let glyph = svg(icon)
        .width(Length::Fixed(NAV_ICON_SIZE))
        .height(Length::Fixed(NAV_ICON_SIZE))
        .style(theme::sidebar_icon(theme::rail_nav_icon(selected)));

    let content: Element<'a, Message> = if badge > 0 {
        iced::widget::stack![
            container(glyph)
                .width(Length::Fixed(ICON_SIZE))
                .height(Length::Fixed(ICON_SIZE))
                .center_x(Length::Fixed(ICON_SIZE))
                .center_y(Length::Fixed(ICON_SIZE)),
            container(
                container(
                    text(badge_label(badge))
                        .size(theme::TEXT_SM - 2.0)
                        .color(theme::TEXT_1)
                )
                .padding([0.0, 3.0])
                .style(theme::ping_badge)
            )
            .align_right(Fill),
        ]
        .into()
    } else {
        container(glyph)
            .width(Length::Fixed(ICON_SIZE))
            .height(Length::Fixed(ICON_SIZE))
            .center_x(Length::Fixed(ICON_SIZE))
            .center_y(Length::Fixed(ICON_SIZE))
            .into()
    };

    button(content)
        .width(Length::Fixed(ICON_SIZE))
        .height(Length::Fixed(ICON_SIZE))
        .padding(0)
        .style(theme::rail_nav_button(selected))
        .on_press(message)
        .into()
}

fn badge_label(count: usize) -> String {
    if count > 9 {
        "9+".to_owned()
    } else {
        count.to_string()
    }
}

pub fn account_menu<'a>(ws: &'a Workspace, avatars: &'a AvatarPreviews) -> Element<'a, Message> {
    let presence = ws
        .presence
        .get(&ws.self_user_id)
        .copied()
        .unwrap_or(Presence::Unknown);
    let display_name = ws.display_name(&ws.self_user_id);
    let status = presence_label(presence);

    let header = row![
        user_avatar(ws, avatars, 44.0, 10.0),
        column![
            text(display_name)
                .size(theme::TEXT_MD)
                .color(theme::TEXT_1)
                .font(iced::Font {
                    weight: font::Weight::Semibold,
                    ..iced::Font::default()
                }),
            text(status).size(theme::TEXT_SM).color(theme::TEXT_3),
        ]
        .spacing(theme::SPACE_XS),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center);

    let presence_action = if presence == Presence::Active {
        menu_button(
            "Set offline",
            false,
            Message::SelfPresenceSelected(Presence::Away),
        )
    } else {
        menu_button(
            "Set active",
            false,
            Message::SelfPresenceSelected(Presence::Active),
        )
    };

    let menu = column![
        header,
        theme::divider(),
        presence_action,
        theme::divider(),
        menu_button("Settings", false, Message::SettingsOpened),
        menu_button("Sign out", false, Message::SignOutPressed),
    ]
    .spacing(theme::SPACE_SM);

    container(menu)
        .width(Length::Fixed(MENU_WIDTH))
        .padding(theme::SPACE_MD)
        .style(theme::account_menu)
        .into()
}

fn account_avatar<'a>(ws: &'a Workspace, avatars: &'a AvatarPreviews) -> Element<'a, Message> {
    user_avatar(ws, avatars, ICON_SIZE, AVATAR_RADIUS)
}

fn user_avatar<'a>(
    ws: &'a Workspace,
    avatars: &'a AvatarPreviews,
    size_px: f32,
    radius: f32,
) -> Element<'a, Message> {
    let size = Length::Fixed(size_px);
    if ws.avatar_url(&ws.self_user_id).is_some() {
        if let Some(FilePreview::Loaded(handle)) = avatars.get(&ws.self_user_id) {
            return image(handle.clone())
                .width(size)
                .height(size)
                .content_fit(ContentFit::Cover)
                .border_radius(radius)
                .into();
        }
    }

    let initial = ws
        .display_name(&ws.self_user_id)
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_owned());

    container(
        text(initial)
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: font::Weight::Bold,
                ..iced::Font::default()
            }),
    )
    .width(size)
    .height(size)
    .center_x(size)
    .center_y(size)
    .style(theme::account_avatar_placeholder)
    .into()
}

fn menu_button<'a>(label: &str, selected: bool, message: Message) -> Element<'a, Message> {
    let marker = if selected { "●" } else { " " };
    button(
        row![
            text(marker)
                .size(theme::TEXT_SM)
                .color(if selected {
                    theme::ONLINE
                } else {
                    theme::TEXT_4
                })
                .width(Length::Fixed(16.0)),
            text(label.to_owned()).size(theme::TEXT_MD),
        ]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([theme::SPACE_SM, theme::SPACE_SM])
    .style(theme::account_menu_button)
    .on_press(message)
    .into()
}

fn presence_label(presence: Presence) -> &'static str {
    match presence {
        Presence::Active => "Active",
        Presence::Away => "Offline",
        Presence::Unknown => "Offline",
    }
}
