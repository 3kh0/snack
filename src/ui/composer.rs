use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::text_editor::{Action, Binding, Content, Edit, KeyPress, Motion, Status};
use iced::widget::{container, text_editor};
use iced::{Element, Fill, Length};

use super::theme;
use crate::app::{ComposerTarget, FormatMark, Message};

pub fn view<'a>(
    content: &'a Content,
    placeholder_label: &str,
    target: ComposerTarget,
) -> Element<'a, Message> {
    let placeholder = format!("Message {placeholder_label}");
    editor_owned(content, placeholder, target, Message::SendPressed)
}

pub fn thread_view<'a>(content: &'a Content, target: ComposerTarget) -> Element<'a, Message> {
    editor(
        content,
        "Reply in thread",
        target,
        Message::ThreadSendPressed,
    )
}

fn editor_owned<'a>(
    content: &'a Content,
    placeholder: String,
    target: ComposerTarget,
    send: Message,
) -> Element<'a, Message> {
    let input = text_editor(content)
        .placeholder(placeholder)
        .on_action(move |action| Message::ComposerAction { target, action })
        .key_binding(move |press| key_binding(press, target, send.clone()))
        .size(theme::TEXT_MD)
        .padding(theme::SPACE_SM)
        .height(Length::Shrink)
        .style(theme::editor);

    container(input).width(Fill).padding(theme::SPACE_MD).into()
}

fn editor<'a>(
    content: &'a Content,
    placeholder: &'a str,
    target: ComposerTarget,
    send: Message,
) -> Element<'a, Message> {
    let input = text_editor(content)
        .placeholder(placeholder)
        .on_action(move |action| Message::ComposerAction { target, action })
        .key_binding(move |press| key_binding(press, target, send.clone()))
        .size(theme::TEXT_MD)
        .padding(theme::SPACE_SM)
        .height(Length::Shrink)
        .style(theme::editor);

    container(input).width(Fill).padding(theme::SPACE_MD).into()
}

fn key_binding(press: KeyPress, target: ComposerTarget, send: Message) -> Option<Binding<Message>> {
    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }

    let KeyPress {
        key,
        modifiers,
        physical_key,
        ..
    } = &press;

    if let Some(mark) = format_mark_for(*modifiers, key, *physical_key) {
        return Some(Binding::Custom(Message::ComposerFormat { target, mark }));
    }

    // Slack: Enter sends, Shift+Enter inserts a newline.
    if is_enter(key, &press.modified_key) {
        return Some(if modifiers.shift() {
            Binding::Enter
        } else {
            Binding::Custom(send)
        });
    }

    Binding::from_key_press(press)
}

fn is_enter(key: &Key, modified_key: &Key) -> bool {
    matches!(key, Key::Named(Named::Enter)) || matches!(modified_key, Key::Named(Named::Enter))
}

fn format_mark_for(
    modifiers: Modifiers,
    key: &Key,
    physical_key: iced::keyboard::key::Physical,
) -> Option<FormatMark> {
    if !modifiers.command() {
        return None;
    }
    let c = key.to_latin(physical_key)?;
    match c {
        'b' if !modifiers.shift() && !modifiers.alt() => Some(FormatMark::Bold),
        'i' if !modifiers.shift() && !modifiers.alt() => Some(FormatMark::Italic),
        'x' if modifiers.shift() && !modifiers.alt() => Some(FormatMark::Strike),
        'c' if modifiers.shift() && modifiers.alt() => Some(FormatMark::CodeBlock),
        'c' if modifiers.shift() && !modifiers.alt() => Some(FormatMark::Code),
        '9' if modifiers.shift() && !modifiers.alt() => Some(FormatMark::Quote),
        _ => None,
    }
}

pub fn apply_format(content: &mut Content, mark: FormatMark) {
    let selected = content.selection().unwrap_or_default();
    let (replacement, cursor_back) = wrap_selection(&selected, mark);
    content.perform(Action::Edit(Edit::Paste(Arc::new(replacement))));
    for _ in 0..cursor_back {
        content.perform(Action::Move(Motion::Left));
    }
}

pub fn wrap_selection(selected: &str, mark: FormatMark) -> (String, usize) {
    match mark {
        FormatMark::Bold => toggle_pair(selected, "*", "*"),
        FormatMark::Italic => toggle_pair(selected, "_", "_"),
        FormatMark::Strike => toggle_pair(selected, "~", "~"),
        FormatMark::Code => toggle_pair(selected, "`", "`"),
        FormatMark::CodeBlock => toggle_pair(selected, "```\n", "\n```"),
        FormatMark::Quote => {
            if selected.is_empty() {
                ("> ".to_owned(), 0)
            } else if selected
                .lines()
                .all(|line| line.starts_with("> ") || line == ">")
            {
                let unquoted = selected
                    .lines()
                    .map(|line| {
                        line.strip_prefix("> ")
                            .or_else(|| line.strip_prefix('>'))
                            .unwrap_or(line)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (unquoted, 0)
            } else {
                let quoted = selected
                    .lines()
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                (quoted, 0)
            }
        }
    }
}

fn toggle_pair(selected: &str, open: &str, close: &str) -> (String, usize) {
    if selected.is_empty() {
        return (format!("{open}{close}"), close.chars().count());
    }
    if let Some(inner) = selected
        .strip_prefix(open)
        .and_then(|rest| rest.strip_suffix(close))
    {
        return (inner.to_owned(), 0);
    }
    (format!("{open}{selected}{close}"), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_wraps_and_unwraps() {
        assert_eq!(wrap_selection("hi", FormatMark::Bold), ("*hi*".into(), 0));
        assert_eq!(wrap_selection("*hi*", FormatMark::Bold), ("hi".into(), 0));
        assert_eq!(wrap_selection("", FormatMark::Bold), ("**".into(), 1));
    }

    #[test]
    fn italic_code_strike() {
        assert_eq!(wrap_selection("x", FormatMark::Italic), ("_x_".into(), 0));
        assert_eq!(wrap_selection("x", FormatMark::Code), ("`x`".into(), 0));
        assert_eq!(wrap_selection("x", FormatMark::Strike), ("~x~".into(), 0));
    }

    #[test]
    fn code_block_and_quote() {
        assert_eq!(
            wrap_selection("fn main() {}", FormatMark::CodeBlock),
            ("```\nfn main() {}\n```".into(), 0)
        );
        assert_eq!(
            wrap_selection("a\nb", FormatMark::Quote),
            ("> a\n> b".into(), 0)
        );
        assert_eq!(
            wrap_selection("> a\n> b", FormatMark::Quote),
            ("a\nb".into(), 0)
        );
    }
}
