use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
