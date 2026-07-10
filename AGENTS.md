# AGENTS.md

Guidance for agents working in this repository.

## Project Shape

Snack is a Rust desktop Slack client built with Iced.

Important boundaries:

- `src/app.rs` is the app facade and shared state/message type surface.
- `src/app/update.rs` owns reducer-style update logic and async effects.
- `src/app/view.rs` owns top-level rendering.
- `src/app/subscription.rs` owns Iced subscriptions and periodic ticks.
- `src/app/agent.rs` is the optional live control plane (`SNACK_AGENT=1`).
- `src/app/ui_visual.rs` is headless UI capture tests for agents.
- `src/ui/` contains reusable Slack UI widgets and styling (message bodies:
  `message.rs`, `blocks.rs`, `selectable.rs`).
- `src/slack/` contains Slack API, realtime, transport, models, and events.
- `src/config.rs` is the session/settings persistence boundary.
- `src/cache.rs` is the local SQLite/cache persistence boundary.

Keep changes inside the smallest boundary that matches the task. Do not fold
feature logic back into `src/app.rs` unless it is truly shared app surface.

## Iced Documentation Rule

Do not guess Iced APIs from memory. Iced changes quickly, and this project uses
a pinned git dependency rather than a plain crates.io release.

For any question, explanation, or code change involving Iced application setup,
widgets, layout, styling, tasks, subscriptions, themes, images, SVGs, canvas,
or runtime behavior, check the current Iced docs first:

https://docs.rs/iced/latest/iced/

If documentation access is unavailable, say that explicitly and validate the
assumption with the compiler. Prefer small compile-backed changes over broad
rewrites based on remembered Iced examples.

When docs.rs `latest` disagrees with this repo's pinned git revision in
`Cargo.toml`, the repository wins. Use docs to orient yourself, then confirm
against `cargo check --locked` or focused compiler feedback.

## Development Commands

Use locked Cargo commands by default:

```sh
cargo fmt --check
cargo check --locked
cargo test --locked
```

For most Rust changes, run `cargo fmt --check` and `cargo test --locked` before
calling the work done. Use `cargo check --locked` for faster iteration while
editing, especially around Iced API changes.

If a build fails with stale dependency artifacts under `target/debug/deps`, a
clean rebuild has fixed that class of local issue before:

```sh
cargo clean
cargo build --locked
```

Do not treat local environment noise, such as shell startup warnings, as the
root cause of Rust or app failures without evidence.

## Persistence And Secrets

Be careful around `src/config.rs`.

- The app stores Slack session secrets through the configured secret backend.
- Tests should not touch the real macOS Keychain or platform keyring.
- Keep test-only secret isolation behind `cfg(test)`.
- When changing session format, preserve migration behavior and add round-trip
  tests for both current and legacy shapes.

The app should not introduce repeated keychain prompts on boot or during tests.

## Performance Expectations

This is intended to feel fast in dev and release builds.

- Do not add synchronous disk or network work to the Iced update/view path.
- Prefer async tasks or background work for cache writes and Slack calls.
- Keep rendered message lists bounded or lazily computed where possible.
- Be careful with periodic subscriptions; avoid always-on ticks unless needed.
- Preserve `[profile.dev]` settings unless there is a measured reason to change
  them.

## UI Expectations

Snack should feel like a focused desktop Slack client, not a marketing page.

- Keep the UI quiet, dense, and readable.
- Use the existing `src/ui/theme.rs` constants and helper styles.
- Prefer existing UI modules over one-off widget styling.
- Keep controls stable in size; avoid layout shifts on hover, loading, or text
  changes.
- Do not add decorative chrome that competes with channels, messages, threads,
  and search.

## Slack Behavior

Slack-facing behavior needs defensive handling.

- Respect rate limits and `Retry-After` behavior.
- Preserve realtime generation guards and stale-event protection.
- Keep warm-boot/cache paths working when network calls fail.
- Do not assume all Slack messages are plain text; Block Kit, files, reactions,
  threads, edits, deletes, and notifications already exist in the product
  surface.

## Testing Guidance

Add focused tests when changing:

- session/config persistence,
- cache serialization or warm boot behavior,
- Slack API pagination/rate-limit handling,
- realtime event handling,
- message/thread/reaction/file/search state transitions,
- UI logic that can be tested through pure helpers.

Prefer small regression tests that encode the bug or behavior contract. Avoid
large fixture churn unless the task specifically requires it.

## Agent UI Verification

Agents should **not** wait on a human to `cargo run`, click around, and paste
screenshots for ordinary UI work. Use offline fixtures and/or the live control
plane below, then **read the PNGs yourself** (image-read tool) before claiming
layout is correct.

| Mode | When | Entry point |
| --- | --- | --- |
| Offline fixtures | Chrome, layout, message rendering, modals — no real Slack data needed | `scripts/agent-ui-check.sh` |
| Live control plane | Real channels/messages, palette ranking, search, warm cache, realtime | `SNACK_AGENT=1` + `scripts/agentctl.sh` |

Still run `cargo fmt --check` and `cargo test --locked` (or a focused subset)
for logic. Captures are not a substitute for unit tests.

### Offline fixture captures (no Slack session)

```sh
scripts/agent-ui-check.sh
```

What it does:

- Runs `ui_visual` tests with `iced_test::Simulator` (no window, no Slack network).
- Seeds offline fixture state from `src/app/tests.rs` helpers:
  - `test_app`, `login_app`, `settings_app`, `search_app`
  - `multi_paragraph_emoji_app` — multi-paragraph rich_text + custom emoji
    (reproduces the `#ship` “Hack Piano” layout class of bugs)
- Writes PNGs under `tmp/agent-ui/` (override with `SNACK_UI_CAPTURE_DIR`).
- Uses `ICED_TEST_BACKEND=tiny-skia` by default for a stable software renderer.
- Capture tests live in `src/app/ui_visual.rs`.

After the script finishes, **read the PNGs** and verify layout, copy, and chrome.

Rules:

- Fixtures only — do not put tokens or real session secrets in tests.
- Do not claim visual verification without running this harness (or having live
  screenshots you inspected).
- When you add a new screen, modal, or message-layout path, add a `ui_visual_*`
  capture test in `src/app/ui_visual.rs` (and a fixture helper if needed).
- Optional pixel regression:
  `SNACK_UI_SNAPSHOT=1 cargo test --locked ui_visual_optional`
  writes/checks `snapshots/ui/*.sha256` (machine/font sensitive — opt-in only).

### Live control plane (real session + drive the UI)

For features that need real data (quick switcher ranking, search hits, warm
cache, live message layout), run Snack with the agent socket and drive it via
`scripts/agentctl.sh`.

Implementation: `src/app/agent.rs` (Unix socket NDJSON → injects normal app
`Message`s). Wired only when `SNACK_AGENT` is set.

#### Boot

```sh
# Prefer a built binary once code is compiled (faster restarts).
cargo build --locked

# Clear a stale socket if a previous agent run died hard.
rm -f "${TMPDIR:-/tmp}/snack-agent.sock" "${TMPDIR:-/tmp}/snack-agent.sock.path"

# Uses the normal Snack session / Keychain (macOS).
SNACK_AGENT=1 ./target/debug/snack
# equivalent: SNACK_AGENT=1 cargo run --locked
```

Socket path: `SNACK_AGENT_SOCK`, else `$TMPDIR/snack-agent.sock` (also written to
`$TMPDIR/snack-agent.sock.path` for discovery). If `agentctl` gets
`Connection refused`, remove the stale sock and restart with `SNACK_AGENT=1`.

#### Drive the UI

```sh
scripts/agentctl.sh ping
scripts/agentctl.sh wait signed_in=true
scripts/agentctl.sh state                    # JSON snapshot
scripts/agentctl.sh open-palette
scripts/agentctl.sh set-query ship
scripts/agentctl.sh wait 'entries>=1'
scripts/agentctl.sh submit
# Channel switch is async — poll until active channel matches.
scripts/agentctl.sh wait channel=ship
scripts/agentctl.sh screenshot tmp/agent-ui/live-ship.png
```

Useful commands (full list: `scripts/agentctl.sh help` or `agentctl help`):

| Command | Notes |
| --- | --- |
| `state` | Screen, active channel, palette entries, recent messages, search, toasts |
| `open-palette` / `set-query` / `move` / `submit` | Quick switcher; prefer this over `select-channel` when name resolution is ambiguous |
| `select-channel <id\|name>` | Direct open; fails if the name is not found in the loaded workspace map |
| `search` / `clear-search` | Message search overlay |
| `screenshot [path]` | Live window PNG for multimodal inspection |
| `wait <predicate>` | Poll `state` until match (`channel=…`, `signed_in=true`, `entries>=N`, …) |
| `allow-destructive` / `send` | Composer send; blocked unless destructive mode is enabled |

#### Live workflow tips

- After `submit` / channel open, **wait or poll `state`** — `active_channel` can lag
  the submit response by a frame or network history load.
- Prefer `state` for structural checks; use `screenshot` when layout/typography
  matters, then **read the PNG**.
- Recent messages in `state` are text snippets only; full Block Kit layout needs
  a screenshot or an offline fixture built from known blocks.
- Do not assume the viewport shows a particular historical message — the live
  list is scrolled to recent. For a fixed layout repro, use
  `multi_paragraph_emoji_app` offline rather than scrolling the live client.
- Destructive actions (`send`) require `SNACK_AGENT_ALLOW_DESTRUCTIVE=1` or
  `scripts/agentctl.sh allow-destructive true`. Never enable that casually.
- Live mode uses the real Slack session. Never print tokens, cookies, or secrets.
- Prefer offline `agent-ui-check.sh` when live data is not needed.

### Message rendering notes (for UI work)

- Message bodies: `src/ui/message.rs` + `src/ui/blocks.rs` + selectable text in
  `src/ui/selectable.rs`.
- Slack often packs multi-paragraph posts as **one** `rich_text_section` with
  embedded `\n` in text leaves. Block rendering **must** split those into
  separate lines (`split_segments_on_newlines` in `blocks.rs`).
- Standard emoji → Unicode via `state::emoji_glyph` and render with
  `SelectableText`.
- Custom workspace emoji (image URL known) forces the `emoji_body` wrap path so
  images can sit inline. That path is word-chip + `Row::wrap`; it is more fragile
  than `SelectableText`. After changing it, re-run
  `ui_visual_multi_paragraph_custom_emoji_message` and inspect the PNG.
- Do not reintroduce “one big line with `\n` inside a wrapping row of text
  chips” — that produces floating mid-line words (the old `#ship` Hack Piano bug).

## Working Style

- Start from the concrete file, error, route, or behavior the user named.
- Read the existing code before proposing architecture.
- Keep edits scoped and behavior-preserving unless the user asked for a redesign.
- Report exactly which checks passed and which were not run.
- If a task is routed through a plan or handoff file, update that file as part of
  the work and keep its next steps testable.
