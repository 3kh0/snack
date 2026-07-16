use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::slack::models::{TeamId, UserId};

pub type AccountId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub d_cookie: String,
    pub workspaces: BTreeMap<TeamId, WorkspaceSession>,
}

impl Session {
    pub fn account_label(&self) -> String {
        let Some(workspace) = self.workspaces.values().next() else {
            return "Slack account".to_owned();
        };
        let more = self.workspaces.len().saturating_sub(1);
        if more == 0 {
            format!("{} · {}", workspace.name, workspace.user_id)
        } else {
            format!("{} +{more} · {}", workspace.name, workspace.user_id)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Accounts {
    pub active_account: AccountId,
    pub sessions: BTreeMap<AccountId, Session>,
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

impl From<&Session> for SessionMeta {
    fn from(session: &Session) -> Self {
        Self {
            workspaces: session
                .workspaces
                .values()
                .map(WorkspaceMeta::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountsMeta {
    active_account: AccountId,
    accounts: BTreeMap<AccountId, SessionMeta>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StoredMeta {
    Accounts(AccountsMeta),
    Legacy(SessionMeta),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSecrets {
    d_cookie: String,
    tokens: BTreeMap<TeamId, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccountsSecrets {
    accounts: BTreeMap<AccountId, SessionSecrets>,
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
    8.0
}
fn default_panel_radius() -> f32 {
    8.0
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
    "session_v3"
}

fn legacy_secret_account() -> &'static str {
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

fn load_legacy_secrets(meta: &SessionMeta) -> Result<Option<SessionSecrets>, AppError> {
    if let Some(json) = get_secret(legacy_secret_account())? {
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
    Ok(Some(SessionSecrets { d_cookie, tokens }))
}

fn session_from_parts(meta: SessionMeta, secrets: &SessionSecrets) -> Option<Session> {
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
    (!workspaces.is_empty()).then(|| Session {
        d_cookie: secrets.d_cookie.clone(),
        workspaces,
    })
}

fn metadata(accounts: &Accounts) -> AccountsMeta {
    AccountsMeta {
        active_account: accounts.active_account.clone(),
        accounts: accounts
            .sessions
            .iter()
            .map(|(id, session)| (id.clone(), SessionMeta::from(session)))
            .collect(),
    }
}

fn write_metadata_value(meta: &AccountsMeta) -> Result<(), AppError> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(meta)?;
    let path = session_path()?;
    let temp = dir.join(format!(".session-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temp, json)?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(temp);
            Err(error.into())
        }
    }
}

fn write_metadata(accounts: &Accounts) -> Result<(), AppError> {
    write_metadata_value(&metadata(accounts))
}

fn write_accounts(accounts: &Accounts) -> Result<(), AppError> {
    let secrets = AccountsSecrets {
        accounts: accounts
            .sessions
            .iter()
            .map(|(id, session)| (id.clone(), SessionSecrets::from(session)))
            .collect(),
    };
    set_secret(secret_account(), &serde_json::to_string(&secrets)?)?;
    write_metadata(accounts)
}

fn remove_legacy_secrets(meta: &SessionMeta) {
    for workspace in &meta.workspaces {
        delete_secret(&token_account(&workspace.team_id));
    }
    delete_secret("d_cookie");
    delete_secret(legacy_secret_account());
}

fn migrate_legacy(meta: SessionMeta) -> Result<Option<Accounts>, AppError> {
    let Some(secrets) = load_legacy_secrets(&meta)? else {
        return Ok(None);
    };
    let Some(session) = session_from_parts(meta.clone(), &secrets) else {
        return Ok(None);
    };
    let account_id = uuid::Uuid::new_v4().to_string();
    let accounts = Accounts {
        active_account: account_id.clone(),
        sessions: BTreeMap::from([(account_id, session)]),
    };
    write_accounts(&accounts)?;
    remove_legacy_secrets(&meta);
    Ok(Some(accounts))
}

pub fn load_accounts() -> Result<Option<Accounts>, AppError> {
    let path = session_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let meta = match serde_json::from_slice::<StoredMeta>(&bytes)? {
        StoredMeta::Accounts(meta) => meta,
        StoredMeta::Legacy(meta) => return migrate_legacy(meta),
    };
    let Some(json) = get_secret(secret_account())? else {
        return Ok(None);
    };
    let secrets: AccountsSecrets = serde_json::from_str(&json)?;
    let sessions: BTreeMap<_, _> = meta
        .accounts
        .into_iter()
        .filter_map(|(id, session_meta)| {
            let session = session_from_parts(session_meta, secrets.accounts.get(&id)?)?;
            Some((id, session))
        })
        .collect();
    if sessions.is_empty() {
        return Ok(None);
    }
    let active_account = if sessions.contains_key(&meta.active_account) {
        meta.active_account
    } else {
        sessions
            .keys()
            .next()
            .cloned()
            .expect("sessions is not empty")
    };
    Ok(Some(Accounts {
        active_account,
        sessions,
    }))
}

pub fn load_session() -> Result<Option<Session>, AppError> {
    let Some(accounts) = load_accounts()? else {
        return Ok(None);
    };
    Ok(accounts.sessions.get(&accounts.active_account).cloned())
}

fn same_identity(a: &Session, b: &Session) -> bool {
    a.workspaces.values().any(|workspace| {
        b.workspaces
            .get(&workspace.team_id)
            .is_some_and(|other| other.user_id == workspace.user_id)
    })
}

/// Add or refresh a Slack account and make it active.
pub fn save_session(session: &Session) -> Result<(), AppError> {
    let saved = match load_accounts() {
        Ok(accounts) => accounts,
        Err(AppError::Json(error)) => {
            tracing::warn!(%error, "replacing corrupt saved session during sign-in");
            None
        }
        Err(error) => return Err(error),
    };
    let mut accounts = saved.unwrap_or_else(|| {
        let account_id = uuid::Uuid::new_v4().to_string();
        Accounts {
            active_account: account_id,
            sessions: BTreeMap::new(),
        }
    });
    let account_id = accounts
        .sessions
        .iter()
        .find_map(|(id, saved)| same_identity(saved, session).then(|| id.clone()))
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    accounts.active_account = account_id.clone();
    accounts.sessions.insert(account_id, session.clone());
    write_accounts(&accounts)
}

pub fn set_active_account(account_id: &str) -> Result<(), AppError> {
    let bytes = match std::fs::read(session_path()?) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                std::io::Error::new(std::io::ErrorKind::NotFound, "no saved accounts").into(),
            );
        }
        Err(e) => return Err(e.into()),
    };
    let mut meta = match serde_json::from_slice::<StoredMeta>(&bytes)? {
        StoredMeta::Accounts(meta) => meta,
        StoredMeta::Legacy(_) => {
            let Some(accounts) = load_accounts()? else {
                return Err(
                    std::io::Error::new(std::io::ErrorKind::NotFound, "no saved accounts").into(),
                );
            };
            metadata(&accounts)
        }
    };
    if !meta.accounts.contains_key(account_id) {
        return Err(
            std::io::Error::new(std::io::ErrorKind::NotFound, "saved account not found").into(),
        );
    }
    meta.active_account = account_id.to_owned();
    write_metadata_value(&meta)
}

pub fn remove_account(account_id: &str) -> Result<Option<AccountId>, AppError> {
    let Some(mut accounts) = load_accounts()? else {
        clear_session()?;
        return Ok(None);
    };
    accounts.sessions.remove(account_id);
    if accounts.sessions.is_empty() {
        clear_session()?;
        return Ok(None);
    }
    if accounts.active_account == account_id {
        accounts.active_account = accounts
            .sessions
            .keys()
            .next()
            .cloned()
            .expect("sessions is not empty");
    }
    write_accounts(&accounts)?;
    Ok(Some(accounts.active_account))
}

pub fn clear_session() -> Result<(), AppError> {
    if let Ok(meta_bytes) = std::fs::read(session_path()?)
        && let Ok(StoredMeta::Legacy(meta)) = serde_json::from_slice::<StoredMeta>(&meta_bytes)
    {
        for workspace in &meta.workspaces {
            delete_secret(&token_account(&workspace.team_id));
        }
    }
    delete_secret("d_cookie");
    delete_secret(legacy_secret_account());
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
    fn saves_switches_and_removes_multiple_accounts() {
        let _guard = test_lock();
        reset();

        let first = session();
        save_session(&first).expect("save first account");
        let first_id = load_accounts()
            .expect("load accounts")
            .expect("accounts")
            .active_account;

        let mut second = session();
        second.d_cookie = "xoxd-second".into();
        second.workspaces.get_mut("T_ONE").unwrap().user_id = "U_OTHER".into();
        second.workspaces.remove("T_TWO");
        save_session(&second).expect("save second account");

        let accounts = load_accounts().expect("load accounts").expect("accounts");
        assert_eq!(accounts.sessions.len(), 2);
        assert_ne!(accounts.active_account, first_id);
        assert_eq!(
            accounts.sessions[&accounts.active_account].d_cookie,
            "xoxd-second"
        );

        set_active_account(&first_id).expect("switch account");
        assert_eq!(
            load_session().expect("load session").unwrap().d_cookie,
            "xoxd-cookie"
        );

        let remaining = remove_account(&first_id)
            .expect("remove account")
            .expect("remaining account");
        let accounts = load_accounts().expect("load accounts").expect("accounts");
        assert_eq!(accounts.sessions.len(), 1);
        assert_eq!(accounts.active_account, remaining);
        assert_eq!(accounts.sessions[&remaining].d_cookie, "xoxd-second");
    }

    #[test]
    fn signing_in_again_refreshes_matching_account() {
        let _guard = test_lock();
        reset();

        let first = session();
        save_session(&first).expect("save account");
        let mut refreshed = first;
        refreshed.d_cookie = "xoxd-refreshed".into();
        refreshed.workspaces.get_mut("T_ONE").unwrap().token = "xoxc-refreshed".into();
        save_session(&refreshed).expect("refresh account");

        let accounts = load_accounts().expect("load accounts").expect("accounts");
        assert_eq!(accounts.sessions.len(), 1);
        let active = &accounts.sessions[&accounts.active_account];
        assert_eq!(active.d_cookie, "xoxd-refreshed");
        assert_eq!(active.workspaces["T_ONE"].token, "xoxc-refreshed");
    }

    #[test]
    fn sign_in_recovers_from_corrupt_session_metadata() {
        let _guard = test_lock();
        reset();
        std::fs::create_dir_all(config_dir().expect("config dir")).expect("create config dir");
        std::fs::write(session_path().expect("session path"), b"{").expect("write corrupt session");

        save_session(&session()).expect("replace corrupt session");

        let loaded = load_session().expect("load session").expect("session");
        assert_eq!(loaded.d_cookie, "xoxd-cookie");
        assert_eq!(loaded.workspaces["T_ONE"].token, "xoxc-one");
    }

    #[test]
    fn migrates_previous_single_session_format() {
        let _guard = test_lock();
        reset();

        let session = session();
        let meta = SessionMeta::from(&session);
        std::fs::create_dir_all(config_dir().expect("config dir")).expect("create config dir");
        std::fs::write(
            session_path().expect("session path"),
            serde_json::to_string_pretty(&meta).expect("serialize metadata"),
        )
        .expect("write metadata");
        set_secret(
            legacy_secret_account(),
            &serde_json::to_string(&SessionSecrets::from(&session)).expect("serialize secrets"),
        )
        .expect("save legacy secrets");

        let accounts = load_accounts().expect("load accounts").expect("accounts");

        assert_eq!(accounts.sessions.len(), 1);
        assert_eq!(
            accounts.sessions[&accounts.active_account].workspaces["T_ONE"].token,
            "xoxc-one"
        );
        let secrets = test_secrets().lock().expect("test secret store poisoned");
        assert!(secrets.contains_key(secret_account()));
        assert!(!secrets.contains_key(legacy_secret_account()));
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
        assert!(
            !test_secrets()
                .lock()
                .expect("test secret store poisoned")
                .contains_key("d_cookie")
        );
    }
}
