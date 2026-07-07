use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};
// ty codex
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;

pub const TEXT_SM: f32 = 12.0;
pub const TEXT_MD: f32 = 14.0;
pub const TEXT_LG: f32 = 16.0;

pub const SIDEBAR_WIDTH: f32 = 220.0;

pub const SIDEBAR_BG: Color = Color::from_rgb(0.14, 0.11, 0.16);
pub const SIDEBAR_FG: Color = Color::from_rgb(0.82, 0.80, 0.85);
pub const SIDEBAR_ACTIVE_BG: Color = Color::from_rgb(0.06, 0.44, 0.75);
pub const SIDEBAR_ACTIVE_FG: Color = Color::from_rgb(1.0, 1.0, 1.0);
pub const MUTED: Color = Color::from_rgb(0.55, 0.55, 0.58);
pub const REACTION_BG: Color = Color::from_rgb(0.90, 0.92, 0.96);

pub fn sidebar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SIDEBAR_BG)),
        text_color: Some(SIDEBAR_FG),
        ..container::Style::default()
    }
}

pub fn channel_row(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let bg = if active {
            Some(Background::Color(SIDEBAR_ACTIVE_BG))
        } else if hovered {
            Some(Background::Color(Color {
                a: 0.10,
                ..SIDEBAR_ACTIVE_FG
            }))
        } else {
            None
        };
        button::Style {
            background: bg,
            text_color: if active {
                SIDEBAR_ACTIVE_FG
            } else {
                SIDEBAR_FG
            },
            border: Border::default().rounded(4.0),
            ..button::Style::default()
        }
    }
}

pub fn reaction_chip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(REACTION_BG)),
        border: Border::default().rounded(10.0),
        ..container::Style::default()
    }
}

pub fn reaction_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let background = if active {
            Color::from_rgb(0.82, 0.91, 1.0)
        } else if hovered {
            Color::from_rgb(0.84, 0.88, 0.94)
        } else {
            REACTION_BG
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: Color::from_rgb(0.14, 0.16, 0.20),
            border: Border::default().rounded(10.0),
            ..button::Style::default()
        }
    }
}
