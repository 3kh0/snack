use std::collections::HashMap;
use std::time::Duration;

use iced::widget::{Space, button, column, container, row, scrollable, svg, text, tooltip};
use iced::{Alignment, Element, Fill, Length, Padding, font};

use super::{blocks, icons, message, theme};
use crate::app::{ActivityState, FilePreview, Message};
use crate::slack::models::{ActivityItem, Message as SlackMessage};
use crate::state::{self, Workspace};

type AvatarPreviews = HashMap<String, FilePreview>;

const AVATAR: f32 = 36.0;
const UNREAD_BAR: f32 = 3.0;

pub fn list_panel<'a>(
    ws: &'a Workspace,
    activity: &'a ActivityState,
    avatars: &'a AvatarPreviews,
    emoji_previews: &'a AvatarPreviews,
    elapsed: Duration,
) -> Element<'a, Message> {
    let unread = activity.items.iter().filter(|i| i.is_unread).count();
    let count_chip = container(
        text(unread.to_string())
            .size(theme::TEXT_SM)
            .color(theme::BG_BASE),
    )
    .padding([1.0, theme::SPACE_SM])
    .style(theme::activity_count_pill);

    let header = row![
        text("Activity")
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: font::Weight::Bold,
                ..iced::Font::default()
            }),
        count_chip,
        Space::new().width(Fill),
        button(text("Unread").size(theme::TEXT_SM))
            .style(theme::reaction_button(activity.unread_only))
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .on_press(Message::ActivityUnreadOnlyToggled),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center)
    .width(Fill);

    let body: Element<'a, Message> = if activity.loading && activity.items.is_empty() {
        placeholder("Loading activity…")
    } else if activity.items.is_empty() {
        placeholder("No activity yet.")
    } else if activity.unread_only && unread == 0 {
        placeholder("No unread activity.")
    } else {
        let mut list = column![].spacing(0).width(Fill);
        let mut current_day: Option<String> = None;
        for item in activity
            .items
            .iter()
            .filter(|item| !activity.unread_only || item.is_unread)
        {
            let day = state::date_key_for_ts(&item.feed_ts);
            if day != current_day {
                current_day = day;
                list = list.push(date_header(&item.feed_ts));
            }
            list = list.push(activity_row(
                ws,
                activity,
                item,
                avatars,
                emoji_previews,
                elapsed,
            ));
        }
        scrollable(list.padding(Padding::ZERO.right(theme::SPACE_SM)))
            .style(theme::scrollbar)
            .height(Fill)
            .into()
    };

    let content = column![header, body]
        .spacing(theme::SPACE_MD)
        .width(Fill)
        .height(Fill);

    container(content)
        .width(Length::Fixed(400.0))
        .height(Fill)
        .padding(theme::SPACE_MD)
        .style(theme::panel)
        .into()
}

fn date_header<'a>(feed_ts: &str) -> Element<'a, Message> {
    container(
        text(state::format_ts_date_label(feed_ts))
            .size(theme::TEXT_SM)
            .color(theme::TEXT_3)
            .font(iced::Font {
                weight: font::Weight::Semibold,
                ..iced::Font::default()
            }),
    )
    .center_x(Fill)
    .padding([theme::SPACE_MD, 0.0])
    .into()
}

fn activity_row<'a>(
    ws: &'a Workspace,
    activity: &'a ActivityState,
    item: &'a ActivityItem,
    avatars: &'a AvatarPreviews,
    emoji_previews: &'a AvatarPreviews,
    elapsed: Duration,
) -> Element<'a, Message> {
    let active = activity.selected.as_deref() == Some(item.key.as_str());
    let msg = hydrated_msg(activity, item);

    let bar = container(Space::new())
        .width(Length::Fixed(UNREAD_BAR))
        .height(Fill)
        .style(if item.is_unread {
            theme::activity_unread_bar
        } else {
            theme::activity_read_bar
        });

    let mut header = row![
        text(verb(item))
            .size(theme::TEXT_MD)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: font::Weight::Semibold,
                ..iced::Font::default()
            }),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center);
    if let Some(target) = target_element(ws, item) {
        header = header.push(target);
    }
    header = header.push(Space::new().width(Fill));
    header = header.push(
        text(time_label(item))
            .size(theme::TEXT_SM)
            .color(theme::TEXT_4),
    );
    if let Some(count) = badge_count(item) {
        header = header.push(count_badge(count));
    }

    let preview = preview_string(ws, activity, item);
    let mut col = column![header].spacing(theme::SPACE_XS).width(Fill);
    if !preview.is_empty() {
        col = col.push(message::inline_line(
            ws,
            &preview,
            emoji_previews,
            elapsed,
            theme::TEXT_MD,
            if item.is_unread {
                theme::TEXT_1
            } else {
                theme::TEXT_3
            },
        ));
    }

    let avatar_user = item
        .author()
        .or_else(|| msg.and_then(|m| m.user.as_deref()));
    let avatar: Element<Message> = if avatar_user == Some(ws.self_user_id.as_str()) {
        let glyph = container(
            svg(icons::reply())
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0))
                .style(theme::sidebar_icon(theme::TEXT_3)),
        )
        .width(Length::Fixed(AVATAR))
        .height(Length::Fixed(AVATAR))
        .center_x(Length::Fixed(AVATAR))
        .center_y(Length::Fixed(AVATAR));
        let tip = format!(
            "Replied: {}",
            if preview.is_empty() {
                "…"
            } else {
                preview.as_str()
            }
        );
        tooltip(
            glyph,
            container(text(tip).size(theme::TEXT_SM).color(theme::TEXT_1))
                .padding([theme::SPACE_XS, theme::SPACE_SM])
                .style(theme::tooltip_bubble),
            tooltip::Position::Right,
        )
        .gap(theme::SPACE_XS)
        .into()
    } else {
        message::avatar_with_size(
            avatar_user,
            avatar_user.and_then(|u| ws.avatar_url(u)).as_deref(),
            avatars,
            avatar_user
                .map(|u| ws.display_name(u))
                .and_then(|n| n.chars().next()),
            AVATAR,
            theme::CONTROL_RADIUS,
        )
    };

    let inner = row![bar, avatar, col,]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center)
        .padding(Padding::ZERO.right(theme::SPACE_SM));

    button(inner)
        .width(Fill)
        .padding([theme::SPACE_SM, 0.0])
        .style(theme::activity_row(active, item.is_unread))
        .on_press(Message::ActivitySelected(item.key.clone()))
        .into()
}

fn target_element<'a>(ws: &'a Workspace, item: &'a ActivityItem) -> Option<Element<'a, Message>> {
    if matches!(item.item.kind.as_str(), "dm" | "bot_dm_bundle") {
        return None;
    }
    let id = item.channel()?;
    if is_dm_channel(ws, item) {
        return Some(text("DM").size(theme::TEXT_SM).color(theme::TEXT_2).into());
    }
    let channel = ws.channels.get(id);
    let name = channel
        .map(|c| state::channel_display_name(ws, c))
        .unwrap_or_else(|| id.to_owned());
    let private = channel.map(|c| c.is_private || c.is_group).unwrap_or(false);
    let glyph = svg(if private { icons::lock() } else { icons::tag() })
        .width(Length::Fixed(12.0))
        .height(Length::Fixed(12.0))
        .style(theme::sidebar_icon(theme::TEXT_3));

    Some(
        container(
            row![glyph, text(name).size(theme::TEXT_SM).color(theme::TEXT_2)]
                .spacing(theme::SPACE_XS)
                .align_y(Alignment::Center),
        )
        .padding([1.0, theme::SPACE_SM])
        .style(theme::activity_channel_chip)
        .into(),
    )
}

fn is_dm_channel(ws: &Workspace, item: &ActivityItem) -> bool {
    item.channel()
        .and_then(|id| ws.channels.get(id))
        .map(|c| c.is_im || c.is_mpim)
        .unwrap_or(false)
}

fn count_badge<'a>(count: u32) -> Element<'a, Message> {
    let label = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    container(text(label).size(theme::TEXT_SM).color(theme::BG_BASE))
        .padding([0.0, theme::SPACE_XS + 1.0])
        .style(theme::activity_count_badge)
        .into()
}

fn hydrated_msg<'a>(activity: &'a ActivityState, item: &ActivityItem) -> Option<&'a SlackMessage> {
    let channel = item.channel()?;
    let key = |ts: &str| (channel.to_owned(), ts.to_owned());
    item.preview_ts()
        .and_then(|ts| activity.hydrated.get(&key(ts)))
        .or_else(|| item.ts().and_then(|ts| activity.hydrated.get(&key(ts))))
}

fn preview_string(ws: &Workspace, activity: &ActivityState, item: &ActivityItem) -> String {
    let body = match hydrated_msg(activity, item) {
        Some(msg) => first_line(&blocks::notification_text(ws, msg)),
        None => String::new(),
    };
    if item.item.reaction.is_some() {
        let author = hydrated_msg(activity, item)
            .and_then(|m| m.user.as_deref())
            .map(|u| author_label(ws, u))
            .unwrap_or_else(|| "You".to_owned());
        return format!("{author}: {body}");
    }
    body
}

fn author_label(ws: &Workspace, user: &str) -> String {
    if user == ws.self_user_id {
        "You".to_owned()
    } else {
        ws.display_name(user)
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn verb(item: &ActivityItem) -> &'static str {
    match item.item.kind.as_str() {
        "message_reaction" => "Reacted in",
        "thread_v2" | "thread_reply" => "Thread in",
        "at_user_group" => "Group mention in",
        "at_channel" | "at_everyone" => "Channel mention in",
        "at_user" => "Mention in",
        "keyword" | "unjoined_channel_mention" => "Keyword mention in",
        "dm" | "bot_dm_bundle" => "DM",
        _ => "Activity in",
    }
}

fn badge_count(item: &ActivityItem) -> Option<u32> {
    let count = item
        .item
        .bundle_info
        .as_ref()?
        .payload
        .as_ref()?
        .thread_entry
        .as_ref()?
        .unread_msg_count;
    (count > 0).then_some(count)
}

fn time_label(item: &ActivityItem) -> String {
    if item.feed_ts.is_empty() {
        return String::new();
    }
    let (secs, _) = state::ts_key(&item.feed_ts);
    let now = state::now_secs().max(0) as u64;
    if now < secs {
        return state::format_ts_hm(&item.feed_ts);
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
    } else if elapsed < 86_400 {
        let h = elapsed / 3600;
        if h == 1 {
            "1 hour".to_owned()
        } else {
            format!("{h} hours")
        }
    } else {
        state::format_ts_hm(&item.feed_ts)
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

pub fn unread_count(activity: &ActivityState) -> usize {
    activity.items.iter().filter(|i| i.is_unread).count()
}
