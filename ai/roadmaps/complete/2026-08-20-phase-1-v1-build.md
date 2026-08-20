# Phase 1 — v1 Build (Milestones 1-6)

Created 2026-08-20 · Status: Complete
Roadmap: [2026-08-20-roadmap-phase-1-v1-build.md](2026-08-20-roadmap-phase-1-v1-build.md)

Written retrospectively. v1 was built in one session against live hardware, in six
user-defined milestones with a stop after each for testing.

## Goal

Turn Frigate detections into transient, focus-safe camera popups on a Windows desktop.

## Approach

Milestone-gated, verify-as-you-go. Each milestone ended with the app actually running
against the real Frigate instance and the log read back, rather than a test suite alone.

### Milestone 1 — Scaffold

Tray icon, TOML config with all-problems-at-once validation, rotating `tracing` logs. No
windows. Portable mode (config beside the exe) takes precedence over `%APPDATA%`.

Native message boxes for startup failures, because release builds have no console and a
config error would otherwise be silent.

### Milestone 2 — MQTT

`rumqttc` client on `frigate/events`. Trigger logic extracted into `events.rs` as a pure
function of `(config, event, now)` so the whole decision surface is testable against
canned payloads.

Cadence instrumentation added deliberately, to measure real `update` timing before tuning
the milestone 4 watchdog rather than guessing.

### Milestone 3 — Popup windows

`WebviewWindowBuilder` with decorations off, always-on-top, skip-taskbar, unfocused.
Built hidden, positioned, then shown, to avoid a flash at the default location.

`WS_EX_NOACTIVATE` applied to the HWND before `show()` — `.focused(false)` alone is not
sufficient on Windows because WebView2 can pull focus during initialisation.

Placement uses `Monitor::work_area()`, not `size()`, so bottom-corner popups sit above
the taskbar. Config sizes are logical pixels scaled to physical at open time.

### Milestone 4 — Lifecycle

Activity-driven rather than fixed-duration, after discussion with the user: `update`
extends, `end` or `stationary` starts a linger, with a min-display floor and a
max-display ceiling. The stationary rule exists because Frigate keeps an event open for a
motionless object and sends no `end` — a parked car would otherwise pin a window.

A single 500ms sweeper rather than a timer per popup: extending a deadline becomes a
field write with no task to cancel, and sleep/resume cleans up in one pass.

### Milestone 5 — Local page

`src/popup.html`, vanilla, no build step. Speaks go2rtc's signalling directly over
`/api/ws` rather than embedding `stream.html`, which removed the dependency on go2rtc's
UI files and its outbound call to `go2rtc.org`.

Snapshot and stream race rather than running in sequence, because measurement showed
`frame.jpeg` is often the *slower* path on a cold stream.

### Milestone 6 — Polish

Do-not-disturb with a desaturated tray icon, launch at login, tray camera picker with
pinned live views. Click-through, hover-pause and the close button landed with milestone 5.

## Key Decisions

| Decision | Rationale |
|---|---|
| Tauri v2 over egui/native | WebView2 gives WebRTC for free; no video pipeline to write |
| Sub streams by default | Lower bitrate, faster connect, plenty for 480x270 |
| Own page over go2rtc's `stream.html` | Removes UI-file dependency, shrinks the allow_paths whitelist, no outbound internet call |
| Single sweeper over per-popup timers | No task cancellation; survives sleep/resume |
| Pure logic in `events.rs` / `lifecycle.rs` | The entire decision surface is unit-testable |
| No `clap` | Two flags; hand-rolled parsing is less code than the dependency |
| DND not persisted | Cannot leave yourself silently muted indefinitely |

## Bugs Found

All four passed their unit tests and were caught only by running the real thing.

1. **Overlay script never ran.** `initialization_script` executes before the document is
   parsed, so `document.head` and `document.documentElement` were both null; the eager
   `inject()` threw and took its own `DOMContentLoaded` registration with it. The test
   asserted the generated *string* was right, which it was.
2. **Eviction drew popups on top of each other.** Restacking only happened on expiry, so
   after an eviction the survivor stayed in its old slot and the new window opened at the
   same coordinates. Visible only in the logged y-values.
3. **rumqttc reconnect loop.** The 10 KB incoming default treats an oversized message as
   a connection error; Frigate's retained snapshots are ~14 KB. Raised to 1 MB.
4. **Mislabelled event lifetime.** The cadence map overwrote the start time on every
   update, so "lifetime" was really the gap since the last message — which would have
   mistuned the watchdog.

Plus a recurring test-harness bug: a fixed temp directory shared by parallel tests, each
deleting the others' fixtures. Written three times before being consolidated into
`testutil.rs`.

## Assumptions Corrected

- **Frigate camera names are not go2rtc stream names.** `front_doorbell` vs `doorbell`.
  Every real event was being filtered out as "not in the config" until this surfaced.
- **WebRTC works over UDP 8555** even with TCP 8555 firewalled.
- **`update` arrives every ~200ms**, not once per second.
- **`frame.jpeg` is not reliably fast** — 0.7-3.9s on a cold stream.

## Outcome

All six milestones shipped. 60 unit tests, clippy clean with `-D warnings`. Two
behaviours remain unobserved against real hardware and are explicitly not claimed: the
`stationary` close path and a live mid-session MQTT reconnect.
