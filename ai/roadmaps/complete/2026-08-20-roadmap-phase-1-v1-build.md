# Roadmap — Phase 1: v1 Build

Created 2026-08-20 · Status: Complete
Plan: [2026-08-20-phase-1-v1-build.md](2026-08-20-phase-1-v1-build.md)

## Milestone 1 — Scaffold

- [x] Verify Tauri v2 APIs, plugin names and versions against current docs
- [x] Verify network reachability from this desktop (1984 open, 8555/TCP blocked,
      8971 open, 5000 blocked, 1883 open)
- [x] Generate icon set with no image dependencies
- [x] Tray icon with Quit, Open config file, Open log file
- [x] TOML config: schema, defaults, `deny_unknown_fields`, all-problems-at-once validation
- [x] Portable vs installed config/log paths
- [x] Rotating daily logs, 7 retained
- [x] Native message box for fatal startup errors
- [x] Verified running: tray appears, config parses, log written

## Milestone 2 — MQTT

- [x] `rumqttc` client with credentials from config or environment
- [x] Subscribe on every `ConnAck`, not once at startup
- [x] Exponential backoff, verified 1s/2s/4s/8s/16s against a dead broker
- [x] Lenient event parsing: unknown types, unknown fields, all `sub_label` shapes
- [x] Trigger logic in `events.rs`, pure and unit-tested
- [x] Decision logging with the reason for every skip
- [x] `--simulate` flag and `publish_test_event` example
- [x] Cadence instrumentation to measure real `update` timing
- [x] Verified: connected, subscribed, real publish fired a trigger

## Milestone 3 — Popup windows

- [x] Borderless, always-on-top, skip-taskbar, unfocused
- [x] `WS_EX_NOACTIVATE` applied before `show()`
- [x] Work-area placement with logical-to-physical scaling
- [x] Corner, offset, stack-gap and monitor selection from config
- [x] `--preview` flag for inspecting appearance
- [x] Verified: three popups stacked at the correct coordinates

## Milestone 4 — Lifecycle

- [x] Agreed the activity-driven model with the user, including the stationary trap
- [x] `Timing::apply` as a pure function, 13 rules unit-tested
- [x] Watchdog, linger, min-display floor, max-display ceiling
- [x] Linger pulls the deadline in, never pushes it out
- [x] Per-camera cooldown
- [x] Stacking, eviction at `max_popups`, restack on close
- [x] 500ms sweeper; plan under the lock, apply outside it
- [x] Verified: full new/update/end cycle, cooldown, stacking, eviction, restack

## Milestone 5 — Local page

- [x] Read go2rtc's `video-rtc.js` to get the real signalling protocol
- [x] `src/popup.html`: snapshot and WebRTC racing
- [x] Direct `/api/ws` signalling, no go2rtc UI files
- [x] Video-only negotiation, no audio transceiver
- [x] Header, status dot, close button, hairline border
- [x] `popup_open_ui` / `popup_dismiss` / `popup_hover` commands
- [x] `popup.page` config to fall back to go2rtc's own player
- [x] Verified in Chrome against the live doorbell before wiring into the app

## Milestone 6 — Polish

- [x] Do-not-disturb toggle
- [x] Desaturated tray icon and tooltip reflecting the paused state
- [x] Launch at login, tick read from real registry state
- [x] Tray camera picker with pinned live views
- [x] Detection vs live popup distinction, badge suppressed in Rust
- [x] Coloured borders removed per user feedback
- [x] `bird` added to watched labels

## Cross-cutting

- [x] `cargo clippy --all-targets -- -D warnings` clean
- [x] `cargo fmt` applied
- [x] 60 unit tests, stable across repeated parallel runs
- [x] Regression test for every bug found
- [x] README carries measured network facts
- [x] Git repo initialised, no credentials tracked

## Not Done

- [ ] `stationary` close path observed against a real motionless object
- [ ] Live mid-session MQTT reconnect observed
- [ ] `bird` confirmed present in Frigate's `objects: track:` list
- [ ] Camera-to-stream mappings confirmed visually by the user
