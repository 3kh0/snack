use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::text_editor::{Action, Binding, Content, Edit, KeyPress, Motion, Status};
use iced::widget::{button, column, container, row, text, text_editor};
use iced::{Alignment, Element, Fill, Length};

use super::theme;
use crate::app::{ComposerAttachment, ComposerTarget, FormatMark, Message};

pub fn view<'a>(
    content: &'a Content,
    attachments: &'a [ComposerAttachment],
    placeholder_label: &str,
    target: ComposerTarget,
) -> Element<'a, Message> {
    let placeholder = format!("Message {placeholder_label}");
    editor_owned(
        content,
        attachments,
        placeholder,
        target,
        Message::SendPressed,
    )
}

pub fn thread_view<'a>(
    content: &'a Content,
    attachments: &'a [ComposerAttachment],
    target: ComposerTarget,
) -> Element<'a, Message> {
    editor(
        content,
        attachments,
        "Reply in thread",
        target,
        Message::ThreadSendPressed,
    )
}

fn editor_owned<'a>(
    content: &'a Content,
    attachments: &'a [ComposerAttachment],
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

    composer_shell(input.into(), attachments, target).into()
}

fn editor<'a>(
    content: &'a Content,
    attachments: &'a [ComposerAttachment],
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

    composer_shell(input.into(), attachments, target).into()
}

fn composer_shell<'a>(
    input: Element<'a, Message>,
    attachments: &'a [ComposerAttachment],
    target: ComposerTarget,
) -> iced::widget::Container<'a, Message> {
    let mut body = column![];
    if !attachments.is_empty() {
        body = body.push(attachment_strip(attachments, target));
    }
    body = body.push(input).push(
        row![
            button(text("+").size(20.0))
                .on_press(Message::AttachmentPickerOpened(target))
                .style(theme::action_button)
                .padding([2.0, 8.0]),
            text(
                if attachments.iter().any(|attachment| attachment.uploading) {
                    "Uploading attachments…"
                } else {
                    "Attach files"
                }
            )
            .size(theme::TEXT_SM)
            .color(theme::TEXT_4),
        ]
        .spacing(theme::SPACE_SM)
        .align_y(Alignment::Center),
    );
    container(body.spacing(theme::SPACE_SM))
        .width(Fill)
        .padding(theme::SPACE_MD)
}

fn attachment_strip<'a>(
    attachments: &'a [ComposerAttachment],
    target: ComposerTarget,
) -> Element<'a, Message> {
    let mut strip = row![].spacing(theme::SPACE_SM);
    for attachment in attachments {
        let detail = if attachment.uploading {
            "Uploading…".to_owned()
        } else {
            format_bytes(attachment.bytes)
        };
        strip = strip.push(
            container(
                row![
                    column![
                        text(&attachment.name)
                            .size(theme::TEXT_SM)
                            .color(theme::TEXT_1),
                        text(detail).size(theme::TEXT_SM).color(theme::TEXT_4),
                    ]
                    .spacing(2.0)
                    .width(Length::Fixed(150.0)),
                    button(text("×").size(theme::TEXT_LG))
                        .on_press_maybe((!attachment.uploading).then_some(
                            Message::AttachmentRemoved {
                                target,
                                id: attachment.id,
                            }
                        ))
                        .style(theme::action_button)
                        .padding([0.0, 5.0]),
                ]
                .align_y(Alignment::Center)
                .spacing(theme::SPACE_XS),
            )
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .style(theme::file_attachment),
        );
    }
    strip.into()
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", (bytes + 1023) / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
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

    if modifiers.command()
        && matches!(key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("v"))
    {
        return Some(Binding::Custom(Message::PasteAttachmentsRequested(target)));
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
