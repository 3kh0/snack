use std::collections::{BTreeMap, HashMap};

use crate::slack::models::{Channel, ChannelId, UserId};
use crate::state::{self, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteTarget {
    Channel(ChannelId),
    User { user: UserId, dm: Option<ChannelId> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub target: PaletteTarget,
    pub label: String,
    pub sublabel: String,
}

#[derive(Debug, Clone, Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub entries: Vec<PaletteEntry>,
    pub remote_seq: u64,
    pub remote_channels: BTreeMap<ChannelId, Channel>,
}

const MAX_RESULTS: usize = 12;

fn match_score(haystack: &str, needle: &str) -> Option<i32> {
    let hay = haystack.to_lowercase();
    if hay == needle {
        return Some(1000);
    }
    if hay.starts_with(needle) {
        return Some(800);
    }
    if hay
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.' | ','))
        .any(|word| !word.is_empty() && word.starts_with(needle))
    {
        return Some(600);
    }
    if hay.contains(needle) {
        return Some(400);
    }
    None
}

const PRIORITY_WEIGHT: f64 = 600.0;

struct FrecencyCtx {
    now: i64,
    max_priority: f64,
    max_local: f64,
}

impl FrecencyCtx {
    fn new(ws: &Workspace) -> Self {
        let now = state::now_secs();
        Self {
            now,
            max_priority: ws.priority_scores.values().copied().fold(0.0, f64::max),
            max_local: ws.max_frecency_score(now),
        }
    }

    fn bonus(&self, ws: &Workspace, channel_id: Option<&str>) -> i32 {
        let Some(id) = channel_id else { return 0 };
        let slack = if self.max_priority > 0.0 {
            ws.priority_score(id).unwrap_or(0.0).max(0.0) / self.max_priority
        } else {
            0.0
        };
        let local = if self.max_local > 0.0 {
            ws.frecency_score(id, self.now) / self.max_local
        } else {
            0.0
        };
        let usage_bonus = (slack.max(local) * PRIORITY_WEIGHT) as i32;
        let recent_bonus = ws
            .recent_channels
            .iter()
            .position(|c| c == id)
            .map(|idx| 200i32.saturating_sub(idx as i32 * 10).max(0))
            .unwrap_or(0);
        usage_bonus + recent_bonus
    }
}

struct Scored {
    score: i32,
    unread: u32,
    sort_label: String,
    entry: PaletteEntry,
}

fn entry_for_channel(ws: &Workspace, id: &ChannelId) -> Option<PaletteEntry> {
    let channel = ws.channels.get(id)?;
    if channel.is_im {
        let user = state::dm_user_id(channel)?.to_owned();
        let label = ws.display_name(&user);
        return Some(PaletteEntry {
            sublabel: user_handle(ws, &user),
            target: PaletteTarget::User {
                user,
                dm: Some(id.clone()),
            },
            label,
        });
    }
    Some(PaletteEntry {
        label: state::channel_display_name(ws, channel),
        sublabel: String::new(),
        target: PaletteTarget::Channel(id.clone()),
    })
}

fn user_handle(ws: &Workspace, user: &str) -> String {
    ws.users
        .get(user)
        .and_then(|u| u.name.as_deref())
        .filter(|n| !n.trim().is_empty())
        .map(|n| format!("@{n}"))
        .unwrap_or_default()
}

fn score_channel(
    ws: &Workspace,
    channel: &Channel,
    needle: &str,
    frecency: &FrecencyCtx,
) -> Option<Scored> {
    let label = state::channel_display_name(ws, channel);
    let name_score = state::channel_name(channel);
    let prev_score = channel
        .previous_names
        .iter()
        .filter_map(|prev| match_score(prev, needle))
        .max();
    let score = match_score(&label, needle)
        .into_iter()
        .chain(match_score(&name_score, needle))
        .chain(prev_score.map(|s| s - 200))
        .max()?;
    Some(Scored {
        score: score + frecency.bonus(ws, Some(channel.id.as_str())),
        unread: ws.unread_total(channel),
        sort_label: label.to_lowercase(),
        entry: PaletteEntry {
            label,
            sublabel: channel
                .previous_names
                .first()
                .map(|prev| format!("formerly #{prev}"))
                .unwrap_or_default(),
            target: PaletteTarget::Channel(channel.id.clone()),
        },
    })
}

pub fn recents(ws: &Workspace, active: Option<&str>) -> Vec<PaletteEntry> {
    ws.recent_channels
        .iter()
        .filter(|id| active != Some(id.as_str()))
        .filter_map(|id| entry_for_channel(ws, id))
        .collect()
}

pub fn rank(
    ws: &Workspace,
    query: &str,
    remote_channels: &BTreeMap<ChannelId, Channel>,
) -> Vec<PaletteEntry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return recents(ws, None);
    }

    let mut scored: Vec<Scored> = Vec::new();
    let mut im_by_user: HashMap<&str, &ChannelId> = HashMap::new();
    let frecency = FrecencyCtx::new(ws);

    for channel in ws.channels.values() {
        if channel.is_archived {
            continue;
        }
        if channel.is_im {
            if let Some(user) = state::dm_user_id(channel) {
                im_by_user.insert(user, &channel.id);
            }
            continue;
        }
        if let Some(scored_entry) = score_channel(ws, channel, &needle, &frecency) {
            scored.push(scored_entry);
        }
    }

    for channel in remote_channels.values() {
        if channel.is_archived || channel.is_im {
            continue;
        }
        if ws.channels.contains_key(&channel.id) {
            continue;
        }
        if let Some(scored_entry) = score_channel(ws, channel, &needle, &frecency) {
            scored.push(scored_entry);
        }
    }

    for user in ws.users.values() {
        if user.deleted || user.is_bot || user.id == ws.self_user_id {
            continue;
        }
        let label = ws.display_name(&user.id);
        let handle = user_handle(ws, &user.id);
        let score = match_score(&label, &needle)
            .into_iter()
            .chain(match_score(&handle, &needle))
            .max();
        let Some(score) = score else { continue };
        let dm = im_by_user.get(user.id.as_str()).map(|id| (*id).clone());
        let unread = dm
            .as_ref()
            .and_then(|id| ws.channels.get(id))
            .map(|c| ws.unread_total(c))
            .unwrap_or(0);
        scored.push(Scored {
            score: score + frecency.bonus(ws, dm.as_deref()),
            unread,
            sort_label: label.to_lowercase(),
            entry: PaletteEntry {
                label,
                sublabel: handle,
                target: PaletteTarget::User {
                    user: user.id.clone(),
                    dm,
                },
            },
        });
    }

    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.unread.cmp(&a.unread))
            .then_with(|| a.sort_label.cmp(&b.sort_label))
    });
    scored.truncate(MAX_RESULTS);
    scored.into_iter().map(|s| s.entry).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::*;
    use crate::slack::models::{Channel, User, UserProfile};
    use crate::state::{RealtimeStatus, Workspace};

    fn ws() -> Workspace {
        Workspace {
            team_id: "T".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            self_user_id: "U_SELF".into(),
            activity_unread_count: None,
            channels: BTreeMap::new(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels: Vec::new(),
            last_active_channel: None,
            priority_scores: BTreeMap::new(),
            frecency: BTreeMap::new(),
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            active_huddles: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        }
    }

    fn channel(id: &str, name: &str) -> Channel {
        Channel {
            id: id.into(),
            name: Some(name.into()),
            is_channel: true,
            ..Default::default()
        }
    }

    fn im(id: &str, user: &str) -> Channel {
        Channel {
            id: id.into(),
            is_im: true,
            user: Some(user.into()),
            ..Default::default()
        }
    }

    fn user(id: &str, name: &str, display: &str) -> User {
        User {
            id: id.into(),
            name: Some(name.into()),
            profile: Some(UserProfile {
                display_name: Some(display.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn insert_channel(ws: &mut Workspace, c: Channel) {
        ws.channels.insert(c.id.clone(), c);
    }

    #[test]
    fn finds_channel_by_name() {
        let mut ws = ws();
        insert_channel(&mut ws, channel("C_LOUNGE", "lounge"));
        insert_channel(&mut ws, channel("C_GEN", "general"));

        let hits = rank(&ws, "lounge", &BTreeMap::new());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].label, "lounge");
        assert_eq!(hits[0].target, PaletteTarget::Channel("C_LOUNGE".into()));
    }

    #[test]
    fn prefix_beats_substring() {
        let mut ws = ws();
        insert_channel(&mut ws, channel("C_ANN", "announcements"));
        insert_channel(&mut ws, channel("C_AN", "analytics"));

        let hits = rank(&ws, "an", &BTreeMap::new());
        assert_eq!(hits[0].label, "analytics");
    }

    #[test]
    fn user_entry_folds_existing_dm() {
        let mut ws = ws();
        ws.users
            .insert("U_ALICE".into(), user("U_ALICE", "alice", "Alice"));
        insert_channel(&mut ws, im("D_ALICE", "U_ALICE"));

        let hits = rank(&ws, "alice", &BTreeMap::new());
        assert_eq!(hits.len(), 1, "person listed once, not duplicated by DM");
        assert_eq!(
            hits[0].target,
            PaletteTarget::User {
                user: "U_ALICE".into(),
                dm: Some("D_ALICE".into()),
            }
        );
    }

    #[test]
    fn user_without_dm_has_no_channel() {
        let mut ws = ws();
        ws.users.insert("U_BOB".into(), user("U_BOB", "bob", "Bob"));

        let hits = rank(&ws, "bob", &BTreeMap::new());
        assert_eq!(
            hits[0].target,
            PaletteTarget::User {
                user: "U_BOB".into(),
                dm: None,
            }
        );
    }

    #[test]
    fn skips_self_deleted_and_bots() {
        let mut ws = ws();
        ws.users.insert("U_SELF".into(), user("U_SELF", "me", "Me"));
        let mut gone = user("U_GONE", "ghost", "Ghost");
        gone.deleted = true;
        ws.users.insert("U_GONE".into(), gone);
        let mut bot = user("U_BOT", "botty", "Botty");
        bot.is_bot = true;
        ws.users.insert("U_BOT".into(), bot);

        assert!(rank(&ws, "me", &BTreeMap::new()).is_empty());
        assert!(rank(&ws, "ghost", &BTreeMap::new()).is_empty());
        assert!(rank(&ws, "botty", &BTreeMap::new()).is_empty());
    }

    #[test]
    fn previous_name_matches_with_formerly_sublabel() {
        let mut ws = ws();
        let mut renamed = channel("C_NEW", "new-name");
        renamed.previous_names = vec!["old-name".into()];
        insert_channel(&mut ws, renamed);

        let hits = rank(&ws, "old", &BTreeMap::new());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].label, "new-name");
        assert_eq!(hits[0].sublabel, "formerly #old-name");
    }

    #[test]
    fn remote_channels_appear_without_being_in_workspace() {
        let ws = ws();
        let mut remote = BTreeMap::new();
        remote.insert("C_REMOTE".into(), channel("C_REMOTE", "remote-only"));

        let hits = rank(&ws, "remote", &remote);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].target, PaletteTarget::Channel("C_REMOTE".into()));
        assert!(!ws.channels.contains_key("C_REMOTE"));
    }

    #[test]
    fn recents_are_ordered_and_skip_active() {
        let mut ws = ws();
        insert_channel(&mut ws, channel("C1", "one"));
        insert_channel(&mut ws, channel("C2", "two"));
        insert_channel(&mut ws, channel("C3", "three"));
        ws.touch_recent(&"C1".into());
        ws.touch_recent(&"C2".into());
        ws.touch_recent(&"C3".into());

        let entries = recents(&ws, Some("C3"));
        let labels: Vec<_> = entries.iter().map(|e| e.label.clone()).collect();
        assert_eq!(labels, ["two", "one"]);
    }

    #[test]
    fn touch_recent_dedupes_and_caps() {
        let mut ws = ws();
        ws.touch_recent(&"A".into());
        ws.touch_recent(&"B".into());
        ws.touch_recent(&"A".into());
        assert_eq!(ws.recent_channels, vec!["A".to_string(), "B".to_string()]);

        for i in 0..40 {
            ws.touch_recent(&format!("X{i}"));
        }
        assert_eq!(ws.recent_channels.len(), crate::state::RECENT_CHANNELS_MAX);
    }
}
