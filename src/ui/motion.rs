use std::time::Duration;

use iced::animation::{self, Animation};
use iced::time::Instant;
use iced::widget::transition::Transition;
use iced::widget::{container, float, mouse_area, space, transition};
use iced::{Background, Color, Element, Length, Vector};

// Short, ease-out only — firm desktop feel, not springy toys.
const LAYER: Duration = Duration::from_millis(140);
const PANEL: Duration = Duration::from_millis(160);
const MICRO: Duration = Duration::from_millis(80);

const CURVE: animation::Easing = animation::Easing::EaseOutCubic;

fn layer_anim() -> Animation<bool> {
    Animation::new(false).duration(LAYER).easing(CURVE)
}

fn panel_anim() -> Animation<bool> {
    Animation::new(false).duration(PANEL).easing(CURVE)
}

fn micro_anim() -> Animation<bool> {
    Animation::new(false).duration(MICRO).easing(CURVE)
}

/// Progress in \[0, 1\] for an open/close animation (easing already applied).
#[inline]
pub fn t(anim: &Animation<bool>, at: Instant) -> f32 {
    anim.interpolate(0.0, 1.0, at).clamp(0.0, 1.0)
}

#[inline]
pub fn closing(anim: &Animation<bool>) -> bool {
    !anim.value()
}

pub fn overlay<'a, Message, E>(
    open: bool,
    view: impl Fn(&Animation<bool>, Instant) -> E + 'a,
) -> Transition<'a, Message, iced::Theme, iced::Renderer, Animation<bool>>
where
    Message: 'a,
    E: Into<Element<'a, Message>>,
{
    transition(open, layer_anim, view)
}

pub fn panel_reveal<'a, Message, E>(
    open: bool,
    view: impl Fn(&Animation<bool>, Instant) -> E + 'a,
) -> Transition<'a, Message, iced::Theme, iced::Renderer, Animation<bool>>
where
    Message: 'a,
    E: Into<Element<'a, Message>>,
{
    transition(open, panel_anim, view)
}

pub fn micro_reveal<'a, Message, E>(
    open: bool,
    view: impl Fn(&Animation<bool>, Instant) -> E + 'a,
) -> Transition<'a, Message, iced::Theme, iced::Renderer, Animation<bool>>
where
    Message: 'a,
    E: Into<Element<'a, Message>>,
{
    transition(open, micro_anim, view)
}

pub fn scrim<'a, Message: Clone + 'a>(progress: f32, on_press: Message) -> Element<'a, Message> {
    let p = progress.clamp(0.0, 1.0);
    let color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.45 * p,
    };
    let area = mouse_area(
        container(space::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(color)),
                ..container::Style::default()
            }),
    );
    if p > 0.05 {
        area.on_press(on_press).into()
    } else {
        area.into()
    }
}

pub fn slide_y<'a, Message: 'a>(
    content: Element<'a, Message>,
    progress: f32,
    dy: f32,
) -> Element<'a, Message> {
    let offset = dy * (1.0 - progress.clamp(0.0, 1.0));
    translate(content, 0.0, offset)
}

/// Opacity ramp with a tiny lead-in so content doesn't pop before the scrim.
pub fn fade(progress: f32) -> f32 {
    ((progress.clamp(0.0, 1.0) - 0.04) / 0.96).clamp(0.0, 1.0)
}

/// Enter from a small vertical offset — no scale (scale reads as cheap).
pub fn zoom_y<'a, Message: 'a>(
    content: Element<'a, Message>,
    progress: f32,
    dy: f32,
) -> Element<'a, Message> {
    let offset = dy * (1.0 - progress.clamp(0.0, 1.0));
    translate(content, 0.0, offset)
}

#[derive(Clone, Copy)]
pub enum ExitEdge {
    Top,
    Bottom,
}

pub fn fly_y<'a, Message: 'a>(
    content: Element<'a, Message>,
    progress: f32,
    closing: bool,
    dy_enter: f32,
    exit: ExitEdge,
) -> Element<'a, Message> {
    let p = progress.clamp(0.0, 1.0);
    if closing {
        float(content)
            .translate(move |bounds, viewport| {
                let travel = match exit {
                    ExitEdge::Top => -(bounds.y + bounds.height + 16.0),
                    ExitEdge::Bottom => viewport.height - bounds.y + 16.0,
                };
                Vector::new(0.0, travel * (1.0 - p))
            })
            .into()
    } else {
        let offset = dy_enter * (1.0 - p);
        translate(content, 0.0, offset)
    }
}

pub fn collapse_x<'a, Message: 'a>(
    content: Element<'a, Message>,
    progress: f32,
    width: f32,
) -> Element<'a, Message> {
    container(content)
        .width(Length::Fixed(width * progress.clamp(0.0, 1.0)))
        .height(Length::Fill)
        .clip(true)
        .into()
}

fn translate<'a, Message: 'a>(
    content: Element<'a, Message>,
    x: f32,
    y: f32,
) -> Element<'a, Message> {
    float(content)
        .translate(move |_bounds, _viewport| Vector::new(x, y))
        .into()
}
