//! Headless UI captures for agents.
//!
//! Run via `scripts/agent-ui-check.sh` or:
//! `ICED_TEST_BACKEND=tiny-skia cargo test --locked ui_visual -- --nocapture`
//!
//! PNGs land under `SNACK_UI_CAPTURE_DIR` (default `tmp/agent-ui/`). Agents should
//! open those images after a UI change instead of waiting on a human to
//! `cargo run` and screenshot.

use std::fs;
use std::path::{Path, PathBuf};

use iced::{Event, Settings, Size, mouse, time, window};
use iced_test::{Error, Simulator};

use super::tests::{
    account_menu_app, activity_app, dms_app, login_app, multi_paragraph_emoji_app, profile_app,
    search_app, settings_app, test_app, thread_unread_app,
};
use super::update::update;
use super::view::view;
use super::{App, Message};
use crate::ui;

const VIEWPORT: Size = Size::new(1280.0, 800.0);

fn capture_dir() -> PathBuf {
    let dir = std::env::var("SNACK_UI_CAPTURE_DIR").unwrap_or_else(|_| "tmp/agent-ui".into());
    let path = PathBuf::from(dir);
    fs::create_dir_all(&path).expect("create capture dir");
    path
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), VIEWPORT, view(app))
}

fn capture(app: &App, name: &str) -> Result<PathBuf, Error> {
    let mut ui = sim(app);
    let now = time::Instant::now();
    let _ = ui.simulate([Event::Window(window::Event::RedrawRequested(now))]);
    std::thread::sleep(std::time::Duration::from_millis(180));
    let theme = ui::theme::midnight();
    let snapshot = ui.snapshot(&theme)?;

    let path = capture_dir().join(name);
    let stem = path.with_extension("");
    remove_captures(&stem);
    assert!(
        snapshot.matches_image(&stem)?,
        "failed to write capture for {name}"
    );

    let written = find_capture(&stem).unwrap_or_else(|| stem.with_extension("png"));
    eprintln!("ui_visual: wrote {}", written.display());
    Ok(written)
}

fn remove_captures(stem: &Path) {
    let Some(parent) = stem.parent() else {
        return;
    };
    let Some(base) = stem.file_name().map(|name| name.to_string_lossy()) else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let matches = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .is_some_and(|name| name.starts_with(base.as_ref()) && name.ends_with(".png"));
        if matches {
            let _ = fs::remove_file(path);
        }
    }
}

fn find_capture(stem: &Path) -> Option<PathBuf> {
    let parent = stem.parent()?;
    let base = stem.file_name()?.to_string_lossy();
    let entries = fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy();
        if name.starts_with(base.as_ref()) && name.ends_with(".png") {
            return Some(path);
        }
    }
    None
}

fn drain_messages(ui: Simulator<'_, Message>) -> Vec<Message> {
    ui.into_messages().collect()
}

fn apply_messages(app: &mut App, messages: impl IntoIterator<Item = Message>) {
    for message in messages {
        let _ = update(app, message);
    }
}

#[test]
fn ui_visual_login_screen_renders() -> Result<(), Error> {
    let app = login_app();
    let mut ui = sim(&app);
    ui.find("Sign in")?;
    ui.find("Sign in to your Slack workspace.")?;
    drop(ui);
    capture(&app, "login")?;
    Ok(())
}

#[test]
fn ui_visual_main_channel_renders() -> Result<(), Error> {
    let app = test_app();
    let mut ui = sim(&app);
    ui.find("general")?;
    ui.find("Alice")?;
    ui.find("Bob")?;
    ui.find("You")?;
    drop(ui);
    capture(&app, "main-general")?;
    Ok(())
}

#[test]
fn ui_visual_profile_hover_card_renders() -> Result<(), Error> {
    let mut app = profile_app();
    app.profile_hover = Some(super::ProfileHoverState {
        user: "U_ALICE".into(),
        key: "message-name:false:1783372300.000100".into(),
        generation: 1,
        visible: true,
        source_hovered: true,
        card_hovered: false,
        position: Some(iced::Point::new(360.0, 96.0)),
    });
    let mut ui = sim(&app);
    ui.find("Product designer")?;
    ui.find("Growing good interfaces")?;
    ui.find("Message")?;
    drop(ui);
    capture(&app, "profile-hover-card")?;
    Ok(())
}

#[test]
fn ui_visual_message_author_emits_profile_hover() -> Result<(), Error> {
    let app = test_app();
    let mut ui = sim(&app);
    let alice = ui.find("Alice")?;
    let center = alice
        .visible_bounds()
        .expect("Alice author should be visible")
        .center();
    ui.point_at(center);
    let _ = ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position: center })]);
    let messages = drain_messages(ui);
    assert!(messages.iter().any(|message| matches!(
        message,
        Message::ProfileHoverEntered { user, .. } if user == "U_ALICE"
    )));
    Ok(())
}

#[test]
fn ui_visual_profile_pane_renders() -> Result<(), Error> {
    let mut app = profile_app();
    app.profile_open = true;
    app.profile_pane = Some(super::ProfilePaneState {
        user: "U_ALICE".into(),
        loading: false,
        error: None,
    });
    let mut ui = sim(&app);
    ui.find("Profile")?;
    ui.find("Contact information")?;
    ui.find("Recent DMs")?;
    ui.find("See all conversations with Alice")?;
    ui.find("About me")?;
    ui.find("Manager")?;
    drop(ui);
    capture(&app, "profile-pane")?;
    Ok(())
}

#[test]
fn ui_visual_dm_header_is_compact_and_centered() -> Result<(), Error> {
    let mut app = profile_app();
    app.active_channel = Some("D_ALICE".into());
    let mut ui = sim(&app);
    ui.find("Alice")?;
    drop(ui);
    capture(&app, "dm-header-compact")?;
    Ok(())
}

#[test]
fn ui_visual_channel_thread_profile_headers_align() -> Result<(), Error> {
    let mut app = profile_app();
    apply_messages(
        &mut app,
        [Message::ThreadOpened {
            channel: "C_GENERAL".into(),
            ts: "1783372300.000100".into(),
            unread_range: None,
        }],
    );
    app.profile_open = true;
    app.profile_pane = Some(super::ProfilePaneState {
        user: "U_ALICE".into(),
        loading: false,
        error: None,
    });
    let mut ui = sim(&app);
    ui.find("general")?;
    ui.find("Thread")?;
    ui.find("Profile")?;
    drop(ui);
    capture(&app, "channel-thread-profile-headers")?;
    Ok(())
}

#[test]
fn ui_visual_channel_huddle_indicator() -> Result<(), Error> {
    let mut app = test_app();
    let team = app.active_team.clone().expect("active team");
    let ws = app.workspaces.get_mut(&team).expect("workspace");
    ws.apply_room(crate::slack::models::Room {
        id: "R_TEST".into(),
        channels: vec!["C_GENERAL".into()],
        participants: vec!["U_ALICE".into(), "U_BOB".into()],
        huddle_link: Some("https://app.slack.com/huddle/T_TEST/C_GENERAL".into()),
        ..Default::default()
    });

    let mut ui = sim(&app);
    ui.find("general")?;
    ui.find("Huddle · 2")?;
    drop(ui);
    capture(&app, "channel-huddle")?;
    Ok(())
}

#[test]
fn ui_visual_chat_paused_pill() -> Result<(), Error> {
    let mut app = test_app();
    apply_messages(
        &mut app,
        [Message::ChannelScrolled {
            channel: "C_GENERAL".into(),
            y: 100.0,
            bottom_gap: 300.0,
        }],
    );
    apply_messages(
        &mut app,
        [Message::Realtime(
            "T_TEST".into(),
            1,
            crate::slack::events::RtEvent::Message(crate::slack::models::Message {
                user: Some("U_ALICE".into()),
                ts: Some("9999999999.000001".into()),
                channel: Some("C_GENERAL".into()),
                text: Some("posted while you were reading".into()),
                ..Default::default()
            }),
        )],
    );

    let mut ui = sim(&app);
    ui.find("Chat paused · 1 new message")?;
    drop(ui);
    capture(&app, "chat-paused-pill")?;
    Ok(())
}

#[test]
fn ui_visual_thread_unread_divider() -> Result<(), Error> {
    let app = thread_unread_app();
    let mut ui = sim(&app);
    ui.find("Thread")?;
    ui.find("NEW")?;
    drop(ui);
    capture(&app, "thread-unread-divider")?;
    Ok(())
}

#[test]
fn ui_visual_switch_channel_by_text() -> Result<(), Error> {
    let mut app = test_app();
    assert_eq!(app.active_channel.as_deref(), Some("C_GENERAL"));

    let messages = {
        let mut ui = sim(&app);
        ui.find("dev")?;
        ui.click("dev")?;
        drain_messages(ui)
    };
    apply_messages(&mut app, messages);

    assert_eq!(
        app.active_channel.as_deref(),
        Some("C_DEV"),
        "clicking sidebar label should select #dev"
    );

    let mut ui = sim(&app);
    ui.find("Bob")?;
    ui.find("Alice")?;
    drop(ui);
    capture(&app, "main-dev")?;
    Ok(())
}

#[test]
fn ui_visual_settings_modal_renders() -> Result<(), Error> {
    let app = settings_app();
    let mut ui = sim(&app);
    ui.find("Settings")?;
    ui.find("Accent")?;
    ui.find("Done")?;
    capture(&app, "settings")?;
    Ok(())
}

#[test]
fn ui_visual_search_results_render() -> Result<(), Error> {
    let app = search_app();
    let mut ui = sim(&app);
    ui.find("Search")?;
    ui.find("#general")?;
    ui.find("morning standup notes")?;
    capture(&app, "search")?;
    Ok(())
}

#[test]
fn ui_visual_activity_unread_filter() -> Result<(), Error> {
    let mut app = activity_app();
    let messages = {
        let mut ui = sim(&app);
        ui.find("Activity")?;
        ui.find("Unread design review")?;
        ui.find("Read launch recap")?;
        ui.click("Unread")?;
        drain_messages(ui)
    };
    apply_messages(&mut app, messages);

    assert!(app.activity.unread_only);
    let mut ui = sim(&app);
    ui.find("Unread design review")?;
    assert!(
        ui.find("Read launch recap").is_err(),
        "read activity should be hidden by the unread filter"
    );
    drop(ui);
    capture(&app, "activity-unread")?;
    Ok(())
}

#[test]
fn ui_visual_activity_channel_post() -> Result<(), Error> {
    let app = activity_app();
    let mut ui = sim(&app);
    ui.find("Post in")?;
    ui.find("dev")?;
    ui.find("Subscribed channel post")?;
    drop(ui);
    capture(&app, "activity-channel-post")?;
    Ok(())
}

#[test]
fn ui_visual_dms_list_renders() -> Result<(), Error> {
    let app = dms_app();
    let mut ui = sim(&app);
    ui.find("Direct messages")?;
    ui.find("Alice")?;
    ui.find("unread dm from alice")?;
    ui.find("Bob")?;
    ui.find("You: read reply to bob")?;
    drop(ui);
    capture(&app, "dms")?;
    Ok(())
}

#[test]
fn ui_visual_dms_unread_and_name_filters() -> Result<(), Error> {
    let mut app = dms_app();
    apply_messages(&mut app, [Message::DmsUnreadOnlyToggled(true)]);
    assert!(app.dms.unread_only);
    let mut ui = sim(&app);
    ui.find("Alice")?;
    assert!(
        ui.find("You: read reply to bob").is_err(),
        "read DM should be hidden by the unread filter"
    );
    drop(ui);

    apply_messages(
        &mut app,
        [
            Message::DmsUnreadOnlyToggled(false),
            Message::DmsFilterChanged("bob".into()),
        ],
    );
    let mut ui = sim(&app);
    ui.find("Bob")?;
    assert!(
        ui.find("unread dm from alice").is_err(),
        "filter should hide non-matching DMs"
    );
    Ok(())
}

#[test]
fn ui_visual_dm_unread_clears_after_realtime_mark() -> Result<(), Error> {
    let mut app = dms_app();
    apply_messages(
        &mut app,
        [
            Message::Realtime(
                "T_TEST".into(),
                1,
                crate::slack::events::RtEvent::ChannelMarked {
                    channel: "D_ALICE".into(),
                    ts: "1783372300.000100".into(),
                    unread_count: Some(0),
                    mention_count: Some(0),
                },
            ),
            Message::DmsUnreadOnlyToggled(true),
        ],
    );

    let ws = app.workspaces.get("T_TEST").unwrap();
    let channel = ws.channels.get("D_ALICE").unwrap();
    assert_eq!(channel.last_read.as_deref(), Some("1783372300.000100"));
    assert_eq!(ws.unread_total(channel), 0);

    let mut ui = sim(&app);
    assert!(
        ui.find("unread dm from alice").is_err(),
        "DM read on another client should leave the unread filter"
    );
    Ok(())
}

#[test]
fn ui_visual_dm_history_loading_and_failure_states() -> Result<(), Error> {
    let mut app = dms_app();
    app.active_channel = Some("D_ALICE".into());

    let mut ui = sim(&app);
    ui.find("Loading messages…")?;
    drop(ui);

    apply_messages(
        &mut app,
        [Message::HistoryLoaded(
            "T_TEST".into(),
            "D_ALICE".into(),
            crate::app::HistoryLoadKind::Latest,
            Err(crate::slack::Error::Transport("offline".into())),
        )],
    );
    let mut ui = sim(&app);
    ui.find("Couldn't load messages.")?;
    ui.find("Retry")?;
    drop(ui);
    capture(&app, "dm-history-failed")?;
    Ok(())
}

#[test]
fn ui_visual_optional_snapshot_hash() -> Result<(), Error> {
    if std::env::var_os("SNACK_UI_SNAPSHOT").is_none() {
        return Ok(());
    }

    let app = test_app();
    let mut ui = sim(&app);
    let theme = ui::theme::midnight();
    let snapshot = ui.snapshot(&theme)?;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("snapshots/ui");
    fs::create_dir_all(&dir).expect("create snapshot dir");
    assert!(
        snapshot.matches_hash(dir.join("main-general"))?,
        "main-general snapshot hash mismatch — re-run with a clean snapshots/ui if intentional"
    );
    Ok(())
}
