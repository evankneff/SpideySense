# frigate-popup — MVP

Version 1.0 · Status: shipped and verified · Last updated 2026-08-20
Goal: prove that a transient, focus-safe camera popup is genuinely more useful than a
notification.

## What This MVP Is

The MVP answered one question: **can a camera feed appear, be understood, and disappear
without ever interrupting what the user was doing?**

Everything else — stacking, cooldowns, the tray picker, do-not-disturb — is refinement
on top of that single behaviour.

## The Core Loop

```
person walks up to the door
        │
        ▼
Frigate publishes  frigate/events  {type: "new", after: {camera, label, ...}}
        │
        ▼
trigger evaluation ──► filtered out ──► logged with the reason, nothing happens
        │                               (wrong label / camera / zone / cooldown / DND)
        ▼ fires
popup opens, bottom-right, borderless, always on top
WS_EX_NOACTIVATE applied BEFORE show  ──►  focus never moves
        │
        ▼
snapshot and WebRTC race; first to arrive paints
        │
        ▼
user glances (0 clicks)
        │
        ▼
person leaves → Frigate sends `end` → linger countdown → window closes
        │
        ▼
back to exactly the state before it opened
```

Repeatable indefinitely. The cooldown stops one person producing a stream of windows.

## Screens In Scope

There is one screen. There is no main window.

### The popup (`src/popup.html`)

**Purpose:** show one camera, legibly, for a few seconds.

**Layout:** 480x270 by default, positioned in a configurable corner of the monitor's
work area (never under the taskbar). Video fills the frame edge to edge (`object-fit:
cover`) because the doorbell is 4:3 against a 16:9 window. A header strip across the top
carries the camera name, a detection badge, the time, and a close button. A 1px neutral
hairline separates the window from a light desktop.

**Behaviour:**
- Snapshot fades in if it wins the race; video fades over it once it truly has frames
- Bottom-left status dot: amber "connecting" → green "live", then fades out
- Hover pauses the auto-close
- Click opens the Frigate UI for that camera; ✕ closes immediately
- Stacks vertically when more than one is open; survivors slide down when one closes

**Two variants:**

| | Detection | Live (tray picker / `--preview`) |
|---|---|---|
| Badge | label name, e.g. PERSON | none |
| Timestamp | frozen at trigger time | ticks live |
| Auto-close | yes | no |

### The tray icon

The entire rest of the UI. Camera picker, do-not-disturb, launch at login, open config,
open log, quit.

## Data Persistence

| Data | Stored? |
|---|---|
| Config | Yes — TOML, hand-edited |
| Logs | Yes — rotating daily, 7 retained |
| Cooldown state | No — in memory, resets on restart |
| Do-not-disturb | No — deliberately resets, so you cannot stay muted forever |
| Launch at login | Yes — in the Windows registry, via the autostart plugin |
| Event history | No |

No database. No cache. Nothing to migrate.

## Out of Scope for MVP

| Feature | When |
|---|---|
| Tray camera picker | Shipped anyway, after v1 core |
| Do-not-disturb, autostart | Milestone 6, shipped |
| Snapshot-first page | Milestone 5, shipped |
| Per-camera required zones | Config support shipped; never exercised in anger |
| Multi-monitor routing | Deferred, no demand |
| Non-Windows | Not planned |

## Demo Script

Under three minutes:

1. **Show the tray icon.** No window, no taskbar entry. "The whole app is this icon."
2. **Trigger a detection** — `cargo run --example publish_test_event -- front_doorbell person 12`
   A popup appears bottom-right with the live doorbell feed.
3. **Type into another window while it is up.** Not a single keystroke is lost. This is
   the point of the whole project.
4. **Wait.** It closes itself ~15s after the `end` event.
5. **Fire two cameras at once.** They stack. Fire a third — the oldest is evicted and the
   survivor slides down to close the gap.
6. **Tray → Show camera → Garage Camera.** Opens pinned, no badge, ticking clock, stays
   until dismissed with ✕.
7. **Tray → Pause notifications.** The icon desaturates. Fire an event; the log says
   `skipped: notifications are paused`.

## Definition of Done

All met as of 2026-08-20:

- [x] Popup opens on a real Frigate detection over MQTT
- [x] Keyboard focus is never taken (`WS_EX_NOACTIVATE` before show)
- [x] Popup closes itself; never before the floor, never past the ceiling
- [x] Per-camera cooldown suppresses repeats
- [x] Multiple popups stack; oldest evicted at the cap; survivors restack
- [x] Live video plays via WebRTC (verified over UDP 8555 with TCP firewalled)
- [x] Snapshot and stream race; neither assumed faster
- [x] Click opens the Frigate UI; ✕ closes; hover pauses
- [x] Tray: camera picker, DND with a visibly different icon, autostart, config, log, quit
- [x] Config validated on startup with every problem reported at once
- [x] Malformed JSON and broker outages cannot crash or wedge the app
- [x] Rotating logs
- [x] `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt` applied
- [x] 60 unit tests covering all pure logic, stable across repeated parallel runs

Not done, and honestly marked as such:

- [ ] `stationary` close path observed against a real motionless object
- [ ] Live mid-session MQTT reconnect observed (only dead-broker backoff verified)
