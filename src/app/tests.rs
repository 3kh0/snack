use std::collections::{BTreeMap, HashMap};

use serde_json::json;

use super::update::{
    emoji_preview_from_bytes, needs_user_hydration, notification_for_message, preferred_channel,
    unique_download_path, update,
};
use super::*;
use crate::slack::events::RtEvent;
use crate::slack::models::{
    Channel, HistoryPage, Message as SlackMessage, SearchItem, SearchMessagesPage,
    SearchPagination, SentMessage, User, UserProfile,
};
use crate::slack::realtime::Connection;
use crate::state::{ChannelMessages, Presence, RealtimeStatus};

const SELF_USER: &str = "U_SELF";

fn msg(user: &str, ts: &str, text: &str) -> SlackMessage {
    SlackMessage {
        user: Some(user.into()),
        ts: Some(ts.into()),
        text: Some(text.into()),
        ..Default::default()
    }
}

fn loaded_channel(user: &str, ts: &str, text: &str) -> ChannelMessages {
    let mut cm = ChannelMessages::default();
    cm.upsert(msg(user, ts, text));
    cm.loaded = true;
    cm
}

fn test_workspace() -> Workspace {
    let mut channels = BTreeMap::new();
    for (id, name) in [("C_GENERAL", "general"), ("C_DEV", "dev")] {
        channels.insert(
            id.into(),
            Channel {
                id: id.into(),
                name: Some(name.into()),
                is_channel: true,
                ..Default::default()
            },
        );
    }

    let messages = HashMap::from([
        (
            "C_GENERAL".into(),
            loaded_channel("U_ALICE", "1783372300.000100", "morning"),
        ),
        (
            "C_DEV".into(),
            loaded_channel("U_BOB", "1783370000.000100", "question"),
        ),
    ]);

    Workspace {
        team_id: "T_TEST".into(),
        name: "Test".into(),
        url: "https://test.slack.com".into(),
        self_user_id: SELF_USER.into(),
        channels,
        starred_order: Vec::new(),
        dm_order: Vec::new(),
        last_active_channel: None,
        priority_scores: BTreeMap::new(),
        hide_read_channels_unless_starred: false,
        priority_sidebar_section: false,
        users: HashMap::new(),
        custom_emoji: HashMap::new(),
        messages,
        typing: HashMap::new(),
        presence: HashMap::new(),
        rt: RealtimeStatus::default(),
        rt_generation: 1,
    }
}

fn test_app() -> App {
    let mut app = App::empty();
    let ws = test_workspace();
    let team = ws.team_id.clone();
    app.active_channel = ws.channels.keys().next().cloned();
    app.workspaces.insert(team.clone(), ws);
    app.active_team = Some(team);
    app.screen = Screen::Main;
    app
}

fn add_second_workspace(app: &mut App) {
    let mut ws = test_workspace();
    ws.team_id = "T_SECOND".into();
    ws.name = "Second".into();
    ws.url = "https://second.slack.com".into();
    ws.channels.clear();
    ws.channels.insert(
        "C_SECOND".into(),
        Channel {
            id: "C_SECOND".into(),
            name: Some("second".into()),
            is_channel: true,
            ..Default::default()
        },
    );
    ws.messages = HashMap::from([(
        "C_SECOND".into(),
        loaded_channel("U_SECOND", "1783375000.000100", "second workspace"),
    )]);
    app.workspaces.insert(ws.team_id.clone(), ws);
}

#[test]
fn account_menu_toggles_and_closes_for_settings() {
    let mut app = test_app();

    let _ = update(&mut app, Message::AccountMenuToggled);
    assert!(app.show_account_menu);

    let _ = update(&mut app, Message::SettingsOpened);
    assert!(!app.show_account_menu);
    assert!(app.show_settings);
}

#[test]
fn self_presence_selection_updates_active_workspace() {
    let mut app = test_app();
    app.show_account_menu = true;

    let _ = update(&mut app, Message::SelfPresenceSelected(Presence::Active));

    let ws = app.active_workspace().unwrap();
    assert_eq!(ws.presence.get(SELF_USER), Some(&Presence::Active));
    assert!(!app.show_account_menu);
}

#[test]
fn notification_created_for_inactive_channel_mention() {
    let app = test_app();
    let ws = app.active_workspace().unwrap();
    let message = SlackMessage {
        user: Some("U_ALICE".into()),
        text: Some("hi <@U_SELF>".into()),
        ..Default::default()
    };

    let notification = notification_for_message(ws, "C_DEV", &message, Some("C_GENERAL"))
        .expect("mention should notify");

    assert_eq!(notification.title, "U_ALICE in #dev");
    assert_eq!(notification.body, "hi <@U_SELF>");
}

#[test]
fn notification_suppresses_self_and_active_channel_messages() {
    let app = test_app();
    let ws = app.active_workspace().unwrap();
    let self_message = SlackMessage {
        user: Some(SELF_USER.into()),
        text: Some("self <@U_SELF>".into()),
        ..Default::default()
    };
    assert!(notification_for_message(ws, "C_DEV", &self_message, Some("C_GENERAL")).is_none());

    let active_message = SlackMessage {
        user: Some("U_ALICE".into()),
        text: Some("active <@U_SELF>".into()),
        ..Default::default()
    };
    assert!(
        notification_for_message(ws, "C_GENERAL", &active_message, Some("C_GENERAL")).is_none()
    );
}

#[test]
fn notification_detects_block_user_mentions() {
    let app = test_app();
    let ws = app.active_workspace().unwrap();
    let message = SlackMessage {
        user: Some("U_ALICE".into()),
        blocks: vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_section",
                "elements": [
                    {"type": "text", "text": "cc "},
                    {"type": "user", "user_id": "U_SELF"}
                ]
            }]
        })],
        ..Default::default()
    };

    let notification = notification_for_message(ws, "C_DEV", &message, Some("C_GENERAL"))
        .expect("block mention should notify");

    assert_eq!(notification.body, "cc @U_SELF");
}

#[test]
fn boot_selects_first_channel() {
    let app = test_app();
    assert_eq!(app.screen, Screen::Main);
    assert!(app.active_team.is_some());
    assert!(app.active_channel.is_some());
    assert!(!app.active_workspace().unwrap().channels.is_empty());
}

#[test]
fn channel_selection_preserves_loaded_messages() {
    let mut app = test_app();
    let _ = update(&mut app, Message::ChannelSelected("C_DEV".into()));
    assert_eq!(app.active_channel.as_deref(), Some("C_DEV"));
    let ws = app.active_workspace().unwrap();
    assert!(!ws.messages.get("C_GENERAL").unwrap().messages.is_empty());
    assert!(!ws.messages.get("C_DEV").unwrap().messages.is_empty());
}

#[test]
fn channel_selection_records_last_active_channel() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();

    let _ = update(&mut app, Message::ChannelSelected("C_DEV".into()));

    let ws = &app.workspaces[&team];
    assert_eq!(ws.last_active_channel.as_deref(), Some("C_DEV"));
    assert_eq!(preferred_channel(&app, &team).as_deref(), Some("C_DEV"));
}

#[test]
fn gif_emoji_preview_decodes_as_animation() {
    let mut bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut bytes, 1, 1, &[]).unwrap();
        encoder.set_repeat(gif::Repeat::Infinite).unwrap();
        let mut first = vec![255, 0, 0, 255];
        let mut frame = gif::Frame::from_rgba(1, 1, &mut first);
        frame.delay = 2;
        encoder.write_frame(&frame).unwrap();
        let mut second = vec![0, 255, 0, 255];
        let mut frame = gif::Frame::from_rgba(1, 1, &mut second);
        frame.delay = 3;
        encoder.write_frame(&frame).unwrap();
    }

    match emoji_preview_from_bytes(bytes) {
        FilePreview::Animated {
            frames,
            delays,
            total,
        } => {
            assert_eq!(frames.len(), 2);
            assert_eq!(
                delays,
                vec![
                    std::time::Duration::from_millis(20),
                    std::time::Duration::from_millis(30)
                ]
            );
            assert_eq!(total, std::time::Duration::from_millis(50));
        }
        other => panic!("expected animated preview, got {other:?}"),
    }
}

#[test]
fn known_user_without_avatar_still_gets_profile_hydration() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let ws = app.workspaces.get_mut(&team).unwrap();
    ws.users.insert(
        "U_ALICE".into(),
        User {
            id: "U_ALICE".into(),
            profile: Some(UserProfile {
                display_name: Some("Alice".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let ws = app.workspaces.get(&team).unwrap();
    assert!(needs_user_hydration(
        ws,
        &app.avatar_profile_hydrated,
        "U_ALICE"
    ));

    app.avatar_profile_hydrated.insert("U_ALICE".into());
    let ws = app.workspaces.get(&team).unwrap();
    assert!(!needs_user_hydration(
        ws,
        &app.avatar_profile_hydrated,
        "U_ALICE"
    ));
}

#[test]
fn known_user_with_avatar_skips_profile_hydration() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let ws = app.workspaces.get_mut(&team).unwrap();
    ws.users.insert(
        "U_ALICE".into(),
        User {
            id: "U_ALICE".into(),
            profile: Some(UserProfile {
                image_48: Some("https://example.test/alice.png".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let ws = app.workspaces.get(&team).unwrap();
    assert!(!needs_user_hydration(
        ws,
        &app.avatar_profile_hydrated,
        "U_ALICE"
    ));
}

#[test]
fn workspace_selection_switches_active_workspace_and_channel() {
    let mut app = test_app();
    add_second_workspace(&mut app);
    app.composer_text = "draft".into();
    app.thread_composer_text = "reply draft".into();
    app.active_thread = Some(("C_GENERAL".into(), "1783372300.000100".into()));

    let _ = update(&mut app, Message::WorkspaceSelected("T_SECOND".into()));

    assert_eq!(app.active_team.as_deref(), Some("T_SECOND"));
    assert_eq!(app.active_channel.as_deref(), Some("C_SECOND"));
    assert!(app.active_thread.is_none());
    assert!(app.composer_text.is_empty());
    assert!(app.thread_composer_text.is_empty());
}

#[test]
fn workspace_selection_remembers_previous_channel() {
    let mut app = test_app();
    add_second_workspace(&mut app);
    let _ = update(&mut app, Message::ChannelSelected("C_DEV".into()));
    let _ = update(&mut app, Message::WorkspaceSelected("T_SECOND".into()));
    let _ = update(&mut app, Message::WorkspaceSelected("T_TEST".into()));

    assert_eq!(app.active_team.as_deref(), Some("T_TEST"));
    assert_eq!(app.active_channel.as_deref(), Some("C_DEV"));
}

#[tokio::test]
async fn unique_download_path_avoids_overwrite() {
    let dir = std::env::temp_dir().join(format!("snack-test-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("report.pdf"), b"old")
        .await
        .unwrap();

    let path = unique_download_path(&dir, "report.pdf").await.unwrap();

    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("report-1.pdf")
    );
}

#[test]
fn thread_open_tracks_selected_root() {
    let mut app = test_app();
    let _ = update(
        &mut app,
        Message::ThreadOpened {
            channel: "C_GENERAL".into(),
            ts: "1783372300.000100".into(),
        },
    );

    assert_eq!(app.active_channel.as_deref(), Some("C_GENERAL"));
    assert_eq!(
        app.active_thread.as_ref(),
        Some(&("C_GENERAL".into(), "1783372300.000100".into()))
    );
}

#[test]
fn thread_loaded_stores_replies() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let root_ts = "1783372300.000100".to_owned();
    let _ = update(
        &mut app,
        Message::ThreadLoaded {
            team: team.clone(),
            channel: "C_GENERAL".into(),
            root_ts: root_ts.clone(),
            result: Ok(HistoryPage {
                messages: vec![
                    msg("U_ALICE", &root_ts, "morning"),
                    SlackMessage {
                        thread_ts: Some(root_ts.clone()),
                        ..msg("U_BOB", "1783372310.000100", "reply")
                    },
                ],
                ..Default::default()
            }),
        },
    );

    let cm = &app.threads[&(team, "C_GENERAL".into(), root_ts)];
    assert!(cm.loaded);
    assert_eq!(cm.messages.len(), 2);
}

#[test]
fn optimistic_thread_reply_inserts_pending_without_transport() {
    let mut app = test_app();
    let root_ts = "1783372300.000100".to_owned();
    app.active_thread = Some(("C_GENERAL".into(), root_ts.clone()));
    app.thread_composer_text = "thread answer".into();
    let _ = update(&mut app, Message::ThreadSendPressed);

    assert!(app.thread_composer_text.is_empty());
    let team = app.active_team.clone().unwrap();
    let cm = &app.threads[&(team, "C_GENERAL".into(), root_ts)];
    let reply = cm.messages.last().unwrap();
    assert_eq!(reply.text.as_deref(), Some("thread answer"));
    assert_eq!(reply.thread_ts.as_deref(), Some("1783372300.000100"));
    assert!(cm.is_pending(reply.ts.as_deref().unwrap()));
}

#[test]
fn realtime_thread_reply_updates_open_thread_not_channel() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let root_ts = "1783372300.000100".to_owned();
    app.active_thread = Some(("C_GENERAL".into(), root_ts.clone()));
    app.threads.insert(
        (team.clone(), "C_GENERAL".into(), root_ts.clone()),
        ChannelMessages::default(),
    );
    let before = app.workspaces[&team].messages["C_GENERAL"].messages.len();
    let ev = RtEvent::Message(SlackMessage {
        user: Some("U_BOB".into()),
        ts: Some("1783372310.000100".into()),
        channel: Some("C_GENERAL".into()),
        thread_ts: Some(root_ts.clone()),
        text: Some("reply".into()),
        ..Default::default()
    });
    let _ = update(&mut app, Message::Realtime(team.clone(), 1, ev));

    assert_eq!(
        app.workspaces[&team].messages["C_GENERAL"].messages.len(),
        before
    );
    assert_eq!(
        app.threads[&(team, "C_GENERAL".into(), root_ts)].messages[0]
            .text
            .as_deref(),
        Some("reply")
    );
}

#[test]
fn empty_send_is_noop() {
    let mut app = test_app();
    app.active_channel = Some("C_GENERAL".into());
    let before = app.active_workspace().unwrap().messages["C_GENERAL"]
        .messages
        .len();
    app.composer_text = "   ".into();
    let _ = update(&mut app, Message::SendPressed);
    let after = app.active_workspace().unwrap().messages["C_GENERAL"]
        .messages
        .len();
    assert_eq!(before, after);
}

#[test]
fn optimistic_send_inserts_pending_without_transport() {
    let mut app = test_app();
    app.active_channel = Some("C_GENERAL".into());
    app.composer_text = "hello world".into();
    let _ = update(&mut app, Message::SendPressed);

    assert!(app.composer_text.is_empty());
    let cm = &app.active_workspace().unwrap().messages["C_GENERAL"];
    let last = cm.messages.last().unwrap();
    assert_eq!(last.text.as_deref(), Some("hello world"));
    assert_eq!(last.user.as_deref(), Some(SELF_USER));
    let ts = last.ts.clone().unwrap();
    assert!(cm.is_pending(&ts));
}

#[test]
fn message_sent_clears_pending() {
    let mut app = test_app();
    app.active_channel = Some("C_GENERAL".into());
    app.composer_text = "confirm me".into();
    let _ = update(&mut app, Message::SendPressed);

    let cid = {
        let cm = &app.active_workspace().unwrap().messages["C_GENERAL"];
        cm.messages
            .iter()
            .find(|m| m.text.as_deref() == Some("confirm me"))
            .unwrap()
            .client_msg_id
            .clone()
            .unwrap()
    };

    let team = app.active_team.clone().unwrap();
    let _ = update(
        &mut app,
        Message::MessageSent {
            team,
            channel: "C_GENERAL".into(),
            client_msg_id: cid,
            result: Ok(SentMessage {
                channel: "C_GENERAL".into(),
                ts: "1783372400.111111".into(),
                message: SlackMessage {
                    user: Some(SELF_USER.into()),
                    text: Some("confirm me".into()),
                    ts: Some("1783372400.111111".into()),
                    ..Default::default()
                },
            }),
        },
    );

    let cm = &app.active_workspace().unwrap().messages["C_GENERAL"];
    let msg = cm
        .messages
        .iter()
        .find(|m| m.text.as_deref() == Some("confirm me"))
        .unwrap();
    assert!(!cm.is_pending(msg.ts.as_deref().unwrap()));
}

#[test]
fn realtime_message_upserts_into_channel() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let before = app.workspaces[&team].messages["C_GENERAL"].messages.len();
    let ev = RtEvent::Message(SlackMessage {
        user: Some("U_ALICE".into()),
        ts: Some("9999999999.000001".into()),
        channel: Some("C_GENERAL".into()),
        text: Some("live!".into()),
        ..Default::default()
    });
    let _ = update(&mut app, Message::Realtime(team.clone(), 1, ev));
    let after = app.workspaces[&team].messages["C_GENERAL"].messages.len();
    assert_eq!(after, before + 1);
}

#[test]
fn realtime_delete_removes_message() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let ev = RtEvent::MessageDeleted {
        channel: "C_GENERAL".into(),
        deleted_ts: "1783372300.000100".into(),
    };
    let _ = update(&mut app, Message::Realtime(team.clone(), 1, ev));
    let exists = app.workspaces[&team].messages["C_GENERAL"]
        .messages
        .iter()
        .any(|m| m.ts.as_deref() == Some("1783372300.000100"));
    assert!(!exists);
}

#[test]
fn rt_connected_stores_connection() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    let conn = Connection::from_sender(tx);
    let _ = update(&mut app, Message::RtConnected(team.clone(), 2, conn));
    assert!(app.workspaces[&team].rt.is_connected());
    assert_eq!(app.workspaces[&team].rt_generation, 2);
    let _ = update(&mut app, Message::RtDisconnected(team.clone(), 1));
    assert!(app.workspaces[&team].rt.is_connected());
    let _ = update(&mut app, Message::RtDisconnected(team.clone(), 2));
    assert!(!app.workspaces[&team].rt.is_connected());
}

#[test]
fn stale_realtime_event_is_ignored() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    app.workspaces.get_mut(&team).unwrap().rt_generation = 5;
    let before = app.workspaces[&team].messages["C_GENERAL"].messages.len();
    let ev = RtEvent::Message(SlackMessage {
        user: Some("U_ALICE".into()),
        ts: Some("9999999999.000002".into()),
        channel: Some("C_GENERAL".into()),
        text: Some("stale".into()),
        ..Default::default()
    });
    let _ = update(&mut app, Message::Realtime(team.clone(), 4, ev));
    let after = app.workspaces[&team].messages["C_GENERAL"].messages.len();
    assert_eq!(after, before);
}

#[test]
fn edit_pressed_populates_editor_with_current_text() {
    let mut app = test_app();
    let _ = update(
        &mut app,
        Message::EditPressed {
            channel: "C_GENERAL".into(),
            ts: "1783372300.000100".into(),
        },
    );
    assert_eq!(app.edit_text, "morning");
    assert_eq!(
        app.editing.as_ref(),
        Some(&("C_GENERAL".into(), "1783372300.000100".into()))
    );
}

#[test]
fn edit_submit_optimistically_updates_text_and_marks_edited() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    app.editing = Some(("C_GENERAL".into(), "1783372300.000100".into()));
    app.edit_text = "morning (updated)".into();

    let _ = update(&mut app, Message::EditSubmit);

    assert!(app.editing.is_none());
    assert!(app.edit_text.is_empty());
    let cm = &app.workspaces[&team].messages["C_GENERAL"];
    let msg = cm
        .messages
        .iter()
        .find(|m| m.ts.as_deref() == Some("1783372300.000100"))
        .unwrap();
    assert_eq!(msg.text.as_deref(), Some("morning (updated)"));
    assert!(msg.edited.is_some());
}

#[test]
fn empty_edit_submit_keeps_editor_open() {
    let mut app = test_app();
    app.editing = Some(("C_GENERAL".into(), "1783372300.000100".into()));
    app.edit_text = "   ".into();

    let _ = update(&mut app, Message::EditSubmit);

    assert!(app.editing.is_some());
}

#[test]
fn edit_applies_to_open_thread_copy() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let root_ts = "1783372300.000100".to_owned();
    let mut cm = ChannelMessages::default();
    cm.upsert(msg("U_BOB", "1783372310.000100", "reply"));
    app.threads
        .insert((team.clone(), "C_GENERAL".into(), root_ts.clone()), cm);

    app.editing = Some(("C_GENERAL".into(), "1783372310.000100".into()));
    app.edit_text = "reply (fixed)".into();
    let _ = update(&mut app, Message::EditSubmit);

    let cm = &app.threads[&(team, "C_GENERAL".into(), root_ts)];
    assert_eq!(cm.messages[0].text.as_deref(), Some("reply (fixed)"));
    assert!(cm.messages[0].edited.is_some());
}

#[test]
fn message_deleted_ok_removes_from_channel_and_threads() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    let root_ts = "1783372300.000100".to_owned();
    let mut cm = ChannelMessages::default();
    cm.upsert(msg("U_ALICE", &root_ts, "morning"));
    app.threads
        .insert((team.clone(), "C_GENERAL".into(), root_ts.clone()), cm);

    let _ = update(
        &mut app,
        Message::MessageDeleted {
            team: team.clone(),
            channel: "C_GENERAL".into(),
            ts: root_ts.clone(),
            result: Ok(()),
        },
    );

    assert!(
        !app.workspaces[&team].messages["C_GENERAL"]
            .messages
            .iter()
            .any(|m| m.ts.as_deref() == Some(root_ts.as_str()))
    );
    assert!(
        app.threads[&(team, "C_GENERAL".into(), root_ts)]
            .messages
            .is_empty()
    );
}

#[test]
fn selecting_other_channel_cancels_edit() {
    let mut app = test_app();
    app.editing = Some(("C_GENERAL".into(), "1783372300.000100".into()));
    app.edit_text = "in progress".into();
    let _ = update(&mut app, Message::ChannelSelected("C_DEV".into()));
    assert!(app.editing.is_none());
    assert!(app.edit_text.is_empty());
}

fn search_page(page: u32, page_count: u32, total: u64) -> SearchMessagesPage {
    SearchMessagesPage {
        items: vec![SearchItem {
            channel: Some(Channel {
                id: "C_GENERAL".into(),
                name: Some("general".into()),
                is_channel: true,
                ..Default::default()
            }),
            messages: vec![SlackMessage {
                thread_ts: Some("1783372200.000000".into()),
                ..msg("U_ALICE", "1783372300.000100", "morning standup")
            }],
            ..Default::default()
        }],
        pagination: Some(SearchPagination {
            page: Some(page),
            page_count: Some(page_count),
            total_count: Some(total),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn searching(app: &App) -> SearchState {
    SearchState {
        query: "standup".into(),
        team: app.active_team.clone().unwrap(),
        page: 1,
        page_count: 0,
        total: 0,
        hits: Vec::new(),
        loading: true,
    }
}

#[test]
fn search_submitted_starts_loading_state() {
    let mut app = test_app();
    let _ = update(&mut app, Message::SearchInputChanged("  standup ".into()));
    let _ = update(&mut app, Message::SearchSubmitted);
    let state = app.search.as_ref().expect("search active");
    assert_eq!(state.query, "standup");
    assert_eq!(state.page, 1);
    assert!(state.loading);
}

#[test]
fn empty_search_is_noop() {
    let mut app = test_app();
    app.search_input = "   ".into();
    let _ = update(&mut app, Message::SearchSubmitted);
    assert!(app.search.is_none());
}

#[test]
fn search_loaded_populates_hits_and_pagination() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    app.search = Some(searching(&app));

    let _ = update(
        &mut app,
        Message::SearchLoaded {
            team: team.clone(),
            query: "standup".into(),
            page: 1,
            result: Ok(search_page(1, 3, 42)),
        },
    );

    let state = app.search.as_ref().unwrap();
    assert!(!state.loading);
    assert_eq!(state.hits.len(), 1);
    assert_eq!(state.hits[0].channel, "C_GENERAL");
    assert_eq!(state.hits[0].channel_label, "#general");
    assert_eq!(state.page_count, 3);
    assert_eq!(state.total, 42);
}

#[test]
fn search_loaded_ignores_stale_query() {
    let mut app = test_app();
    let team = app.active_team.clone().unwrap();
    app.search = Some(searching(&app));

    let _ = update(
        &mut app,
        Message::SearchLoaded {
            team,
            query: "different".into(),
            page: 1,
            result: Ok(search_page(1, 3, 42)),
        },
    );

    let state = app.search.as_ref().unwrap();
    assert!(state.hits.is_empty());
    assert!(state.loading);
}

#[test]
fn search_page_request_out_of_bounds_is_noop() {
    let mut app = test_app();
    let mut state = searching(&app);
    state.page = 1;
    state.page_count = 3;
    state.loading = false;
    app.search = Some(state);

    let _ = update(&mut app, Message::SearchPageRequested(9));

    let state = app.search.as_ref().unwrap();
    assert_eq!(state.page, 1);
    assert!(!state.loading);
}

#[test]
fn search_result_opens_channel_and_thread_and_clears_search() {
    let mut app = test_app();
    app.active_channel = Some("C_DEV".into());
    app.search = Some(searching(&app));

    let _ = update(
        &mut app,
        Message::SearchResultSelected {
            channel: "C_GENERAL".into(),
            ts: "1783372300.000100".into(),
            thread_ts: Some("1783372200.000000".into()),
        },
    );

    assert!(app.search.is_none());
    assert_eq!(app.active_channel.as_deref(), Some("C_GENERAL"));
    assert_eq!(
        app.active_thread.as_ref(),
        Some(&("C_GENERAL".into(), "1783372200.000000".into()))
    );
}
