use std::path::Path;

const INFO_PLIST: &[u8] = include_bytes!("../assets/macos/Info.plist");
const APP_ICON: &[u8] = include_bytes!("../assets/icons/snack.icns");

pub fn ensure_app_bundle() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the Snack executable: {error}"))?;
    if is_app_bundle_executable(&executable) {
        return mac_usernotifications::check_bundle()
            .map_err(|error| format!("invalid macOS application bundle: {error}"));
    }

    let app = executable
        .parent()
        .ok_or_else(|| "Snack executable has no parent dir".to_owned())?
        .join("Snack.app");
    let macos = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    let bundled_executable = macos.join("snack");

    if app.exists() {
        std::fs::remove_dir_all(&app)
            .map_err(|error| format!("could not refresh {}: {error}", app.display()))?;
    }
    std::fs::create_dir_all(&macos)
        .and_then(|()| std::fs::create_dir_all(&resources))
        .map_err(|error| format!("could not create {}: {error}", app.display()))?;
    std::fs::copy(&executable, &bundled_executable)
        .map_err(|error| format!("could not copy Snack into its app bundle: {error}"))?;
    std::fs::write(app.join("Contents/Info.plist"), INFO_PLIST)
        .and_then(|()| std::fs::write(resources.join("snack.icns"), APP_ICON))
        .map_err(|error| format!("could not write Snack app resources: {error}"))?;

    let status = std::process::Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app)
        .status()
        .map_err(|error| format!("could not sign {}: {error}", app.display()))?;
    if !status.success() {
        return Err(format!(
            "codesign failed for {} with {status}",
            app.display()
        ));
    }

    let status = std::process::Command::new(&bundled_executable)
        .args(std::env::args_os().skip(1))
        .status()
        .map_err(|error| format!("could not launch {}: {error}", app.display()))?;
    std::process::exit(status.code().unwrap_or(1));
}

fn is_app_bundle_executable(executable: &Path) -> bool {
    executable.parent().is_some_and(|macos| {
        macos.file_name().is_some_and(|name| name == "MacOS")
            && macos
                .parent()
                .and_then(Path::parent)
                .is_some_and(|app| app.extension().is_some_and(|extension| extension == "app"))
    })
}

#[cfg(test)]
mod tests {
    use super::is_app_bundle_executable;
    use std::path::Path;

    #[test]
    fn recognizes_executable_inside_app_bundle() {
        assert!(is_app_bundle_executable(Path::new(
            "/tmp/Snack.app/Contents/MacOS/snack"
        )));
        assert!(!is_app_bundle_executable(Path::new(
            "/tmp/target/debug/snack"
        )));
    }
}
