use iced::theme::palette::Seed;
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
pub const THREAD_WIDTH: f32 = 340.0;

// AMOLED: pure black surfaces, white highlights, green accent.
pub const ACCENT: Color = Color::from_rgb(0.13, 0.86, 0.36);
pub const ACCENT_DIM: Color = Color::from_rgb(0.09, 0.58, 0.25);

pub const SIDEBAR_BG: Color = Color::from_rgb(0.0, 0.0, 0.0);
pub const SIDEBAR_FG: Color = Color::from_rgb(0.90, 0.90, 0.90);
pub const SIDEBAR_ACTIVE_BG: Color = ACCENT;
pub const SIDEBAR_ACTIVE_FG: Color = Color::from_rgb(0.0, 0.0, 0.0);
pub const MUTED: Color = Color::from_rgb(0.50, 0.50, 0.50);
pub const REACTION_BG: Color = Color::from_rgb(0.08, 0.08, 0.08);

// Root AMOLED theme: pure black background, white text, green accent.
pub fn amoled() -> Theme {
    Theme::custom(
        "AMOLED".to_owned(),
        Seed {
            background: Color::from_rgb(0.0, 0.0, 0.0),
            text: Color::from_rgb(0.92, 0.92, 0.92),
            primary: ACCENT,
            success: ACCENT,
            warning: Color::from_rgb(0.85, 0.65, 0.20),
            danger: Color::from_rgb(0.90, 0.30, 0.30),
        },
    )
}

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
                a: 0.14,
                ..Color::from_rgb(1.0, 1.0, 1.0)
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
        text_color: Some(Color::from_rgb(0.90, 0.90, 0.90)),
        border: Border {
            color: Color::from_rgb(0.20, 0.20, 0.20),
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn reaction_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let background = if active {
            ACCENT_DIM
        } else if hovered {
            Color::from_rgb(0.16, 0.16, 0.16)
        } else {
            REACTION_BG
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: if active {
                Color::from_rgb(0.0, 0.0, 0.0)
            } else {
                Color::from_rgb(0.90, 0.90, 0.90)
            },
            border: Border {
                color: Color::from_rgb(0.20, 0.20, 0.20),
                width: 1.0,
                radius: 10.0.into(),
            },
            ..button::Style::default()
        }
    }
}

pub fn file_attachment(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.05, 0.05, 0.05))),
        text_color: Some(Color::from_rgb(0.90, 0.90, 0.90)),
        border: Border {
            color: Color::from_rgb(0.20, 0.20, 0.20),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn link_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: None,
        text_color: if hovered { ACCENT_DIM } else { ACCENT },
        ..button::Style::default()
    }
}

pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.0, 0.0, 0.0))),
        text_color: Some(Color::from_rgb(0.90, 0.90, 0.90)),
        border: Border {
            color: Color::from_rgb(0.16, 0.16, 0.16),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
