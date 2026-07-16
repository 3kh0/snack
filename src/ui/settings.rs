use iced::widget::{Space, button, column, container, row, slider, stack, text};
use iced::{Alignment, Element, Fill, Length};

use super::{motion, theme};
use crate::app::Message;
use crate::config::{AccentColor, Settings};

const CARD_WIDTH: f32 = 380.0;
const SWATCH_SIZE: f32 = 28.0;

pub fn modal<'a>(
    base: Element<'a, Message>,
    settings: &'a Settings,
    open: bool,
) -> Element<'a, Message> {
    let layers = motion::overlay(open, move |anim, at| {
        let progress = motion::t(anim, at);
        let alpha = motion::fade(progress);
        let scrim = motion::scrim(progress, Message::SettingsClosed);
        let card = motion::zoom_y(card(settings, alpha), progress, -8.0);
        let centered = container(card)
            .center_x(Fill)
            .center_y(Fill)
            .padding(theme::SPACE_MD);

        Element::from(stack![scrim, centered].width(Fill).height(Fill))
    })
    .on_finish_maybe((!open).then_some(Message::SettingsDismissed));

    stack![base, layers].into()
}

fn card<'a>(s: &Settings, alpha: f32) -> Element<'a, Message> {
    let title = text("Settings")
        .size(theme::TEXT_LG)
        .color(theme::fade(theme::TEXT_1, alpha))
        .font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::default()
        });

    let body = column![
        title,
        theme::divider_faded(alpha),
        accent_section(s.accent, alpha),
        slider_row(
            "Panel gap",
            format!("{} px", s.gap as i32),
            4.0..=24.0,
            s.gap,
            Message::SettingsGapChanged,
            alpha,
        ),
        slider_row(
            "Corner radius",
            format!("{} px", s.panel_radius as i32),
            0.0..=20.0,
            s.panel_radius,
            Message::SettingsRadiusChanged,
            alpha,
        ),
        slider_row(
            "Border thickness",
            format!("{} px", s.border_thickness as i32),
            0.0..=4.0,
            s.border_thickness,
            Message::SettingsBorderChanged,
            alpha,
        ),
        theme::divider_faded(alpha),
        actions(alpha),
    ]
    .spacing(theme::SPACE_MD)
    .width(Fill);

    container(body)
        .width(Length::Fixed(CARD_WIDTH))
        .padding(theme::SPACE_MD)
        .style(theme::fade_container(theme::panel, alpha))
        .into()
}

fn accent_section<'a>(selected: AccentColor, alpha: f32) -> Element<'a, Message> {
    let mut swatches = row![].spacing(theme::SPACE_SM).align_y(Alignment::Center);
    for color in AccentColor::ALL {
        swatches = swatches.push(
            button(Space::new())
                .width(Length::Fixed(SWATCH_SIZE))
                .height(Length::Fixed(SWATCH_SIZE))
                .style(theme::fade_button(
                    theme::swatch(theme::accent_swatch(color), color == selected),
                    alpha,
                ))
                .on_press(Message::SettingsAccentSelected(color)),
        );
    }

    column![
        text("Accent")
            .size(theme::TEXT_MD)
            .color(theme::fade(theme::TEXT_2, alpha))
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::default()
            }),
        swatches,
    ]
    .spacing(theme::SPACE_SM)
    .into()
}

fn slider_row<'a>(
    label: &'a str,
    value_label: String,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    on_change: fn(f32) -> Message,
    alpha: f32,
) -> Element<'a, Message> {
    column![
        row![
            text(label)
                .size(theme::TEXT_MD)
                .color(theme::fade(theme::TEXT_2, alpha)),
            Space::new().width(Fill),
            text(value_label)
                .size(theme::TEXT_SM)
                .color(theme::fade(theme::TEXT_4, alpha)),
        ]
        .align_y(Alignment::Center),
        slider(range, value, on_change)
            .step(1.0)
            .style(theme::fade_slider(alpha)),
    ]
    .spacing(theme::SPACE_XS)
    .into()
}

fn actions<'a>(alpha: f32) -> Element<'a, Message> {
    row![
        button(text("Reset").size(theme::TEXT_SM))
            .style(theme::fade_button(theme::secondary_button, alpha))
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .on_press(Message::SettingsReset),
        Space::new().width(Fill),
        button(text("Done").size(theme::TEXT_SM))
            .style(theme::fade_button(theme::primary_button, alpha))
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .on_press(Message::SettingsClosed),
    ]
    .align_y(Alignment::Center)
    .into()
}
