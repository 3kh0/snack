use iced::widget::{button, column, container, row, text};
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
    container(
        column![
            text("Snack").size(ui::theme::TEXT_LG),
            text("Sign in to your Slack workspace.").size(ui::theme::TEXT_MD),
            text("Opens a new window for the Slack sign-in flow.").size(ui::theme::TEXT_SM),
            button(text("Sign in").size(ui::theme::TEXT_MD)).on_press(Message::SignInPressed),
            button(text("Reload session").size(ui::theme::TEXT_SM)).on_press(Message::RetryAuth),
        ]
        .spacing(12),
    )
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn main_view(app: &App) -> Element<'_, Message> {
    let Some(ws) = app.active_workspace() else {
        return center_text("No workspace");
    };

    let sidebar = ui::sidebar::view(
        &app.workspaces,
        app.active_team.as_deref(),
        ws,
        app.active_channel.as_deref(),
        &app.search_input,
    );

    if let Some(state) = app.search.as_ref() {
        let content = row![ui::search::view(ws, state)].width(Fill).height(Fill);
        return row![sidebar, content].width(Fill).height(Fill).into();
    }

    let editing_for = |channel_id: &str| -> Option<(&str, &str)> {
        app.editing
            .as_ref()
            .filter(|(channel, _)| channel == channel_id)
            .map(|(_, ts)| (ts.as_str(), app.edit_text.as_str()))
    };

    let main: Element<'_, Message> = match app.active_channel.as_deref() {
        Some(channel_id) => {
            let label = ws
                .channels
                .get(channel_id)
                .map(crate::state::channel_label)
                .unwrap_or_else(|| channel_id.to_owned());
            column![
                container(ui::channel::view(
                    ws,
                    channel_id,
                    &app.file_previews,
                    &app.avatar_previews,
                    editing_for(channel_id),
                ))
                .height(Fill),
                ui::composer::view(&app.composer_text, &label),
            ]
            .width(Fill)
            .height(Fill)
            .into()
        }
        None => center_text("Select a channel"),
    };

    let content = if let (Some(team), Some((channel, root_ts))) =
        (app.active_team.as_ref(), app.active_thread.as_ref())
    {
        let replies = app
            .threads
            .get(&(team.clone(), channel.clone(), root_ts.clone()));
        let root = ui::thread::root_message(ws, channel, root_ts);
        row![
            main,
            ui::thread::view(
                ws,
                channel,
                root,
                replies,
                &app.thread_composer_text,
                &app.file_previews,
                &app.avatar_previews,
                editing_for(channel),
            )
        ]
        .width(Fill)
        .height(Fill)
    } else {
        row![main].width(Fill).height(Fill)
    };

    row![sidebar, content].width(Fill).height(Fill).into()
}

fn center_text(label: &str) -> Element<'_, Message> {
    container(text(label.to_owned()).size(ui::theme::TEXT_LG))
        .center_x(Fill)
        .center_y(Fill)
        .into()
}
