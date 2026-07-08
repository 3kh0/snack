use iced::widget::{Space, button, column, container, text};
use iced::{Alignment, Element, Length};

use super::theme;
use crate::app::Message;

pub const RAIL_WIDTH: f32 = 60.0;

const ICON_SIZE: f32 = 40.0;

pub fn view<'a>() -> Element<'a, Message> {
    let placeholder = container(Space::new())
        .width(Length::Fixed(ICON_SIZE))
        .height(Length::Fixed(ICON_SIZE))
        .style(theme::rail_icon_placeholder);

    let cog = button(
        container(text("⚙").size(theme::TEXT_LG))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(ICON_SIZE))
    .height(Length::Fixed(ICON_SIZE))
    .style(theme::rail_button)
    .on_press(Message::SettingsOpened);

    let body = column![placeholder, cog]
        .spacing(theme::SPACE_SM)
        .align_x(Alignment::Center)
        .padding(theme::SPACE_SM)
        .width(Length::Fill)
        .height(Length::Fill);

    container(body)
        .width(Length::Fixed(RAIL_WIDTH))
        .height(Length::Fill)
        .style(theme::panel)
        .into()
}
