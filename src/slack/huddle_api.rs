//! Slack huddle ("rooms") API surface.
//!
//! Huddles are undocumented. Realtime `sh_room_*` frames report
//! `media_backend_type: "free_willy"` — Slack's in-house WebRTC media backend
//! (not Amazon Chime) — and the media server/signaling credentials come from
//! `rooms.join`. The request builders here follow the same shape as
//! [`super::api`]; join/leave/info response bodies are intentionally left as
//! raw JSON until captured live (see [`capture_rooms_join`]). Do not add
//! speculative response structs before then.

use std::time::Duration;

use serde_json::Value;

use crate::config::WorkspaceSession;

use super::Error;
use super::client::{PreparedRequest, SlackClient, redact_secrets};
use super::models::{ChannelId, Room};
use super::transport::Transport;

/// `rooms.info` — details for an existing huddle/room.
///
/// Parameter names are provisional until confirmed by a live capture; the
/// unit tests only pin the endpoint + token so a corrected field name later
/// does not silently break the request shape.
pub fn rooms_info(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    room: String,
) -> PreparedRequest {
    client.rest_form(workspace, "rooms.info", vec![("room", room)])
}

/// `rooms.join` — join an existing huddle. Returns the assigned media server +
/// signaling credentials for the `free_willy` backend.
///
/// Confirmed live: this method requires `channel_id` (huddles are joined by
/// channel, not room id).
pub fn rooms_join(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel_id: String,
) -> PreparedRequest {
    client.rest_form(workspace, "rooms.join", vec![("channel_id", channel_id)])
}

/// Leaving a huddle: the method name is **not yet identified** — `rooms.leave`
/// returns `unknown_method`. Do not call a guessed leave/close method: some
/// (`rooms.close`) may end the huddle for everyone. The real method must be
/// captured from the official client before a live join is safe.
pub fn rooms_leave_unknown(
    client: &SlackClient,
    workspace: &WorkspaceSession,
    room: String,
) -> PreparedRequest {
    client.rest_form(workspace, "rooms.leave", vec![("room", room)])
}

/// Read-only Phase 0 capture: dump the raw JSON that reveals the huddle wire
/// format, with all secrets redacted. Safe to run against a live workspace —
/// it never joins or mutates a huddle.
///
/// `conversations.info` on a channel with an active huddle carries the huddle
/// / room object (snack currently drops it into `Channel::extra`). If that
/// object exposes a room id, we also fetch `rooms.info` for it.
pub async fn capture_channel_huddle(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    channel: ChannelId,
) -> Result<Value, Error> {
    let info = transport
        .execute(super::api::conversations_info(
            client,
            workspace,
            channel.clone(),
        ))
        .await?;
    dump("conversations.info", &info);
    report_huddle_mentions("conversations.info", &info);

    // Huddle presence is not reliably on conversations.info; the boot payload
    // and counts are the other REST surfaces that can carry active-huddle
    // rooms. Scan both for anything huddle/room-shaped so we learn where it
    // lives without guessing a field path.
    match transport
        .execute(super::api::user_boot(client, workspace))
        .await
    {
        Ok(boot) => report_huddle_mentions("client.userBoot", &boot),
        Err(e) => tracing::warn!(error = %e, "client.userBoot capture failed"),
    }
    match transport
        .execute(super::api::client_counts(client, workspace))
        .await
    {
        Ok(counts) => report_huddle_mentions("client.counts", &counts),
        Err(e) => tracing::warn!(error = %e, "client.counts capture failed"),
    }

    if let Some(room) = extract_room_id(&info) {
        tracing::info!(%room, "found active huddle room id; capturing rooms.info");
        match transport
            .execute(rooms_info(client, workspace, room.clone()))
            .await
        {
            Ok(room_info) => dump("rooms.info", &room_info),
            Err(e) => tracing::warn!(%room, error = %e, "rooms.info capture failed"),
        }
    } else {
        tracing::info!(
            "no huddle room id found in REST payloads; if a huddle is active, its \
             state is likely only on the realtime websocket (see SNACK_HUDDLE_TRACE)"
        );
    }

    Ok(info)
}

/// Walk a JSON payload and print any subtree whose key mentions a huddle/room,
/// or any string that looks like a room id (`R…`). Redacts secrets. This is a
/// discovery aid for Phase 0 — it tells us which endpoint carries huddle state.
fn report_huddle_mentions(source: &str, value: &Value) {
    fn walk(path: &str, value: &Value, hits: &mut Vec<(String, Value)>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    let lower = key.to_ascii_lowercase();
                    if lower.contains("huddle") || lower.contains("room") || lower == "calls" {
                        hits.push((child_path.clone(), child.clone()));
                    }
                    walk(&child_path, child, hits);
                }
            }
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    walk(&format!("{path}[{i}]"), item, hits);
                }
            }
            Value::String(s) if is_room_id(s) => {
                hits.push((format!("{path} (room-id?)"), value.clone()));
            }
            _ => {}
        }
    }
    let mut hits = Vec::new();
    walk("", value, &mut hits);
    if hits.is_empty() {
        println!("----- {source}: no huddle/room mentions -----");
        return;
    }
    println!("----- {source}: huddle/room mentions -----");
    for (path, subtree) in hits {
        let pretty = serde_json::to_string(&subtree).unwrap_or_else(|_| subtree.to_string());
        // Cap noisy subtrees so a whole channel list does not flood the log.
        let pretty = if pretty.len() > 600 {
            format!("{}… ({} bytes)", &pretty[..600], pretty.len())
        } else {
            pretty
        };
        println!("  {path} = {}", redact_secrets(&pretty));
    }
    println!("----- end {source} mentions -----");
}

fn is_room_id(s: &str) -> bool {
    s.len() >= 8 && s.starts_with('R') && s[1..].bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Phase 0 realtime capture: connect to the flannel websocket exactly like
/// [`super::realtime`] and print incoming frames for `duration`, highlighting
/// any huddle/room/call events (their shapes are what Phase 1/2 must parse).
///
/// Run this while a huddle is toggled in the official client — start it *after*
/// the trace connects so the `room`/huddle start event is captured live.
pub async fn trace_realtime(
    workspace: &WorkspaceSession,
    d_cookie: &str,
    user_agent: &str,
    duration: Duration,
) -> Result<(), Error> {
    use wreq::ws::message::Message as WsMessage;

    let ws_url = super::realtime::flannel_url(&workspace.token, &workspace.team_id);
    let http = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome140)
        .build()
        .map_err(|e| Error::Transport(format!("client build: {e}")))?;
    let response = http
        .websocket(&ws_url)
        .header("User-Agent", user_agent)
        .header("Cookie", format!("d={d_cookie}"))
        .send()
        .await
        .map_err(|e| Error::Transport(format!("ws handshake: {e}")))?;
    let mut socket = response
        .into_websocket()
        .await
        .map_err(|e| Error::Transport(format!("ws upgrade: {e}")))?;

    println!(
        "===== realtime trace connected ({}s) — start/stop a huddle now =====",
        duration.as_secs()
    );

    let sleep = tokio::time::sleep(duration);
    tokio::pin!(sleep);
    let mut seen_types: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            incoming = socket.recv() => match incoming {
                Some(Ok(WsMessage::Text(text))) => inspect_frame(text.as_str(), &mut seen_types),
                Some(Ok(WsMessage::Close(_))) | None => {
                    println!("===== realtime trace: socket closed =====");
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(Error::Transport(format!("recv: {e}"))),
            }
        }
    }

    println!("===== realtime trace event-type histogram =====");
    for (kind, count) in &seen_types {
        println!("  {kind}: {count}");
    }
    println!("===== end realtime trace =====");
    Ok(())
}

/// Audio-gate capture (CONSENTED, outward-facing): discover the active huddle
/// room for `channel`, call `rooms.join` (by `channel_id`) and dump the
/// media-server/signaling response. `room_override` skips discovery.
///
/// WARNING: the correct leave method is not yet known (`rooms.leave` is
/// `unknown_method`), so this can leave you visibly in the huddle. It prints a
/// loud reminder to leave manually in the official client. Do not run this
/// unless you can manually leave.
pub async fn capture_rooms_join(
    transport: &Transport,
    client: &SlackClient,
    workspace: &WorkspaceSession,
    d_cookie: &str,
    user_agent: &str,
    channel: &str,
    room_override: Option<String>,
    discover_timeout: Duration,
) -> Result<(), Error> {
    let room_id = match room_override {
        Some(room) => room,
        None => {
            discover_active_room(workspace, d_cookie, user_agent, channel, discover_timeout).await?
        }
    };

    println!("===== joining huddle in {channel} (room {room_id}) =====");
    match transport
        .execute(rooms_join(client, workspace, channel.to_owned()))
        .await
    {
        Ok(body) => {
            dump("rooms.join", &body);
            println!(
                "!!! joined successfully and there is NO known leave method — \
                 leave the huddle MANUALLY in the official Slack client now !!!"
            );
        }
        // ok:false bodies surface as Api(error); the error string still tells us
        // whether the endpoint/params are right (e.g. invalid_arguments).
        Err(e) => println!("rooms.join error: {e}"),
    }
    Ok(())
}

/// Connect to flannel and wait until an active huddle room appears for `channel`.
async fn discover_active_room(
    workspace: &WorkspaceSession,
    d_cookie: &str,
    user_agent: &str,
    channel: &str,
    timeout: Duration,
) -> Result<String, Error> {
    use wreq::ws::message::Message as WsMessage;

    let ws_url = super::realtime::flannel_url(&workspace.token, &workspace.team_id);
    let http = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome140)
        .build()
        .map_err(|e| Error::Transport(format!("client build: {e}")))?;
    let response = http
        .websocket(&ws_url)
        .header("User-Agent", user_agent)
        .header("Cookie", format!("d={d_cookie}"))
        .send()
        .await
        .map_err(|e| Error::Transport(format!("ws handshake: {e}")))?;
    let mut socket = response
        .into_websocket()
        .await
        .map_err(|e| Error::Transport(format!("ws upgrade: {e}")))?;

    println!("===== waiting for an active huddle in {channel} — start one now =====");
    let sleep = tokio::time::sleep(timeout);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => {
                return Err(Error::Transport("no active huddle seen before timeout".into()));
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(WsMessage::Text(text))) => {
                    if let Some(room) = room_for_channel(text.as_str(), channel) {
                        println!("discovered active room {room}");
                        return Ok(room);
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None => {
                    return Err(Error::Transport("socket closed before a huddle appeared".into()));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(Error::Transport(format!("recv: {e}"))),
            }
        }
    }
}

fn room_for_channel(text: &str, channel: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    let room: Room = serde_json::from_value(value.get("room")?.clone()).ok()?;
    (room.is_active() && room.channel().map(String::as_str) == Some(channel)).then_some(room.id)
}

fn inspect_frame(text: &str, seen: &mut std::collections::BTreeMap<String, u32>) {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return,
    };
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("<no-type>")
        .to_owned();
    *seen.entry(kind.clone()).or_default() += 1;

    let lower = text.to_ascii_lowercase();
    let huddle_ish = kind.contains("room")
        || kind.contains("huddle")
        || kind.contains("call")
        || lower.contains("huddle")
        || lower.contains("\"room\"")
        || lower.contains("room_id");
    if huddle_ish {
        let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_owned());
        println!("----- huddle-ish frame: type={kind} -----");
        println!("{}", redact_secrets(&pretty));
        println!("----- end frame -----");
    }
}

/// Best-effort scan for a room id anywhere in the channel payload, so we do not
/// depend on a guessed key path before the shape is confirmed.
fn extract_room_id(value: &Value) -> Option<String> {
    fn walk(value: &Value, out: &mut Option<String>) {
        if out.is_some() {
            return;
        }
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if (key == "room_id" || key == "room" || key == "id")
                        && child.as_str().is_some_and(|s| s.starts_with('R'))
                    {
                        *out = child.as_str().map(str::to_owned);
                        return;
                    }
                    walk(child, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let mut out = None;
    walk(value, &mut out);
    out
}

fn dump(label: &str, value: &Value) {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    println!("===== {label} (redacted) =====");
    println!("{}", redact_secrets(&pretty));
    println!("===== end {label} =====");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::client::RequestBody;

    fn workspace() -> WorkspaceSession {
        WorkspaceSession {
            team_id: "T123".into(),
            enterprise_id: Some("E123".into()),
            user_id: "U080A3QP42C".into(),
            name: "Hack Club".into(),
            url: "https://hackclub.slack.com".into(),
            token: "xoxc-test-token".into(),
        }
    }

    fn form_fields(req: &PreparedRequest) -> &Vec<(String, String)> {
        match &req.body {
            RequestBody::Form(fields) => fields,
            other => panic!("expected form body, got {other:?}"),
        }
    }

    #[test]
    fn rooms_info_targets_endpoint_with_room() {
        let request = rooms_info(&SlackClient::default(), &workspace(), "R123".into());
        let fields = form_fields(&request);
        assert!(request.url.contains("/api/rooms.info?"));
        assert!(fields.contains(&("room".into(), "R123".into())));
        assert!(fields.contains(&("token".into(), "xoxc-test-token".into())));
        assert!(!request.redacted_debug().contains("xoxc-test-token"));
    }

    #[test]
    fn rooms_join_targets_endpoint_with_channel_id() {
        let request = rooms_join(&SlackClient::default(), &workspace(), "C123".into());
        assert!(request.url.contains("/api/rooms.join?"));
        assert!(form_fields(&request).contains(&("channel_id".into(), "C123".into())));
    }

    #[test]
    fn extract_room_id_finds_prefixed_id_anywhere() {
        let value = serde_json::json!({
            "channel": { "huddle": { "room": { "id": "R09ABCDEF" } } }
        });
        assert_eq!(extract_room_id(&value), Some("R09ABCDEF".into()));
    }

    #[test]
    fn extract_room_id_ignores_non_room_ids() {
        let value = serde_json::json!({ "channel": { "id": "C123", "name": "general" } });
        assert_eq!(extract_room_id(&value), None);
    }
}
