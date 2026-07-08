//! Rich text whose contents can be selected with the cursor and copied.
//!
//! iced ships no selectable *static* text: `text`/`rich_text` render styled
//! spans but expose no selection, while `text_editor` selects but can't do
//! rich formatting or clickable links. This widget bridges the two so message
//! bodies keep styling *and* gain drag-to-select + copy.
//!
//! The trick: each grapheme cluster becomes its own [`Span`]. `Paragraph::
//! hit_span` then returns a **global** cluster index (unlike `hit_test`, whose
//! `CharOffset` drops the line and is ambiguous once text wraps), and
//! `Paragraph::span_bounds` gives that cluster's on-screen rects for drawing
//! the highlight. Selection is therefore a normalized inclusive cluster range.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::Renderer as _;
use iced::advanced::text::{
    Alignment, Difference, Ellipsis, LineHeight, Paragraph, Shaping, Span, Text, Wrapping,
};
use iced::advanced::widget::text::{Style as TextStyle, draw as draw_paragraph};
use iced::advanced::widget::{Widget, tree};
use iced::advanced::{Renderer as _, Shell, clipboard, mouse};
use iced::{
    Background, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector,
    alignment, keyboard,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::Message;

type Renderer = iced::Renderer;
type Theme = iced::Theme;
type IcedParagraph = <Renderer as iced::advanced::text::Renderer>::Paragraph;

/// A run of body text sharing one style, before it is exploded into clusters.
pub struct Segment {
    pub text: String,
    pub mono: bool,
    pub color: Option<Color>,
}

impl Segment {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            mono: false,
            color: None,
        }
    }
}

pub struct SelectableText {
    spans: Vec<Span<'static, (), Font>>,
    size: Pixels,
    color: Color,
    selection_color: Color,
    width: Length,
}

impl SelectableText {
    /// Builds a selectable block from styled segments joined into one
    /// paragraph. Newlines inside segment text create wrapped lines that
    /// selection spans naturally.
    pub fn new(segments: &[Segment], size: f32, color: Color, selection_color: Color) -> Self {
        let mut spans = Vec::new();
        for seg in segments {
            let font = seg.mono.then_some(Font::MONOSPACE);
            for cluster in seg.text.graphemes(true) {
                let mut span = Span::new(cluster.to_owned());
                if let Some(font) = font {
                    span = span.font(font);
                }
                if let Some(color) = seg.color {
                    span = span.color(color);
                }
                spans.push(span.to_static());
            }
        }
        Self {
            spans,
            size: Pixels(size),
            color,
            selection_color,
            width: Length::Fill,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    fn selected_text(&self, (lo, hi): (usize, usize)) -> String {
        self.spans
            .get(lo..=hi)
            .map(|s| s.iter().map(|span| span.text.as_ref()).collect())
            .unwrap_or_default()
    }

    /// Maps a cursor position to a cluster index. Falls back to the nearest
    /// cluster (weighting the vertical axis so the correct line wins) when the
    /// cursor sits past a line end or in inter-glyph gaps, where `hit_span`
    /// returns `None`.
    fn locate(&self, state: &State, layout: Layout<'_>, cursor: mouse::Cursor) -> Option<usize> {
        let bounds = layout.bounds();
        let point = cursor.position()?;
        let local = Point::new(point.x - bounds.x, point.y - bounds.y);

        if let Some(index) = state.paragraph.hit_span(local) {
            return Some(index);
        }

        if self.spans.is_empty() {
            return None;
        }
        let mut best = None;
        let mut best_dist = f32::MAX;
        for k in 0..self.spans.len() {
            for r in state.paragraph.span_bounds(k) {
                let cx = local.x.clamp(r.x, r.x + r.width);
                let cy = local.y.clamp(r.y, r.y + r.height);
                let dx = local.x - cx;
                let dy = (local.y - cy) * 4.0;
                let dist = dx * dx + dy * dy;
                if dist < best_dist {
                    best_dist = dist;
                    best = Some(k);
                }
            }
        }
        best.or(Some(self.spans.len() - 1))
    }
}

struct State {
    spans: Vec<Span<'static, (), Font>>,
    paragraph: IcedParagraph,
    drag_anchor: Option<usize>,
    selection: Option<(usize, usize)>,
    hovered: bool,
}

impl Widget<Message, Theme, Renderer> for SelectableText {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            spans: Vec::new(),
            paragraph: IcedParagraph::default(),
            drag_anchor: None,
            selection: None,
            hovered: false,
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        layout::sized(limits, self.width, Length::Shrink, |limits| {
            let bounds = limits.max();
            let font = renderer.default_font();
            let hint_factor = renderer.scale_factor();
            let text_with_spans = || Text {
                content: self.spans.as_slice(),
                bounds,
                size: self.size,
                line_height: LineHeight::default(),
                font,
                align_x: Alignment::Default,
                align_y: alignment::Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::Word,
                ellipsis: Ellipsis::default(),
                hint_factor,
            };

            if state.spans != self.spans {
                state.paragraph = IcedParagraph::with_spans(text_with_spans());
                state.spans = self.spans.clone();
            } else {
                match state.paragraph.compare(Text {
                    content: (),
                    bounds,
                    size: self.size,
                    line_height: LineHeight::default(),
                    font,
                    align_x: Alignment::Default,
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: Wrapping::Word,
                    ellipsis: Ellipsis::default(),
                    hint_factor,
                }) {
                    Difference::None => {}
                    Difference::Bounds => state.paragraph.resize(bounds),
                    Difference::Shape => {
                        state.paragraph = IcedParagraph::with_spans(text_with_spans());
                    }
                }
            }

            state.paragraph.min_bounds()
        })
    }

    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        let translation = layout.position() - Point::ORIGIN;

        if let Some((lo, hi)) = state.selection {
            for k in lo..=hi {
                for r in state.paragraph.span_bounds(k) {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: r + translation,
                            ..Default::default()
                        },
                        self.selection_color,
                    );
                }
            }
        }

        // Span decorations (highlight / underline / strikethrough) for future
        // rich formatting; plain bodies simply skip this.
        for (index, span) in self.spans.iter().enumerate() {
            if span.highlight.is_none() && !span.underline && !span.strikethrough {
                continue;
            }
            let regions = state.paragraph.span_bounds(index);
            if let Some(highlight) = span.highlight {
                for r in &regions {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                r.position() - Vector::new(span.padding.left, span.padding.top),
                                r.size() + Size::new(span.padding.x(), span.padding.y()),
                            ) + translation,
                            border: highlight.border,
                            ..Default::default()
                        },
                        highlight.background,
                    );
                }
            }
            if span.underline || span.strikethrough {
                let size = span.size.unwrap_or(self.size);
                let line_height = span.line_height.unwrap_or_default().to_absolute(size);
                let color = span.color.unwrap_or(self.color);
                let baseline =
                    translation + Vector::new(0.0, size.0 + (line_height.0 - size.0) / 2.0);
                if span.underline {
                    for r in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    r.position() + baseline - Vector::new(0.0, size.0 * 0.08),
                                    Size::new(r.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
                if span.strikethrough {
                    for r in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    r.position() + baseline - Vector::new(0.0, size.0 / 2.0),
                                    Size::new(r.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
            }
        }

        draw_paragraph(
            renderer,
            defaults,
            bounds,
            &state.paragraph,
            TextStyle {
                color: Some(self.color),
            },
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        {
            let state = tree.state.downcast_mut::<State>();
            state.hovered = cursor.is_over(bounds);
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    let index = self.locate(tree.state.downcast_ref::<State>(), layout, cursor);
                    let state = tree.state.downcast_mut::<State>();
                    state.drag_anchor = index;
                    state.selection = None;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let anchor = tree.state.downcast_ref::<State>().drag_anchor;
                if let Some(anchor) = anchor {
                    if let Some(index) =
                        self.locate(tree.state.downcast_ref::<State>(), layout, cursor)
                    {
                        let selection = Some((anchor.min(index), anchor.max(index)));
                        let state = tree.state.downcast_mut::<State>();
                        if state.selection != selection {
                            state.selection = selection;
                            shell.request_redraw();
                        }
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let state = tree.state.downcast_mut::<State>();
                if state.drag_anchor.take().is_some() {
                    shell.request_redraw();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                let is_copy = modifiers.contains(keyboard::Modifiers::COMMAND)
                    && matches!(key.as_ref(), keyboard::Key::Character(c) if c.eq_ignore_ascii_case("c"));
                let state = tree.state.downcast_ref::<State>();
                if let Some(selection) = state.selection.filter(|_| is_copy && state.hovered) {
                    let text = self.selected_text(selection);
                    if !text.is_empty() {
                        shell.write_clipboard(clipboard::Content::Text(text));
                        shell.capture_event();
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<State>().hovered {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a> From<SelectableText> for Element<'a, Message, Theme, Renderer> {
    fn from(widget: SelectableText) -> Self {
        Element::new(widget)
    }
}
