use iced::widget::{column, container, text};
use iced::{Element, Fill};

#[derive(Debug, Default)]
struct App {
    status: String,
}

#[derive(Debug, Clone)]
enum Message {}

pub fn run() -> iced::Result {
    iced::application(
        || App {
            status: "ready".to_owned(),
        },
        update,
        view,
    )
    .title("Snack")
    .centered()
    .run()
}

fn update(_state: &mut App, _message: Message) {}

fn view(state: &App) -> Element<'_, Message> {
    container(column![
        text("Snack").size(36),
        text(&state.status).size(16),
        text("wire me up!").size(14),
    ])
    .center_x(Fill)
    .center_y(Fill)
    .padding(24)
    .into()
}
