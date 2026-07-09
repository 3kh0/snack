# AGENTS.md

Guidance for agents working in this repository.

## Project Shape

Snack is a Rust desktop Slack client built with Iced.

Important boundaries:

- `src/app.rs` is the app facade and shared state/message type surface.
- `src/app/update.rs` owns reducer-style update logic and async effects.
- `src/app/view.rs` owns top-level rendering.
- `src/app/subscription.rs` owns Iced subscriptions and periodic ticks.
- `src/ui/` contains reusable Slack UI widgets and styling.
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

## Working Style

- Start from the concrete file, error, route, or behavior the user named.
- Read the existing code before proposing architecture.
- Keep edits scoped and behavior-preserving unless the user asked for a redesign.
- Report exactly which checks passed and which were not run.
- If a task is routed through a plan or handoff file, update that file as part of
  the work and keep its next steps testable.
