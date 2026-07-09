use std::sync::{LazyLock, RwLock};

use iced::theme::palette::Seed;
use iced::widget::{button, container, scrollable, slider, text_input};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme, Vector};

use crate::config::{AccentColor, Settings};

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;

pub const TEXT_SM: f32 = 12.0;
pub const TEXT_MD: f32 = 14.0;
pub const TEXT_LG: f32 = 16.0;

pub const SIDEBAR_WIDTH: f32 = 240.0;
pub const THREAD_WIDTH: f32 = 360.0;

pub const CONTROL_RADIUS: f32 = 8.0;

// midnight theme (converted from https://github.com/refact0r/midnight-discord)

pub const BG_BASE: Color = Color::from_rgb(0.085, 0.095, 0.115); // bg-4, window + gaps
pub const BG_PANEL: Color = Color::from_rgb(0.110, 0.123, 0.150); // bg-3, panels
pub const BG_ELEV: Color = Color::from_rgb(0.136, 0.152, 0.184); // bg-2, buttons/inputs
pub const BG_ELEV_HI: Color = Color::from_rgb(0.170, 0.190, 0.230); // bg-1, pressed

pub const TEXT_1: Color = Color::from_rgb(0.930, 0.943, 0.973); // bright white text
pub const TEXT_2: Color = Color::from_rgb(0.625, 0.675, 0.775); // headings / important
pub const TEXT_3: Color = Color::from_rgb(0.520, 0.573, 0.680); // normal text
pub const TEXT_4: Color = Color::from_rgb(0.340, 0.380, 0.460); // channels / icons
pub const TEXT_5: Color = Color::from_rgb(0.213, 0.238, 0.288); // muted / timestamps

pub const ONLINE: Color = Color::from_rgb(0.289, 0.707, 0.580); // green-2
pub const PING: Color = Color::from_rgb(0.851, 0.278, 0.310); // discord-ish red badge
pub const MENTION_FG: Color = Color::from_rgb(0.150, 0.742, 0.957);
pub const MENTION_BG: Color = Color {
    r: 0.080,
    g: 0.365,
    b: 0.520,
    a: 0.52,
};
pub const BROADCAST_FG: Color = Color::from_rgb(1.0, 0.765, 0.160);
pub const BROADCAST_BG: Color = Color {
    r: 0.520,
    g: 0.355,
    b: 0.020,
    a: 0.58,
};

pub const SIDEBAR_ICON: f32 = 16.0; // svg glyph size
pub const SIDEBAR_ICON_SLOT: f32 = 24.0; // fixed leading column so labels align
pub const SIDEBAR_AVATAR: f32 = 20.0; // dm avatar / group-count chip
pub const SIDEBAR_AVATAR_RADIUS: f32 = 4.5;
pub const PRESENCE_DOT: f32 = 8.0; // online/offline indicator on avatars
pub const PING_BADGE_H: f32 = 18.0; // normalized ping badge height

pub const MUTED: Color = TEXT_5;
pub const SIDEBAR_FG: Color = TEXT_3;

#[derive(Clone, Copy)]
struct Vars {
    accent: [Color; 5],
    gap: f32,
    panel_radius: f32,
    border_thickness: f32,
}

impl Default for Vars {
    fn default() -> Self {
        Vars {
            accent: accent_ramp(AccentColor::Blue),
            gap: 12.0,
            panel_radius: 12.0,
            border_thickness: 1.0,
        }
    }
}

static VARS: LazyLock<RwLock<Vars>> = LazyLock::new(|| RwLock::new(Vars::default()));

fn vars() -> Vars {
    *VARS.read().expect("theme vars poisoned")
}

pub fn apply(settings: &Settings) {
    *VARS.write().expect("theme vars poisoned") = Vars {
        accent: accent_ramp(settings.accent),
        gap: settings.gap,
        panel_radius: settings.panel_radius,
        border_thickness: settings.border_thickness,
    };
}

pub fn accent() -> Color {
    vars().accent[1]
}
pub fn accent_bright() -> Color {
    vars().accent[0]
}
/// Translucent accent used to paint the text-selection highlight.
pub fn selection() -> Color {
    Color {
        a: 0.30,
        ..vars().accent[1]
    }
}
pub fn accent_3() -> Color {
    vars().accent[2]
}
pub fn accent_4() -> Color {
    vars().accent[3]
}
pub fn accent_5() -> Color {
    vars().accent[4]
}
pub fn gap() -> f32 {
    vars().gap
}
pub fn panel_radius() -> f32 {
    vars().panel_radius
}
pub fn border_thickness() -> f32 {
    vars().border_thickness
}

pub fn accent_swatch(color: AccentColor) -> Color {
    accent_ramp(color)[1]
}

fn accent_ramp(color: AccentColor) -> [Color; 5] {
    let (chroma, hue, lightness) = match color {
        AccentColor::Blue => (0.10, 215.0, [75.0, 70.0, 65.0, 60.0, 55.0]),
        AccentColor::Red => (0.12, 0.0, [75.0, 70.0, 65.0, 60.0, 55.0]),
        AccentColor::Green => (0.11, 170.0, [75.0, 70.0, 65.0, 60.0, 55.0]),
        AccentColor::Yellow => (0.11, 90.0, [80.0, 75.0, 70.0, 65.0, 60.0]),
        AccentColor::Purple => (0.11, 310.0, [75.0, 70.0, 65.0, 60.0, 55.0]),
    };
    std::array::from_fn(|i| oklch_to_color(lightness[i] / 100.0, chroma, hue))
}

fn oklch_to_color(l: f32, c: f32, hue_deg: f32) -> Color {
    let h = hue_deg.to_radians();
    let a = c * h.cos();
    let b = c * h.sin();

    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;

    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;

    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let b = -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;

    Color::from_rgb(linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b))
}

fn linear_to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

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
pub const BORDER: Color = Color {
    r: OVERLAY.0,
    g: OVERLAY.1,
    b: OVERLAY.2,
    a: 0.22,
};

pub fn midnight() -> Theme {
    Theme::custom(
        "Midnight".to_owned(),
        Seed {
            background: BG_BASE,
            text: TEXT_1,
            primary: accent(),
            success: ONLINE,
            warning: Color::from_rgb(0.80, 0.66, 0.36),
            danger: Color::from_rgb(0.78, 0.42, 0.42),
        },
    )
}

pub fn root(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_BASE)),
        text_color: Some(TEXT_3),
        ..container::Style::default()
    }
}

pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_PANEL)),
        text_color: Some(TEXT_3),
        border: Border {
            color: BORDER,
            width: border_thickness(),
            radius: panel_radius().into(),
        },
        ..container::Style::default()
    }
}

pub fn sidebar(theme: &Theme) -> container::Style {
    panel(theme)
}

pub fn channel_row(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let (bg, text_color) = match (active, status) {
            (true, button::Status::Pressed) => {
                (Some(Background::Color(Color { a: 0.28, ..ACTIVE })), TEXT_1)
            }
            (true, _) => (Some(Background::Color(ACTIVE)), TEXT_1),
            (false, button::Status::Pressed) => {
                (Some(Background::Color(Color { a: 0.16, ..HOVER })), TEXT_1)
            }
            (false, button::Status::Hovered) => (Some(Background::Color(HOVER)), TEXT_2),
            (false, _) => (None, TEXT_3),
        };
        button::Style {
            background: bg,
            text_color,
            border: Border::default().rounded(CONTROL_RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn ping_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PING)),
        text_color: Some(TEXT_1),
        border: Border::default().rounded(999.0),
        ..container::Style::default()
    }
}

pub fn app_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: 0.06, ..TEXT_1 })),
        text_color: Some(Color::from_rgb(0.725, 0.729, 0.741)),
        border: Border::default().rounded(3.0),
        ..container::Style::default()
    }
}

pub fn vip_badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: 0.92, ..TEXT_1 })),
        text_color: Some(BG_BASE),
        border: Border::default().rounded(4.0),
        ..container::Style::default()
    }
}

pub fn sidebar_icon(
    color: Color,
) -> impl Fn(&Theme, iced::widget::svg::Status) -> iced::widget::svg::Style {
    move |_theme, _status| iced::widget::svg::Style { color: Some(color) }
}

/// Floating Slack-style action bar overlaid on a hovered message.
pub fn action_bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV)),
        text_color: Some(TEXT_2),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        shadow: Shadow {
            color: Color { a: 0.35, ..BG_BASE },
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..container::Style::default()
    }
}

/// Compact icon/text button used inside the [`action_bar`].
pub fn action_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: hovered.then_some(Background::Color(HOVER)),
        text_color: if hovered { TEXT_1 } else { TEXT_3 },
        border: Border::default().rounded(CONTROL_RADIUS - 3.0),
        ..button::Style::default()
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

pub fn date_separator_label(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_BASE)),
        text_color: Some(TEXT_2),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn inline_mention(broadcast: bool) -> impl Fn(&Theme) -> container::Style {
    move |_theme| {
        let (background, text_color) = if broadcast {
            (BROADCAST_BG, BROADCAST_FG)
        } else {
            (MENTION_BG, MENTION_FG)
        };
        container::Style {
            background: Some(Background::Color(background)),
            text_color: Some(text_color),
            border: Border::default().rounded(4.0),
            ..container::Style::default()
        }
    }
}

pub fn inline_mention_button(broadcast: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let (background, text_color) = if broadcast {
            (BROADCAST_BG, BROADCAST_FG)
        } else {
            (MENTION_BG, MENTION_FG)
        };
        button::Style {
            background: Some(Background::Color(
                if matches!(status, button::Status::Hovered) {
                    Color {
                        a: 0.78,
                        ..background
                    }
                } else {
                    background
                },
            )),
            text_color,
            border: Border::default().rounded(4.0),
            ..button::Style::default()
        }
    }
}

pub fn reaction_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let (background, border, text_color) = if active {
            (
                Color {
                    a: 0.20,
                    ..accent()
                },
                accent(),
                accent_bright(),
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
        text_color: if hovered { accent_bright() } else { accent() },
        ..button::Style::default()
    }
}

pub fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: Some(Background::Color(if hovered {
            BG_ELEV_HI
        } else {
            BG_ELEV
        })),
        text_color: TEXT_2,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..button::Style::default()
    }
}

pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: Some(Background::Color(if hovered {
            accent_4()
        } else {
            accent_5()
        })),
        text_color: TEXT_1,
        border: Border::default().rounded(CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => accent(),
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
        selection: Color {
            a: 0.30,
            ..accent()
        },
    }
}

pub fn presence_online(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ONLINE)),
        border: Border {
            color: BG_PANEL,
            width: 1.5,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn presence_offline(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_PANEL)),
        border: Border {
            color: TEXT_4,
            width: 1.5,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn avatar_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV_HI)),
        text_color: Some(accent_bright()),
        border: Border::default().rounded(SIDEBAR_AVATAR_RADIUS),
        ..container::Style::default()
    }
}

pub fn account_avatar_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(accent_5())),
        text_color: Some(TEXT_1),
        border: Border::default().rounded(8.0),
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.02
    }

    #[test]
    fn oklch_matches_midnight_blue_ramp() {
        let c = accent_swatch(AccentColor::Blue);
        assert!(close(c.r, 0.274), "r={}", c.r);
        assert!(close(c.g, 0.683), "g={}", c.g);
        assert!(close(c.b, 0.773), "b={}", c.b);
    }

    #[test]
    fn accent_families_differ_in_hue() {
        let blue = accent_swatch(AccentColor::Blue);
        let red = accent_swatch(AccentColor::Red);
        assert!(red.r > red.b);
        assert!(blue.b > blue.r);
    }
}

pub fn rail_icon_placeholder(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV_HI)),
        text_color: Some(TEXT_4),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn rail_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: hovered.then_some(Background::Color(HOVER)),
        text_color: if hovered { accent_bright() } else { TEXT_3 },
        border: Border::default().rounded(CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn account_menu(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_ELEV)),
        text_color: Some(TEXT_2),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        shadow: Shadow {
            color: Color { a: 0.35, ..BG_BASE },
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

pub fn account_menu_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered);
    button::Style {
        background: hovered.then_some(Background::Color(HOVER)),
        text_color: if hovered { TEXT_1 } else { TEXT_2 },
        border: Border::default().rounded(CONTROL_RADIUS - 2.0),
        ..button::Style::default()
    }
}

pub fn overlay_dim(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.55,
        })),
        ..container::Style::default()
    }
}

pub fn swatch(color: Color, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let border_color = if selected {
            TEXT_1
        } else if hovered {
            Color { a: 0.6, ..TEXT_2 }
        } else {
            Color { a: 0.0, ..color }
        };
        button::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: border_color,
                width: if selected { 2.5 } else { 1.5 },
                radius: CONTROL_RADIUS.into(),
            },
            ..button::Style::default()
        }
    }
}

pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    divider_faded(1.0)
}

pub fn divider_faded<'a, Message: 'a>(alpha: f32) -> Element<'a, Message> {
    container(iced::widget::Space::new().width(Length::Fill).height(1.0))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_theme| container::Style {
            background: Some(Background::Color(BORDER.scale_alpha(alpha))),
            ..container::Style::default()
        })
        .into()
}

pub fn fade(color: Color, alpha: f32) -> Color {
    color.scale_alpha(alpha)
}

pub fn fade_container(
    style: impl Fn(&Theme) -> container::Style,
    alpha: f32,
) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let mut s = style(theme);
        s.background = s.background.map(|b| b.scale_alpha(alpha));
        s.text_color = s.text_color.map(|c| c.scale_alpha(alpha));
        s.border.color = s.border.color.scale_alpha(alpha);
        s.shadow.color = s.shadow.color.scale_alpha(alpha);
        s
    }
}

pub fn fade_button(
    style: impl Fn(&Theme, button::Status) -> button::Style,
    alpha: f32,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut s = style(theme, status);
        s.background = s.background.map(|b| b.scale_alpha(alpha));
        s.text_color = s.text_color.scale_alpha(alpha);
        s.border.color = s.border.color.scale_alpha(alpha);
        s.shadow.color = s.shadow.color.scale_alpha(alpha);
        s
    }
}

pub fn fade_input(
    style: impl Fn(&Theme, text_input::Status) -> text_input::Style,
    alpha: f32,
) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| {
        let mut s = style(theme, status);
        s.background = s.background.scale_alpha(alpha);
        s.border.color = s.border.color.scale_alpha(alpha);
        s.icon = s.icon.scale_alpha(alpha);
        s.placeholder = s.placeholder.scale_alpha(alpha);
        s.value = s.value.scale_alpha(alpha);
        s.selection = s.selection.scale_alpha(alpha);
        s
    }
}

pub fn fade_slider(alpha: f32) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |theme, status| {
        let mut s = slider::default(theme, status);
        s.rail.backgrounds.0 = s.rail.backgrounds.0.scale_alpha(alpha);
        s.rail.backgrounds.1 = s.rail.backgrounds.1.scale_alpha(alpha);
        s.rail.border.color = s.rail.border.color.scale_alpha(alpha);
        s.handle.background = s.handle.background.scale_alpha(alpha);
        s.handle.border_color = s.handle.border_color.scale_alpha(alpha);
        s
    }
}

pub fn fade_scrollbar(alpha: f32) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut s = scrollbar(theme, status);
        s.vertical_rail.scroller.background =
            s.vertical_rail.scroller.background.scale_alpha(alpha);
        s.horizontal_rail.scroller.background =
            s.horizontal_rail.scroller.background.scale_alpha(alpha);
        s
    }
}

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
