use std::collections::BTreeMap;

use iced::widget::{Column, button, column, container, scrollable, text, text_input};
use iced::{Element, Fill, Length, font};

use super::theme;
use crate::app::Message;
use crate::slack::models::{Channel, TeamId};
use crate::state::{self, Presence, Workspace};

#[derive(Debug, Default)]
struct SidebarSections<'a> {
    vip_unreads: Vec<&'a Channel>,
    dms: Vec<&'a Channel>,
    starred: Vec<&'a Channel>,
    other_unreads: Vec<&'a Channel>,
}

fn grouped<'a>(ws: &'a Workspace, active: Option<&str>) -> SidebarSections<'a> {
    let mut sections = SidebarSections::default();
    for c in ws.channels.values() {
        if ws.is_starred_channel(c) {
            sections.starred.push(c);
            continue;
        }

        if c.is_im || c.is_mpim {
            if has_unreads(ws, c)
                || active == Some(c.id.as_str())
                || ws.should_show_unstarred_read_channels()
            {
                sections.dms.push(c);
            }
            continue;
        }

        if is_vip(ws, c) && has_unreads(ws, c) {
            sections.vip_unreads.push(c);
            continue;
        }

        if has_unreads(ws, c)
            || active == Some(c.id.as_str())
            || ws.should_show_unstarred_read_channels()
        {
            sections.other_unreads.push(c);
        }
    }
    sort_priority_section(&mut sections.vip_unreads, ws);
    sort_dm_section(&mut sections.dms, ws);
    sort_ordered_section(&mut sections.starred, &ws.starred_order, ws);
    sort_unread_section(&mut sections.other_unreads, ws);
    sections
}

fn sort_unread_section(channels: &mut [&Channel], ws: &Workspace) {
    channels.sort_by(|a, b| {
        unread_total(ws, b)
            .cmp(&unread_total(ws, a))
            .then_with(|| sidebar_label(ws, a).cmp(&sidebar_label(ws, b)))
    });
}

fn sort_priority_section(channels: &mut [&Channel], ws: &Workspace) {
    channels.sort_by(|a, b| {
        ws.priority_score(&b.id)
            .unwrap_or(0.0)
            .total_cmp(&ws.priority_score(&a.id).unwrap_or(0.0))
            .then_with(|| unread_total(ws, b).cmp(&unread_total(ws, a)))
            .then_with(|| sidebar_label(ws, a).cmp(&sidebar_label(ws, b)))
    });
}

fn sort_dm_section(channels: &mut [&Channel], ws: &Workspace) {
    channels.sort_by(|a, b| {
        order_index(&ws.dm_order, &a.id)
            .cmp(&order_index(&ws.dm_order, &b.id))
            .then_with(|| b.updated.unwrap_or(0).cmp(&a.updated.unwrap_or(0)))
            .then_with(|| unread_total(ws, b).cmp(&unread_total(ws, a)))
            .then_with(|| sidebar_label(ws, a).cmp(&sidebar_label(ws, b)))
    });
}

fn sort_ordered_section(channels: &mut [&Channel], order: &[String], ws: &Workspace) {
    channels.sort_by(|a, b| {
        order_index(order, &a.id)
            .cmp(&order_index(order, &b.id))
            .then_with(|| sidebar_label(ws, a).cmp(&sidebar_label(ws, b)))
    });
}

fn order_index(order: &[String], id: &str) -> usize {
    order
        .iter()
        .position(|ordered_id| ordered_id == id)
        .unwrap_or(usize::MAX)
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

fn push_section<'a>(
    mut list: Column<'a, Message>,
    ws: &'a Workspace,
    active: Option<&str>,
    title: &'static str,
    channels: Vec<&'a Channel>,
) -> Column<'a, Message> {
    if channels.is_empty() {
        return list;
    }
    list = list.push(section_header(title));
    for c in channels {
        list = list.push(channel_button(ws, c, active == Some(c.id.as_str())));
    }
    list
}

fn channel_button<'a>(ws: &Workspace, c: &Channel, active: bool) -> Element<'a, Message> {
    let mut label = sidebar_label(ws, c);
    if c.is_archived {
        label = format!("{label} (archived)");
    }
    let font = if has_unreads(ws, c) {
        iced::Font {
            weight: font::Weight::Bold,
            ..iced::Font::default()
        }
    } else {
        iced::Font::default()
    };
    button(text(label).size(theme::TEXT_MD).font(font))
        .width(Fill)
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .style(theme::channel_row(active))
        .on_press(Message::ChannelSelected(c.id.clone()))
        .into()
}

fn workspace_button<'a>(ws: &Workspace, active: bool) -> Element<'a, Message> {
    let status = if ws.rt.is_connected() { "●" } else { "○" };
    button(text(format!("{status} {}", ws.name)).size(theme::TEXT_MD))
        .width(Fill)
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .style(theme::channel_row(active))
        .on_press(Message::WorkspaceSelected(ws.team_id.clone()))
        .into()
}

pub fn view<'a>(
    workspaces: &BTreeMap<TeamId, Workspace>,
    active_team: Option<&str>,
    ws: &'a Workspace,
    active: Option<&str>,
    search_input: &str,
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

    list = push_section(list, ws, active, "VIP unreads", sections.vip_unreads);
    list = push_section(list, ws, active, "Direct messages", sections.dms);
    list = push_section(list, ws, active, "Starred", sections.starred);
    list = push_section(list, ws, active, "Other channels", sections.other_unreads);

    let header = container(text(ws.name.clone()).size(theme::TEXT_LG).font(iced::Font {
        weight: iced::font::Weight::Bold,
        ..iced::Font::default()
    }))
    .padding(theme::SPACE_MD);

    let search = container(
        text_input("Search messages", search_input)
            .on_input(Message::SearchInputChanged)
            .on_submit(Message::SearchSubmitted)
            .size(theme::TEXT_SM)
            .padding(theme::SPACE_SM)
            .width(Fill),
    )
    .padding([0.0, theme::SPACE_SM]);

    let body = column![header, search, scrollable(list).height(Fill)]
        .width(Length::Fixed(theme::SIDEBAR_WIDTH))
        .height(Fill);

    container(body)
        .width(Length::Fixed(theme::SIDEBAR_WIDTH))
        .height(Fill)
        .style(theme::sidebar)
        .into()
}

fn sidebar_label(ws: &Workspace, c: &Channel) -> String {
    let mut label = if c.is_im || c.is_mpim {
        dm_label(ws, c)
    } else if c.is_private || c.is_group {
        format!("🔒 {}", channel_name(c))
    } else {
        state::channel_label(c)
    };
    if c.is_im || c.is_mpim {
        label = format!("{} {label}", presence_marker(ws.presence_for_channel(c)));
    }
    if let Some(cm) = ws.messages.get(&c.id) {
        let count = cm.mention_count.max(cm.unread_count);
        if count > 0 {
            label = format!("{label}  {count}");
        }
    }
    label
}

fn channel_name(c: &Channel) -> String {
    c.name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(c.id.as_str())
        .to_owned()
}

fn dm_label(ws: &Workspace, c: &Channel) -> String {
    if c.is_im {
        if let Some(user) = state::dm_user_id(c) {
            return ws.display_name(user);
        }
    }
    if c.is_mpim {
        if let Some(name) = c.name.as_deref().and_then(mpdm_name_label) {
            return name;
        }
    }
    state::channel_label(c).trim_start_matches('#').to_owned()
}

fn mpdm_name_label(name: &str) -> Option<String> {
    let rest = name.strip_prefix("mpdm-")?;
    let rest = rest
        .rsplit_once('-')
        .and_then(|(prefix, suffix)| suffix.parse::<u32>().ok().map(|_| prefix))
        .unwrap_or(rest);
    let names: Vec<_> = rest
        .split("--")
        .map(|name| name.replace('.', " "))
        .filter(|name| !name.trim().is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

fn presence_marker(presence: Presence) -> &'static str {
    match presence {
        Presence::Active => "●",
        Presence::Away => "○",
        Presence::Unknown => " ",
    }
}

fn has_unreads(ws: &Workspace, c: &Channel) -> bool {
    unread_total(ws, c) > 0
}

fn unread_total(ws: &Workspace, c: &Channel) -> u32 {
    ws.unread_total(c)
}

fn is_vip(ws: &Workspace, c: &Channel) -> bool {
    if ws.priority_sidebar_section && ws.priority_score(&c.id).is_some() {
        return true;
    }
    c.extra.iter().any(|(key, value)| {
        let key = key.to_ascii_lowercase();
        key.contains("vip")
            || key.contains("priority") && value.as_bool().unwrap_or(false)
            || value_names_vip(value)
    })
}

fn value_names_vip(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => value.to_ascii_lowercase().contains("vip"),
        serde_json::Value::Array(values) => values.iter().any(value_names_vip),
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.to_ascii_lowercase().contains("vip") || value_names_vip(value)),
        _ => false,
    }
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
            channels: channels
                .into_iter()
                .map(|channel| (channel.id.clone(), channel))
                .collect::<BTreeMap<_, _>>(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            priority_scores: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            users: HashMap::new(),
            messages,
            typing: HashMap::new(),
            presence: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        }
    }

    fn ids(channels: &[&Channel]) -> Vec<String> {
        channels.iter().map(|channel| channel.id.clone()).collect()
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

        let sections = grouped(&ws, Some("C_ACTIVE"));

        assert_eq!(ids(&sections.vip_unreads), ["C_VIP"]);
        assert!(sections.dms.is_empty());
        assert_eq!(ids(&sections.starred), ["C_STAR"]);
        assert_eq!(ids(&sections.other_unreads), ["C_UNREAD", "C_ACTIVE"]);
    }

    #[test]
    fn uses_slack_sidebar_ordering_sources() {
        let mut ws = workspace(
            vec![
                channel("C_LOW", "alpha-vip"),
                channel("C_HIGH", "zulu-vip"),
                channel("C_STAR_2", "announcements"),
                channel("C_STAR_1", "community"),
                dm("D_LATE", "late"),
                dm("D_FIRST", "first"),
            ],
            &[("C_LOW", 1), ("C_HIGH", 1)],
        );
        ws.priority_sidebar_section = true;
        ws.priority_scores.insert("C_LOW".into(), 0.2);
        ws.priority_scores.insert("C_HIGH".into(), 0.9);
        ws.starred_order = vec!["C_STAR_1".into(), "C_STAR_2".into()];
        ws.dm_order = vec!["D_FIRST".into(), "D_LATE".into()];

        let sections = grouped(&ws, None);

        assert_eq!(ids(&sections.vip_unreads), ["C_HIGH", "C_LOW"]);
        assert_eq!(ids(&sections.starred), ["C_STAR_1", "C_STAR_2"]);
        assert_eq!(ids(&sections.dms), ["D_FIRST", "D_LATE"]);
    }

    #[test]
    fn hides_read_dms_when_slack_prefers_unread_sidebar() {
        let mut ws = workspace(
            vec![dm("D_READ", "read"), dm("D_UNREAD", "unread")],
            &[("D_UNREAD", 1)],
        );
        ws.hide_read_channels_unless_starred = true;

        let sections = grouped(&ws, None);

        assert_eq!(ids(&sections.dms), ["D_UNREAD"]);
    }

    #[test]
    fn channel_labels_use_symbols_not_lock_text() {
        let public = channel("C_PUBLIC", "public-room");
        let mut private = channel("G_PRIVATE", "private-room");
        private.is_private = true;
        let ws = workspace(vec![public, private], &[]);

        assert_eq!(
            sidebar_label(&ws, ws.channels.get("C_PUBLIC").unwrap()),
            "#public-room"
        );
        assert_eq!(
            sidebar_label(&ws, ws.channels.get("G_PRIVATE").unwrap()),
            "🔒 private-room"
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

        let label = sidebar_label(&ws, ws.channels.get("G_MPDM").unwrap());

        assert_eq!(label.trim(), "aarav54897, echo, alanlichen1");
        assert!(!label.contains("lock"));
        assert!(!label.contains("🔒"));
        assert!(!label.contains("mpdm-"));
    }
}
