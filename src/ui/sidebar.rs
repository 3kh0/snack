use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use iced::widget::text::Wrapping;
use iced::widget::{Column, button, column, container, image, row, scrollable, svg, text};
use iced::{Alignment, Color, ContentFit, Element, Fill, Length, font};

use super::{icons, theme};
use crate::app::{FilePreview, Message};
use crate::slack::models::{Channel, TeamId, UserId};
use crate::state::{self, Workspace, channel_display_name};

type AvatarPreviews = HashMap<UserId, FilePreview>;

#[derive(Debug)]
struct SidebarGroup<'a> {
    title: String,
    channels: Vec<&'a Channel>,
}

fn grouped<'a>(ws: &'a Workspace, active: Option<&str>) -> Vec<SidebarGroup<'a>> {
    let sections = ws.resolved_sidebar_sections();

    let mut custom: HashMap<&str, usize> = HashMap::new();
    for (i, section) in sections.iter().enumerate() {
        for id in &section.channel_ids {
            custom.entry(id.as_str()).or_insert(i);
        }
    }
    let index_of = |kind: &str| sections.iter().position(|s| s.kind == kind);
    let vip = index_of("priority");
    let stars = index_of("stars");
    let dms = index_of("direct_messages");
    let connect = index_of("slack_connect");
    let channels = index_of("channels");

    let mut groups: Vec<Vec<&Channel>> = vec![Vec::new(); sections.len()];
    for c in ws.channels.values() {
        let target = if vip.is_some() && is_vip(ws, c) && has_unreads(ws, c) {
            vip
        } else if let Some(&section) = custom.get(c.id.as_str()) {
            Some(section)
        } else if ws.is_starred_channel(c) && stars.is_some() {
            stars
        } else if c.is_im || c.is_mpim {
            dms
        } else if c.is_ext_shared && connect.is_some() {
            connect
        } else {
            channels
        };
        let Some(target) = target else { continue };
        let visible = sections[target].show_all
            || has_unreads(ws, c)
            || active == Some(c.id.as_str())
            || ws.should_show_unstarred_read_channels();
        if visible {
            groups[target].push(c);
        }
    }

    sections
        .into_iter()
        .zip(groups)
        .map(|(section, mut channels)| {
            match section.sort {
                state::SectionSort::Recent => sort_recent_section(&mut channels, ws),
                state::SectionSort::Priority => sort_priority_section(&mut channels, ws),
                state::SectionSort::Alpha => sort_alpha_section(&mut channels, ws),
            }
            SidebarGroup {
                title: section.title,
                channels,
            }
        })
        .collect()
}

fn sort_recent_section(channels: &mut [&Channel], ws: &Workspace) {
    channels.sort_by(|a, b| {
        ws.channel_recency(b)
            .cmp(&ws.channel_recency(a))
            .then_with(|| name_cmp(ws, a, b))
    });
}

fn sort_priority_section(channels: &mut [&Channel], ws: &Workspace) {
    channels.sort_by(|a, b| {
        ws.priority_score(&b.id)
            .unwrap_or(0.0)
            .total_cmp(&ws.priority_score(&a.id).unwrap_or(0.0))
            .then_with(|| name_cmp(ws, a, b))
    });
}

fn sort_alpha_section(channels: &mut [&Channel], ws: &Workspace) {
    channels.sort_by(|a, b| {
        (mention_count(ws, b) > 0)
            .cmp(&(mention_count(ws, a) > 0))
            .then_with(|| name_cmp(ws, a, b))
    });
}

fn name_cmp(ws: &Workspace, a: &Channel, b: &Channel) -> Ordering {
    natural_cmp(&channel_display_name(ws, a), &channel_display_name(ws, b))
        .then_with(|| a.id.cmp(&b.id))
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        return match (a.peek().copied(), b.peek().copied()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                match take_number(&mut a).cmp(&take_number(&mut b)) {
                    Ordering::Equal => continue,
                    unequal => unequal,
                }
            }
            (Some(x), Some(y)) => match x.to_lowercase().cmp(y.to_lowercase()) {
                Ordering::Equal => {
                    a.next();
                    b.next();
                    continue;
                }
                unequal => unequal,
            },
        };
    }
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> u64 {
    let mut n: u64 = 0;
    while let Some(c) = chars.peek().and_then(|c| c.to_digit(10)) {
        n = n.saturating_mul(10).saturating_add(u64::from(c));
        chars.next();
    }
    n
}

fn section_header<'a>(title: &str) -> Element<'a, Message> {
    container(
        text(title.to_ascii_uppercase())
            .size(theme::TEXT_SM - 1.0)
            .color(theme::TEXT_4)
            .font(iced::Font {
                weight: font::Weight::Semibold,
                ..iced::Font::default()
            }),
    )
    .padding([theme::SPACE_SM, theme::SPACE_SM])
    .into()
}

fn push_section<'a>(
    mut list: Column<'a, Message>,
    ws: &'a Workspace,
    avatars: &AvatarPreviews,
    active: Option<&str>,
    title: &str,
    channels: Vec<&'a Channel>,
) -> Column<'a, Message> {
    if channels.is_empty() {
        return list;
    }
    list = list.push(section_header(title));
    for c in channels {
        list = list.push(channel_button(
            ws,
            avatars,
            c,
            active == Some(c.id.as_str()),
        ));
    }
    list
}

fn channel_button<'a>(
    ws: &Workspace,
    avatars: &AvatarPreviews,
    c: &Channel,
    active: bool,
) -> Element<'a, Message> {
    let mut name = channel_display_name(ws, c);
    if c.is_archived {
        name = format!("{name} (archived)");
    }

    let unread = has_unreads(ws, c);
    let mentions = mention_count(ws, c);

    // unread (no ping): white + slightly bolder. read: muted normal.
    let fg = if active || unread {
        theme::TEXT_1
    } else {
        theme::TEXT_3
    };
    let weight = if unread {
        font::Weight::Semibold
    } else {
        font::Weight::Normal
    };

    // name fills remaining width and truncates (single line) so the badge stays pinned right
    let label = text(name)
        .size(theme::TEXT_MD)
        .color(fg)
        .wrapping(Wrapping::None)
        .width(Fill)
        .font(iced::Font {
            weight,
            ..iced::Font::default()
        });

    let mut row = row![channel_icon(ws, avatars, c, fg), label]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center);

    if mentions > 0 {
        row = row.push(ping_badge(mentions));
    }

    button(row)
        .width(Fill)
        .padding([3.0, theme::SPACE_SM])
        .style(theme::channel_row(active))
        .on_press(Message::ChannelSelected(c.id.clone()))
        .into()
}

/// Fixed-width leading slot so every channel/DM label starts at the same x.
fn icon_slot<'a>(inner: Element<'a, Message>) -> Element<'a, Message> {
    container(inner)
        .width(Length::Fixed(theme::SIDEBAR_ICON_SLOT))
        .center_x(Length::Fixed(theme::SIDEBAR_ICON_SLOT))
        .center_y(Length::Fixed(theme::SIDEBAR_AVATAR))
        .into()
}

fn channel_icon<'a>(
    ws: &Workspace,
    avatars: &AvatarPreviews,
    c: &Channel,
    color: Color,
) -> Element<'a, Message> {
    // 1:1 DM -> the other person's profile picture
    if c.is_im {
        return icon_slot(dm_avatar(ws, avatars, c));
    }
    // group DM -> chip with number of people
    if c.is_mpim {
        let count = group_member_count(c).unwrap_or(0);
        let chip = container(
            text(count.to_string())
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
        .style(theme::avatar_placeholder);
        return icon_slot(chip.into());
    }
    // public / private channel -> material glyph
    let handle = if c.is_private || c.is_group {
        icons::lock()
    } else {
        icons::tag()
    };
    icon_slot(
        svg(handle)
            .width(Length::Fixed(theme::SIDEBAR_ICON))
            .height(Length::Fixed(theme::SIDEBAR_ICON))
            .style(theme::sidebar_icon(color))
            .into(),
    )
}

fn dm_avatar<'a>(ws: &Workspace, avatars: &AvatarPreviews, c: &Channel) -> Element<'a, Message> {
    let size = Length::Fixed(theme::SIDEBAR_AVATAR);
    let user = state::dm_user_id(c);
    if let Some(user) = user {
        if ws.avatar_url(user).is_some() {
            if let Some(FilePreview::Loaded(handle)) = avatars.get(user) {
                return image(handle.clone())
                    .width(size)
                    .height(size)
                    .content_fit(ContentFit::Cover)
                    .border_radius(theme::SIDEBAR_AVATAR / 2.0)
                    .into();
            }
        }
    }
    // fallback: initial on a placeholder, tinted by presence
    let initial = channel_display_name(ws, c)
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

fn ping_badge<'a>(count: u32) -> Element<'a, Message> {
    let label = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    container(
        text(label)
            .size(theme::TEXT_SM)
            .color(theme::TEXT_1)
            .wrapping(Wrapping::None)
            .font(iced::Font {
                weight: font::Weight::Bold,
                ..iced::Font::default()
            }),
    )
    .height(Length::Fixed(theme::PING_BADGE_H))
    .center_y(Length::Fixed(theme::PING_BADGE_H))
    .padding([0.0, theme::SPACE_SM])
    .style(theme::ping_badge)
    .into()
}

fn workspace_button<'a>(ws: &Workspace, active: bool) -> Element<'a, Message> {
    let connected = ws.rt.is_connected();
    let dot = text(if connected { "●" } else { "○" })
        .size(theme::TEXT_SM)
        .color(if connected {
            theme::ONLINE
        } else {
            theme::TEXT_5
        });
    button(
        iced::widget::row![dot, text(ws.name.clone()).size(theme::TEXT_MD)]
            .spacing(theme::SPACE_SM)
            .align_y(iced::Alignment::Center),
    )
    .width(Fill)
    .padding([3.0, theme::SPACE_SM])
    .style(theme::channel_row(active))
    .on_press(Message::WorkspaceSelected(ws.team_id.clone()))
    .into()
}

fn jump_to_button<'a>() -> Element<'a, Message> {
    let hint = if cfg!(target_os = "macos") {
        "⌘K"
    } else {
        "Ctrl K"
    };
    let inner = row![
        svg(icons::search())
            .width(Length::Fixed(theme::SIDEBAR_ICON))
            .height(Length::Fixed(theme::SIDEBAR_ICON))
            .style(theme::sidebar_icon(theme::TEXT_4)),
        text("Jump to…").size(theme::TEXT_SM).color(theme::TEXT_3),
        iced::widget::Space::new().width(Fill),
        text(hint).size(theme::TEXT_SM).color(theme::TEXT_5),
    ]
    .spacing(theme::SPACE_SM)
    .align_y(Alignment::Center);

    button(inner)
        .width(Fill)
        .padding([theme::SPACE_XS + 1.0, theme::SPACE_SM])
        .style(theme::channel_row(false))
        .on_press(Message::PaletteToggled)
        .into()
}

pub fn view<'a>(
    workspaces: &BTreeMap<TeamId, Workspace>,
    active_team: Option<&str>,
    ws: &'a Workspace,
    active: Option<&str>,
    avatars: &'a AvatarPreviews,
    width: f32,
) -> Element<'a, Message> {
    let sections = grouped(ws, active);

    let mut list = Column::new()
        .spacing(theme::SPACE_XS)
        .push(section_header("Workspaces"));
    for team_ws in workspaces.values() {
        list = list.push(workspace_button(
            team_ws,
            active_team == Some(team_ws.team_id.as_str()),
        ));
    }

    for group in sections {
        list = push_section(list, ws, avatars, active, &group.title, group.channels);
    }

    let header = container(
        text(ws.name.clone())
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::default()
            }),
    )
    .padding([theme::SPACE_SM, theme::SPACE_SM]);

    let search = container(jump_to_button()).padding([0.0, theme::SPACE_SM]);

    let body = column![
        header,
        search,
        scrollable(list).style(theme::scrollbar).height(Fill)
    ]
    .spacing(theme::SPACE_XS)
    .width(Length::Fixed(width))
    .height(Fill);

    container(body)
        .width(Length::Fixed(width))
        .height(Fill)
        .style(theme::sidebar)
        .into()
}

fn mention_count(ws: &Workspace, c: &Channel) -> u32 {
    ws.messages
        .get(&c.id)
        .map(|cm| cm.mention_count)
        .unwrap_or(0)
}

/// Number of people in a group DM, parsed from the `mpdm-a--b--c-1` name.
pub(super) fn group_member_count(c: &Channel) -> Option<usize> {
    let name = c.name.as_deref()?;
    let rest = name.strip_prefix("mpdm-")?;
    let rest = rest
        .rsplit_once('-')
        .and_then(|(prefix, suffix)| suffix.parse::<u32>().ok().map(|_| prefix))
        .unwrap_or(rest);
    let count = rest
        .split("--")
        .filter(|name| !name.trim().is_empty())
        .count();
    (count > 0).then_some(count)
}

fn has_unreads(ws: &Workspace, c: &Channel) -> bool {
    unread_total(ws, c) > 0
}

fn unread_total(ws: &Workspace, c: &Channel) -> u32 {
    ws.unread_total(c)
}

fn is_vip(ws: &Workspace, c: &Channel) -> bool {
    state::is_vip_channel(ws, c)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use serde_json::json;

    use super::*;
    use crate::slack::models::Channel;
    use crate::state::{ChannelMessages, RealtimeStatus};

    fn channel(id: &str, name: &str) -> Channel {
        Channel {
            id: id.into(),
            name: Some(name.into()),
            is_channel: true,
            ..Default::default()
        }
    }

    fn dm(id: &str, name: &str) -> Channel {
        Channel {
            id: id.into(),
            name: Some(name.into()),
            is_im: true,
            ..Default::default()
        }
    }

    fn workspace(channels: Vec<Channel>, unreads: &[(&str, u32)]) -> Workspace {
        let mut messages = HashMap::new();
        for (channel, count) in unreads {
            messages.insert(
                (*channel).to_owned(),
                ChannelMessages {
                    unread_count: *count,
                    ..Default::default()
                },
            );
        }
        Workspace {
            team_id: "T".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            self_user_id: "U_SELF".into(),
            activity_unread_count: None,
            channels: channels
                .into_iter()
                .map(|channel| (channel.id.clone(), channel))
                .collect::<BTreeMap<_, _>>(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            vip_users: std::collections::HashSet::new(),
            sidebar: Default::default(),
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages,
            typing: HashMap::new(),
            presence: HashMap::new(),
            active_huddles: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        }
    }

    fn ids(channels: &[&Channel]) -> Vec<String> {
        channels.iter().map(|channel| channel.id.clone()).collect()
    }

    fn section_ids(groups: &[SidebarGroup<'_>], title: &str) -> Vec<String> {
        groups
            .iter()
            .find(|group| group.title == title)
            .map(|group| ids(&group.channels))
            .unwrap_or_default()
    }

    fn section_titles(groups: &[SidebarGroup<'_>]) -> Vec<String> {
        groups
            .iter()
            .filter(|group| !group.channels.is_empty())
            .map(|group| group.title.clone())
            .collect()
    }

    #[test]
    fn groups_sidebar_like_slack_unread_priority() {
        let mut vip = channel("C_VIP", "lilys-nest");
        vip.extra
            .insert("sidebar_section_name".into(), json!("VIP unreads"));
        let mut starred = channel("C_STAR", "announcements");
        starred.is_starred = true;
        let quiet = channel("C_QUIET", "general");
        let unread = channel("C_UNREAD", "community");
        let active_read = channel("C_ACTIVE", "community-logs");
        let mut ws = workspace(
            vec![
                quiet,
                unread,
                starred,
                vip,
                active_read,
                dm("D_ALICE", "alice"),
            ],
            &[("C_VIP", 1), ("C_UNREAD", 2)],
        );
        ws.hide_read_channels_unless_starred = true;
        ws.priority_sidebar_section = true;

        let sections = grouped(&ws, Some("C_ACTIVE"));

        assert_eq!(section_ids(&sections, "VIP unreads"), ["C_VIP"]);
        assert!(section_ids(&sections, "Direct messages").is_empty());
        assert_eq!(section_ids(&sections, "Starred"), ["C_STAR"]);
        assert_eq!(section_ids(&sections, "Channels"), ["C_UNREAD", "C_ACTIVE"]);
    }

    #[test]
    fn hidden_sections_stay_hidden() {
        use crate::slack::models::{ChannelIdsPage, ChannelSection};

        let mut ws = workspace(vec![channel("C_G", "grouped")], &[("C_G", 1)]);
        ws.apply_channel_sections(crate::slack::models::ChannelSectionsPage {
            channel_sections: vec![ChannelSection {
                channel_section_id: "L_UG".into(),
                name: "helpers".into(),
                kind: "user_group".into(),
                channel_ids_page: ChannelIdsPage {
                    channel_ids: vec!["C_G".into()],
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        });
        ws.sidebar.hidden_sections.insert("L_UG".into());

        let sections = grouped(&ws, None);

        assert!(!section_titles(&sections).contains(&"helpers".to_string()));
    }

    #[test]
    fn external_channels_get_their_own_section_and_stay_visible_when_read() {
        let mut ext = channel("C_EXT", "vercel-embassy");
        ext.is_ext_shared = true;
        let read = channel("C_READ", "quiet");
        let mut ws = workspace(vec![ext, read], &[]);
        ws.hide_read_channels_unless_starred = true;

        let sections = grouped(&ws, None);

        assert_eq!(section_ids(&sections, "External connections"), ["C_EXT"]);
        assert!(section_ids(&sections, "Channels").is_empty());
    }

    #[test]
    fn hides_read_dms_when_slack_prefers_unread_sidebar() {
        let mut ws = workspace(
            vec![dm("D_READ", "read"), dm("D_UNREAD", "unread")],
            &[("D_UNREAD", 1)],
        );
        ws.hide_read_channels_unless_starred = true;

        let sections = grouped(&ws, None);

        assert_eq!(section_ids(&sections, "Direct messages"), ["D_UNREAD"]);
    }

    #[test]
    fn channel_labels_use_symbols_not_lock_text() {
        let public = channel("C_PUBLIC", "public-room");
        let mut private = channel("G_PRIVATE", "private-room");
        private.is_private = true;
        let ws = workspace(vec![public, private], &[]);

        // icons now carry the #/lock affordance; text is the bare name
        assert_eq!(
            channel_display_name(&ws, ws.channels.get("C_PUBLIC").unwrap()),
            "public-room"
        );
        assert_eq!(
            channel_display_name(&ws, ws.channels.get("G_PRIVATE").unwrap()),
            "private-room"
        );
    }

    #[test]
    fn dm_labels_do_not_use_private_channel_formatting() {
        let mut mpdm = dm("G_MPDM", "mpdm-aarav54897--echo--alanlichen1-1");
        mpdm.is_im = false;
        mpdm.is_mpim = true;
        mpdm.is_group = true;
        mpdm.is_private = true;
        let ws = workspace(vec![mpdm], &[]);

        let label = channel_display_name(&ws, ws.channels.get("G_MPDM").unwrap());

        assert_eq!(label.trim(), "aarav54897, echo, alanlichen1");
        assert!(!label.contains("lock"));
        assert!(!label.contains("🔒"));
        assert!(!label.contains("mpdm-"));
    }

    #[test]
    fn group_member_count_parses_mpdm_name() {
        let mut mpdm = dm("G_MPDM", "mpdm-aarav54897--echo--alanlichen1-1");
        mpdm.is_im = false;
        mpdm.is_mpim = true;
        assert_eq!(group_member_count(&mpdm), Some(3));

        let public = channel("C_PUBLIC", "general");
        assert_eq!(group_member_count(&public), None);
    }
}
