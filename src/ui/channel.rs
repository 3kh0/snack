use iced::widget::{Column, column, container, scrollable, text};
use iced::{Element, Fill};

use super::{message, theme};
use crate::app::{FilePreview, Message};
use crate::state::{self, Workspace};
use std::collections::HashMap;

pub fn view<'a>(
    ws: &Workspace,
    channel_id: &str,
    file_previews: &HashMap<String, FilePreview>,
    editing: Option<(&str, &str)>,
) -> Element<'a, Message> {
    let label = ws
        .channels
        .get(channel_id)
        .map(state::channel_label)
        .unwrap_or_else(|| channel_id.to_owned());

    let header = container(text(label).size(theme::TEXT_LG).font(iced::Font {
        weight: iced::font::Weight::Bold,
        ..iced::Font::default()
    }))
    .padding(theme::SPACE_MD);

    let list: Element<'a, Message> = match ws.messages.get(channel_id) {
        Some(cm) if !cm.messages.is_empty() => {
            let mut col = Column::new().spacing(theme::SPACE_XS);
            for m in &cm.messages {
                let pending = m.ts.as_deref().map(|ts| cm.is_pending(ts)).unwrap_or(false);
                let edit = editing
                    .filter(|(ts, _)| Some(*ts) == m.ts.as_deref())
                    .map(|(_, value)| value);
                col = col.push(message::row(
                    ws,
                    channel_id,
                    m,
                    pending,
                    file_previews,
                    edit,
                ));
            }
            scrollable(col).height(Fill).into()
        }
        _ => message::empty_placeholder(),
    };

    let typing = ws.typing_names(channel_id);
    let footer: Element<'a, Message> = if typing.is_empty() {
        container(text("")).height(theme::TEXT_MD).into()
    } else {
        container(
            text(typing_line(&typing))
                .size(theme::TEXT_SM)
                .color(theme::MUTED),
        )
        .padding([0.0, theme::SPACE_MD])
        .into()
    };

    column![header, container(list).height(Fill), footer]
        .width(Fill)
        .height(Fill)
        .into()
}

fn typing_line(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => format!("{a} is typing…"),
        [a, b] => format!("{a} and {b} are typing…"),
        _ => format!("{} people are typing…", names.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::typing_line;

    #[test]
    fn typing_line_variants() {
        assert_eq!(typing_line(&[]), "");
        assert_eq!(typing_line(&["alice".into()]), "alice is typing…");
        assert_eq!(
            typing_line(&["alice".into(), "bob".into()]),
            "alice and bob are typing…"
        );
        assert_eq!(
            typing_line(&["a".into(), "b".into(), "c".into()]),
            "3 people are typing…"
        );
    }
}
