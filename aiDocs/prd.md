# frigate-popup — Product Requirements

Version 1.0 · Status: v1 shipped · Last updated 2026-08-20
Platform: Windows 11 desktop · Scope: single-user LAN homelab

## Product Overview

### What it is

A tray-resident Windows app that turns Frigate NVR detections into small, borderless,
always-on-top camera popups. A person walks up to the front door; a 480x270 window
appears in the corner with the live feed; it closes itself when they leave.

### What it is not

Not an NVR client. No main window, no event history, no recordings browser, no settings
UI. Clicking a popup hands off to the Frigate web UI rather than reimplementing it.

### The problem

Frigate already detects people accurately and already sends notifications. But the
existing options all fail the same way for someone working at their desk:

- **Phone notifications** are in the wrong place. You are at the computer.
- **Home Assistant dashboards** require you to go look. By the time you do, the person
  has gone.
- **Windows toast notifications** are text, appear in a queue, and are dismissed without
  being read. A toast saying "person detected" does not answer "who is at my door?"
- **Keeping a Frigate tab open** costs a monitor and a browser tab, and shows all
  cameras all the time, which means you stop seeing any of them.

The gap is a glanceable, transient, zero-interaction view of *the one camera that
matters right now*.

### Positioning

Think of the motion-alert popups in a movie hacker setup: they appear, you glance, they
vanish. The product succeeds when the user never consciously interacts with it.

### Anti-features (deliberately excluded)

| Excluded | Why |
|---|---|
| Sound / audio | The popup must not interrupt. Silent by construction — no audio transceiver is negotiated |
| Focus stealing | Non-negotiable. The user is typing in another window |
| Notification queue / history | A missed popup is fine. Frigate holds the history |
| Acknowledgement or dismissal requirement | Anything requiring a click is a failure |
| Configuration UI | A commented TOML file is faster to edit than a settings screen |

## Target Users

### Primary

Self-hosted homelab users running Frigate on a separate machine, who spend the day at a
Windows desktop. Comfortable editing a TOML file and reading a log. Have already set up
Frigate, go2rtc, and MQTT.

### Secondary

None currently. The app assumes a LAN, a Frigate instance, and a Windows 11 desktop.

### Not for

- Anyone wanting a general camera viewer or NVR UI
- Non-Frigate camera systems
- Anyone who needs remote or mobile access
- Users who want a GUI installer and onboarding flow

### Personas

**Evan — the repo owner.** Five cameras around the house, Frigate on a Proxmox box, works
from a Windows desktop most of the day. Wants to know when someone walks up to the door
without leaving his editor, and does not want to hear about it when it is just him
walking to the garage. Comfortable running `cargo run` and reading `tracing` output.

<!-- TODO: There is exactly one real user today. Add personas only if this is ever
     released more widely. -->

## Core Features

### 1. Detection popup

**Purpose:** answer "who is there?" in under two seconds without interrupting anything.

**Flow:**
```
Frigate publishes frigate/events (type: new)
  → trigger evaluation (label, camera, zones, cooldown, DND)
  → popup opens in the configured corner, WS_EX_NOACTIVATE applied before show
  → snapshot and WebRTC race; whichever lands first paints
  → `update` events extend the deadline
  → `end` or `stationary` starts the linger countdown
  → popup closes, survivors restack
```

**Rules:**
- Only `type == "new"` opens a popup. `update` and `end` only affect existing ones.
- Per-camera cooldown (default 60s) so one person does not produce a stream of windows.
- Max simultaneous popups (default 2); a new detection evicts the oldest.
- Never closes before `min_display_seconds`; never survives past `max_display_seconds`.

### 2. Tray camera picker

**Purpose:** "just let me look at the driveway."

Every configured camera is listed under **Show camera**, including disabled ones (marked
`alerts off`) — asking for a feed by hand is a different intent from wanting alerts from
it. Opens a **pinned** popup: no auto-close, no cooldown, no detection badge.

### 3. Do-not-disturb

Tray toggle. The tray icon desaturates and the tooltip changes, because an invisible
paused state is one you forget you are in. Deliberately not persisted across restarts, so
the user cannot leave themselves silently muted indefinitely.

### 4. Snapshot-first live page

Both the still frame and the WebRTC connection start at the same instant. Measured on
this deployment, `frame.jpeg` takes 0.7–3.9s on a cold stream — often slower than WebRTC
— so neither is assumed to be the fast path. Whichever arrives first is shown, and the
video fades over the snapshot once it genuinely has frames.

### 5. Interactions

| Action | Result |
|---|---|
| Click the popup | Opens that camera in the Frigate UI in the default browser |
| Click the ✕ | Closes immediately, survivors restack |
| Hover | Pauses the auto-close timer |

## Technical Requirements

| Layer | Choice |
|---|---|
| Language / shell | Rust 2021 + Tauri v2 |
| Transport | MQTT (`rumqttc`) for events, WebRTC (go2rtc) for video |
| Frontend | One vanilla HTML file, no build step |
| Config | TOML in `%APPDATA%\frigate-popup\`, or beside the exe for portable mode |
| Logs | Rotating daily, 7 retained |

**Performance targets** (all met):

| Target | Measured |
|---|---|
| Time from event to window on screen | < 500ms |
| Time to first visible frame | 0.04–3.9s, dominated by go2rtc cold start |
| Focus stolen | Never |
| Idle CPU | Negligible; one 500ms sweeper tick |

**Privacy / security:**
- LAN only. No outbound internet calls — the local page removed the
  `https://go2rtc.org/manifest.json` fetch that go2rtc's own player makes.
- Credentials never in source or in git.

## Design Principles

1. **Could this steal focus or interrupt typing?** If yes, redesign. Everything else is
   negotiable; this is not.
2. **Is it glanceable in under two seconds?** If it needs reading, it does not belong.
3. **Does it claim something that may not be true?** Suppress rather than guess. The
   detection badge is emptied in Rust for hand-opened windows precisely because a badge
   that lies is worse than no badge.
4. **Is this Frigate's job?** Link out instead of rebuilding.
5. **Does this add a knob nobody will turn?** Prefer a good default.

## Success Metrics

### Qualitative (the real ones)

| Metric | How to tell | Why it matters |
|---|---|---|
| Never lost a keystroke | User notices a dropped character while typing | The one hard requirement |
| Glanced, did not interact | User can describe who was at the door without having clicked | The product works when it is not used |
| Not annoying enough to disable | Do-not-disturb usage stays near zero | Cooldown and label filters are tuned right |

### Quantitative

| Metric | Target | Status |
|---|---|---|
| False popups (wrong label/camera) | 0 | Met — filters verified against real events |
| Popups that never close | 0 | Met — watchdog + hard ceiling |
| Crashes from malformed input | 0 | Met — lenient parsing, no panics after startup |
| Unit tests | Cover all pure logic | 60 tests, clippy clean |

### Failure indicators

| Indicator | Detection | Meaning |
|---|---|---|
| DND left on for days | Tray state | Too noisy; tune cooldown or labels |
| Popups ignored entirely | User feedback | Wrong cameras, or the corner is wrong |
| Log fills with reconnects | `connection error` lines | Broker or packet-size regression |

## Competitive Landscape

| Alternative | Why it falls short here |
|---|---|
| Frigate mobile notifications | Wrong device when you are at the desk |
| Home Assistant dashboard | Requires going to look |
| Windows toast notifications | Text, queued, dismissed unread |
| Always-open Frigate tab | Costs a monitor; shows everything, so you see nothing |
| Blue Iris / Agent DVR alerts | Heavier, Windows-NVR-centric, not Frigate |

## Out of Scope for v1

| Feature | Rationale |
|---|---|
| Multi-monitor per-camera routing | One `monitor` setting is enough for one desk |
| Recording / clip playback | Frigate's job |
| Notification history | A missed popup is acceptable by design |
| Non-Windows support | Focus suppression is Win32-specific |
| Config UI | TOML is faster to edit |
| Frigate authentication for snapshots | go2rtc `frame.jpeg` needs no auth |

## Open Questions

1. Is the `stationary` close path correct in practice? Never observed firing.
2. Does a real mid-session MQTT drop resubscribe cleanly? Only dead-broker backoff has
   been verified.
3. Is `watchdog_seconds = 20` right? Measured `end` arrives 2.4–6.0s after the last
   update, so 20s is conservative. Could drop to ~12s.
4. Is `bird` actually in Frigate's `objects: track:` list? If not, bird popups can never
   fire regardless of this app's config.
5. Should do-not-disturb persist across restarts?
6. Is a single accent-free hairline border the right visual, or should popups be fully
   edgeless?
