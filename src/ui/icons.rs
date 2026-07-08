//! Google Material Design icons (filled 24px), embedded as SVG.
//! Paths lifted from https://github.com/google/material-design-icons

use std::sync::LazyLock;

use iced::widget::svg;

macro_rules! material_icon {
    ($name:ident, $path:literal) => {
        static $name: LazyLock<svg::Handle> = LazyLock::new(|| {
            svg::Handle::from_memory(
                concat!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d=""#,
                    $path,
                    r#""/></svg>"#,
                )
                .as_bytes(),
            )
        });
    };
}

// "tag" — public channel hash
material_icon!(
    TAG,
    "M20 10V8h-4V4h-2v4h-4V4H8v4H4v2h4v4H4v2h4v4h2v-4h4v4h2v-4h4v-2h-4v-4h4zm-6 4h-4v-4h4v4z"
);

// "lock" — private channel / group
material_icon!(
    LOCK,
    "M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2z"
);

pub fn tag() -> svg::Handle {
    TAG.clone()
}

pub fn lock() -> svg::Handle {
    LOCK.clone()
}
