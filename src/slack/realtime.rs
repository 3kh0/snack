use std::time::Duration;

use iced::Subscription;
use iced::futures::{SinkExt, Stream};
use serde_json::Value;
use tokio::sync::mpsc;

use super::events::{RawEvent, RtEvent};
use super::models::{Message as SlackMessage, TeamId};

#[derive(Debug, Clone)]
pub enum RtUpdate {
    Connected {
        generation: u64,
        connection: Connection,
    },
    Event {
        generation: u64,
        event: RtEvent,
    },
    Disconnected {
        generation: u64,
    },
}

#[derive(Debug, Clone)]
pub struct Connection {
    tx: mpsc::Sender<String>,
}

impl Connection {
    pub fn send(&self, frame: String) {
        let _ = self.tx.try_send(frame);
    }

    pub fn from_sender(tx: mpsc::Sender<String>) -> Self {
        Self { tx }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectParams {
    pub team: TeamId,
    pub ws_url: String,
    pub d_cookie: String,
    pub user_agent: String,
}

pub fn flannel_url(token: &str, team_id: &str) -> String {
    format!("wss://wss-primary.slack.com/?token={token}&flannel=3&gateway_server={team_id}-1")
}

pub fn user_typing_frame(channel: &str) -> String {
    format!(
        r#"{{"type":"user_typing","channel":{}}}"#,
        serde_json::json!(channel)
    )
}

pub fn presence_query_frame(ids: &[String]) -> String {
    serde_json::json!({ "type": "presence_query", "ids": ids }).to_string()
}

pub fn connect(params: ConnectParams) -> Subscription<(TeamId, RtUpdate)> {
    Subscription::run_with(params.clone(), build_live_stream)
}

fn build_live_stream(params: &ConnectParams) -> impl Stream<Item = (TeamId, RtUpdate)> + use<> {
    let params = params.clone();
    iced::stream::channel(64, move |mut output| async move {
        let mut backoff = Duration::from_secs(1);
        let mut generation = 0;
        loop {
            generation += 1;
            match run_connection(&params, generation, &mut output).await {
                Ok(()) => backoff = Duration::from_secs(1),
                Err(e) => {
                    tracing::warn!(team = %params.team, error = %e, "flannel connection ended");
                }
            }
            let _ = output
                .send((params.team.clone(), RtUpdate::Disconnected { generation }))
                .await;
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    })
}

type OutSink = iced::futures::channel::mpsc::Sender<(TeamId, RtUpdate)>;

async fn run_connection(
    params: &ConnectParams,
    generation: u64,
    output: &mut OutSink,
) -> Result<(), String> {
    let http = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome140)
        .build()
        .map_err(|e| format!("client build: {e}"))?;

    let response = http
        .websocket(&params.ws_url)
        .header("User-Agent", params.user_agent.as_str())
        .header("Cookie", format!("d={}", params.d_cookie))
        .send()
        .await
        .map_err(|e| format!("ws handshake: {e}"))?;
    let mut socket = response
        .into_websocket()
        .await
        .map_err(|e| format!("ws upgrade: {e}"))?;

    let (tx, mut rx) = mpsc::channel::<String>(64);
    if output
        .send((
            params.team.clone(),
            RtUpdate::Connected {
                generation,
                connection: Connection { tx },
            },
        ))
        .await
        .is_err()
    {
        return Ok(());
    }

    let mut ping_id: u64 = 0;
    let mut ping = tokio::time::interval(Duration::from_secs(15));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(wreq::ws::message::Message::Text(text))) => {
                    if let Some(event) = parse_event(text.as_str()) {
                        if output
                            .send((
                                params.team.clone(),
                                RtUpdate::Event { generation, event },
                            ))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                }
                Some(Ok(wreq::ws::message::Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(format!("recv: {e}")),
            },
            outbound = rx.recv() => match outbound {
                Some(frame) => {
                    if let Err(e) = socket.send(wreq::ws::message::Message::text(frame)).await {
                        return Err(format!("send: {e}"));
                    }
                }
                None => return Ok(()),
            },
            _ = ping.tick() => {
                ping_id += 1;
                let frame = format!(r#"{{"type":"ping","id":{ping_id}}}"#);
                if let Err(e) = socket.send(wreq::ws::message::Message::text(frame)).await {
                    return Err(format!("ping: {e}"));
                }
            }
        }
    }
}

pub fn parse_event(text: &str) -> Option<RtEvent> {
    let value: Value = serde_json::from_str(text).ok()?;
    let kind = value.get("type").and_then(Value::as_str)?;
    match kind {
        "message" => {
            let channel = value.get("channel").and_then(Value::as_str)?.to_owned();
            match value.get("subtype").and_then(Value::as_str) {
                Some("message_changed") => {
                    let nested = value.get("message")?.clone();
                    let mut message: SlackMessage = serde_json::from_value(nested).ok()?;
                    message.channel.get_or_insert(channel.clone());
                    Some(RtEvent::MessageChanged { channel, message })
                }
                Some("message_replied") => {
                    let nested = value.get("message")?.clone();
                    let mut message: SlackMessage = serde_json::from_value(nested).ok()?;
                    message.channel.get_or_insert(channel);
                    Some(RtEvent::Message(message))
                }
                Some("message_deleted") => {
                    let deleted_ts = value.get("deleted_ts").and_then(Value::as_str)?.to_owned();
                    Some(RtEvent::MessageDeleted {
                        channel,
                        deleted_ts,
                    })
                }
                _ => {
                    let mut message: SlackMessage = serde_json::from_value(value).ok()?;
                    message.channel.get_or_insert(channel);
                    Some(RtEvent::Message(message))
                }
            }
        }
        "user_typing" => Some(RtEvent::UserTyping {
            channel: value.get("channel").and_then(Value::as_str)?.to_owned(),
            user: value.get("user").and_then(Value::as_str)?.to_owned(),
        }),
        "presence_change" => Some(RtEvent::PresenceChange {
            user: value
                .get("user")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            presence: value
                .get("presence")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        }),
        "reaction_added" => parse_reaction_event(value, true),
        "reaction_removed" => parse_reaction_event(value, false),
        "activity" => {
            let entry = value.get("entry")?.clone();
            let item: super::models::ActivityItem = serde_json::from_value(entry).ok()?;
            Some(RtEvent::ActivityUpdated(item))
        }
        _ => {
            let raw: RawEvent = serde_json::from_value(value).ok()?;
            Some(RtEvent::Unknown(raw))
        }
    }
}

fn parse_reaction_event(value: Value, added: bool) -> Option<RtEvent> {
    let item = value.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let channel = item.get("channel").and_then(Value::as_str)?.to_owned();
    let ts = item.get("ts").and_then(Value::as_str)?.to_owned();
    let user = value
        .get("user")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let reaction = value
        .get("reaction")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if reaction.is_empty() {
        return None;
    }
    Some(if added {
        RtEvent::ReactionAdded {
            channel,
            ts,
            user,
            reaction,
        }
    } else {
        RtEvent::ReactionRemoved {
            channel,
            ts,
            user,
            reaction,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_message() {
        let frame = r#"{"type":"message","channel":"C1","user":"U1","ts":"1.2","text":"hi"}"#;
        match parse_event(frame) {
            Some(RtEvent::Message(m)) => {
                assert_eq!(m.channel.as_deref(), Some("C1"));
                assert_eq!(m.text.as_deref(), Some("hi"));
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn parses_message_changed_and_deleted() {
        let changed = r#"{"type":"message","subtype":"message_changed","channel":"C1","message":{"ts":"1.2","text":"edited"}}"#;
        match parse_event(changed) {
            Some(RtEvent::MessageChanged { channel, message }) => {
                assert_eq!(channel, "C1");
                assert_eq!(message.text.as_deref(), Some("edited"));
                assert_eq!(message.channel.as_deref(), Some("C1"));
            }
            other => panic!("expected MessageChanged, got {other:?}"),
        }

        let deleted =
            r#"{"type":"message","subtype":"message_deleted","channel":"C1","deleted_ts":"1.2"}"#;
        match parse_event(deleted) {
            Some(RtEvent::MessageDeleted {
                channel,
                deleted_ts,
            }) => {
                assert_eq!(channel, "C1");
                assert_eq!(deleted_ts, "1.2");
            }
            other => panic!("expected MessageDeleted, got {other:?}"),
        }
    }

    #[test]
    fn parses_message_replied_as_nested_message() {
        let replied = r#"{"type":"message","subtype":"message_replied","channel":"C1","message":{"type":"message","user":"U1","text":"actual","ts":"1.2"}}"#;
        match parse_event(replied) {
            Some(RtEvent::Message(message)) => {
                assert_eq!(message.user.as_deref(), Some("U1"));
                assert_eq!(message.text.as_deref(), Some("actual"));
                assert_eq!(message.channel.as_deref(), Some("C1"));
                assert_ne!(message.subtype.as_deref(), Some("message_replied"));
            }
            other => panic!("expected nested Message, got {other:?}"),
        }
    }

    #[test]
    fn parses_typing() {
        let frame = r#"{"type":"user_typing","channel":"C1","user":"U9"}"#;
        assert!(matches!(
            parse_event(frame),
            Some(RtEvent::UserTyping { .. })
        ));
    }

    #[test]
    fn parses_reaction_events_for_messages() {
        let added = r#"{"type":"reaction_added","user":"U1","reaction":"thumbsup","item":{"type":"message","channel":"C1","ts":"1.2"}}"#;
        match parse_event(added) {
            Some(RtEvent::ReactionAdded {
                channel,
                ts,
                user,
                reaction,
            }) => {
                assert_eq!(channel, "C1");
                assert_eq!(ts, "1.2");
                assert_eq!(user, "U1");
                assert_eq!(reaction, "thumbsup");
            }
            other => panic!("expected ReactionAdded, got {other:?}"),
        }

        let removed = r#"{"type":"reaction_removed","user":"U1","reaction":"eyes","item":{"type":"message","channel":"C1","ts":"1.2"}}"#;
        assert!(matches!(
            parse_event(removed),
            Some(RtEvent::ReactionRemoved { reaction, .. }) if reaction == "eyes"
        ));
    }

    #[test]
    fn parses_activity_updated_entry() {
        let frame = r#"{"type":"activity","subtype":"activity_updated","key":"dm-D1","entry":{"is_unread":true,"feed_ts":"1783834090.99","key":"dm-D1","item":{"type":"dm","bundle_info":{"payload":{"dm_entry":{"latest_message":{"ts":"1783834090.38","channel":"D1"}}}}}}}"#;
        match parse_event(frame) {
            Some(RtEvent::ActivityUpdated(item)) => {
                assert_eq!(item.key, "dm-D1");
                assert!(item.is_unread);
                assert_eq!(item.channel(), Some("D1"));
                assert_eq!(item.ts(), Some("1783834090.38"));
            }
            other => panic!("expected ActivityUpdated, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_is_unknown_not_none() {
        let frame = r#"{"type":"pref_change","name":"x"}"#;
        assert!(matches!(parse_event(frame), Some(RtEvent::Unknown(_))));
    }

    #[test]
    fn non_event_frames_are_none() {
        assert!(parse_event("not json").is_none());
        assert!(parse_event(r#"{"reply_to":1,"ok":true}"#).is_none());
    }

    #[test]
    fn flannel_url_contains_token_and_gateway() {
        let url = flannel_url("xoxc-abc", "T123");
        assert!(url.contains("token=xoxc-abc"));
        assert!(url.contains("gateway_server=T123-1"));
        assert!(url.starts_with("wss://"));
    }

    #[test]
    fn user_typing_frame_contains_channel() {
        assert_eq!(
            user_typing_frame("C1"),
            r#"{"type":"user_typing","channel":"C1"}"#
        );
    }

    #[test]
    fn presence_query_frame_contains_ids() {
        let frame = presence_query_frame(&["U1".into(), "U2".into()]);
        let value: serde_json::Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(value["type"], "presence_query");
        assert_eq!(value["ids"], serde_json::json!(["U1", "U2"]));
    }
}
