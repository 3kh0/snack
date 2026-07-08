use iced::widget::{container, row, text_input};
use iced::{Element, Fill};

use super::theme;
use crate::app::Message;

pub fn view<'a>(value: &str, placeholder_label: &str) -> Element<'a, Message> {
    let input = text_input(&format!("Message {placeholder_label}"), value)
        .on_input(Message::ComposerChanged)
        .on_submit(Message::SendPressed)
        .style(theme::input)
        .padding(theme::SPACE_SM)
        .width(Fill);

    container(row![input].spacing(theme::SPACE_SM))
        .padding(theme::SPACE_MD)
        .into()
}
