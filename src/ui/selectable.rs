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
    Background, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size,
    Vector, alignment, keyboard,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::Message;

type Renderer = iced::Renderer;
type Theme = iced::Theme;
type IcedParagraph = <Renderer as iced::advanced::text::Renderer>::Paragraph;

const CHIP_PAD_Y: f32 = 1.0;
const CHIP_PAD_X: f32 = 3.0;
const CHIP_RADIUS: f32 = 4.0;

/// A run of body text sharing one style, before it is exploded into clusters.
pub struct Segment {
    pub text: String,
    pub channel: Option<String>,
    pub mono: bool,
    pub color: Option<Color>,
    pub background: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Segment {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            channel: None,
            mono: false,
            color: None,
            background: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct HighlightRun {
    start: usize,
    end: usize,
    color: Color,
}

pub struct SelectableText {
    spans: Vec<Span<'static, (), Font>>,
    channels: Vec<Option<String>>,
    highlight_runs: Vec<HighlightRun>,
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
        let (spans, channels, highlight_runs) = explode_segments(segments);
        Self {
            spans,
            channels,
            highlight_runs,
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

    fn channel_at(&self, index: usize) -> Option<&str> {
        self.channels.get(index).and_then(Option::as_deref)
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

fn styled_font(seg: &Segment) -> Font {
    let mut font = if seg.mono {
        Font::MONOSPACE
    } else {
        Font::DEFAULT
    };
    if seg.bold {
        font.weight = iced::font::Weight::Bold;
    }
    if seg.italic {
        font.style = iced::font::Style::Italic;
    }
    font
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

        for run in &self.highlight_runs {
            let mut regions = Vec::new();
            for index in run.start..=run.end {
                regions.extend(state.paragraph.span_bounds(index));
            }
            for r in merge_line_regions(regions) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            r.position() - Vector::new(CHIP_PAD_X, CHIP_PAD_Y),
                            r.size() + Size::new(CHIP_PAD_X * 2.0, CHIP_PAD_Y * 2.0),
                        ) + translation,
                        border: Border::default().rounded(CHIP_RADIUS),
                        ..Default::default()
                    },
                    Background::Color(run.color),
                );
            }
        }

        for (index, span) in self.spans.iter().enumerate() {
            if !span.underline && !span.strikethrough {
                continue;
            }
            let regions = state.paragraph.span_bounds(index);
            let size = span.size.unwrap_or(self.size);
            let line_height = span.line_height.unwrap_or_default().to_absolute(size);
            let color = span.color.unwrap_or(self.color);
            let baseline = translation + Vector::new(0.0, size.0 + (line_height.0 - size.0) / 2.0);
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
                let index = cursor
                    .is_over(bounds)
                    .then(|| self.locate(tree.state.downcast_ref::<State>(), layout, cursor))
                    .flatten();
                let state = tree.state.downcast_mut::<State>();
                let anchor = state.drag_anchor.take();
                if state.selection.is_none()
                    && anchor == index
                    && let Some(channel) = index.and_then(|index| self.channel_at(index))
                {
                    shell.publish(Message::ChannelSelected(channel.to_owned()));
                    shell.capture_event();
                }
                if anchor.is_some() {
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
        let state = tree.state.downcast_ref::<State>();
        if state.hovered
            && self
                .locate(state, _layout, _cursor)
                .and_then(|index| self.channel_at(index))
                .is_some()
        {
            mouse::Interaction::Pointer
        } else if state.hovered {
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

fn explode_segments(
    segments: &[Segment],
) -> (
    Vec<Span<'static, (), Font>>,
    Vec<Option<String>>,
    Vec<HighlightRun>,
) {
    let mut spans = Vec::new();
    let mut channels = Vec::new();
    let mut highlight_runs = Vec::new();

    for seg in segments {
        let styled_font = (seg.mono || seg.bold || seg.italic).then(|| styled_font(seg));
        let run_start = spans.len();
        for cluster in seg.text.graphemes(true) {
            let mut span = Span::new(cluster.to_owned());
            if let Some(font) = styled_font {
                span = span.font(font);
            }
            if let Some(color) = seg.color {
                span = span.color(color);
            }
            if seg.underline {
                span = span.underline(true);
            }
            if seg.strikethrough {
                span = span.strikethrough(true);
            }
            spans.push(span.to_static());
            channels.push(seg.channel.clone());
        }
        if let Some(color) = seg.background {
            if spans.len() > run_start {
                highlight_runs.push(HighlightRun {
                    start: run_start,
                    end: spans.len() - 1,
                    color,
                });
            }
        }
    }

    (spans, channels, highlight_runs)
}

fn merge_line_regions(mut regions: Vec<Rectangle>) -> Vec<Rectangle> {
    if regions.is_empty() {
        return regions;
    }
    regions.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut merged = Vec::new();
    let mut current = regions[0];
    for region in regions.into_iter().skip(1) {
        let same_line =
            (region.y - current.y).abs() < 0.5 && (region.height - current.height).abs() < 0.5;
        let contiguous = same_line && region.x <= current.x + current.width + 1.0;
        if contiguous {
            let right = (current.x + current.width).max(region.x + region.width);
            current.width = right - current.x;
            current.height = current.height.max(region.height);
        } else {
            merged.push(current);
            current = region;
        }
    }
    merged.push(current);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention_segment(text: &str, channel: Option<&str>, bg: Color) -> Segment {
        Segment {
            text: text.into(),
            channel: channel.map(str::to_owned),
            mono: false,
            color: Some(Color::from_rgb(0.1, 0.7, 0.9)),
            background: Some(bg),
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }

    #[test]
    fn channel_mention_is_one_highlight_run_not_per_grapheme() {
        let bg = Color {
            r: 0.08,
            g: 0.36,
            b: 0.52,
            a: 0.52,
        };
        let segments = vec![
            Segment::plain("see "),
            mention_segment("#what-is-my-slack-id", Some("C1"), bg),
            Segment::plain(" please"),
        ];
        let (spans, channels, runs) = explode_segments(&segments);

        assert_eq!(
            spans.len(),
            "see #what-is-my-slack-id please".chars().count()
        );
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start, 4);
        assert_eq!(runs[0].end, 4 + "#what-is-my-slack-id".chars().count() - 1);
        assert_eq!(runs[0].color, bg);

        for index in runs[0].start..=runs[0].end {
            assert_eq!(channels[index].as_deref(), Some("C1"));
            assert!(spans[index].highlight.is_none());
        }
        assert!(channels[0].is_none());
        assert!(channels[spans.len() - 1].is_none());
    }

    #[test]
    fn adjacent_mentions_stay_separate_chips() {
        let bg = Color::from_rgb(0.1, 0.2, 0.3);
        let segments = vec![
            mention_segment("#a", Some("C1"), bg),
            Segment::plain(" "),
            mention_segment("#b", Some("C2"), bg),
        ];
        let (_, channels, runs) = explode_segments(&segments);
        assert_eq!(runs.len(), 2);
        assert_eq!(channels[runs[0].start].as_deref(), Some("C1"));
        assert_eq!(channels[runs[1].start].as_deref(), Some("C2"));
    }

    #[test]
    fn merge_line_regions_joins_same_line_boxes() {
        let a = Rectangle::new(Point::new(0.0, 0.0), Size::new(8.0, 14.0));
        let b = Rectangle::new(Point::new(8.0, 0.0), Size::new(8.0, 14.0));
        let c = Rectangle::new(Point::new(0.0, 16.0), Size::new(8.0, 14.0));
        let merged = merge_line_regions(vec![a, b, c]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].x, 0.0);
        assert_eq!(merged[0].width, 16.0);
        assert_eq!(merged[1].y, 16.0);
        assert_eq!(merged[1].width, 8.0);
    }
}
