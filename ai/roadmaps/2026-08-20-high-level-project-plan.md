# frigate-popup — High-Level Project Plan

Created 2026-08-20 · Status: Active (Phases 0-3 complete, Phase 4 open)

## Engineering Philosophy

This is a single-user desktop utility with roughly 2,000 lines of Rust. It does not need
architecture. Things to actively resist:

- Abstractions before the second use. There is one MQTT broker, one video backend, one
  platform — no traits "for flexibility".
- Generic base types "just in case". `Popups` is a `Vec`; that is correct.
- Layered indirection for what is really a filter and a timer.
- Config knobs nobody will turn. Every setting added is a setting to document, validate
  and keep working.
- Dead code and "we might need this later" stubs. Delete it; git remembers.

The one place to spend effort: **keeping pure logic pure.** `events.rs` and
`lifecycle.rs` touch no clock, no socket and no window, which is why the whole decision
surface is unit-testable. Preserve that boundary even when it is briefly inconvenient.

The second place: **verify against real hardware.** Four v1 bugs passed their unit tests
cleanly and were only caught by reading a real log or real pixel coordinates.

## Phase 0 — Foundation (complete)

- [x] Verify Tauri v2 APIs against current docs rather than memory
- [x] Confirm network reachability: go2rtc 1984, WebRTC 8555, MQTT 1883, Frigate 8971
- [x] Scaffold the Tauri app: tray, no main window
- [x] Config schema, defaults, validation reporting all problems at once
- [x] Rotating file logging via `tracing`
- [x] Icon generation with no image dependencies

## Phase 1 — MVP (complete)

Reference: [../../aiDocs/mvp.md](../../aiDocs/mvp.md)

- [x] MQTT client, subscribe to `frigate/events`, reconnect with backoff
- [x] Trigger logic: label, camera, zone, false positive, cooldown, DND
- [x] Popup window with focus suppression and work-area placement
- [x] Lifecycle: watchdog, linger, min/max display, stacking, eviction, restack
- [x] `--simulate` and `publish_test_event` for testing without walking outside
- [x] Verified end to end against the live Frigate instance

## Phase 2 — Polish & Stability (complete)

- [x] Snapshot-first local page, direct go2rtc signalling over `/api/ws`
- [x] Click-through to Frigate UI, hover-pause, close button
- [x] Do-not-disturb with a visibly different tray icon
- [x] Launch at login via `tauri-plugin-autostart`
- [x] Detection vs live popup distinction (badge suppressed in Rust, not CSS)
- [x] Malformed payloads, broker outages and poisoned locks all handled without panic
- [x] `cargo clippy -- -D warnings` clean; 60 tests stable across parallel runs

## Phase 3 — Feature Build (complete)

- [x] Tray camera picker with pinned live views
- [x] `--preview` for inspecting popup appearance
- [x] `discover_cameras` to derive real Frigate names from retained MQTT topics
- [x] `bird` added to watched labels

## Phase 4 — Future Planning (deferred)

Nothing here gets built without evidence from real use.

**Verification debt — do these before adding features:**

- [ ] Observe the `stationary` close path firing against a real motionless object
- [ ] Observe a live mid-session MQTT reconnect and resubscribe
- [ ] Confirm `bird` is in Frigate's `objects: track:` list, or bird popups can never fire
- [ ] Confirm the three camera-to-stream mappings visually (`front_doorbell` →
      `doorbell_sub`, `garage_camera` → `garage_sub`, `side_camera` → `sidedoor_sub`)

**Security follow-ups (outside this codebase):**

- [ ] Apply go2rtc `api.allow_paths`, now only `/api/ws` and `/api/frame.jpeg`, and
      re-test Frigate's own live view afterwards

**Hypotheses, unvalidated:**

- [ ] Tune `watchdog_seconds` down from 20s — measured `end` arrives 2.4-6.0s after the
      last update, so 20s is conservative
- [ ] Per-camera popup size, for the 4:3 doorbell vs 16:9 others
- [ ] Persist do-not-disturb across restarts, if the current reset proves annoying
- [ ] Sub-label support ("Evan" vs an unknown person) to suppress alerts for household
      members
- [ ] Multi-monitor routing per camera

## Phase Plan & Roadmap Docs

| Phase | Plan | Roadmap | Status |
|---|---|---|---|
| 0-3 | [complete/2026-08-20-phase-1-v1-build.md](complete/2026-08-20-phase-1-v1-build.md) | [complete/2026-08-20-roadmap-phase-1-v1-build.md](complete/2026-08-20-roadmap-phase-1-v1-build.md) | Complete |
| 4 | not started | not started | Deferred |

Each phase gets two documents when work begins: `YYYY-MM-DD-phase-N-name.md` for the
implementation plan and `YYYY-MM-DD-roadmap-phase-N-name.md` for milestone tracking. Both
reference each other, and both move to `complete/` when the phase closes.
