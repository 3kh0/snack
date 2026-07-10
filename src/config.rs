use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::slack::models::{TeamId, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub d_cookie: String,
    pub workspaces: BTreeMap<TeamId, WorkspaceSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSession {
    pub team_id: TeamId,
    pub enterprise_id: Option<TeamId>,
    pub user_id: UserId,
    pub name: String,
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceMeta {
    team_id: TeamId,
    enterprise_id: Option<TeamId>,
    user_id: UserId,
    name: String,
    url: String,
}

impl From<&WorkspaceSession> for WorkspaceMeta {
    fn from(w: &WorkspaceSession) -> Self {
        Self {
            team_id: w.team_id.clone(),
            enterprise_id: w.enterprise_id.clone(),
            user_id: w.user_id.clone(),
            name: w.name.clone(),
            url: w.url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMeta {
    workspaces: Vec<WorkspaceMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSecrets {
    d_cookie: String,
    tokens: BTreeMap<TeamId, String>,
}

impl From<&Session> for SessionSecrets {
    fn from(session: &Session) -> Self {
        Self {
            d_cookie: session.d_cookie.clone(),
            tokens: session
                .workspaces
                .values()
                .map(|w| (w.team_id.clone(), w.token.clone()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccentColor {
    #[default]
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
}

impl AccentColor {
    pub const ALL: [AccentColor; 5] = [
        AccentColor::Blue,
        AccentColor::Red,
        AccentColor::Green,
        AccentColor::Yellow,
        AccentColor::Purple,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AccentColor::Blue => "Blue",
            AccentColor::Red => "Red",
            AccentColor::Green => "Green",
            AccentColor::Yellow => "Yellow",
            AccentColor::Purple => "Purple",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub accent: AccentColor,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_panel_radius")]
    pub panel_radius: f32,
    /// Border thickness around the main panels (`--border-thickness`).
    #[serde(default = "default_border_thickness")]
    pub border_thickness: f32,
    /// Channel sidebar width in px (user-draggable).
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
}

fn default_gap() -> f32 {
    12.0
}
fn default_panel_radius() -> f32 {
    12.0
}
fn default_border_thickness() -> f32 {
    1.0
}
fn default_sidebar_width() -> f32 {
    240.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            accent: AccentColor::default(),
            gap: default_gap(),
            panel_radius: default_panel_radius(),
            border_thickness: default_border_thickness(),
            sidebar_width: default_sidebar_width(),
        }
    }
}

/// Clamp range for the draggable sidebar width.
pub const SIDEBAR_WIDTH_MIN: f32 = 180.0;
pub const SIDEBAR_WIDTH_MAX: f32 = 520.0;

fn settings_path() -> Result<PathBuf, AppError> {
    Ok(config_dir()?.join("settings.json"))
}

/// Load appearance settings, falling back to defaults on any error.
pub fn load_settings() -> Settings {
    let Ok(path) = settings_path() else {
        return Settings::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(settings: &Settings) -> Result<(), AppError> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(settings_path()?, json)?;
    Ok(())
}

pub fn config_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    {
        return Ok(std::env::temp_dir().join(format!("snack-test-{}", std::process::id())));
    }

    #[cfg(not(test))]
    {
        let dirs = directories::ProjectDirs::from("com", "echonet", "snack").ok_or_else(|| {
            AppError::Io(std::io::Error::other("no home directory for config path"))
        })?;
        Ok(dirs.config_dir().to_path_buf())
    }
}

pub fn data_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    {
        return Ok(std::env::temp_dir().join(format!("snack-test-data-{}", std::process::id())));
    }

    #[cfg(not(test))]
    {
        let dirs = directories::ProjectDirs::from("com", "echonet", "snack").ok_or_else(|| {
            AppError::Io(std::io::Error::other("no home directory for data path"))
        })?;
        Ok(dirs.data_local_dir().to_path_buf())
    }
}

fn session_path() -> Result<PathBuf, AppError> {
    Ok(config_dir()?.join("session.json"))
}

fn secret_account() -> &'static str {
    "session_v2"
}

fn token_account(team_id: &str) -> String {
    format!("xoxc:{team_id}")
}

#[cfg(not(test))]
fn entry(account: &str) -> Result<keyring::Entry, AppError> {
    Ok(keyring::Entry::new("com.echonet.snack", account)?)
}

#[cfg(all(not(test), debug_assertions))]
fn dev_secret_path() -> Result<PathBuf, AppError> {
    Ok(config_dir()?.join("session.secrets.dev.json"))
}

#[cfg(all(not(test), debug_assertions))]
fn read_dev_secrets() -> Result<BTreeMap<String, String>, AppError> {
    match std::fs::read(dev_secret_path()?) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(all(not(test), debug_assertions))]
fn write_dev_secrets(secrets: &BTreeMap<String, String>) -> Result<(), AppError> {
    let path = dev_secret_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(secrets)?;
    std::fs::write(&path, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(all(not(test), debug_assertions))]
fn set_secret(account: &str, value: &str) -> Result<(), AppError> {
    let mut secrets = read_dev_secrets()?;
    secrets.insert(account.to_owned(), value.to_owned());
    write_dev_secrets(&secrets)
}

#[cfg(all(not(test), not(debug_assertions)))]
fn set_secret(account: &str, value: &str) -> Result<(), AppError> {
    Ok(entry(account)?.set_password(value)?)
}

#[cfg(all(not(test), debug_assertions))]
fn get_secret(account: &str) -> Result<Option<String>, AppError> {
    Ok(read_dev_secrets()?.remove(account))
}

#[cfg(all(not(test), not(debug_assertions)))]
fn get_secret(account: &str) -> Result<Option<String>, AppError> {
    match entry(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(all(not(test), debug_assertions))]
fn delete_secret(account: &str) {
    if let Ok(mut secrets) = read_dev_secrets() {
        secrets.remove(account);
        let _ = write_dev_secrets(&secrets);
    }
}

#[cfg(all(not(test), not(debug_assertions)))]
fn delete_secret(account: &str) {
    if let Ok(e) = entry(account) {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
fn test_secrets() -> &'static Mutex<BTreeMap<String, String>> {
    static SECRETS: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    SECRETS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(test)]
fn set_secret(account: &str, value: &str) -> Result<(), AppError> {
    test_secrets()
        .lock()
        .expect("test secret store poisoned")
        .insert(account.to_owned(), value.to_owned());
    Ok(())
}

#[cfg(test)]
fn get_secret(account: &str) -> Result<Option<String>, AppError> {
    Ok(test_secrets()
        .lock()
        .expect("test secret store poisoned")
        .get(account)
        .cloned())
}

#[cfg(test)]
fn delete_secret(account: &str) {
    let _ = test_secrets()
        .lock()
        .expect("test secret store poisoned")
        .remove(account);
}

fn save_secrets(session: &Session) -> Result<(), AppError> {
    let secrets = SessionSecrets::from(session);
    set_secret(secret_account(), &serde_json::to_string(&secrets)?)
}

fn load_secrets(meta: &SessionMeta) -> Result<Option<SessionSecrets>, AppError> {
    if let Some(json) = get_secret(secret_account())? {
        return Ok(Some(serde_json::from_str(&json)?));
    }

    let Some(d_cookie) = get_secret("d_cookie")? else {
        return Ok(None);
    };
    let mut tokens = BTreeMap::new();
    for w in &meta.workspaces {
        if let Some(token) = get_secret(&token_account(&w.team_id))? {
            tokens.insert(w.team_id.clone(), token);
        }
    }
    if tokens.is_empty() {
        return Ok(None);
    }
    let secrets = SessionSecrets { d_cookie, tokens };
    set_secret(secret_account(), &serde_json::to_string(&secrets)?)?;
    Ok(Some(secrets))
}

pub fn save_session(session: &Session) -> Result<(), AppError> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;

    let meta = SessionMeta {
        workspaces: session
            .workspaces
            .values()
            .map(WorkspaceMeta::from)
            .collect(),
    };
    let json = serde_json::to_string_pretty(&meta)?;
    std::fs::write(session_path()?, json)?;

    save_secrets(session)
}

pub fn load_session() -> Result<Option<Session>, AppError> {
    let path = session_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let meta: SessionMeta = serde_json::from_slice(&bytes)?;

    let Some(secrets) = load_secrets(&meta)? else {
        return Ok(None);
    };

    let mut workspaces = BTreeMap::new();
    for w in meta.workspaces {
        let Some(token) = secrets.tokens.get(&w.team_id).cloned() else {
            continue;
        };
        workspaces.insert(
            w.team_id.clone(),
            WorkspaceSession {
                team_id: w.team_id,
                enterprise_id: w.enterprise_id,
                user_id: w.user_id,
                name: w.name,
                url: w.url,
                token,
            },
        );
    }

    if workspaces.is_empty() {
        return Ok(None);
    }
    Ok(Some(Session {
        d_cookie: secrets.d_cookie,
        workspaces,
    }))
}

pub fn clear_session() -> Result<(), AppError> {
    if let Ok(meta_bytes) = std::fs::read(session_path()?) {
        if let Ok(meta) = serde_json::from_slice::<SessionMeta>(&meta_bytes) {
            for w in &meta.workspaces {
                delete_secret(&token_account(&w.team_id));
            }
        }
    }
    delete_secret("d_cookie");
    delete_secret(secret_account());
    match std::fs::remove_file(session_path()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::MutexGuard;

    use super::*;

    fn test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("config test lock poisoned")
    }

    fn reset() {
        let _ = std::fs::remove_dir_all(config_dir().expect("config dir"));
        test_secrets()
            .lock()
            .expect("test secret store poisoned")
            .clear();
    }

    fn session() -> Session {
        Session {
            d_cookie: "xoxd-cookie".into(),
            workspaces: BTreeMap::from([
                (
                    "T_ONE".into(),
                    WorkspaceSession {
                        team_id: "T_ONE".into(),
                        enterprise_id: None,
                        user_id: "U_ONE".into(),
                        name: "One".into(),
                        url: "https://one.slack.com".into(),
                        token: "xoxc-one".into(),
                    },
                ),
                (
                    "T_TWO".into(),
                    WorkspaceSession {
                        team_id: "T_TWO".into(),
                        enterprise_id: Some("E_TWO".into()),
                        user_id: "U_TWO".into(),
                        name: "Two".into(),
                        url: "https://two.slack.com".into(),
                        token: "xoxc-two".into(),
                    },
                ),
            ]),
        }
    }

    #[test]
    fn roundtrips_session_with_single_secret_entry() {
        let _guard = test_lock();
        reset();

        save_session(&session()).expect("save session");

        let metadata =
            std::fs::read_to_string(session_path().expect("session path")).expect("read metadata");
        assert!(!metadata.contains("xoxd-cookie"));
        assert!(!metadata.contains("xoxc-one"));

        let secrets = test_secrets().lock().expect("test secret store poisoned");
        assert!(secrets.contains_key(secret_account()));
        assert!(!secrets.contains_key("d_cookie"));
        assert!(!secrets.contains_key(&token_account("T_ONE")));
        drop(secrets);

        let loaded = load_session().expect("load session").expect("session");
        assert_eq!(loaded.d_cookie, "xoxd-cookie");
        assert_eq!(loaded.workspaces["T_ONE"].token, "xoxc-one");
        assert_eq!(loaded.workspaces["T_TWO"].token, "xoxc-two");
    }

    #[test]
    fn migrates_legacy_secrets_to_single_entry() {
        let _guard = test_lock();
        reset();

        let session = session();
        let meta = SessionMeta {
            workspaces: session
                .workspaces
                .values()
                .map(WorkspaceMeta::from)
                .collect(),
        };
        std::fs::create_dir_all(config_dir().expect("config dir")).expect("create config dir");
        std::fs::write(
            session_path().expect("session path"),
            serde_json::to_string_pretty(&meta).expect("serialize metadata"),
        )
        .expect("write metadata");
        set_secret("d_cookie", &session.d_cookie).expect("set d cookie");
        set_secret(&token_account("T_ONE"), "xoxc-one").expect("set token");
        set_secret(&token_account("T_TWO"), "xoxc-two").expect("set token");

        let loaded = load_session().expect("load session").expect("session");

        assert_eq!(loaded.workspaces.len(), 2);
        assert_eq!(loaded.workspaces["T_ONE"].token, "xoxc-one");
        assert!(
            test_secrets()
                .lock()
                .expect("test secret store poisoned")
                .contains_key(secret_account())
        );
    }
}
