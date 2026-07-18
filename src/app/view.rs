use iced::widget::{button, column, container, opaque, row, stack, text};
use iced::{Element, Fill};

use crate::state::Screen;
use crate::ui;

use super::{App, Message};

pub(super) fn view(app: &App) -> Element<'_, Message> {
    match app.screen {
        Screen::Login => login_view(),
        Screen::Loading => center_text("Loading…"),
        Screen::Main => with_palette(app, with_profile_hover(app, main_view(app))),
    }
}

fn with_profile_hover<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let Some((ws, hover)) = app.active_workspace().zip(app.profile_hover.as_ref()) else {
        return base;
    };
    if !hover.visible || hover.position.is_none() {
        return base;
    }
    stack![
        base,
        ui::profile::hover_overlay(
            ws,
            hover,
            &app.avatar_previews,
            &app.emoji_previews,
            app.emoji_animation_started.elapsed(),
        )
    ]
    .into()
}

fn overlay_host<'a>(
    base: Element<'a, Message>,
    overlay: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    match overlay {
        Some(layer) => stack![base, layer].into(),
        None => stack![base].into(),
    }
}

fn with_palette<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    match (app.palette.as_ref(), app.active_workspace()) {
        (Some(state), Some(ws)) => {
            ui::palette::modal(base, ws, state, &app.avatar_previews, app.palette_open)
        }
        _ => overlay_host(base, None),
    }
}

fn login_view() -> Element<'static, Message> {
    let card = container(
        column![
            column![
                text("Snack")
                    .size(ui::theme::TEXT_LG)
                    .color(ui::theme::TEXT_1)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::default()
                    }),
                text("Sign in to your Slack workspace.")
                    .size(ui::theme::TEXT_MD)
                    .color(ui::theme::TEXT_2),
                text("Opens a new window for the Slack sign-in flow.")
                    .size(ui::theme::TEXT_SM)
                    .color(ui::theme::TEXT_4),
            ]
            .spacing(ui::theme::SPACE_XS),
            button(text("Sign in").size(ui::theme::TEXT_MD))
                .style(ui::theme::primary_button)
                .padding([ui::theme::SPACE_XS + 2.0, ui::theme::SPACE_MD])
                .on_press(Message::SignInPressed),
            button(text("Reload session").size(ui::theme::TEXT_SM))
                .style(ui::theme::link_button)
                .on_press(Message::RetryAuth),
        ]
        .spacing(ui::theme::SPACE_MD),
    )
    .width(iced::Length::Fixed(300.0))
    .padding(ui::theme::SPACE_MD)
    .style(ui::theme::panel);

    container(container(card).center_x(Fill).center_y(Fill))
        .style(ui::theme::root)
        .width(Fill)
        .height(Fill)
        .into()
}

fn main_view(app: &App) -> Element<'_, Message> {
    let Some(ws) = app.active_workspace() else {
        return center_text("No workspace");
    };

    let rail = ui::rail::view(
        ws,
        &app.avatar_previews,
        app.main_view,
        ui::dms::unread_count_total(ws),
        ui::activity::unread_count(ws),
    );

    if app.main_view == crate::state::MainView::Dms {
        let list = ui::dms::list_panel(
            ws,
            &app.dms,
            app.active_channel.as_deref(),
            &app.avatar_previews,
            &app.emoji_previews,
            app.emoji_animation_started.elapsed(),
        );
        let right: Element<'_, Message> =
            match (app.active_team.as_ref(), app.active_thread.as_ref()) {
                (Some(team), Some((channel, root_ts))) if app.thread_open => {
                    thread_static_panel(app, ws, team, channel, root_ts)
                }
                _ if app.active_channel.is_some() => {
                    channel_main_panel(app, ws, "Select a conversation to start messaging.")
                }
                _ => container(center_text("Select a conversation to start messaging."))
                    .width(Fill)
                    .height(Fill)
                    .style(ui::theme::panel)
                    .into(),
            };
        let body = row![rail, list, right]
            .spacing(ui::theme::gap())
            .width(Fill)
            .height(Fill);
        return with_modal(
            app,
            with_account_menu(app, shell(with_profile_pane(app, ws, body.into()))),
        );
    }

    if app.main_view == crate::state::MainView::Activity {
        let list = ui::activity::list_panel(
            ws,
            &app.activity,
            &app.avatar_previews,
            &app.emoji_previews,
            app.emoji_animation_started.elapsed(),
        );
        let right: Element<'_, Message> =
            match (app.active_team.as_ref(), app.active_thread.as_ref()) {
                (Some(team), Some((channel, root_ts))) => {
                    thread_static_panel(app, ws, team, channel, root_ts)
                }
                _ if app.active_channel.is_some() => {
                    channel_main_panel(app, ws, "Select a notification to view the details.")
                }
                _ => container(center_text("Select a notification to view the details."))
                    .width(Fill)
                    .height(Fill)
                    .style(ui::theme::panel)
                    .into(),
            };
        let body = row![rail, list, right]
            .spacing(ui::theme::gap())
            .width(Fill)
            .height(Fill);
        return with_modal(
            app,
            with_account_menu(app, shell(with_profile_pane(app, ws, body.into()))),
        );
    }

    let sidebar_panel = ui::sidebar::view(
        &app.workspaces,
        app.active_team.as_deref(),
        ws,
        app.active_channel.as_deref(),
        &app.avatar_previews,
        app.settings.sidebar_width,
    );
    // Overlay the drag handle on the sidebar's right edge so it takes no layout
    // width and the panel gap stays identical to every other panel gap.
    let sidebar: Element<'_, Message> = iced::widget::stack![
        sidebar_panel,
        container(resize_handle()).align_right(Fill).height(Fill),
    ]
    .into();

    if let Some(state) = app.search.as_ref() {
        let content = container(ui::search::view(ws, state))
            .width(Fill)
            .height(Fill)
            .style(ui::theme::panel);
        return with_modal(
            app,
            with_account_menu(
                app,
                shell(with_profile_pane(
                    app,
                    ws,
                    row![rail, sidebar, content]
                        .spacing(ui::theme::gap())
                        .width(Fill)
                        .height(Fill)
                        .into(),
                )),
            ),
        );
    }

    let editing_for = |channel_id: &str| -> Option<(&str, &str)> {
        app.editing
            .as_ref()
            .filter(|(channel, _)| channel == channel_id)
            .map(|(_, ts)| (ts.as_str(), app.edit_text.as_str()))
    };

    let hovered_for = |in_thread: bool| -> Option<&str> {
        app.hovered_message
            .as_ref()
            .filter(|(thread, _)| *thread == in_thread)
            .map(|(_, ts)| ts.as_str())
    };
    let emoji_animation_elapsed = app.emoji_animation_started.elapsed();

    let main: Element<'_, Message> = channel_main_panel(app, ws, "Select a channel");

    if let (Some(team), Some((channel, root_ts))) =
        (app.active_team.as_ref(), app.active_thread.as_ref())
    {
        let replies = app
            .threads
            .get(&(team.clone(), channel.clone(), root_ts.clone()));
        let root = ui::thread::root_message(ws, channel, root_ts);
        let open = app.thread_open;
        let unread_marker = thread_unread_marker(app, team, channel, root_ts);
        let thread_key = (channel.as_str(), root_ts.as_str());
        let gap = ui::theme::gap();
        let thread_panel = ui::motion::panel_reveal(open, move |anim, at| {
            let progress = ui::motion::t(anim, at);
            let panel = container(ui::thread::view(
                ws,
                channel,
                root_ts,
                root,
                replies,
                &app.thread_composer,
                &app.thread_composer_attachments,
                &app.file_previews,
                &app.avatar_previews,
                &app.emoji_previews,
                emoji_animation_elapsed,
                editing_for(channel),
                hovered_for(true),
                unread_marker,
                app.text_selection.as_ref(),
                &app.pending_file_messages,
                iced::Length::Fixed(ui::theme::THREAD_WIDTH),
                app.profile_hover.as_ref(),
            ))
            .padding(iced::Padding::ZERO.left(gap));
            ui::motion::collapse_x(panel.into(), progress, ui::theme::THREAD_WIDTH + gap)
        })
        .key(thread_key)
        .on_finish_maybe((!open).then_some(Message::ThreadDismissed));

        let body = row![rail, sidebar, main]
            .spacing(gap)
            .width(Fill)
            .height(Fill);
        with_modal(
            app,
            with_account_menu(
                app,
                shell(with_profile_pane(
                    app,
                    ws,
                    row![body, thread_panel].width(Fill).height(Fill).into(),
                )),
            ),
        )
    } else {
        let body = row![rail, sidebar, main]
            .spacing(ui::theme::gap())
            .width(Fill)
            .height(Fill);
        with_modal(
            app,
            with_account_menu(app, shell(with_profile_pane(app, ws, body.into()))),
        )
    }
}

fn channel_main_panel<'a>(
    app: &'a App,
    ws: &'a crate::state::Workspace,
    empty_label: &'a str,
) -> Element<'a, Message> {
    let emoji_animation_elapsed = app.emoji_animation_started.elapsed();
    match app.active_channel.as_deref() {
        Some(channel_id) => {
            let editing = app
                .editing
                .as_ref()
                .filter(|(channel, _)| channel == channel_id)
                .map(|(_, ts)| (ts.as_str(), app.edit_text.as_str()));
            let hovered = app
                .hovered_message
                .as_ref()
                .filter(|(thread, _)| !thread)
                .map(|(_, ts)| ts.as_str());
            let label = ws
                .channels
                .get(channel_id)
                .map(|c| crate::state::channel_display_name(ws, c))
                .unwrap_or_else(|| channel_id.to_owned());
            let body = column![
                container(ui::channel::view(
                    ws,
                    channel_id,
                    &app.file_previews,
                    &app.avatar_previews,
                    &app.emoji_previews,
                    emoji_animation_elapsed,
                    editing,
                    hovered,
                    app.text_selection.as_ref(),
                    &app.pending_file_messages,
                    app.chat_paused.get(channel_id).copied(),
                    app.profile_hover.as_ref(),
                ))
                .height(Fill),
                container(ui::composer::view(
                    &app.composer,
                    &app.composer_attachments,
                    &label,
                    crate::app::ComposerTarget::Channel,
                ))
                .height(iced::Length::Shrink),
            ]
            .width(Fill)
            .height(Fill);
            container(body)
                .width(Fill)
                .height(Fill)
                .style(ui::theme::panel)
                .into()
        }
        None => container(center_text(empty_label))
            .width(Fill)
            .height(Fill)
            .style(ui::theme::panel)
            .into(),
    }
}

fn thread_static_panel<'a>(
    app: &'a App,
    ws: &'a crate::state::Workspace,
    team: &'a str,
    channel: &'a str,
    root_ts: &'a str,
) -> Element<'a, Message> {
    let emoji_animation_elapsed = app.emoji_animation_started.elapsed();
    let replies = app
        .threads
        .get(&(team.to_owned(), channel.to_owned(), root_ts.to_owned()));
    let root = ui::thread::root_message(ws, channel, root_ts);
    let editing = app
        .editing
        .as_ref()
        .filter(|(c, _)| c == channel)
        .map(|(_, ts)| (ts.as_str(), app.edit_text.as_str()));
    let hovered = app
        .hovered_message
        .as_ref()
        .filter(|(thread, _)| *thread)
        .map(|(_, ts)| ts.as_str());
    ui::thread::view(
        ws,
        channel,
        root_ts,
        root,
        replies,
        &app.thread_composer,
        &app.thread_composer_attachments,
        &app.file_previews,
        &app.avatar_previews,
        &app.emoji_previews,
        emoji_animation_elapsed,
        editing,
        hovered,
        thread_unread_marker(app, team, channel, root_ts),
        app.text_selection.as_ref(),
        &app.pending_file_messages,
        Fill,
        app.profile_hover.as_ref(),
    )
}

fn with_profile_pane<'a>(
    app: &'a App,
    ws: &'a crate::state::Workspace,
    base: Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(profile) = app.profile_pane.as_ref() else {
        return base;
    };
    let open = app.profile_open;
    let gap = ui::theme::gap();
    let profile_key = profile.user.clone();
    let panel = ui::motion::panel_reveal(open, move |anim, at| {
        let progress = ui::motion::t(anim, at);
        let pane = container(ui::profile::pane(
            ws,
            profile,
            &app.avatar_previews,
            &app.profile_previews,
            app.profile_fields
                .get(&ws.team_id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            &app.emoji_previews,
            app.emoji_animation_started.elapsed(),
        ))
        .padding(iced::Padding::ZERO.left(gap));
        ui::motion::collapse_x(pane.into(), progress, ui::profile::PANE_WIDTH + gap)
    })
    .key(profile_key)
    .on_finish_maybe((!open).then_some(Message::ProfilePaneDismissed));
    row![base, panel].width(Fill).height(Fill).into()
}

fn thread_unread_marker<'a>(
    app: &'a App,
    team: &str,
    channel: &str,
    root_ts: &str,
) -> Option<&'a str> {
    app.thread_unread_marker
        .as_ref()
        .filter(|((t, c, r), _)| t == team && c == channel && r == root_ts)
        .map(|(_, ts)| ts.as_str())
}

fn resize_handle<'a>() -> Element<'a, Message> {
    use iced::widget::{Space, mouse_area};
    mouse_area(Space::new().width(8.0).height(Fill))
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .on_press(Message::SidebarResizeStarted)
        .into()
}

fn with_modal<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    if app.show_settings {
        ui::settings::modal(base, &app.settings, app.settings_open)
    } else {
        overlay_host(base, None)
    }
}

fn with_account_menu<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let Some(ws) = app.active_workspace().filter(|_| app.show_account_menu) else {
        return overlay_host(base, None);
    };

    let open = app.account_menu_open;
    let menu = ui::motion::overlay(open, move |anim, at| {
        let progress = ui::motion::t(anim, at);
        let card = ui::motion::fly_y(
            opaque(ui::rail::account_menu(
                ws,
                &app.avatar_previews,
                &app.accounts,
                app.active_account.as_deref(),
            )),
            progress,
            ui::motion::closing(anim),
            8.0,
            ui::motion::ExitEdge::Bottom,
        );
        Element::from(
            container(card)
                .align_left(Fill)
                .align_bottom(Fill)
                .padding([
                    ui::theme::gap() + ui::rail::ICON_SIZE + ui::theme::SPACE_LG,
                    ui::theme::gap() + ui::theme::SPACE_SM,
                ]),
        )
    })
    .on_finish_maybe((!open).then_some(Message::AccountMenuDismissed));

    stack![base, menu].into()
}

fn shell(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content)
        .style(ui::theme::root)
        .padding(ui::theme::gap())
        .width(Fill)
        .height(Fill)
        .into()
}

fn center_text(label: &str) -> Element<'_, Message> {
    container(
        text(label.to_owned())
            .size(ui::theme::TEXT_LG)
            .color(ui::theme::TEXT_3),
    )
    .center_x(Fill)
    .center_y(Fill)
    .into()
}
