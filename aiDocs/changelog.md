This is meant to be a CONCISE list of changes to track as we develop this project. When adding to this file, keep comments short and summarized. Always add references back to the source plan docs for each set of changes.

---

## 2026-08-20 — Keyboard camera picker

- Global hotkey (default F20) toggles a keyboard-navigable camera list.
  Up/Down wrap, Home/End, 1-9 jump, Enter opens, Esc dismisses, click-away dismisses.
- Selecting opens a pinned live view, same as the tray picker.
- `tauri-plugin-global-shortcut` 2.3.2; binding is a config string parsed by the plugin,
  so any accelerator it accepts works. Registration failure is logged, not fatal.
- The picker is the one window that deliberately takes focus. Kept as a separate window
  type (`camera-picker`) so the never-steal-focus rule stays absolute for popups.

## 2026-08-20 — Picker focus and placement fixes

- Picker appeared then vanished instantly: a global hotkey is not input to this process,
  so `SetForegroundWindow` was refused, the window never gained focus, and the
  blur-to-close handler fired immediately. Fixed with `AttachThreadInput` activation plus
  ignoring any blur that arrives before focus was ever held.
- Moved from screen centre to just above the tray (bottom-right of the primary monitor's
  work area), matching where a tray context menu opens.

## 2026-08-20 — v1 complete

Initial build, milestones 1-6. See
[ai/roadmaps/complete/2026-08-20-phase-1-v1-build.md](../ai/roadmaps/complete/2026-08-20-phase-1-v1-build.md).

- Tray-only Tauri v2 app: config load + validation, rotating logs, no main window.
- MQTT client on `frigate/events` with reconnect backoff and lenient parsing.
- Trigger logic in `events.rs`: label, camera, zone, false-positive and cooldown filters.
- Popup windows with `WS_EX_NOACTIVATE` focus suppression and work-area placement.
- Activity-driven lifecycle: watchdog, linger, min/max display, stacking, eviction.
- Local `popup.html`: snapshot and WebRTC race, direct go2rtc signalling over `/api/ws`.
- Tray camera picker (pinned live views), do-not-disturb with a paused icon, autostart.
- Added `bird` alongside `person` to the watched labels.
- Tooling: `--preview`, `--simulate`, `discover_cameras`, `publish_test_event`.

Bugs found by running against real hardware, each with a regression test:

- Overlay `initialization_script` threw before the document existed, killing its own
  fallback registration, so the CSS never applied.
- Eviction did not restack survivors, drawing two popups at identical coordinates.
- rumqttc's 10 KB packet default treated Frigate's ~14 KB snapshots as a connection
  error, causing an endless reconnect loop.
- Cadence tracking overwrote the event start time on every update, so logged "lifetime"
  was really the gap since the last message.
- Test helpers shared a fixed temp directory; parallel tests deleted each other's
  fixtures. Consolidated into `testutil.rs`.

Corrections to earlier assumptions:

- Frigate camera names differ from go2rtc stream names (`front_doorbell` vs `doorbell`).
  Every event was being filtered out until this was found.
- WebRTC works over UDP 8555 despite TCP 8555 being firewalled.
- `update` events arrive every ~200ms (p90 670ms), not once per second as assumed.
