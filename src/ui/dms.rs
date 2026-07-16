use std::collections::HashMap;
use std::time::Duration;

use iced::widget::{
    Space, button, column, container, row, scrollable, stack, svg, text, text_input, toggler,
    tooltip,
};
use iced::{Alignment, Element, Fill, Length, Padding, font};

use super::{blocks, icons, message, theme};
use crate::app::{DmsState, FilePreview, Message};
use crate::slack::models::{Channel, DmEntry};
use crate::state::{self, Presence, Workspace};

type AvatarPreviews = HashMap<String, FilePreview>;

const AVATAR: f32 = 32.0;
const UNREAD_BAR: f32 = 2.0;
const PANEL_WIDTH: f32 = 340.0;

pub fn list_panel<'a>(
    ws: &'a Workspace,
    dms: &'a DmsState,
    active_channel: Option<&'a str>,
    avatars: &'a AvatarPreviews,
    emoji_previews: &'a AvatarPreviews,
    elapsed: Duration,
) -> Element<'a, Message> {
    let header = row![
        text("Direct messages")
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: font::Weight::Bold,
                ..iced::Font::default()
            }),
        Space::new().width(Fill),
        text("Unreads").size(theme::TEXT_SM).color(theme::TEXT_2),
        toggler(dms.unread_only)
            .size(theme::TEXT_LG)
            .style(theme::toggler)
            .on_toggle(Message::DmsUnreadOnlyToggled),
        compose_button(),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center)
    .width(Fill);

    let find = text_input("Find a DM…", &dms.filter)
        .on_input(Message::DmsFilterChanged)
        .style(theme::input)
        .size(theme::TEXT_MD)
        .padding([theme::SPACE_XS + 2.0, theme::SPACE_SM])
        .width(Fill);

    let filter = dms.filter.trim().to_lowercase();
    let visible: Vec<&DmEntry> = dms
        .entries
        .iter()
        .filter(|entry| !dms.unread_only || is_unread(ws, entry))
        .filter(|entry| {
            filter.is_empty()
                || display_name(ws, entry)
                    .to_lowercase()
                    .contains(filter.as_str())
        })
        .collect();

    let body: Element<'a, Message> = if dms.loading && visible.is_empty() {
        placeholder("Loading direct messages…")
    } else if visible.is_empty() {
        placeholder(if dms.unread_only {
            "No unread direct messages."
        } else if filter.is_empty() {
            "No direct messages yet."
        } else {
            "No matches."
        })
    } else {
        let mut list = column![].spacing(0).width(Fill);
        for entry in visible {
            list = list.push(dm_row(
                ws,
                entry,
                active_channel == Some(entry.id.as_str()),
                avatars,
                emoji_previews,
                elapsed,
            ));
        }
        scrollable(list.padding(Padding::ZERO.right(theme::SPACE_SM)))
            .on_scroll(|viewport| Message::DmsScrolled {
                remaining: (viewport.content_bounds().height
                    - viewport.bounds().height
                    - viewport.absolute_offset().y)
                    .max(0.0),
            })
            .style(theme::scrollbar)
            .height(Fill)
            .into()
    };

    let content = column![header, find, body]
        .spacing(theme::SPACE_SM)
        .width(Fill)
        .height(Fill);

    container(content)
        .width(Length::Fixed(PANEL_WIDTH))
        .height(Fill)
        .padding(theme::SPACE_SM)
        .style(theme::panel)
        .into()
}

fn compose_button<'a>() -> Element<'a, Message> {
    let glyph = svg(icons::compose())
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .style(theme::sidebar_icon(theme::TEXT_2));
    tooltip(
        button(glyph)
            .padding(theme::SPACE_XS)
            .style(theme::action_button)
            .on_press(Message::PaletteToggled),
        container(
            text("New message")
                .size(theme::TEXT_SM)
                .color(theme::TEXT_1),
        )
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .style(theme::tooltip_bubble),
        tooltip::Position::Bottom,
    )
    .gap(theme::SPACE_XS)
    .into()
}

fn dm_row<'a>(
    ws: &'a Workspace,
    entry: &'a DmEntry,
    active: bool,
    avatars: &'a AvatarPreviews,
    emoji_previews: &'a AvatarPreviews,
    elapsed: Duration,
) -> Element<'a, Message> {
    let unread = is_unread(ws, entry);

    let bar = container(Space::new())
        .width(Length::Fixed(UNREAD_BAR))
        .height(Fill)
        .style(if unread {
            theme::activity_unread_bar
        } else {
            theme::activity_read_bar
        });

    let mut header = row![
        text(display_name(ws, entry))
            .size(theme::TEXT_MD)
            .color(if unread { theme::TEXT_1 } else { theme::TEXT_2 })
            .font(iced::Font {
                weight: font::Weight::Semibold,
                ..iced::Font::default()
            }),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center);
    header = header.push(Space::new().width(Fill));
    if let Some(latest) = entry.latest.as_deref() {
        header = header.push(
            text(time_label(latest))
                .size(theme::TEXT_SM)
                .color(theme::TEXT_4),
        );
    }
    let count = unread_count(ws, entry);
    if count > 0 {
        header = header.push(count_badge(count));
    }

    let mut col = column![header].spacing(theme::SPACE_XS).width(Fill);
    let preview = preview_string(ws, entry);
    if !preview.is_empty() {
        col = col.push(message::inline_line(
            ws,
            &preview,
            emoji_previews,
            elapsed,
            theme::TEXT_MD,
            if unread { theme::TEXT_1 } else { theme::TEXT_3 },
        ));
    }

    let inner = row![bar, avatar(ws, entry, avatars), col]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center)
        .padding(Padding::ZERO.right(theme::SPACE_SM));

    button(inner)
        .width(Fill)
        .padding([theme::SPACE_XS + 2.0, 0.0])
        .style(theme::activity_row(active, unread))
        .on_press(Message::ChannelSelected(entry.id.clone()))
        .into()
}

fn avatar<'a>(
    ws: &'a Workspace,
    entry: &'a DmEntry,
    avatars: &'a AvatarPreviews,
) -> Element<'a, Message> {
    let channel = channel_of(ws, entry);

    if channel.map(|c| c.is_mpim).unwrap_or(false) {
        let label = channel
            .and_then(super::sidebar::group_member_count)
            .map(|count| count.to_string())
            .unwrap_or_else(|| {
                display_name(ws, entry)
                    .chars()
                    .find(|ch| ch.is_alphanumeric())
                    .map(|ch| ch.to_uppercase().collect())
                    .unwrap_or_else(|| "?".to_owned())
            });
        return container(
            text(label)
                .size(theme::TEXT_MD)
                .color(theme::accent_bright())
                .font(iced::Font {
                    weight: font::Weight::Bold,
                    ..iced::Font::default()
                }),
        )
        .width(Length::Fixed(AVATAR))
        .height(Length::Fixed(AVATAR))
        .center_x(Length::Fixed(AVATAR))
        .center_y(Length::Fixed(AVATAR))
        .style(theme::avatar_placeholder)
        .into();
    }

    let user = channel.and_then(state::dm_user_id);
    let image = message::avatar_with_size(
        user,
        user.and_then(|u| ws.avatar_url(u)).as_deref(),
        avatars,
        user.map(|u| ws.display_name(u))
            .and_then(|n| n.chars().next()),
        AVATAR,
        theme::CONTROL_RADIUS,
    );

    let Some(user) = user else {
        return image;
    };
    let presence = ws.presence.get(user).copied().unwrap_or(Presence::Unknown);
    let dot = container(Space::new())
        .width(Length::Fixed(theme::PRESENCE_DOT))
        .height(Length::Fixed(theme::PRESENCE_DOT))
        .style(if presence == Presence::Active {
            theme::presence_online
        } else {
            theme::presence_offline
        });
    stack![
        image,
        container(dot)
            .align_right(Length::Fixed(AVATAR))
            .align_bottom(Length::Fixed(AVATAR)),
    ]
    .width(Length::Fixed(AVATAR))
    .height(Length::Fixed(AVATAR))
    .into()
}

fn channel_of<'a>(ws: &'a Workspace, entry: &'a DmEntry) -> Option<&'a Channel> {
    ws.channels.get(&entry.id).or(entry.channel.as_ref())
}

fn display_name(ws: &Workspace, entry: &DmEntry) -> String {
    channel_of(ws, entry)
        .map(|c| state::channel_display_name(ws, c))
        .unwrap_or_else(|| entry.id.clone())
}

fn preview_string(ws: &Workspace, entry: &DmEntry) -> String {
    let Some(msg) = entry.message.as_ref() else {
        return String::new();
    };
    let body = first_line(&blocks::notification_text(ws, msg));
    let is_mpim = channel_of(ws, entry).map(|c| c.is_mpim).unwrap_or(false);
    match msg.user.as_deref() {
        Some(user) if user == ws.self_user_id => format!("You: {body}"),
        Some(user) if is_mpim => format!("{}: {body}", ws.display_name(user)),
        _ => body,
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn is_unread(ws: &Workspace, entry: &DmEntry) -> bool {
    if unread_count(ws, entry) > 0 {
        return true;
    }
    let last_read = ws
        .messages
        .get(&entry.id)
        .and_then(|cm| cm.last_read.as_deref())
        .or_else(|| channel_of(ws, entry).and_then(|c| c.last_read.as_deref()));
    match (entry.latest.as_deref(), last_read) {
        (Some(latest), Some(last_read)) => state::cmp_ts(Some(latest), Some(last_read)).is_gt(),
        _ => false,
    }
}

fn unread_count(ws: &Workspace, entry: &DmEntry) -> u32 {
    ws.channels
        .get(&entry.id)
        .map(|c| ws.unread_total(c))
        .unwrap_or(0)
}

fn count_badge<'a>(count: u32) -> Element<'a, Message> {
    let label = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    container(text(label).size(theme::TEXT_SM).color(theme::TEXT_1))
        .padding([0.0, theme::SPACE_XS + 1.0])
        .style(theme::ping_badge)
        .into()
}

fn time_label(ts: &str) -> String {
    let (secs, _) = state::ts_key(ts);
    let now = state::now_secs().max(0) as u64;
    if now < secs {
        return state::format_ts_hm(ts);
    }
    let elapsed = now - secs;
    if elapsed < 60 {
        "now".to_owned()
    } else if elapsed < 3600 {
        let m = elapsed / 60;
        if m == 1 {
            "1 min".to_owned()
        } else {
            format!("{m} mins")
        }
    } else {
        state::format_ts_hm(ts)
    }
}

fn placeholder<'a>(label: &str) -> Element<'a, Message> {
    container(
        text(label.to_owned())
            .size(theme::TEXT_MD)
            .color(theme::TEXT_4),
    )
    .center_x(Fill)
    .height(Fill)
    .padding(theme::SPACE_LG)
    .into()
}

pub fn unread_count_total(ws: &Workspace) -> usize {
    ws.channels
        .values()
        .filter(|c| (c.is_im || c.is_mpim) && !c.is_archived)
        .filter(|c| ws.unread_total(c) > 0)
        .count()
}
