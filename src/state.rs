use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use crate::slack::models::{
    Channel, ChannelId, Message as SlackMessage, MessageTs, TeamId, User, UserId,
};
use crate::slack::realtime::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    Loading,
    Main,
}

#[derive(Debug, Clone, Default)]
pub enum RealtimeStatus {
    #[default]
    Disconnected,
    Connected(Connection),
}

impl RealtimeStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self, RealtimeStatus::Connected(_))
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelMessages {
    pub messages: Vec<SlackMessage>,
    pub loaded: bool,
    pub pending: Vec<MessageTs>,
}

impl ChannelMessages {
    pub fn upsert(&mut self, msg: SlackMessage) -> bool {
        let Some(ts) = msg.ts.clone() else {
            self.messages.push(msg);
            return true;
        };
        match self.index_of(&ts) {
            Some(i) => {
                self.messages[i] = msg;
                false
            }
            None => {
                let pos = self
                    .messages
                    .binary_search_by(|m| cmp_ts(m.ts.as_deref(), Some(&ts)))
                    .unwrap_or_else(|e| e);
                self.messages.insert(pos, msg);
                true
            }
        }
    }

    pub fn remove(&mut self, ts: &str) -> bool {
        match self.index_of(ts) {
            Some(i) => {
                self.messages.remove(i);
                self.pending.retain(|p| p != ts);
                true
            }
            None => false,
        }
    }

    pub fn confirm(&mut self, client_msg_id: &str, confirmed: SlackMessage) -> bool {
        let temp_ts = self.messages.iter().find_map(|m| {
            match (m.client_msg_id.as_deref(), m.ts.as_deref()) {
                (Some(cid), Some(ts))
                    if cid == client_msg_id && self.pending.iter().any(|p| p == ts) =>
                {
                    Some(ts.to_owned())
                }
                _ => None,
            }
        });
        if let Some(ts) = temp_ts {
            self.remove(&ts);
            self.upsert(confirmed);
            true
        } else {
            false
        }
    }

    pub fn is_pending(&self, ts: &str) -> bool {
        self.pending.iter().any(|p| p == ts)
    }

    fn index_of(&self, ts: &str) -> Option<usize> {
        self.messages
            .iter()
            .position(|m| m.ts.as_deref() == Some(ts))
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub team_id: TeamId,
    pub name: String,
    pub url: String,
    pub self_user_id: UserId,
    pub channels: BTreeMap<ChannelId, Channel>,
    pub users: HashMap<UserId, User>,
    pub messages: HashMap<ChannelId, ChannelMessages>,
    pub typing: HashMap<ChannelId, Vec<(UserId, Instant)>>,
    pub rt: RealtimeStatus,
}

impl Workspace {
    pub fn from_session(s: &crate::config::WorkspaceSession) -> Self {
        Workspace {
            team_id: s.team_id.clone(),
            name: s.name.clone(),
            url: s.url.clone(),
            self_user_id: s.user_id.clone(),
            channels: BTreeMap::new(),
            users: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            rt: RealtimeStatus::default(),
        }
    }

    pub fn apply_boot(&mut self, boot: crate::slack::models::BootData) {
        if !boot.self_user.id.is_empty() {
            self.self_user_id = boot.self_user.id.clone();
        }
        if let Some(team) = &boot.team {
            if let Some(name) = &team.name {
                self.name = name.clone();
            }
            if let Some(url) = &team.url {
                self.url = url.clone();
            }
        }
        for channel in boot.all_channels() {
            self.channels.insert(channel.id.clone(), channel);
        }
    }

    pub fn display_name(&self, user_id: &str) -> String {
        display_name(self.users.get(user_id), user_id)
    }

    pub fn set_typing(&mut self, channel: &str, user: UserId, now: Instant) {
        let entry = self.typing.entry(channel.to_owned()).or_default();
        entry.retain(|(u, _)| u != &user);
        entry.push((user, now));
    }

    pub fn prune_typing(&mut self, now: Instant, ttl: Duration) -> bool {
        let mut changed = false;
        for entry in self.typing.values_mut() {
            let before = entry.len();
            entry.retain(|(_, seen)| now.duration_since(*seen) < ttl);
            changed |= entry.len() != before;
        }
        self.typing.retain(|_, v| !v.is_empty());
        changed
    }

    pub fn typing_names(&self, channel: &str) -> Vec<String> {
        self.typing
            .get(channel)
            .into_iter()
            .flatten()
            .filter(|(u, _)| u != &self.self_user_id)
            .map(|(u, _)| self.display_name(u))
            .collect()
    }
}

pub fn display_name(user: Option<&User>, user_id: &str) -> String {
    let Some(user) = user else {
        return user_id.to_owned();
    };
    if let Some(profile) = &user.profile {
        if let Some(dn) = non_empty(profile.display_name.as_deref()) {
            return dn.to_owned();
        }
        if let Some(rn) = non_empty(profile.real_name.as_deref()) {
            return rn.to_owned();
        }
    }
    if let Some(rn) = non_empty(user.real_name.as_deref()) {
        return rn.to_owned();
    }
    if let Some(name) = non_empty(user.name.as_deref()) {
        return name.to_owned();
    }
    user_id.to_owned()
}

pub fn channel_label(channel: &Channel) -> String {
    if let Some(name) = non_empty(channel.name.as_deref()) {
        if channel.is_im {
            return name.to_owned();
        }
        return format!("#{name}");
    }
    if channel.is_im {
        return "direct message".to_owned();
    }
    channel.id.clone()
}

pub fn message_text(msg: &SlackMessage) -> String {
    if let Some(text) = non_empty(msg.text.as_deref()) {
        return text.to_owned();
    }
    if let Some(subtype) = non_empty(msg.subtype.as_deref()) {
        return format!("[{subtype}]");
    }
    if !msg.files.is_empty() {
        return "[file]".to_owned();
    }
    if !msg.blocks.is_empty() {
        return "[rich message]".to_owned();
    }
    "[no text]".to_owned()
}

pub fn reaction_summary(reaction: &crate::slack::models::Reaction) -> String {
    format!(":{}: {}", reaction.name, reaction.count.max(1))
}

pub fn ts_key(ts: &str) -> (u64, u64) {
    let mut parts = ts.splitn(2, '.');
    let secs = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let seq = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (secs, seq)
}

pub fn cmp_ts(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => ts_key(a).cmp(&ts_key(b)),
        (None, None) => std::cmp::Ordering::Equal,
        (None, _) => std::cmp::Ordering::Less,
        (_, None) => std::cmp::Ordering::Greater,
    }
}

pub fn format_ts_hm(ts: &str) -> String {
    use chrono::{Local, TimeZone};
    let (secs, _) = ts_key(ts);
    match Local.timestamp_opt(secs as i64, 0).single() {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => secs.to_string(),
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::models::{Reaction, UserProfile};

    fn msg(ts: &str, text: &str) -> SlackMessage {
        SlackMessage {
            ts: Some(ts.to_owned()),
            text: Some(text.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn ts_key_orders_numerically_not_lexically() {
        assert!(ts_key("1783372360.000009") < ts_key("1783372360.000010"));
        assert!(ts_key("999.1") < ts_key("1000.0"));
    }

    #[test]
    fn upsert_keeps_ascending_order_and_dedupes() {
        let mut cm = ChannelMessages::default();
        assert!(cm.upsert(msg("1783372360.741769", "b")));
        assert!(cm.upsert(msg("1783372350.000000", "a")));
        assert!(cm.upsert(msg("1783372370.000000", "c")));
        assert!(!cm.upsert(msg("1783372360.741769", "b-edited")));

        let texts: Vec<_> = cm
            .messages
            .iter()
            .map(|m| m.text.clone().unwrap())
            .collect();
        assert_eq!(texts, vec!["a", "b-edited", "c"]);
    }

    #[test]
    fn remove_by_ts() {
        let mut cm = ChannelMessages::default();
        cm.upsert(msg("100.0", "x"));
        cm.upsert(msg("200.0", "y"));
        assert!(cm.remove("100.0"));
        assert!(!cm.remove("100.0"));
        assert_eq!(cm.messages.len(), 1);
        assert_eq!(cm.messages[0].text.as_deref(), Some("y"));
    }

    #[test]
    fn confirm_reconciles_pending_by_client_msg_id() {
        let mut cm = ChannelMessages::default();
        let mut pending = msg("9999999999.000000", "hi");
        pending.client_msg_id = Some("cid-1".to_owned());
        cm.upsert(pending);
        cm.pending.push("9999999999.000000".to_owned());
        assert!(cm.is_pending("9999999999.000000"));

        let mut confirmed = msg("1783372400.111111", "hi");
        confirmed.client_msg_id = Some("cid-1".to_owned());
        assert!(cm.confirm("cid-1", confirmed));

        assert_eq!(cm.messages.len(), 1);
        assert_eq!(cm.messages[0].ts.as_deref(), Some("1783372400.111111"));
        assert!(!cm.is_pending("1783372400.111111"));
    }

    #[test]
    fn display_name_fallback_chain() {
        let u = User {
            id: "U1".into(),
            name: Some("uname".into()),
            real_name: Some("Real Name".into()),
            profile: Some(UserProfile {
                display_name: Some("Display".into()),
                real_name: Some("Profile Real".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(display_name(Some(&u), "U1"), "Display");

        let u2 = User {
            id: "U2".into(),
            profile: Some(UserProfile {
                display_name: Some("  ".into()),
                real_name: Some("Profile Real".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(display_name(Some(&u2), "U2"), "Profile Real");

        let u3 = User {
            id: "U3".into(),
            name: Some("uname".into()),
            ..Default::default()
        };
        assert_eq!(display_name(Some(&u3), "U3"), "uname");

        assert_eq!(display_name(None, "U4"), "U4");
    }

    #[test]
    fn channel_label_variants() {
        let public = Channel {
            id: "C1".into(),
            name: Some("general".into()),
            is_channel: true,
            ..Default::default()
        };
        assert_eq!(channel_label(&public), "#general");

        let dm = Channel {
            id: "D1".into(),
            name: Some("alice".into()),
            is_im: true,
            ..Default::default()
        };
        assert_eq!(channel_label(&dm), "alice");

        let unnamed = Channel {
            id: "C9".into(),
            ..Default::default()
        };
        assert_eq!(channel_label(&unnamed), "C9");
    }

    #[test]
    fn message_text_fallbacks() {
        assert_eq!(message_text(&msg("1.0", "hello")), "hello");

        let empty = SlackMessage {
            ts: Some("1.0".into()),
            text: Some("   ".into()),
            subtype: Some("channel_join".into()),
            ..Default::default()
        };
        assert_eq!(message_text(&empty), "[channel_join]");

        let bare = SlackMessage {
            ts: Some("1.0".into()),
            ..Default::default()
        };
        assert_eq!(message_text(&bare), "[no text]");
    }

    #[test]
    fn reaction_summary_format() {
        let r = Reaction {
            name: "thumbsup".into(),
            count: 3,
            ..Default::default()
        };
        assert_eq!(reaction_summary(&r), ":thumbsup: 3");
    }

    #[test]
    fn prune_typing_drops_stale() {
        let mut ws = Workspace {
            team_id: "T1".into(),
            name: "test".into(),
            url: "https://t".into(),
            self_user_id: "USELF".into(),
            channels: BTreeMap::new(),
            users: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            rt: RealtimeStatus::default(),
        };
        let now = Instant::now();
        ws.set_typing("C1", "U1".into(), now - Duration::from_secs(10));
        ws.set_typing("C1", "U2".into(), now);
        assert!(ws.prune_typing(now, Duration::from_secs(4)));
        assert_eq!(ws.typing_names("C1"), vec![ws.display_name("U2")]);
    }
}
