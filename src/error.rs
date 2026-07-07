use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Slack error: {0}")]
    Slack(#[from] crate::slack::Error),
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring::Error),
}
