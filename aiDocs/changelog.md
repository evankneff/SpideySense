This is meant to be a CONCISE list of changes to track as we develop this project. When adding to this file, keep comments short and summarized. Always add references back to the source plan docs for each set of changes.

---

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
