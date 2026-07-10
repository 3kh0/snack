pub mod api;
pub mod client;
pub mod edge;
pub mod events;
pub mod models;
pub mod realtime;
pub mod transport;
pub mod xparams;

pub use client::{PreparedRequest, SlackClient, SlackClientConfig};
pub use transport::Transport;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("file upload canceled")]
    UploadCanceled,
    #[error("missing workspace session for team {0}")]
    MissingWorkspace(models::TeamId),
    #[error("Slack API returned error: {0}")]
    Api(String),
    #[error("Slack rate limited request; retry after {retry_after_secs:?} seconds")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("Slack HTTP status {status}; retry after {retry_after_secs:?} seconds")]
    HttpStatus {
        status: u16,
        retry_after_secs: Option<u64>,
    },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("transport not up")]
    TransportNotConfigured,
}
