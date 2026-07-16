use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::svg::Handle as SvgHandle;
use iced::widget::text_editor::{Action, Binding, Content, Edit, KeyPress, Motion, Status};
use iced::widget::{button, column, container, image, row, stack, svg, text, text_editor};
use iced::{Alignment, ContentFit, Element, Fill, Length};
use std::sync::atomic::Ordering;
use unicode_segmentation::UnicodeSegmentation;

use super::theme;
use crate::app::{ComposerAttachment, ComposerTarget, FormatMark, Message};

pub const CHANNEL_INPUT_ID: &str = "composer-channel";
pub const THREAD_INPUT_ID: &str = "composer-thread";

pub fn input_id(target: ComposerTarget) -> &'static str {
    match target {
        ComposerTarget::Channel => CHANNEL_INPUT_ID,
        ComposerTarget::Thread => THREAD_INPUT_ID,
    }
}

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
    let binding_send = send.clone();
    let input = text_editor(content)
        .id(input_id(target))
        .placeholder(placeholder)
        .on_action(move |action| Message::ComposerAction { target, action })
        .key_binding(move |press| key_binding(press, target, binding_send.clone()))
        .size(theme::TEXT_MD)
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .height(Length::Fixed(30.0))
        .style(theme::composer_editor);

    composer_shell(input.into(), attachments, target, send).into()
}

fn editor<'a>(
    content: &'a Content,
    attachments: &'a [ComposerAttachment],
    placeholder: &'a str,
    target: ComposerTarget,
    send: Message,
) -> Element<'a, Message> {
    let binding_send = send.clone();
    let input = text_editor(content)
        .id(input_id(target))
        .placeholder(placeholder)
        .on_action(move |action| Message::ComposerAction { target, action })
        .key_binding(move |press| key_binding(press, target, binding_send.clone()))
        .size(theme::TEXT_MD)
        .padding([theme::SPACE_XS, theme::SPACE_SM])
        .height(Length::Fixed(30.0))
        .style(theme::composer_editor);

    composer_shell(input.into(), attachments, target, send).into()
}

fn composer_shell<'a>(
    input: Element<'a, Message>,
    attachments: &'a [ComposerAttachment],
    target: ComposerTarget,
    send: Message,
) -> iced::widget::Container<'a, Message> {
    let mut body = column![];
    if !attachments.is_empty() {
        body = body.push(attachment_strip(attachments, target));
    }
    body = body.push(
        container(
            row![
                button(
                    container(material_add_icon())
                        .width(Length::Fixed(28.0))
                        .height(Length::Fixed(28.0))
                        .center_x(Fill)
                        .center_y(Fill),
                )
                .on_press(Message::AttachmentPickerOpened(target))
                .style(theme::action_button)
                .padding(0.0)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0)),
                input,
                button(
                    container(material_send_icon())
                        .width(Length::Fixed(28.0))
                        .height(Length::Fixed(28.0))
                        .center_x(Fill)
                        .center_y(Fill),
                )
                .on_press(send)
                .style(theme::action_button)
                .padding(0.0)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0)),
            ]
            .spacing(theme::SPACE_XS)
            .align_y(Alignment::Center),
        )
        .style(theme::file_attachment)
        .padding(theme::SPACE_XS)
        .height(Length::Fixed(36.0)),
    );
    container(body.spacing(theme::SPACE_SM))
        .width(Fill)
        .height(Length::Shrink)
        .padding([theme::SPACE_SM, theme::SPACE_MD])
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
        let preview = attachment_preview(attachment);
        strip = strip.push(
            container(
                row![
                    preview,
                    column![
                        text(truncate_filename(&attachment.name))
                            .size(theme::TEXT_SM)
                            .color(theme::TEXT_1),
                        text(detail).size(theme::TEXT_SM).color(theme::TEXT_4),
                    ]
                    .spacing(2.0)
                    .width(Length::Fixed(128.0)),
                    button(text("×").size(theme::TEXT_LG))
                        .on_press(Message::AttachmentRemoved {
                            target,
                            id: attachment.id,
                        })
                        .style(theme::action_button)
                        .padding([0.0, 5.0]),
                ]
                .align_y(Alignment::Center)
                .spacing(theme::SPACE_XS),
            )
            .width(Length::Fixed(236.0))
            .height(Length::Fixed(76.0))
            .padding([theme::SPACE_XS, theme::SPACE_SM])
            .clip(true)
            .style(theme::file_attachment),
        );
    }
    strip.into()
}

pub fn pending_attachment_strip<'a>(attachments: &'a [ComposerAttachment]) -> Element<'a, Message> {
    let mut strip = row![].spacing(theme::SPACE_SM);
    for attachment in attachments {
        let preview_source = attachment.preview_path.as_ref().unwrap_or(&attachment.path);
        let media: Element<'a, Message> =
            if attachment.preview_path.is_some() || is_previewable_image(&attachment.path) {
                image(ImageHandle::from_path(preview_source))
                    .width(Length::Fixed(220.0))
                    .height(Length::Fixed(140.0))
                    .content_fit(ContentFit::Contain)
                    .into()
            } else {
                container(
                    text(if is_video(&attachment.path) {
                        "VIDEO"
                    } else {
                        "FILE"
                    })
                    .size(theme::TEXT_SM)
                    .color(theme::TEXT_3),
                )
                .width(Length::Fixed(220.0))
                .height(Length::Fixed(96.0))
                .center_x(Fill)
                .center_y(Fill)
                .into()
            };
        let media_height =
            if attachment.preview_path.is_some() || is_previewable_image(&attachment.path) {
                140.0
            } else {
                96.0
            };
        let progress = attachment
            .upload_progress
            .as_ref()
            .map(|progress| {
                progress.load(Ordering::Relaxed) as f32 / attachment.bytes.max(1) as f32
            })
            .unwrap_or(0.0);
        let elapsed = attachment
            .upload_started
            .map(|started| started.elapsed().as_millis() as f32)
            .unwrap_or(0.0);
        let media: Element<'a, Message> = if attachment.uploading {
            let overlay = container(upload_ring(elapsed, progress))
                .width(Length::Fixed(220.0))
                .height(Length::Fixed(media_height))
                .center_x(Fill)
                .center_y(Fill);
            stack![media, overlay].into()
        } else {
            media
        };
        let content = column![
            media,
            text(truncate_filename(&attachment.name))
                .size(theme::TEXT_SM)
                .color(theme::TEXT_1),
            text(format_bytes(attachment.bytes))
                .size(theme::TEXT_SM)
                .color(theme::TEXT_4),
        ]
        .spacing(3.0);
        strip = strip.push(
            container(content)
                .width(Length::Fixed(236.0))
                .padding(theme::SPACE_SM)
                .clip(true)
                .style(theme::file_attachment),
        );
    }
    strip.into()
}

fn attachment_preview<'a>(attachment: &'a ComposerAttachment) -> Element<'a, Message> {
    let preview_source = attachment.preview_path.as_ref().unwrap_or(&attachment.path);
    let preview: Element<'a, Message> =
        if attachment.preview_path.is_some() || is_previewable_image(&attachment.path) {
            image(ImageHandle::from_path(preview_source))
                .width(Length::Fixed(64.0))
                .height(Length::Fixed(64.0))
                .content_fit(ContentFit::Cover)
                .into()
        } else {
            let label = if is_video(&attachment.path) {
                "VIDEO"
            } else {
                "FILE"
            };
            container(text(label).size(theme::TEXT_SM).color(theme::TEXT_3))
                .width(Length::Fixed(64.0))
                .height(Length::Fixed(64.0))
                .center_x(Fill)
                .center_y(Fill)
                .style(theme::file_attachment)
                .into()
        };
    let preview = match (
        attachment.upload_started,
        attachment.upload_progress.as_ref(),
    ) {
        (Some(started), Some(progress)) => stack![
            preview,
            upload_ring(
                started.elapsed().as_millis() as f32,
                progress.load(Ordering::Relaxed) as f32 / attachment.bytes.max(1) as f32,
            )
        ]
        .into(),
        _ => preview,
    };
    container(preview)
        .width(Length::Fixed(64.0))
        .height(Length::Fixed(64.0))
        .clip(true)
        .into()
}

fn upload_ring<'a>(elapsed_ms: f32, progress: f32) -> Element<'a, Message> {
    let rotation = (elapsed_ms % 1_200.0) / 1_200.0 * 360.0;
    let arc = (progress.clamp(0.01, 0.99) * 132.0).max(2.0);
    let gap = 132.0 - arc;
    let markup = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><circle cx="32" cy="32" r="24" fill="#101218" fill-opacity=".66"/><circle cx="32" cy="32" r="21" fill="none" stroke="#F2F4F8" stroke-width="4" stroke-linecap="round" stroke-dasharray="{arc} {gap}" transform="rotate({rotation} 32 32)"/></svg>"##
    );
    svg(SvgHandle::from_memory(markup.into_bytes()))
        .width(Length::Fixed(64.0))
        .height(Length::Fixed(64.0))
        .into()
}

fn material_add_icon<'a>() -> Element<'a, Message> {
    const ADD_ROUNDED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960"><path fill="#AEB8D0" d="M440-440H200q-17 0-28.5-11.5T160-480q0-17 11.5-28.5T200-520h240v-240q0-17 11.5-28.5T480-800q17 0 28.5 11.5T520-760v240h240q17 0 28.5 11.5T800-480q0 17-11.5 28.5T760-440H520v240q0 17-11.5 28.5T480-160q-17 0-28.5-11.5T440-200v-240Z"/></svg>"##;
    svg(SvgHandle::from_memory(ADD_ROUNDED))
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .into()
}

fn material_send_icon<'a>() -> Element<'a, Message> {
    const SEND_ROUNDED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M0 0h24v24H0z" fill="none"/><path fill="#AEB8D0" d="M4.4 19.425q-.5.2-.95-.088T3 18.5V14l8-2l-8-2V5.5q0-.55.45-.837t.95-.088l15.4 6.5q.625.275.625.925t-.625.925z"/></svg>"##;
    svg(SvgHandle::from_memory(SEND_ROUNDED))
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .into()
}

fn truncate_filename(name: &str) -> String {
    const MAX_GRAPHEMES: usize = 22;
    let mut graphemes = name.graphemes(true);
    let head = graphemes.by_ref().take(MAX_GRAPHEMES).collect::<String>();
    if graphemes.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn is_previewable_image(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if matches!(
            extension.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
        )
    )
}

fn is_video(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if matches!(
            extension.to_ascii_lowercase().as_str(),
            "mp4" | "mov" | "m4v" | "webm"
        )
    )
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

    if modifiers.command()
        && matches!(key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("k"))
    {
        return Some(Binding::Custom(Message::PaletteToggled));
    }

    // Slack: Enter sends, Shift+Enter inserts a newline.
    if is_enter(key, &press.modified_key) {
        return Some(if modifiers.shift() {
            Binding::Enter
        } else {
            Binding::Custom(send)
        });
    }

    if modifiers.command() && matches!(key, Key::Character(_)) {
        return None;
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
