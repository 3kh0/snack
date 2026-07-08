use iced::theme::palette::Seed;
use iced::widget::{button, container, scrollable, text_input};
use iced::{Background, Border, Color, Element, Length, Theme};

// Layout metrics (midnight: --gap 12px, rounded panels, thin borders).
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;

pub const TEXT_SM: f32 = 12.0;
pub const TEXT_MD: f32 = 14.0;
pub const TEXT_LG: f32 = 16.0;

pub const SIDEBAR_WIDTH: f32 = 240.0;
pub const THREAD_WIDTH: f32 = 360.0;

/// Gap between panels (mirrors midnight's `--gap`).
pub const GAP: f32 = 12.0;
/// Corner radius of the main panels.
pub const PANEL_RADIUS: f32 = 12.0;
/// Corner radius of buttons / inputs / chips.
pub const CONTROL_RADIUS: f32 = 8.0;

// --- midnight palette (converted from midnight.theme.css) ---

// Backgrounds (bg-4 base .. bg-1 elevated).
pub const BG_BASE: Color = Color::from_rgb(0.085, 0.095, 0.115); // bg-4, window + gaps
pub const BG_PANEL: Color = Color::from_rgb(0.110, 0.123, 0.150); // bg-3, panels
pub const BG_ELEV: Color = Color::from_rgb(0.136, 0.152, 0.184); // bg-2, buttons/inputs
pub const BG_ELEV_HI: Color = Color::from_rgb(0.170, 0.190, 0.230); // bg-1, pressed

// Text hierarchy (text-1 brightest .. text-5 dimmest).
pub const TEXT_1: Color = Color::from_rgb(0.930, 0.943, 0.973); // bright white text
pub const TEXT_2: Color = Color::from_rgb(0.625, 0.675, 0.775); // headings / important
pub const TEXT_3: Color = Color::from_rgb(0.520, 0.573, 0.680); // normal text
pub const TEXT_4: Color = Color::from_rgb(0.340, 0.380, 0.460); // channels / icons
pub const TEXT_5: Color = Color::from_rgb(0.213, 0.238, 0.288); // muted / timestamps

// Accent (blue/cyan — blue-1 .. blue-5).
pub const ACCENT: Color = Color::from_rgb(0.274, 0.683, 0.773); // blue-2
pub const ACCENT_BRIGHT: Color = Color::from_rgb(0.344, 0.745, 0.837); // blue-1
pub const ACCENT_3: Color = Color::from_rgb(0.200, 0.621, 0.710); // blue-3
pub const ACCENT_4: Color = Color::from_rgb(0.107, 0.560, 0.649); // blue-4
pub const ACCENT_5: Color = Color::from_rgb(0.000, 0.502, 0.588); // blue-5

pub const ONLINE: Color = Color::from_rgb(0.289, 0.707, 0.580); // green-2

// Semantic aliases kept for existing call sites.
pub const MUTED: Color = TEXT_5;
pub const SIDEBAR_FG: Color = TEXT_3;
/// Accent used for links / channel labels / attachment titles.
pub const SIDEBAR_ACTIVE_BG: Color = ACCENT;

// Translucent overlays (hover / active) — hsla(220,19%,40%,a).
const OVERLAY: (f32, f32, f32) = (0.33, 0.38, 0.48);
pub const HOVER: Color = Color {
    r: OVERLAY.0,
    g: OVERLAY.1,
    b: OVERLAY.2,
    a: 0.10,
};
pub const ACTIVE: Color = Color {
    r: OVERLAY.0,
    g: OVERLAY.1,
    b: OVERLAY.2,
    a: 0.20,
};
/// Subtle border used around panels and controls.
pub const BORDER: Color = Color {
    r: OVERLAY.0,
    g: OVERLAY.1,
    b: OVERLAY.2,
    a: 0.22,
};

/// Root midnight theme: navy-dark background, blue accent.
pub fn midnight() -> Theme {
    Theme::custom(
        "Midnight".to_owned(),
        Seed {
            background: BG_BASE,
            text: TEXT_1,
            primary: ACCENT,
            success: ONLINE,
            warning: Color::from_rgb(0.80, 0.66, 0.36),
            danger: Color::from_rgb(0.78, 0.42, 0.42),
        },
    )
}

/// Window/background layer behind the panels (the gap color).
pub fn root(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_BASE)),
        text_color: Some(TEXT_3),
        ..container::Style::default()
    }
}

/// A rounded, bordered panel (sidebar / chat / thread).
pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_PANEL)),
        text_color: Some(TEXT_3),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: PANEL_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

/// Backwards-compatible alias used by the sidebar.
pub fn sidebar(theme: &Theme) -> container::Style {
    panel(theme)
}

pub fn channel_row(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let bg = if active {
            Some(Background::Color(ACTIVE))
        } else if hovered {
            Some(Background::Color(HOVER))
        } else {
            None
        };
        button::Style {
            background: bg,
            text_color: if active {
                TEXT_1
            } else if hovered {
                TEXT_2
            } else {
                TEXT_3
            },
            border: Border::default().rounded(CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn reaction_chip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV)),
        text_color: Some(TEXT_2),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn reaction_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let (background, border, text_color) = if active {
            (
                Color {
                    a: 0.20,
                    ..ACCENT
                },
                ACCENT,
                ACCENT_BRIGHT,
            )
        } else if hovered {
            (BG_ELEV_HI, BORDER, TEXT_1)
        } else {
            (BG_ELEV, BORDER, TEXT_2)
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                color: border,
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            ..button::Style::default()
        }
    }
}

pub fn file_attachment(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV)),
        text_color: Some(TEXT_2),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn link_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: None,
        text_color: if hovered { ACCENT_BRIGHT } else { ACCENT },
        ..button::Style::default()
    }
}

/// Secondary (outlined) button, e.g. thread "Close".
pub fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: Some(Background::Color(if hovered { BG_ELEV_HI } else { BG_ELEV })),
        text_color: TEXT_2,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..button::Style::default()
    }
}

/// Filled accent button (primary actions).
pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: Some(Background::Color(if hovered { ACCENT_4 } else { ACCENT_5 })),
        text_color: TEXT_1,
        border: Border::default().rounded(CONTROL_RADIUS),
        ..button::Style::default()
    }
}

/// Rounded, bg-2 text input matching the midnight chatbar.
pub fn input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => ACCENT,
        text_input::Status::Hovered => Color { a: 0.35, ..BORDER },
        _ => BORDER,
    };
    text_input::Style {
        background: Background::Color(BG_ELEV),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        icon: TEXT_4,
        placeholder: TEXT_4,
        value: TEXT_1,
        selection: Color { a: 0.30, ..ACCENT },
    }
}

/// Circular/rounded avatar placeholder background.
pub fn avatar_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV_HI)),
        text_color: Some(ACCENT_BRIGHT),
        border: Border::default().rounded(8.0),
        ..container::Style::default()
    }
}

/// A 1px horizontal divider line using the panel border color.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new().width(Length::Fill).height(1.0))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme| container::Style {
            background: Some(Background::Color(BORDER)),
            ..container::Style::default()
        })
        .into()
}

/// Subtle scrollbar that blends into the panels.
pub fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let dragged = matches!(
        status,
        scrollable::Status::Dragged { .. } | scrollable::Status::Hovered { .. }
    );
    let scroller = scrollable::Scroller {
        background: Background::Color(if dragged {
            Color { a: 0.45, ..ACTIVE }
        } else {
            ACTIVE
        }),
        border: Border::default().rounded(CONTROL_RADIUS),
    };
    let rail = scrollable::Rail {
        background: None,
        border: Border::default().rounded(CONTROL_RADIUS),
        scroller,
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        ..scrollable::default(theme, status)
    }
}
