use std::collections::HashMap;
use std::time::Duration;

use chrono::{FixedOffset, Utc};
use iced::widget::{
    Column, Row, Space, button, column, container, float, image, mouse_area, opaque, row,
    scrollable, text,
};
use iced::{Alignment, ContentFit, Element, Fill, Font, Length, Vector, font};

use super::{message, theme};
use crate::app::{FilePreview, Message, ProfileHoverState, ProfilePaneState};
use crate::slack::models::{TeamProfileField, User, UserId};
use crate::state::{self, Presence, Workspace};

pub const PANE_WIDTH: f32 = 372.0;
const CARD_WIDTH: f32 = 340.0;

type Previews = HashMap<UserId, FilePreview>;

pub fn trigger<'a>(
    content: Element<'a, Message>,
    _ws: &'a Workspace,
    user: &'a str,
    key: impl Into<String>,
    _hover: Option<&'a ProfileHoverState>,
    _avatars: &'a Previews,
) -> Element<'a, Message> {
    let key = key.into();
    let source = mouse_area(
        button(content)
            .padding(0)
            .style(theme::link_button)
            .on_press(Message::ProfilePressed(user.to_owned())),
    )
    .interaction(iced::mouse::Interaction::Pointer)
    .on_enter(Message::ProfileHoverEntered {
        user: user.to_owned(),
        key: key.clone(),
    })
    .on_exit(Message::ProfileHoverExited {
        user: user.to_owned(),
        key: key.clone(),
    });

    source.into()
}

pub fn hover_overlay<'a>(
    ws: &'a Workspace,
    hover: &'a ProfileHoverState,
    avatars: &'a Previews,
    emoji_previews: &'a HashMap<String, FilePreview>,
    emoji_elapsed: Duration,
) -> Element<'a, Message> {
    let point = hover.position.unwrap_or_default();
    let card = mouse_area(opaque(hover_card(
        ws,
        &hover.user,
        avatars,
        emoji_previews,
        emoji_elapsed,
    )))
    .on_enter(Message::ProfileCardEntered)
    .on_exit(Message::ProfileCardExited);
    float(card)
        .translate(move |bounds, viewport| {
            let margin = theme::SPACE_SM;
            let mut x = point.x + 16.0;
            let mut y = point.y + 18.0;
            if x + bounds.width > viewport.x + viewport.width - margin {
                x = point.x - bounds.width - 16.0;
            }
            if y + bounds.height > viewport.y + viewport.height - margin {
                y = point.y - bounds.height - 16.0;
            }
            Vector::new(x - bounds.x, y - bounds.y)
        })
        .into()
}

fn hover_card<'a>(
    ws: &'a Workspace,
    user_id: &'a str,
    avatars: &'a Previews,
    emoji_previews: &'a HashMap<String, FilePreview>,
    emoji_elapsed: Duration,
) -> Element<'a, Message> {
    let user = ws.users.get(user_id);
    let name = state::display_name(user, user_id);
    let profile = user.and_then(|user| user.profile.as_ref());
    let avatar = profile_avatar(user, user_id, avatars, &name, 72.0, 12.0);

    let mut identity = Column::new().spacing(2.0).width(Fill).push(
        button(name_line(ws, user_id, &name, user))
            .padding(0)
            .style(theme::link_button)
            .on_press(Message::ProfilePressed(user_id.to_owned())),
    );
    if let Some(title) = profile.and_then(|profile| non_empty(profile.title.as_deref())) {
        identity = identity.push(text(title).size(theme::TEXT_MD).color(theme::TEXT_3));
    }
    if let Some(pronouns) = profile.and_then(|profile| non_empty(profile.pronouns.as_deref())) {
        identity = identity.push(text(pronouns).size(theme::TEXT_SM).color(theme::TEXT_4));
    }

    let mut body = column![
        row![
            button(avatar)
                .padding(0)
                .style(theme::link_button)
                .on_press(Message::ProfilePressed(user_id.to_owned())),
            identity,
        ]
        .spacing(theme::SPACE_MD)
        .align_y(Alignment::Start),
    ]
    .spacing(theme::SPACE_MD);

    if let Some(status) = status_line(ws, profile, emoji_previews, emoji_elapsed) {
        body = body.push(status);
    }
    if let Some(local) = local_time(user) {
        body = body.push(
            text(format!("◷  {local}"))
                .size(theme::TEXT_MD)
                .color(theme::TEXT_2),
        );
    }
    body = body.push(actions(ws, user_id, true));

    container(body)
        .width(Length::Fixed(CARD_WIDTH))
        .padding(theme::SPACE_MD)
        .style(theme::profile_card)
        .into()
}

pub fn pane<'a>(
    ws: &'a Workspace,
    state: &'a ProfilePaneState,
    avatars: &'a Previews,
    profile_previews: &'a Previews,
    profile_fields: &'a [TeamProfileField],
    emoji_previews: &'a HashMap<String, FilePreview>,
    emoji_elapsed: Duration,
) -> Element<'a, Message> {
    let user = ws.users.get(&state.user);
    let name = state::display_name(user, &state.user);
    let profile = user.and_then(|user| user.profile.as_ref());

    let header = row![
        text("Profile")
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(Font {
                weight: font::Weight::Bold,
                ..Font::default()
            }),
        Space::new().width(Fill),
        button(text("×").size(28.0).color(theme::TEXT_3))
            .padding([0.0, theme::SPACE_XS])
            .style(theme::link_button)
            .on_press(Message::ProfileDismissed),
    ]
    .align_y(Alignment::Center);

    let hero_previews = if matches!(
        profile_previews.get(&state.user),
        Some(FilePreview::Loaded(_))
    ) {
        profile_previews
    } else {
        avatars
    };
    let hero = profile_avatar(user, &state.user, hero_previews, &name, 300.0, 14.0);
    let mut identity = column![name_line(ws, &state.user, &name, user),].spacing(theme::SPACE_XS);
    if let Some(title) = profile.and_then(|profile| non_empty(profile.title.as_deref())) {
        identity = identity.push(text(title).size(theme::TEXT_LG).color(theme::TEXT_2));
    }
    if let Some(pronouns) = profile.and_then(|profile| non_empty(profile.pronouns.as_deref())) {
        identity = identity.push(text(pronouns).size(theme::TEXT_MD).color(theme::TEXT_3));
    }
    identity = identity.push(presence_line(ws, &state.user));
    if let Some(status) = status_line(ws, profile, emoji_previews, emoji_elapsed) {
        identity = identity.push(status);
    }
    if let Some(local) = local_time(user) {
        identity = identity.push(
            text(format!("◷  {local}"))
                .size(theme::TEXT_MD)
                .color(theme::TEXT_2),
        );
    }

    let mut content = Column::new()
        .spacing(theme::SPACE_MD)
        .push(hero)
        .push(identity)
        .push(actions(ws, &state.user, false));
    if state.loading {
        content = content.push(
            text("Loading profile details…")
                .size(theme::TEXT_SM)
                .color(theme::TEXT_4),
        );
    }
    if let Some(error) = &state.error {
        content = content.push(text(error).size(theme::TEXT_SM).color(theme::TEXT_4));
    }
    if let Some(section) = contact_section(profile) {
        content = content.push(theme::divider()).push(section);
    }
    if let Some(section) = recent_dms(ws, &state.user) {
        content = content.push(theme::divider()).push(section);
    }
    if let Some(section) = about_section(ws, profile, profile_fields) {
        content = content.push(theme::divider()).push(section);
    }

    let body = scrollable(container(content).padding(theme::SPACE_MD).width(Fill))
        .style(theme::scrollbar)
        .height(Fill);
    container(column![
        container(header).padding([theme::SPACE_SM, theme::SPACE_MD]),
        theme::divider(),
        body
    ])
    .width(Length::Fixed(PANE_WIDTH))
    .height(Fill)
    .style(theme::panel)
    .into()
}

fn name_line<'a>(
    ws: &Workspace,
    user_id: &str,
    name: &str,
    user: Option<&User>,
) -> Element<'a, Message> {
    let mut line = Row::new()
        .spacing(theme::SPACE_XS)
        .align_y(Alignment::Center)
        .push(
            text(name.to_owned())
                .size(theme::TEXT_LG)
                .color(theme::TEXT_1)
                .font(Font {
                    weight: font::Weight::Bold,
                    ..Font::default()
                }),
        );
    if user.is_some_and(|user| user.is_bot) {
        line = line.push(badge("APP"));
    }
    if ws.vip_users.contains(user_id) {
        line = line.push(badge("VIP"));
    }
    if user.is_some_and(|user| user.deleted) {
        line = line.push(badge("DEACTIVATED"));
    }
    line.into()
}

fn badge<'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(10.0).font(Font {
        weight: font::Weight::Bold,
        ..Font::default()
    }))
    .padding([2.0, 5.0])
    .style(theme::profile_badge)
    .into()
}

fn actions<'a>(ws: &Workspace, user: &str, compact: bool) -> Element<'a, Message> {
    let own = ws.self_user_id == user;
    let padding = if compact { [5.0, 10.0] } else { [8.0, 12.0] };
    let mut actions = Row::new().spacing(theme::SPACE_SM).width(Fill);
    if !own {
        actions = actions.push(
            button(text("Message").size(theme::TEXT_MD))
                .padding(padding)
                .style(theme::secondary_button)
                .on_press(Message::ProfileMessagePressed(user.to_owned())),
        );
    }
    actions = actions.push(
        button(text("Huddle").size(theme::TEXT_MD))
            .padding(padding)
            .style(theme::secondary_button),
    );
    if !own {
        actions = actions.push(
            button(text("VIP").size(theme::TEXT_MD))
                .padding(padding)
                .style(theme::secondary_button),
        );
    }
    actions.into()
}

fn contact_section<'a>(
    profile: Option<&'a crate::slack::models::UserProfile>,
) -> Option<Element<'a, Message>> {
    let profile = profile?;
    let mut rows = Column::new().spacing(theme::SPACE_SM);
    let mut any = false;
    if let Some(email) = non_empty(profile.email.as_deref()) {
        rows = rows.push(field("Email", email));
        any = true;
    }
    if let Some(phone) = non_empty(profile.phone.as_deref()) {
        rows = rows.push(field("Phone", phone));
        any = true;
    }
    any.then(|| section("Contact information", rows))
}

fn recent_dms<'a>(ws: &'a Workspace, user: &str) -> Option<Element<'a, Message>> {
    let mut channels: Vec<_> = ws
        .channels
        .values()
        .filter(|channel| channel.is_im && state::dm_user_id(channel) == Some(user))
        .collect();
    channels.sort_by(|a, b| {
        let a = ws
            .messages
            .get(&a.id)
            .and_then(|messages| messages.latest_ts());
        let b = ws
            .messages
            .get(&b.id)
            .and_then(|messages| messages.latest_ts());
        state::cmp_ts(b.as_deref(), a.as_deref())
    });
    let channel = channels.first()?;
    let entry = button(
        row![
            text(ws.display_name(user)).size(theme::TEXT_MD),
            Space::new().width(Fill),
            text("Open").size(theme::TEXT_SM).color(theme::accent()),
        ]
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(theme::SPACE_SM)
    .style(theme::channel_row(false))
    .on_press(Message::ChannelSelected(channel.id.clone()));
    Some(section("Recent DMs", Column::new().push(entry)))
}

fn about_section<'a>(
    ws: &'a Workspace,
    profile: Option<&'a crate::slack::models::UserProfile>,
    schema: &'a [TeamProfileField],
) -> Option<Element<'a, Message>> {
    let profile = profile?;
    let mut rows = Column::new().spacing(theme::SPACE_SM);
    let mut any = false;
    let schema_has_start_date = schema.iter().any(|field| {
        field
            .label
            .as_deref()
            .is_some_and(|label| label.eq_ignore_ascii_case("start date"))
    });
    if !schema_has_start_date && let Some(start) = valid_start_date(profile.start_date.as_deref()) {
        rows = rows.push(field("Start date", start));
        any = true;
    }
    for definition in schema.iter().filter(|field| !field.is_hidden) {
        let Some(value) = profile.fields.get(&definition.id) else {
            continue;
        };
        let Some(value) = profile_field_text(ws, definition, value) else {
            continue;
        };
        let label = definition
            .label
            .as_deref()
            .and_then(|label| non_empty(Some(label)))
            .unwrap_or("Profile field");
        rows = rows.push(field(label, value));
        any = true;
    }
    any.then(|| section("About me", rows))
}

fn profile_field_text(
    ws: &Workspace,
    definition: &TeamProfileField,
    field: &crate::slack::models::ProfileFieldValue,
) -> Option<String> {
    let raw = field
        .value
        .as_str()
        .and_then(|value| non_empty(Some(value)))
        .map(str::to_owned)
        .or_else(|| {
            field
                .alt
                .as_deref()
                .and_then(|value| non_empty(Some(value)))
                .map(str::to_owned)
        })?;
    if definition
        .field_type
        .as_deref()
        .is_some_and(|kind| kind.contains("user"))
    {
        Some(ws.display_name(&raw))
    } else if definition
        .field_type
        .as_deref()
        .is_some_and(|kind| kind.contains("date"))
    {
        format_profile_date(&raw)
    } else {
        Some(raw)
    }
}

fn format_profile_date(raw: &str) -> Option<String> {
    use chrono::TimeZone;
    match raw.parse::<i64>() {
        Ok(value) if value > 0 => Utc
            .timestamp_opt(value, 0)
            .single()
            .map(|date| date.format("%Y-%m-%d").to_string()),
        Ok(_) => None,
        Err(_) => valid_start_date(Some(raw)).map(str::to_owned),
    }
}

fn valid_start_date(value: Option<&str>) -> Option<&str> {
    non_empty(value).filter(|value| *value != "0" && !value.starts_with("1970-01-01"))
}

fn section<'a>(title: &'a str, rows: Column<'a, Message>) -> Element<'a, Message> {
    column![
        text(title)
            .size(theme::TEXT_LG)
            .color(theme::TEXT_1)
            .font(Font {
                weight: font::Weight::Bold,
                ..Font::default()
            }),
        rows,
    ]
    .spacing(theme::SPACE_SM)
    .into()
}

fn field<'a>(label: impl Into<String>, value: impl Into<String>) -> Element<'a, Message> {
    let label = label.into();
    let value = value.into();
    column![
        text(label)
            .size(theme::TEXT_SM)
            .color(theme::TEXT_4)
            .font(Font {
                weight: font::Weight::Semibold,
                ..Font::default()
            }),
        text(value).size(theme::TEXT_MD).color(theme::TEXT_2),
    ]
    .spacing(2.0)
    .into()
}

fn status_line<'a>(
    ws: &'a Workspace,
    profile: Option<&'a crate::slack::models::UserProfile>,
    emoji_previews: &'a HashMap<String, FilePreview>,
    emoji_elapsed: Duration,
) -> Option<Element<'a, Message>> {
    let profile = profile?;
    let text_value = non_empty(profile.status_text.as_deref());
    let emoji = non_empty(profile.status_emoji.as_deref()).map(|emoji| emoji.trim_matches(':'));
    if text_value.is_none() && emoji.is_none() {
        return None;
    }
    let mut line = Row::new()
        .spacing(theme::SPACE_XS)
        .align_y(Alignment::Center);
    if let Some(emoji) = emoji {
        line = line.push(message::emoji_inline(
            ws,
            emoji,
            emoji_previews,
            emoji_elapsed,
            theme::TEXT_MD,
        ));
    }
    if let Some(value) = text_value {
        line = line.push(text(value).size(theme::TEXT_MD).color(theme::TEXT_2));
    }
    Some(line.into())
}

fn presence_line<'a>(ws: &Workspace, user: &str) -> Element<'a, Message> {
    let presence = ws.presence.get(user).copied().unwrap_or(Presence::Unknown);
    let (dot, label) = match presence {
        Presence::Active => ("●", "Active"),
        Presence::Away => ("○", "Away"),
        Presence::Unknown => ("○", "Offline"),
    };
    text(format!("{dot}  {label}"))
        .size(theme::TEXT_MD)
        .color(if presence == Presence::Active {
            theme::accent()
        } else {
            theme::TEXT_3
        })
        .into()
}

fn local_time(user: Option<&User>) -> Option<String> {
    let offset = user?.tz_offset?;
    let offset = FixedOffset::east_opt(offset)?;
    Some(
        Utc::now()
            .with_timezone(&offset)
            .format("%H:%M local time")
            .to_string(),
    )
}

fn profile_avatar<'a>(
    user: Option<&User>,
    user_id: &str,
    previews: &Previews,
    name: &str,
    size: f32,
    radius: f32,
) -> Element<'a, Message> {
    if let Some(FilePreview::Loaded(handle)) = previews.get(user_id) {
        return image(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .content_fit(ContentFit::Cover)
            .border_radius(radius)
            .into();
    }
    message::avatar_with_size(
        Some(user_id),
        user.and_then(state::user_avatar_url),
        previews,
        name.chars().next(),
        size,
        radius,
    )
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
