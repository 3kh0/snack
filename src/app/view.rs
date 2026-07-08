use iced::widget::{button, column, container, opaque, row, text};
use iced::{Element, Fill};

use crate::state::Screen;
use crate::ui;

use super::{App, Message};

pub(super) fn view(app: &App) -> Element<'_, Message> {
    match app.screen {
        Screen::Login => login_view(),
        Screen::Loading => center_text("Loading…"),
        Screen::Main => main_view(app),
    }
}

fn login_view() -> Element<'static, Message> {
    let card = container(
        column![
            text("Snack")
                .size(ui::theme::TEXT_LG)
                .color(ui::theme::TEXT_1),
            text("Sign in to your Slack workspace.")
                .size(ui::theme::TEXT_MD)
                .color(ui::theme::TEXT_2),
            text("Opens a new window for the Slack sign-in flow.")
                .size(ui::theme::TEXT_SM)
                .color(ui::theme::TEXT_4),
            button(text("Sign in").size(ui::theme::TEXT_MD))
                .style(ui::theme::primary_button)
                .padding([ui::theme::SPACE_SM, ui::theme::SPACE_LG])
                .on_press(Message::SignInPressed),
            button(text("Reload session").size(ui::theme::TEXT_SM))
                .style(ui::theme::link_button)
                .on_press(Message::RetryAuth),
        ]
        .spacing(ui::theme::SPACE_MD),
    )
    .padding(ui::theme::SPACE_LG * 2.0)
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

    let rail = ui::rail::view(ws, &app.avatar_previews);
    let sidebar_panel = ui::sidebar::view(
        &app.workspaces,
        app.active_team.as_deref(),
        ws,
        app.active_channel.as_deref(),
        &app.search_input,
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
            with_account_menu(app, shell(row![rail, sidebar, content])),
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

    let main: Element<'_, Message> = match app.active_channel.as_deref() {
        Some(channel_id) => {
            let label = ws
                .channels
                .get(channel_id)
                .map(crate::state::channel_label)
                .unwrap_or_else(|| channel_id.to_owned());
            let chat = column![
                container(ui::channel::view(
                    ws,
                    channel_id,
                    &app.file_previews,
                    &app.avatar_previews,
                    &app.emoji_previews,
                    emoji_animation_elapsed,
                    editing_for(channel_id),
                    hovered_for(false),
                ))
                .height(Fill),
                ui::composer::view(&app.composer_text, &label),
            ]
            .width(Fill)
            .height(Fill);
            container(chat)
                .width(Fill)
                .height(Fill)
                .style(ui::theme::panel)
                .into()
        }
        None => container(center_text("Select a channel"))
            .width(Fill)
            .height(Fill)
            .style(ui::theme::panel)
            .into(),
    };

    if let (Some(team), Some((channel, root_ts))) =
        (app.active_team.as_ref(), app.active_thread.as_ref())
    {
        let replies = app
            .threads
            .get(&(team.clone(), channel.clone(), root_ts.clone()));
        let root = ui::thread::root_message(ws, channel, root_ts);
        with_modal(
            app,
            with_account_menu(
                app,
                shell(row![
                    rail,
                    sidebar,
                    main,
                    ui::thread::view(
                        ws,
                        channel,
                        root,
                        replies,
                        &app.thread_composer_text,
                        &app.file_previews,
                        &app.avatar_previews,
                        &app.emoji_previews,
                        emoji_animation_elapsed,
                        editing_for(channel),
                        hovered_for(true),
                    )
                ]),
            ),
        )
    } else {
        with_modal(
            app,
            with_account_menu(app, shell(row![rail, sidebar, main])),
        )
    }
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
        ui::settings::modal(base, &app.settings)
    } else {
        base
    }
}

fn with_account_menu<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let Some(ws) = app.active_workspace().filter(|_| app.show_account_menu) else {
        return base;
    };
    iced::widget::stack![
        base,
        container(opaque(ui::rail::account_menu(ws, &app.avatar_previews)))
            .align_left(Fill)
            .align_bottom(Fill)
            .padding([
                ui::theme::gap() + ui::rail::ICON_SIZE + ui::theme::SPACE_LG,
                ui::theme::gap() + ui::theme::SPACE_SM,
            ]),
    ]
    .into()
}

fn shell(content: iced::widget::Row<'_, Message>) -> Element<'_, Message> {
    container(content.spacing(ui::theme::gap()).width(Fill).height(Fill))
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
