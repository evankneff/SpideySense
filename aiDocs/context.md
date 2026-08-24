# frigate-popup — Project Context

Last updated: 2026-08-20

## What This Project Is

A Windows desktop tray application that watches a Frigate NVR over MQTT and, when a
watched object is detected, pops a small borderless always-on-top window showing that
camera's live feed, then closes itself.

It is **not** a Frigate client, a viewer, or an NVR UI. It has no main window, no
settings screen, and no browsing. It shows you a camera for a few seconds and gets out
of the way.

**Core philosophy: glanceable, quiet, and gone when it is no longer relevant. The popup
must never steal keyboard focus — the user is mid-work in another window.**

Built for a single homelab user (the repo owner) running Frigate on a separate LAN
machine. Not currently packaged or documented for third parties.
The repo is public as a portfolio project under MIT. Example configs and test
fixtures therefore use placeholder addresses and generic camera names, never the
owner's real network layout.

## Key Documents

| Document | Purpose |
|---|---|
| [prd.md](prd.md) | Product requirements, users, features, success criteria |
| [architecture.md](architecture.md) | Stack, dependencies, data models, module layout |
| [mvp.md](mvp.md) | The core loop and what "done" meant for v1 |
| [coding-style.md](coding-style.md) | Code standards for this repo |
| [changelog.md](changelog.md) | Running list of changes |
| [../README.md](../README.md) | Operational reference: config, network facts, testing |
| [../ai/roadmaps/](../ai/roadmaps/) | Phase plans and roadmaps |

The README is not redundant with these docs. It holds **measured facts about this
specific deployment** — port reachability, camera name mappings, event cadence numbers.
Check it before assuming anything about the network.

## Current State

**v1 is complete and running.** All six original milestones shipped and were verified
against the live Frigate instance, not just unit-tested.

| Milestone | State |
|---|---|
| 1. Tray, config load + validation, rotating logs | Done |
| 2. MQTT client, event subscription, trigger decisions | Done |
| 3. Popup window on trigger | Done |
| 4. Lifecycle: cooldown, auto-close, stacking, eviction | Done |
| 5. Snapshot-first local page, WebRTC | Done |
| 6. Do-not-disturb, autostart, click-through, hover-pause | Done |

Added beyond the original scope: a tray camera picker and a global-hotkey keyboard
picker (both open pinned live views), `--preview` and `--simulate` flags, and two example
binaries (`discover_cameras`, `publish_test_event`).

72 unit tests, clippy clean with `-D warnings`, verified in CI on Windows.

**One behaviour is implemented and unit-tested but has never been observed firing
against real hardware.** Do not claim it works:

- The `stationary` close path. No tracked object has gone motionless in frame yet.

Retired 2026-08-21: a live mid-session MQTT reconnect **has** now been observed twice,
correlated against Windows Kernel-Power events. Both drops were sleep/resume, not broker
failures. Reconnect and resubscribe followed 1.08s after a 1s backoff and 2.02s after a
2s one.

## Behavior

Development process rules:

- Substantial work gets a plan doc and a roadmap doc in `ai/roadmaps/`, named
  `YYYY-MM-DD-phase-N-name.md` and `YYYY-MM-DD-roadmap-phase-N-name.md`. Each references
  the other.
- When a phase completes, both docs move to `ai/roadmaps/complete/`.
- Research and investigation write-ups go in `ai/guides/`.
- Every meaningful change gets a line in `aiDocs/changelog.md` referencing its plan doc.
- Update `context.md` when the current state changes. Keep documents as living
  artifacts. Stale docs are worse than no docs.

## What's Explicitly Out of Scope

- A main window, dashboard, or multi-camera grid view.
- Browsing recordings, events history, or clips. That is Frigate's job; the popup links
  out to the Frigate UI instead.
- Two-way audio, PTZ control, or any camera write operation.
- Non-Windows platforms. The focus-suppression path is Win32-specific.
- Mobile or remote access. LAN only.
- Any Frigate configuration management. This app reads events; it never writes to
  Frigate.
- Packaging for other people's setups (installers, onboarding, config UI).

## Hard Constraints — Never Violate Without Asking

- **Popups must never take keyboard focus.** The `camera-picker` window is the single
  deliberate exception, because the user summoned it and it cannot read arrow keys
  otherwise. It is a separate window type so the exception cannot leak into popups. `.focused(false)` alone is not sufficient
  on Windows; `WS_EX_NOACTIVATE` is applied to the HWND before the window is shown. Do
  not remove it, and do not call `set_focus()` on a popup.
- **No credentials in source or in the repo.** The MQTT password lives in `config.toml`
  (gitignored) or an environment variable named by `password_env`.
- **No `unwrap()` or `expect()` on runtime paths.** Config parsing at startup may fail
  loudly; everything after startup must degrade rather than panic.
- **A malformed MQTT payload must never kill the client.** Log it truncated and drop it.
- **The detection badge must never appear on a window the user opened by hand.** It is
  suppressed at the Rust boundary, not in CSS. A badge that lies is worse than no badge.
- **The app never exits because a window closed.** `RunEvent::ExitRequested` is
  intercepted; only the tray Quit item calls `app.exit()`.
- **No audio.** No audio transceiver is negotiated at all — quiet by construction, not
  by muting.
- **Do not add a main window** to satisfy a framework convention.

## Code Style

See [coding-style.md](coding-style.md). Summary: small modules, pure logic separated
from I/O, comments only where the logic is non-obvious, `cargo clippy -- -D warnings`
and `cargo fmt` must both be clean.

## Architecture

Full detail in [architecture.md](architecture.md). Summary:

- **State**: `AppState` holds `Arc<Config>` plus two `Arc<Mutex<..>>` — `Triggers`
  (cooldowns, do-not-disturb) and `Popups` (open windows and their deadlines).
- **Concurrency**: one tokio task for MQTT, one 500ms sweeper task for expiry. Window
  work is planned under the lock and applied outside it, so a window call can never
  block another thread holding popup state.
- **Storage**: a single TOML config file. No database, no persisted runtime state.
  Do-not-disturb deliberately resets on restart.
- **Layout**: `src-tauri/src/` holds one module per concern; `src/popup.html` is the
  entire frontend, vanilla, no build step.

## Design Principles

Decision filters. Apply in order:

1. **Could this steal focus or interrupt typing?** If yes, redesign it. This is the one
   requirement the whole product rests on.
2. **Is this information glanceable in under two seconds?** If it needs reading, it does
   not belong in a popup.
3. **Does this claim something happened that did not?** A badge, a label, a timestamp —
   if it can be wrong, suppress it rather than guess.
4. **Is this Frigate's job?** If yes, link out to Frigate instead of rebuilding it.
5. **Does this add a knob nobody will turn?** Prefer a good default over a config option.

## Development Process

- Verify against real hardware before claiming a behaviour works. This project has
  already produced four bugs that unit tests passed straight through: an overlay script
  that never ran, popups drawn on top of each other after eviction, a mislabelled event
  lifetime, and a shared temp directory that made tests delete each other's fixtures.
- When a fix is applied, add the regression test that would have caught it — testing the
  mechanism, not just the output string.
- Keep `README.md` current with measured network facts. Guessing about this LAN has been
  wrong more than once.
