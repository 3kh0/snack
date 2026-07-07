use std::time::Duration;

use iced::Subscription;

use crate::slack::models::TeamId;
use crate::slack::realtime::{self, ConnectParams, RtUpdate};

use super::{App, Message};

pub(super) fn subscription(app: &App) -> Subscription<Message> {
    let mut subs = vec![iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)];

    if let Some(session) = &app.session {
        let user_agent = crate::slack::xparams::Identity::from_capture().user_agent;
        for ws in session.workspaces.values() {
            let params = ConnectParams {
                team: ws.team_id.clone(),
                ws_url: realtime::flannel_url(&ws.token, &ws.team_id),
                d_cookie: session.d_cookie.clone(),
                user_agent: user_agent.clone(),
            };
            subs.push(realtime::connect(params).map(map_rt_update));
        }
    }

    Subscription::batch(subs)
}

fn map_rt_update((team, update): (TeamId, RtUpdate)) -> Message {
    match update {
        RtUpdate::Connected {
            generation,
            connection,
        } => Message::RtConnected(team, generation, connection),
        RtUpdate::Event { generation, event } => Message::Realtime(team, generation, event),
        RtUpdate::Disconnected { generation } => Message::RtDisconnected(team, generation),
    }
}
