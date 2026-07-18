use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::config::WorkspaceSession;
use crate::error::AppError;
use crate::slack::models::{Channel, Message as SlackMessage, User};
use crate::state::{ChannelMessages, RealtimeStatus, Workspace};

const SCHEMA_VERSION: i64 = 1;
const MAX_CACHED_MESSAGES_PER_CHANNEL: usize = 200;

pub struct Cache {
    conn: Connection,
}

impl Cache {
    fn default_path(account_id: &str) -> Result<std::path::PathBuf, AppError> {
        if uuid::Uuid::parse_str(account_id).is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid account id for cache",
            )
            .into());
        }
        let dir = crate::config::data_dir()?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("cache-{account_id}.sqlite")))
    }

    pub fn open_default(account_id: &str, adopt_legacy: bool) -> Result<Self, AppError> {
        let path = Self::default_path(account_id)?;
        let legacy = crate::config::data_dir()?.join("cache.sqlite");
        if adopt_legacy
            && !path.exists()
            && legacy.exists()
            && std::fs::rename(&legacy, &path).is_err()
            && std::fs::copy(&legacy, &path).is_ok()
        {
            let _ = std::fs::remove_file(legacy);
        } else if !adopt_legacy && legacy.exists() {
            let _ = std::fs::remove_file(legacy);
        }
        Self::open(path)
    }

    pub fn remove_default(account_id: &str) -> Result<(), AppError> {
        match std::fs::remove_file(Self::default_path(account_id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        let cache = Self { conn };
        cache.migrate()?;
        Ok(cache)
    }

    pub fn load_workspace(
        &self,
        session: &WorkspaceSession,
    ) -> Result<Option<Workspace>, AppError> {
        let Some((name, url, self_user_id, last_active_channel, recent_channels, frecency)) = self
            .conn
            .query_row(
                "select name, url, self_user_id, last_active_channel, recent_channels, frecency from workspaces where team_id = ?1",
                params![session.team_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(None);
        };

        let recent_channels = recent_channels
            .as_deref()
            .and_then(|json| serde_json::from_str::<Vec<String>>(json).ok())
            .unwrap_or_default();
        let frecency = frecency
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        let mut ws = Workspace {
            team_id: session.team_id.clone(),
            name,
            url,
            self_user_id,
            activity_unread_count: None,
            channels: Default::default(),
            starred_order: Vec::new(),
            dm_order: Vec::new(),
            recent_channels,
            last_active_channel,
            priority_scores: Default::default(),
            frecency,
            hide_read_channels_unless_starred: false,
            priority_sidebar_section: false,
            vip_users: std::collections::HashSet::new(),
            sidebar: Default::default(),
            users: HashMap::new(),
            custom_emoji: HashMap::new(),
            messages: HashMap::new(),
            typing: HashMap::new(),
            presence: HashMap::new(),
            active_huddles: HashMap::new(),
            rt: RealtimeStatus::default(),
            rt_generation: 0,
        };

        let mut stmt = self
            .conn
            .prepare("select json from channels where team_id = ?1")?;
        let rows = stmt.query_map(params![session.team_id], |row| row.get::<_, String>(0))?;
        for row in rows {
            let channel: Channel = serde_json::from_str(&row?)?;
            ws.channels.insert(channel.id.clone(), channel);
        }

        let mut stmt = self
            .conn
            .prepare("select json from users where team_id = ?1")?;
        let rows = stmt.query_map(params![session.team_id], |row| row.get::<_, String>(0))?;
        for row in rows {
            let value: serde_json::Value = serde_json::from_str(&row?)?;
            let user: User = serde_json::from_value(value)?;
            ws.users.insert(user.id.clone(), user);
        }

        let mut stmt = self.conn.prepare(
            "select channel_id, ts, json, pending from messages where team_id = ?1 order by channel_id, ts",
        )?;
        let rows = stmt.query_map(params![session.team_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?;
        for row in rows {
            let (channel_id, ts, json, pending) = row?;
            let msg: SlackMessage = serde_json::from_str(&json)?;
            let cm = ws.messages.entry(channel_id).or_default();
            cm.upsert(msg);
            cm.loaded = true;
            cm.has_more_older = true;
            if pending {
                cm.pending.push(ts);
            }
        }

        let mut stmt = self.conn.prepare(
            "select channel_id, last_read, unread_count, mention_count from channel_state where team_id = ?1",
        )?;
        let rows = stmt.query_map(params![session.team_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, u32>(3)?,
            ))
        })?;
        for row in rows {
            let (channel_id, last_read, unread_count, mention_count) = row?;
            let cm = ws.messages.entry(channel_id).or_default();
            cm.last_read = last_read;
            cm.unread_count = unread_count;
            cm.mention_count = mention_count;
        }

        Ok(Some(ws))
    }

    pub fn save_workspace(&self, ws: &Workspace) -> Result<(), AppError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "insert into workspaces (team_id, name, url, self_user_id, last_active_channel, recent_channels, frecency)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             on conflict(team_id) do update set
               name = excluded.name,
               url = excluded.url,
               self_user_id = excluded.self_user_id,
               last_active_channel = excluded.last_active_channel,
               recent_channels = excluded.recent_channels,
               frecency = excluded.frecency",
            params![
                ws.team_id,
                ws.name,
                ws.url,
                ws.self_user_id,
                ws.last_active_channel,
                serde_json::to_string(&ws.recent_channels)?,
                serde_json::to_string(&ws.frecency)?,
            ],
        )?;

        tx.execute(
            "delete from channels where team_id = ?1",
            params![ws.team_id],
        )?;
        for channel in ws.channels.values() {
            tx.execute(
                "insert into channels (team_id, channel_id, json) values (?1, ?2, ?3)",
                params![ws.team_id, channel.id, serde_json::to_string(channel)?],
            )?;
        }

        tx.execute("delete from users where team_id = ?1", params![ws.team_id])?;
        for user in ws.users.values() {
            let json = serde_json::to_string(&serde_json::to_value(user)?)?;
            tx.execute(
                "insert into users (team_id, user_id, json) values (?1, ?2, ?3)",
                params![ws.team_id, user.id, json],
            )?;
        }

        tx.execute(
            "delete from messages where team_id = ?1",
            params![ws.team_id],
        )?;
        tx.execute(
            "delete from channel_state where team_id = ?1",
            params![ws.team_id],
        )?;
        for (channel_id, cm) in &ws.messages {
            tx.execute(
                "insert into channel_state (team_id, channel_id, last_read, unread_count, mention_count)
                 values (?1, ?2, ?3, ?4, ?5)",
                params![
                    ws.team_id,
                    channel_id,
                    cm.last_read,
                    cm.unread_count,
                    cm.mention_count
                ],
            )?;
            let mut cached_messages: Vec<_> = cm
                .messages
                .iter()
                .rev()
                .take(MAX_CACHED_MESSAGES_PER_CHANNEL)
                .collect();
            cached_messages.reverse();
            for msg in cached_messages {
                let Some(ts) = msg.ts.as_deref() else {
                    continue;
                };
                tx.execute(
                    "insert into messages (team_id, channel_id, ts, json, pending)
                     values (?1, ?2, ?3, ?4, ?5)",
                    params![
                        ws.team_id,
                        channel_id,
                        ts,
                        serde_json::to_string(msg)?,
                        cm.is_pending(ts)
                    ],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), AppError> {
        self.conn.execute_batch(
            "
            pragma foreign_keys = on;
            create table if not exists meta (
                key text primary key,
                value text not null
            );
            create table if not exists workspaces (
                team_id text primary key,
                name text not null,
                url text not null,
                self_user_id text not null,
                last_active_channel text,
                recent_channels text
            );
            create table if not exists channels (
                team_id text not null,
                channel_id text not null,
                json text not null,
                primary key (team_id, channel_id)
            );
            create table if not exists users (
                team_id text not null,
                user_id text not null,
                json text not null,
                primary key (team_id, user_id)
            );
            create table if not exists messages (
                team_id text not null,
                channel_id text not null,
                ts text not null,
                json text not null,
                pending integer not null default 0,
                primary key (team_id, channel_id, ts)
            );
            create table if not exists channel_state (
                team_id text not null,
                channel_id text not null,
                last_read text,
                unread_count integer not null default 0,
                mention_count integer not null default 0,
                primary key (team_id, channel_id)
            );
            ",
        )?;
        let _ = self.conn.execute(
            "alter table workspaces add column last_active_channel text",
            [],
        );
        let _ = self
            .conn
            .execute("alter table workspaces add column recent_channels text", []);
        let _ = self
            .conn
            .execute("alter table workspaces add column frecency text", []);
        self.conn.execute(
            "insert into meta (key, value) values ('schema_version', ?1)
             on conflict(key) do update set value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::models::{Channel, User};

    fn session() -> WorkspaceSession {
        WorkspaceSession {
            team_id: "T1".into(),
            enterprise_id: None,
            user_id: "U_SELF".into(),
            name: "Test".into(),
            url: "https://test.slack.com".into(),
            token: "xoxc-test".into(),
        }
    }

    #[test]
    fn roundtrips_workspace_cache() {
        let cache = Cache::open(":memory:").unwrap();
        let session = session();
        let mut ws = Workspace::from_session(&session);
        ws.channels.insert(
            "C1".into(),
            Channel {
                id: "C1".into(),
                name: Some("general".into()),
                is_channel: true,
                ..Default::default()
            },
        );
        let mut cm = ChannelMessages::default();
        cm.upsert(SlackMessage {
            ts: Some("1.000001".into()),
            text: Some("cached".into()),
            channel: Some("C1".into()),
            ..Default::default()
        });
        cm.loaded = true;
        cm.last_read = Some("1.000001".into());
        ws.messages.insert("C1".into(), cm);
        ws.last_active_channel = Some("C1".into());
        ws.touch_recent(&"C1".into());
        ws.record_visit(&"C1".into(), 1_000_000);

        cache.save_workspace(&ws).unwrap();
        let loaded = cache.load_workspace(&session).unwrap().unwrap();

        assert_eq!(loaded.channels["C1"].name.as_deref(), Some("general"));
        assert_eq!(
            loaded.messages["C1"].messages[0].text.as_deref(),
            Some("cached")
        );
        assert_eq!(loaded.messages["C1"].last_read.as_deref(), Some("1.000001"));
        assert_eq!(loaded.last_active_channel.as_deref(), Some("C1"));
        assert_eq!(loaded.recent_channels, vec!["C1".to_string()]);
        assert_eq!(loaded.frecency_score("C1", 1_000_000), 1.0);
    }

    #[test]
    fn loads_legacy_user_json_with_duplicate_flattened_keys() {
        let cache = Cache::open(":memory:").unwrap();
        let session = session();
        cache
            .save_workspace(&Workspace::from_session(&session))
            .unwrap();
        cache
            .conn
            .execute(
                "insert into users (team_id, user_id, json) values (?1, ?2, ?3)",
                params![
                    session.team_id,
                    "U1",
                    r#"{"id":"U1","deleted":false,"is_bot":false,"deleted":true}"#
                ],
            )
            .unwrap();

        let loaded = cache.load_workspace(&session).unwrap().unwrap();
        assert!(loaded.users["U1"].deleted);
    }

    #[test]
    fn cache_serialization_collapses_typed_keys_left_in_user_extra() {
        let cache = Cache::open(":memory:").unwrap();
        let session = session();
        let mut workspace = Workspace::from_session(&session);
        workspace.users.insert(
            "U1".into(),
            User {
                id: "U1".into(),
                extra: std::collections::BTreeMap::from([(
                    "deleted".into(),
                    serde_json::json!(true),
                )]),
                ..Default::default()
            },
        );

        cache.save_workspace(&workspace).unwrap();
        let loaded = cache.load_workspace(&session).unwrap().unwrap();
        assert!(loaded.users["U1"].deleted);
    }
}
