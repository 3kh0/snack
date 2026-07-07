pub mod api;
pub mod client;
pub mod edge;
pub mod events;
pub mod models;
pub mod realtime;
pub mod xparams;

pub use client::{PreparedRequest, SlackClient, SlackClientConfig};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing workspace session for team {0}")]
    MissingWorkspace(models::TeamId),
    #[error("Slack API returned error: {0}")]
    Api(String),
    #[error("transport not up")]
    TransportNotConfigured,
}
