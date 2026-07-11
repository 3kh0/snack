use windows_registry::CURRENT_USER;

pub const APP_ID: &str = "com.echonet.snack";

const APP_ICON: &[u8] = include_bytes!("../assets/icons/icon-256.png");
static IDENTITY_REGISTERED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

pub fn ensure_notification_identity() -> Result<(), String> {
    if IDENTITY_REGISTERED.get().is_some() {
        return Ok(());
    }

    let project_dirs = directories::ProjectDirs::from("com", "echonet", "Snack")
        .ok_or_else(|| "Windows did not provide a local application data directory".to_owned())?;
    let icon_path = project_dirs.data_local_dir().join("snack-notification.png");
    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create notification icon directory: {error}"))?;
    }
    let icon_is_current = std::fs::read(&icon_path).is_ok_and(|bytes| bytes == APP_ICON);
    if !icon_is_current {
        std::fs::write(&icon_path, APP_ICON)
            .map_err(|error| format!("could not write notification icon: {error}"))?;
    }

    let key = CURRENT_USER
        .create(format!(r"SOFTWARE\Classes\AppUserModelId\{APP_ID}"))
        .map_err(|error| format!("could not register Snack's AppUserModelID: {error}"))?;
    key.set_string("DisplayName", "Snack")
        .and_then(|()| key.set_string("IconBackgroundColor", "0"))
        .and_then(|()| key.set_hstring("IconUri", &icon_path.as_path().into()))
        .map_err(|error| format!("could not register Snack's notification icon: {error}"))?;
    let _ = IDENTITY_REGISTERED.set(());
    Ok(())
}
