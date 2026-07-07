use std::collections::BTreeMap;
use std::path::PathBuf;

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

pub fn config_dir() -> Result<PathBuf, AppError> {
    let dirs = directories::ProjectDirs::from("com", "hackclub", "snack")
        .ok_or_else(|| AppError::Io(std::io::Error::other("no home directory for config path")))?;
    Ok(dirs.config_dir().to_path_buf())
}

fn session_path() -> Result<PathBuf, AppError> {
    Ok(config_dir()?.join("session.json"))
}

fn token_account(team_id: &str) -> String {
    format!("xoxc:{team_id}")
}

fn entry(account: &str) -> Result<keyring::Entry, AppError> {
    Ok(keyring::Entry::new("com.client.snack", account)?)
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

    entry("d_cookie")?.set_password(&session.d_cookie)?;
    for w in session.workspaces.values() {
        entry(&token_account(&w.team_id))?.set_password(&w.token)?;
    }
    Ok(())
}

pub fn load_session() -> Result<Option<Session>, AppError> {
    let path = session_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let meta: SessionMeta = serde_json::from_slice(&bytes)?;

    let d_cookie = match entry("d_cookie")?.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let mut workspaces = BTreeMap::new();
    for w in meta.workspaces {
        let token = match entry(&token_account(&w.team_id))?.get_password() {
            Ok(token) => token,
            Err(keyring::Error::NoEntry) => continue,
            Err(e) => return Err(e.into()),
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
        d_cookie,
        workspaces,
    }))
}

pub fn clear_session() -> Result<(), AppError> {
    if let Ok(meta_bytes) = std::fs::read(session_path()?) {
        if let Ok(meta) = serde_json::from_slice::<SessionMeta>(&meta_bytes) {
            for w in &meta.workspaces {
                if let Ok(e) = entry(&token_account(&w.team_id)) {
                    let _ = e.delete_credential();
                }
            }
        }
    }
    if let Ok(e) = entry("d_cookie") {
        let _ = e.delete_credential();
    }
    match std::fs::remove_file(session_path()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
