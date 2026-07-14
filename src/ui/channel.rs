use iced::widget::{
    Column, Id, Row, Space, button, column, container, image, mouse_area, row, scrollable, stack,
    text,
};
use iced::{Alignment, ContentFit, Element, Fill, Length, font};

use super::{message, theme};
use crate::app::{FilePreview, Message, PendingFileMessage, TextSelection, TextSelectionSurface};
use crate::slack::models::{Channel, UserId};
use crate::state::{self, Presence, Workspace};
use std::collections::HashMap;
use std::time::Duration;

pub const VISIBLE_MESSAGE_LIMIT: usize = 200;

const HEADER_AVATAR: f32 = 28.0;
const HEADER_AVATAR_RADIUS: f32 = 6.0;

type AvatarPreviews = HashMap<UserId, FilePreview>;

pub fn scrollable_id(channel_id: &str) -> Id {
    Id::from(format!("channel-messages:{channel_id}"))
}

pub fn view<'a>(
    ws: &Workspace,
    channel_id: &str,
    file_previews: &HashMap<String, FilePreview>,
    avatar_previews: &HashMap<String, FilePreview>,
    emoji_previews: &HashMap<String, FilePreview>,
    emoji_animation_elapsed: Duration,
    editing: Option<(&str, &str)>,
    hovered_ts: Option<&str>,
    text_selection: Option<&TextSelection>,
    pending_file_messages: &'a [PendingFileMessage],
) -> Element<'a, Message> {
    let header = channel_header(ws, channel_id, avatar_previews);

    let list: Element<'a, Message> = match ws.messages.get(channel_id) {
        Some(cm) if !cm.messages.is_empty() => {
            let mut col = Column::new().spacing(theme::SPACE_XS);
            let mut visible: Vec<_> = cm
                .messages
                .iter()
                .rev()
                .filter(|m| state::is_channel_timeline_visible(m))
                .take(VISIBLE_MESSAGE_LIMIT)
                .collect();
            visible.reverse();
            let mut last_date = None;
            let mut previous_group_message = None;
            let surface = TextSelectionSurface::Channel {
                channel: channel_id.to_owned(),
            };
            for (message_index, m) in visible.into_iter().enumerate() {
                if let Some(ts) = m.ts.as_deref() {
                    let date = state::date_key_for_ts(ts);
                    if date.is_some() && date != last_date {
                        col = col.push(date_separator(state::format_ts_date_label(ts)));
                        previous_group_message = None;
                        last_date = date;
                    }
                }
                let pending = m.ts.as_deref().map(|ts| cm.is_pending(ts)).unwrap_or(false);
                let edit = editing
                    .filter(|(ts, _)| Some(*ts) == m.ts.as_deref())
                    .map(|(_, value)| value);
                let hovered = m.ts.as_deref().is_some() && m.ts.as_deref() == hovered_ts;
                let compact = edit.is_none()
                    && previous_group_message
                        .is_some_and(|previous| same_message_group(previous, m));
                let row = message::row(
                    ws,
                    channel_id,
                    m,
                    pending,
                    compact,
                    false,
                    hovered,
                    file_previews,
                    avatar_previews,
                    emoji_previews,
                    emoji_animation_elapsed,
                    edit,
                    surface.clone(),
                    message_index,
                    text_selection,
                    pending_file_messages
                        .iter()
                        .find(|pending| {
                            pending.team == ws.team_id
                                && pending.channel == channel_id
                                && pending.thread_ts.is_none()
                                && m.client_msg_id.as_deref()
                                    == Some(pending.client_msg_id.as_str())
                        })
                        .map(|pending| pending.attachments.as_slice()),
                );
                let row: Element<'a, Message> = match m.ts.clone() {
                    Some(ts) => mouse_area(row)
                        .on_enter(Message::MessageHovered {
                            in_thread: false,
                            ts,
                        })
                        .on_exit(Message::MessageUnhovered)
                        .into(),
                    None => row,
                };
                col = col.push(row);
                previous_group_message = Some(m);
            }
            scrollable(col)
                .id(scrollable_id(channel_id))
                .on_scroll({
                    let channel_id = channel_id.to_owned();
                    move |viewport| Message::ChannelScrolled {
                        channel: channel_id.clone(),
                        y: viewport.absolute_offset().y,
                    }
                })
                .style(theme::scrollbar)
                .height(Fill)
                .into()
        }
        Some(cm) if cm.history_failed => history_failed_placeholder(channel_id),
        Some(cm) if cm.loaded => message::empty_placeholder("No messages yet."),
        _ => message::empty_placeholder("Loading messages…"),
    };

    let typing = ws.typing_names(channel_id);
    let footer: Element<'a, Message> = if typing.is_empty() {
        container(text("")).height(theme::TEXT_MD).into()
    } else {
        container(
            text(typing_line(&typing))
                .size(theme::TEXT_SM)
                .color(theme::MUTED),
        )
        .padding([0.0, theme::SPACE_MD])
        .into()
    };

    column![
        header,
        theme::divider(),
        container(list).height(Fill),
        footer
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

fn same_message_group(
    previous: &crate::slack::models::Message,
    current: &crate::slack::models::Message,
) -> bool {
    let has_author =
        previous.user.is_some() || previous.bot_id.is_some() || previous.username.is_some();
    if !has_author {
        return false;
    }
    let same_author = previous.user.as_deref() == current.user.as_deref()
        && previous.bot_id.as_deref() == current.bot_id.as_deref()
        && previous.username.as_deref() == current.username.as_deref();
    if !same_author {
        return false;
    }
    let (Some(previous_ts), Some(current_ts)) = (previous.ts.as_deref(), current.ts.as_deref())
    else {
        return false;
    };
    if state::date_key_for_ts(previous_ts) != state::date_key_for_ts(current_ts) {
        return false;
    }
    let previous_secs = state::ts_key(previous_ts).0;
    let current_secs = state::ts_key(current_ts).0;
    current_secs.saturating_sub(previous_secs) <= 300
}

fn history_failed_placeholder<'a>(channel_id: &str) -> Element<'a, Message> {
    container(
        column![
            text("Couldn't load messages.")
                .size(theme::TEXT_MD)
                .color(theme::MUTED),
            button(text("Retry").size(theme::TEXT_SM))
                .padding([theme::SPACE_XS, theme::SPACE_MD])
                .style(theme::secondary_button)
                .on_press(Message::ChannelSelected(channel_id.to_owned())),
        ]
        .spacing(theme::SPACE_SM)
        .align_x(Alignment::Center),
    )
    .center_x(Fill)
    .padding(theme::SPACE_LG)
    .into()
}

fn channel_header<'a>(
    ws: &Workspace,
    channel_id: &str,
    avatars: &AvatarPreviews,
) -> Element<'a, Message> {
    let Some(channel) = ws.channels.get(channel_id) else {
        return plain_header(channel_id.to_owned());
    };

    if channel.is_im {
        return dm_header(ws, channel, avatars);
    }

    let label = if channel.is_mpim {
        state::channel_display_name(ws, channel)
    } else {
        state::channel_label(channel)
    };
    plain_header(label)
}

fn plain_header<'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::default()
            }),
    )
    .padding(theme::SPACE_MD)
    .width(Fill)
    .into()
}

fn dm_header<'a>(
    ws: &Workspace,
    channel: &Channel,
    avatars: &AvatarPreviews,
) -> Element<'a, Message> {
    let name = state::channel_display_name(ws, channel);
    let presence = ws.presence_for_channel(channel);
    let avatar = dm_avatar_with_presence(ws, avatars, channel, &name, presence);

    let mut title = row![
        avatar,
        text(name)
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::default()
            }),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center);

    if state::is_vip_channel(ws, channel) {
        title = title.push(vip_badge());
    }

    container(title).padding(theme::SPACE_MD).width(Fill).into()
}

fn dm_avatar_with_presence<'a>(
    ws: &Workspace,
    avatars: &AvatarPreviews,
    channel: &Channel,
    label: &str,
    presence: Presence,
) -> Element<'a, Message> {
    let size = Length::Fixed(HEADER_AVATAR);
    let base: Element<'a, Message> = if let Some(user) = state::dm_user_id(channel) {
        if ws.avatar_url(user).is_some() {
            if let Some(FilePreview::Loaded(handle)) = avatars.get(user) {
                image(handle.clone())
                    .width(size)
                    .height(size)
                    .content_fit(ContentFit::Cover)
                    .border_radius(HEADER_AVATAR_RADIUS)
                    .into()
            } else {
                avatar_placeholder(label)
            }
        } else {
            avatar_placeholder(label)
        }
    } else {
        avatar_placeholder(label)
    };

    stack![base, presence_badge(presence)].into()
}

fn avatar_placeholder<'a>(label: &str) -> Element<'a, Message> {
    let size = Length::Fixed(HEADER_AVATAR);
    let initial = label
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_owned());
    container(text(initial).size(theme::TEXT_MD).font(iced::Font {
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
        .width(Length::Fixed(HEADER_AVATAR))
        .height(Length::Fixed(HEADER_AVATAR))
        .align_right(Length::Fixed(HEADER_AVATAR))
        .align_bottom(Length::Fixed(HEADER_AVATAR))
        .into()
}

fn vip_badge<'a>() -> Element<'a, Message> {
    container(text("VIP").size(10.0).font(iced::Font {
        weight: font::Weight::Bold,
        ..iced::Font::default()
    }))
    .padding([2.0, 6.0])
    .style(theme::vip_badge)
    .into()
}

fn date_separator<'a>(label: String) -> Element<'a, Message> {
    let line = || {
        container(Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
            .height(Length::Fixed(1.0))
            .width(Fill)
            .style(|_theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(theme::BORDER)),
                ..Default::default()
            })
    };

    Row::new()
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM)
        .push(line())
        .push(
            container(text(label).size(theme::TEXT_SM).color(theme::TEXT_2))
                .padding([4.0, 12.0])
                .style(theme::date_separator_label),
        )
        .push(line())
        .padding([theme::SPACE_SM, theme::SPACE_MD])
        .into()
}

fn typing_line(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => format!("{a} is typing…"),
        [a, b] => format!("{a} and {b} are typing…"),
        _ => format!("{} people are typing…", names.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::{same_message_group, typing_line};
    use crate::slack::models::Message as SlackMessage;

    fn msg(user: &str, ts: &str) -> SlackMessage {
        SlackMessage {
            user: Some(user.to_owned()),
            ts: Some(ts.to_owned()),
            text: Some("hi".to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn typing_line_variants() {
        assert_eq!(typing_line(&[]), "");
        assert_eq!(typing_line(&["alice".into()]), "alice is typing…");
        assert_eq!(
            typing_line(&["alice".into(), "bob".into()]),
            "alice and bob are typing…"
        );
        assert_eq!(
            typing_line(&["a".into(), "b".into(), "c".into()]),
            "3 people are typing…"
        );
    }

    #[test]
    fn same_message_group_requires_nearby_same_sender() {
        assert!(same_message_group(
            &msg("U1", "1783372400.000001"),
            &msg("U1", "1783372450.000001")
        ));
        assert!(!same_message_group(
            &msg("U1", "1783372400.000001"),
            &msg("U2", "1783372450.000001")
        ));
        assert!(!same_message_group(
            &msg("U1", "1783372400.000001"),
            &msg("U1", "1783372801.000001")
        ));
        assert!(!same_message_group(
            &SlackMessage {
                ts: Some("1783372400.000001".to_owned()),
                ..Default::default()
            },
            &SlackMessage {
                ts: Some("1783372450.000001".to_owned()),
                ..Default::default()
            }
        ));
    }
}
