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

material_icon!(
    SEARCH,
    "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z"
);

material_icon!(
    DOWNLOAD,
    "M12 16q-.2 0-.375-.075T11.3 15.2l-4-4q-.3-.3-.3-.7t.3-.7q.3-.3.713-.313T8.725 9.8L11 12.075V4q0-.425.288-.713T12 3q.425 0 .713.288T13 4v8.075L15.275 9.8q.3-.3.713-.313t.712.313q.3.3.3.7t-.3.7l-4 4q-.15.15-.325.225T12 16ZM5 21q-.825 0-1.413-.588T3 19v-2q0-.425.288-.713T4 16q.425 0 .713.288T5 17v2h14v-2q0-.425.288-.713T20 16q.425 0 .713.288T21 17v2q0 .825-.588 1.413T19 21H5Z"
);

pub fn tag() -> svg::Handle {
    TAG.clone()
}

pub fn lock() -> svg::Handle {
    LOCK.clone()
}

pub fn search() -> svg::Handle {
    SEARCH.clone()
}

pub fn download() -> svg::Handle {
    DOWNLOAD.clone()
}
