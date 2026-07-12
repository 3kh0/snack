use std::collections::HashMap;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
#[cfg(unix)]
use std::time::Duration;

use iced::Subscription;
#[cfg(unix)]
use iced::futures::SinkExt;
#[cfg(unix)]
use iced::futures::channel::mpsc as iced_mpsc;
use iced::window;
use iced::window::Screenshot;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use super::palette::PaletteTarget;
use super::update::update;
use super::{App, Message};
use crate::state::Screen;

static REPLIES: OnceLock<Mutex<HashMap<u64, oneshot::Sender<AgentResponse>>>> = OnceLock::new();
static ALLOW_DESTRUCTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static NEXT_FALLBACK_ID: AtomicU64 = AtomicU64::new(1);

fn replies() -> &'static Mutex<HashMap<u64, oneshot::Sender<AgentResponse>>> {
    REPLIES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(unix)]
pub fn enabled() -> bool {
    std::env::var_os("SNACK_AGENT").is_some()
}

#[cfg(not(unix))]
pub fn enabled() -> bool {
    false
}

pub fn socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("SNACK_AGENT_SOCK") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("snack-agent.sock")
}

pub fn allow_destructive() -> bool {
    ALLOW_DESTRUCTIVE.load(Ordering::Relaxed)
        || std::env::var_os("SNACK_AGENT_ALLOW_DESTRUCTIVE").is_some()
}

pub fn set_allow_destructive(enabled: bool) {
    ALLOW_DESTRUCTIVE.store(enabled, Ordering::Relaxed);
}

#[cfg(unix)]
pub fn subscription() -> Subscription<Message> {
    Subscription::run(agent_stream)
}

#[cfg(not(unix))]
pub fn subscription() -> Subscription<Message> {
    Subscription::none()
}

#[cfg(unix)]
fn agent_stream() -> impl futures::Stream<Item = Message> {
    iced::stream::channel(64, |output| async move {
        if let Err(e) = serve(output).await {
            tracing::error!(error = %e, "agent control server stopped");
        }
    })
}

#[cfg(unix)]
async fn serve(output: iced_mpsc::Sender<Message>) -> Result<(), String> {
    let path = socket_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create sock dir: {e}"))?;
    }

    let listener =
        UnixListener::bind(&path).map_err(|e| format!("bind {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "agent control listening (SNACK_AGENT)");

    let marker = std::env::temp_dir().join("snack-agent.sock.path");
    let _ = std::fs::write(&marker, path.to_string_lossy().as_bytes());

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| format!("accept: {e}"))?;
        let output = output.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, output).await {
                tracing::debug!(error = %e, "agent client disconnected");
            }
        });
    }
}

#[cfg(unix)]
async fn handle_client(
    stream: UnixStream,
    mut output: iced_mpsc::Sender<Message>,
) -> Result<(), String> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await.map_err(|e| format!("read: {e}"))? {
        let line = line.trim().to_owned();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<AgentRequest>(&line) {
            Ok(req) => dispatch_request(req, &mut output).await,
            Err(e) => AgentResponse {
                id: 0,
                ok: false,
                data: None,
                error: Some(format!("invalid request json: {e}")),
            },
        };

        let payload =
            serde_json::to_string(&response).map_err(|e| format!("encode response: {e}"))?;
        writer
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("write: {e}"))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| format!("write: {e}"))?;
    }

    Ok(())
}

#[cfg(unix)]
async fn dispatch_request(
    req: AgentRequest,
    output: &mut iced_mpsc::Sender<Message>,
) -> AgentResponse {
    let id = if req.id == 0 {
        NEXT_FALLBACK_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        req.id
    };

    match &req.cmd {
        AgentCommand::Ping => {
            return AgentResponse::ok(
                id,
                json!({
                    "pong": true,
                    "socket": socket_path().display().to_string(),
                    "allow_destructive": allow_destructive(),
                }),
            );
        }
        AgentCommand::Help => {
            return AgentResponse::ok(id, help_data());
        }
        _ => {}
    }

    if req.cmd.is_destructive() && !allow_destructive() {
        return AgentResponse::err(
            id,
            "destructive command blocked; set SNACK_AGENT_ALLOW_DESTRUCTIVE=1 or send allow-destructive",
        );
    }

    let (tx, rx) = oneshot::channel();
    {
        let mut map = replies().lock().expect("agent replies poisoned");
        map.insert(id, tx);
    }

    if output
        .send(Message::AgentRequest {
            id,
            command: req.cmd,
        })
        .await
        .is_err()
    {
        let _ = take_reply(id);
        return AgentResponse::err(id, "app runtime not accepting agent commands");
    }

    match tokio::time::timeout(Duration::from_secs(30), rx).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => AgentResponse::err(id, "reply channel closed"),
        Err(_) => {
            let _ = take_reply(id);
            AgentResponse::err(id, "timed out waiting for app to handle command")
        }
    }
}

fn take_reply(id: u64) -> Option<oneshot::Sender<AgentResponse>> {
    replies()
        .lock()
        .expect("agent replies poisoned")
        .remove(&id)
}

pub fn complete(id: u64, response: AgentResponse) {
    if let Some(tx) = take_reply(id) {
        let _ = tx.send(response);
    }
}

pub fn handle(app: &mut App, id: u64, command: AgentCommand) -> iced::Task<Message> {
    match command {
        AgentCommand::Ping | AgentCommand::Help => {
            complete(id, AgentResponse::ok(id, json!({})));
            iced::Task::none()
        }
        AgentCommand::State => {
            complete(id, AgentResponse::ok(id, dump_state(app)));
            iced::Task::none()
        }
        AgentCommand::AllowDestructive { enabled } => {
            set_allow_destructive(enabled);
            complete(
                id,
                AgentResponse::ok(id, json!({ "allow_destructive": allow_destructive() })),
            );
            iced::Task::none()
        }
        AgentCommand::OpenPalette => {
            let task = if app.palette_open {
                iced::Task::none()
            } else {
                update(app, Message::PaletteToggled)
            };
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "palette_open": app.palette_open,
                        "entries": palette_entries_json(app),
                    }),
                ),
            );
            task
        }
        AgentCommand::ClosePalette => {
            let task = if app.palette.is_some() {
                let mut t = update(app, Message::PaletteClosed);
                t = t.chain(update(app, Message::PaletteDismissed));
                t
            } else {
                iced::Task::none()
            };
            complete(id, AgentResponse::ok(id, json!({ "palette_open": false })));
            task
        }
        AgentCommand::SetQuery { query } => {
            if app.palette.is_none() {
                let _ = update(app, Message::PaletteToggled);
            }
            let task = update(app, Message::PaletteQueryChanged(query));
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "palette_open": app.palette_open,
                        "query": app.palette.as_ref().map(|p| p.query.clone()).unwrap_or_default(),
                        "entries": palette_entries_json(app),
                        "selected": app.palette.as_ref().map(|p| p.selected).unwrap_or(0),
                    }),
                ),
            );
            task
        }
        AgentCommand::Type { text } => handle(app, id, AgentCommand::SetQuery { query: text }),
        AgentCommand::Move { delta } => {
            if app.palette.is_none() {
                complete(id, AgentResponse::err(id, "palette is not open"));
                return iced::Task::none();
            }
            let task = update(app, Message::PaletteMoved(delta));
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "selected": app.palette.as_ref().map(|p| p.selected).unwrap_or(0),
                        "entries": palette_entries_json(app),
                    }),
                ),
            );
            task
        }
        AgentCommand::Submit => {
            if app.palette.is_none() {
                complete(id, AgentResponse::err(id, "palette is not open"));
                return iced::Task::none();
            }
            let selected = app.palette.as_ref().map(|p| p.selected).unwrap_or(0);
            let label = app
                .palette
                .as_ref()
                .and_then(|p| p.entries.get(selected))
                .map(|e| e.label.clone());
            let task = update(app, Message::PaletteSubmitted);
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "submitted_index": selected,
                        "submitted_label": label,
                        "active_channel": app.active_channel,
                        "palette_open": app.palette_open,
                    }),
                ),
            );
            task
        }
        AgentCommand::SelectEntry { index } => {
            if app.palette.is_none() {
                complete(id, AgentResponse::err(id, "palette is not open"));
                return iced::Task::none();
            }
            let task = update(app, Message::PaletteEntryPressed(index));
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "index": index,
                        "active_channel": app.active_channel,
                        "palette_open": app.palette_open,
                    }),
                ),
            );
            task
        }
        AgentCommand::SelectChannel { channel } => match resolve_channel(app, &channel) {
            Some(id_channel) => {
                let task = update(app, Message::ChannelSelected(id_channel.clone()));
                complete(
                    id,
                    AgentResponse::ok(
                        id,
                        json!({
                            "active_channel": id_channel,
                            "active_team": app.active_team,
                        }),
                    ),
                );
                task
            }
            None => {
                complete(
                    id,
                    AgentResponse::err(id, format!("channel not found: {channel}")),
                );
                iced::Task::none()
            }
        },
        AgentCommand::SelectWorkspace { team } => {
            if app.workspaces.contains_key(&team) {
                let task = update(app, Message::WorkspaceSelected(team.clone()));
                complete(
                    id,
                    AgentResponse::ok(
                        id,
                        json!({
                            "active_team": app.active_team,
                            "active_channel": app.active_channel,
                        }),
                    ),
                );
                task
            } else {
                complete(
                    id,
                    AgentResponse::err(id, format!("workspace not found: {team}")),
                );
                iced::Task::none()
            }
        }
        AgentCommand::Search { query } => {
            app.search_input = query.clone();
            let task = update(app, Message::SearchSubmitted);
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "query": query,
                        "search_active": app.search.is_some(),
                        "loading": app.search.as_ref().map(|s| s.loading).unwrap_or(false),
                    }),
                ),
            );
            task
        }
        AgentCommand::ClearSearch => {
            let task = update(app, Message::SearchCleared);
            complete(id, AgentResponse::ok(id, json!({ "search_active": false })));
            task
        }
        AgentCommand::OpenSettings => {
            let task = update(app, Message::SettingsOpened);
            complete(id, AgentResponse::ok(id, json!({ "settings_open": true })));
            task
        }
        AgentCommand::CloseSettings => {
            let mut task = update(app, Message::SettingsClosed);
            task = task.chain(update(app, Message::SettingsDismissed));
            complete(id, AgentResponse::ok(id, json!({ "settings_open": false })));
            task
        }
        AgentCommand::Screenshot { path } => {
            let path = resolve_screenshot_path(path);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            window::latest().then(move |maybe_id| {
                let path = path.clone();
                match maybe_id {
                    Some(window_id) => {
                        let path = path.clone();
                        window::screenshot(window_id).map(move |screenshot| {
                            Message::AgentScreenshotCaptured {
                                id,
                                path: path.clone(),
                                result: Ok(screenshot),
                            }
                        })
                    }
                    None => iced::Task::done(Message::AgentScreenshotCaptured {
                        id,
                        path,
                        result: Err("no window available for screenshot".into()),
                    }),
                }
            })
        }
        AgentCommand::Send => {
            let task = update(app, Message::SendPressed);
            complete(id, AgentResponse::ok(id, json!({ "sent": true })));
            task
        }
        AgentCommand::Toast { text } => {
            app.toast(text.clone());
            complete(id, AgentResponse::ok(id, json!({ "toast": text })));
            iced::Task::none()
        }
        AgentCommand::MainView { view } => {
            let target = match view.trim().to_ascii_lowercase().as_str() {
                "activity" | "notifications" | "bell" => crate::state::MainView::Activity,
                "home" | "channels" => crate::state::MainView::Home,
                other => {
                    complete(
                        id,
                        AgentResponse::err(id, format!("unknown main view: {other}")),
                    );
                    return iced::Task::none();
                }
            };
            let task = update(app, Message::MainViewSelected(target));
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "main_view": main_view_label(app.main_view),
                        "activity_loading": app.activity.loading,
                        "activity_item_count": app.activity.items.len(),
                    }),
                ),
            );
            task
        }
        AgentCommand::ActivitySelect { index } => {
            let Some(key) = app.activity.items.get(index).map(|i| i.key.clone()) else {
                complete(id, AgentResponse::err(id, format!("no activity item at {index}")));
                return iced::Task::none();
            };
            let task = update(app, Message::ActivitySelected(key.clone()));
            complete(
                id,
                AgentResponse::ok(
                    id,
                    json!({
                        "key": key,
                        "active_channel": app.active_channel,
                        "active_thread": app.active_thread.as_ref().map(|(c, ts)| json!({"channel": c, "ts": ts})),
                        "thread_open": app.thread_open,
                    }),
                ),
            );
            task
        }
    }
}

fn main_view_label(view: crate::state::MainView) -> &'static str {
    match view {
        crate::state::MainView::Home => "home",
        crate::state::MainView::Activity => "activity",
    }
}

pub fn handle_screenshot(
    id: u64,
    path: PathBuf,
    result: Result<Screenshot, String>,
) -> iced::Task<Message> {
    match result {
        Ok(screenshot) => match write_png(&path, &screenshot) {
            Ok(()) => {
                complete(
                    id,
                    AgentResponse::ok(
                        id,
                        json!({
                            "path": path.display().to_string(),
                            "width": screenshot.size.width,
                            "height": screenshot.size.height,
                        }),
                    ),
                );
            }
            Err(e) => complete(id, AgentResponse::err(id, e)),
        },
        Err(e) => complete(id, AgentResponse::err(id, e)),
    }
    iced::Task::none()
}

fn write_png(path: &Path, screenshot: &Screenshot) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("create png: {e}"))?;
    let mut encoder = png::Encoder::new(file, screenshot.size.width, screenshot.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(screenshot.as_ref())
        .map_err(|e| format!("png data: {e}"))?;
    writer.finish().map_err(|e| format!("png finish: {e}"))?;
    Ok(())
}

fn resolve_screenshot_path(path: Option<String>) -> PathBuf {
    match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => {
            let dir = PathBuf::from("tmp/agent-ui");
            let _ = std::fs::create_dir_all(&dir);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            dir.join(format!("live-{stamp}.png"))
        }
    }
}

fn resolve_channel(app: &App, needle: &str) -> Option<String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let ws = app.active_workspace()?;
    if ws.channels.contains_key(needle) {
        return Some(needle.to_owned());
    }
    let lower = needle.trim_start_matches('#').to_ascii_lowercase();
    ws.channels
        .values()
        .find(|c| {
            c.name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case(&lower))
                .unwrap_or(false)
                || c.id.eq_ignore_ascii_case(needle)
        })
        .map(|c| c.id.clone())
}

pub fn dump_state(app: &App) -> Value {
    let screen = match app.screen {
        Screen::Login => "login",
        Screen::Loading => "loading",
        Screen::Main => "main",
    };

    let workspaces: Vec<Value> = app
        .workspaces
        .values()
        .map(|ws| {
            json!({
                "team_id": ws.team_id,
                "name": ws.name,
                "channel_count": ws.channels.len(),
                "rt_connected": ws.rt.is_connected(),
            })
        })
        .collect();

    let active_channel_name = app.active_workspace().and_then(|ws| {
        app.active_channel
            .as_ref()
            .and_then(|id| ws.channels.get(id))
            .and_then(|c| c.name.clone())
    });

    let recent_messages = app.active_workspace().and_then(|ws| {
        let channel = app.active_channel.as_ref()?;
        let cm = ws.messages.get(channel)?;
        let msgs: Vec<Value> = cm
            .messages
            .iter()
            .rev()
            .filter(|m| crate::state::is_channel_timeline_visible(m))
            .take(12)
            .map(|m| {
                json!({
                    "ts": m.ts,
                    "user": m.user,
                    "author": ws.message_author_name(m),
                    "text": crate::state::message_text(m),
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        Some(msgs)
    });

    let search = app.search.as_ref().map(|s| {
        json!({
            "query": s.query,
            "loading": s.loading,
            "page": s.page,
            "page_count": s.page_count,
            "total": s.total,
            "hit_count": s.hits.len(),
            "hits": s.hits.iter().take(20).map(|h| {
                json!({
                    "channel": h.channel,
                    "channel_label": h.channel_label,
                    "ts": h.message.ts,
                    "text": crate::state::message_text(&h.message),
                    "author": app.active_workspace()
                        .map(|ws| ws.message_author_name(&h.message))
                        .unwrap_or_default(),
                })
            }).collect::<Vec<_>>(),
        })
    });

    let activity = json!({
        "view": main_view_label(app.main_view),
        "loading": app.activity.loading,
        "loaded": app.activity.loaded,
        "selected": app.activity.selected,
        "item_count": app.activity.items.len(),
        "unread": app.activity.items.iter().filter(|i| i.is_unread).count(),
        "items": app.activity.items.iter().take(20).map(|i| json!({
            "key": i.key,
            "kind": i.item.kind,
            "channel": i.channel(),
            "ts": i.ts(),
            "thread_ts": i.thread_ts(),
            "identity": i.identity(),
            "author": i.author(),
            "is_unread": i.is_unread,
        })).collect::<Vec<_>>(),
    });

    json!({
        "screen": screen,
        "signed_in": app.session.is_some(),
        "main_view": main_view_label(app.main_view),
        "activity": activity,
        "active_team": app.active_team,
        "active_channel": app.active_channel,
        "active_channel_name": active_channel_name,
        "thread_open": app.thread_open,
        "active_thread": app.active_thread.as_ref().map(|(c, ts)| json!({"channel": c, "ts": ts})),
        "palette_open": app.palette_open,
        "palette": app.palette.as_ref().map(|p| {
            json!({
                "query": p.query,
                "selected": p.selected,
                "remote_seq": p.remote_seq,
                "entry_count": p.entries.len(),
                "entries": palette_entries_json(app),
            })
        }),
        "settings_open": app.settings_open || app.show_settings,
        "account_menu_open": app.account_menu_open || app.show_account_menu,
        "search_input": app.search_input,
        "search": search,
        "workspaces": workspaces,
        "recent_messages": recent_messages,
        "toasts": app.errors.iter().rev().take(8).map(|t| &t.text).collect::<Vec<_>>(),
        "allow_destructive": allow_destructive(),
        "agent_socket": socket_path().display().to_string(),
    })
}

fn palette_entries_json(app: &App) -> Vec<Value> {
    app.palette
        .as_ref()
        .map(|p| {
            p.entries
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let target = match &e.target {
                        PaletteTarget::Channel(id) => json!({"kind": "channel", "id": id}),
                        PaletteTarget::User { user, dm } => {
                            json!({"kind": "user", "user": user, "dm": dm})
                        }
                    };
                    json!({
                        "index": i,
                        "label": e.label,
                        "sublabel": e.sublabel,
                        "target": target,
                        "selected": i == p.selected,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn help_data() -> Value {
    json!({
        "commands": [
            {"cmd": "ping", "desc": "liveness check"},
            {"cmd": "help", "desc": "this help"},
            {"cmd": "state", "desc": "JSON snapshot of UI state"},
            {"cmd": "open-palette", "desc": "open quick switcher"},
            {"cmd": "close-palette", "desc": "close quick switcher"},
            {"cmd": "set-query", "args": {"query": "string"}, "desc": "set palette query"},
            {"cmd": "type", "args": {"text": "string"}, "desc": "alias for set-query"},
            {"cmd": "move", "args": {"delta": "isize"}, "desc": "move palette selection"},
            {"cmd": "submit", "desc": "activate selected palette entry"},
            {"cmd": "select-entry", "args": {"index": "usize"}, "desc": "activate palette entry by index"},
            {"cmd": "select-channel", "args": {"channel": "id or name"}, "desc": "open a channel"},
            {"cmd": "select-workspace", "args": {"team": "team id"}, "desc": "switch workspace"},
            {"cmd": "search", "args": {"query": "string"}, "desc": "run message search"},
            {"cmd": "clear-search", "desc": "close search"},
            {"cmd": "open-settings", "desc": "open settings"},
            {"cmd": "close-settings", "desc": "close settings"},
            {"cmd": "screenshot", "args": {"path": "optional"}, "desc": "capture window PNG"},
            {"cmd": "main-view", "args": {"view": "home|activity"}, "desc": "switch far-rail surface"},
            {"cmd": "activity-select", "args": {"index": "usize"}, "desc": "open an activity item in the right panel"},
            {"cmd": "toast", "args": {"text": "string"}, "desc": "show a toast"},
            {"cmd": "allow-destructive", "args": {"enabled": "bool"}, "desc": "allow send/etc"},
            {"cmd": "send", "desc": "send composer (requires allow-destructive)"},
        ],
        "notes": [
            "Start app with SNACK_AGENT=1 cargo run",
            "Socket default: $TMPDIR/snack-agent.sock (override SNACK_AGENT_SOCK)",
            "Use scripts/agentctl.sh for a CLI wrapper",
            "Live data uses your real Slack session",
        ]
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentRequest {
    #[serde(default)]
    pub id: u64,
    #[serde(flatten)]
    pub cmd: AgentCommand,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum AgentCommand {
    Ping,
    Help,
    State,
    OpenPalette,
    ClosePalette,
    SetQuery {
        query: String,
    },
    Type {
        text: String,
    },
    Move {
        #[serde(default = "default_move_delta")]
        delta: isize,
    },
    Submit,
    SelectEntry {
        index: usize,
    },
    SelectChannel {
        channel: String,
    },
    SelectWorkspace {
        team: String,
    },
    Search {
        query: String,
    },
    ClearSearch,
    OpenSettings,
    CloseSettings,
    Screenshot {
        #[serde(default)]
        path: Option<String>,
    },
    AllowDestructive {
        enabled: bool,
    },
    Send,
    Toast {
        text: String,
    },
    MainView {
        view: String,
    },
    ActivitySelect {
        index: usize,
    },
}

fn default_move_delta() -> isize {
    1
}

impl AgentCommand {
    fn is_destructive(&self) -> bool {
        matches!(self, AgentCommand::Send)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_core_commands() {
        let ping: AgentRequest = serde_json::from_str(r#"{"id":1,"cmd":"ping"}"#).unwrap();
        assert!(matches!(ping.cmd, AgentCommand::Ping));

        let query: AgentRequest =
            serde_json::from_str(r#"{"id":2,"cmd":"set-query","query":"dev"}"#).unwrap();
        match query.cmd {
            AgentCommand::SetQuery { query } => assert_eq!(query, "dev"),
            other => panic!("unexpected {other:?}"),
        }

        let mov: AgentRequest =
            serde_json::from_str(r#"{"id":3,"cmd":"move","delta":-1}"#).unwrap();
        match mov.cmd {
            AgentCommand::Move { delta } => assert_eq!(delta, -1),
            other => panic!("unexpected {other:?}"),
        }

        let shot: AgentRequest =
            serde_json::from_str(r#"{"cmd":"screenshot","path":"tmp/x.png"}"#).unwrap();
        match shot.cmd {
            AgentCommand::Screenshot { path } => {
                assert_eq!(path.as_deref(), Some("tmp/x.png"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn dump_state_from_login_fixture_shape() {
        let app = App::empty();
        let state = dump_state(&app);
        assert_eq!(state["screen"], "login");
        assert_eq!(state["signed_in"], false);
        assert!(state.get("agent_socket").is_some());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AgentResponse {
    fn ok(id: u64, data: Value) -> Self {
        Self {
            id,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    fn err(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            data: None,
            error: Some(error.into()),
        }
    }
}
