This is meant to be a CONCISE list of changes to track as we develop this project. When adding to this file, keep comments short and summarized. Always add references back to the source plan docs for each set of changes.

---

## 2026-08-24 - v0.2.0, and releases build themselves

- Version bumped to 0.2.0 in `Cargo.toml` and `tauri.conf.json`.
- New `.github/workflows/release.yml`: pushing a `v*` tag builds the NSIS installer on
  `windows-latest`, stages it alongside the portable exe, writes `SHA256SUMS.txt` and
  publishes the release. A published binary is therefore reproducible from a tag rather
  than hand-built on one laptop.
- The workflow re-runs the test suite first: a tag can point at a commit that never went
  through CI, so it does not assume the suite must have passed.
- Upload is re-runnable - it attaches to an existing release instead of failing on retry.

## 2026-08-24 - Hotkey cycles the picker instead of toggling it

- The global hotkey is now three-state: advance the selection if the picker is open,
  close the newest pinned view if one is on screen, otherwise open the picker. Alt+Tab
  shape, so the whole interaction is one hand: tap, tap, Enter.
- Advancing is a Tauri event the picker page listens for, calling its existing `move(1)`.
  The hotkey is a second input source for the selection logic that already drove the
  arrow keys, so the two cannot drift apart.
- Detection popups are excluded from the close rule; they expire on their own, and
  closing them here would make the key's meaning depend on whether a detection had just
  fired. `Popups::newest_pinned` skips them.
- `commands::dismiss_pinned` extracted so the hotkey closes a popup through the same
  forget-close-restack path as its close button rather than a parallel implementation.
- Consequence: a second press no longer dismisses the picker. Esc and click-away do, and
  the picker's hint line says so.
- Verified by pressing the key against the running app.
- 72 unit tests, up from 68.

## 2026-08-24 - Hotkey conflict diagnosed, not worked around

- `F20` briefly failed to register: another application owned it. Confirmed with a direct
  `RegisterHotKey` probe returning `ERROR_HOTKEY_ALREADY_REGISTERED` (1409) for F20 while
  F17 was free, which ruled out a teardown race from the previous process.
- No code change. The app already degrades correctly - logs the conflict and carries on
  without the hotkey.

## 2026-08-21 - Reload config from the tray

- New tray item re-reads and validates `config.toml`, swaps it in, and rebuilds the menu.
- `AppState.config` was an `Arc<Config>` cloned into five long-lived places, so nothing
  could replace it. Now a `ConfigSlot` (`Arc<RwLock<Arc<Config>>>`) that readers snapshot
  from, so a reload never blocks a dispatch and each reader sees a whole config, never a
  mix. `dispatch` takes one snapshot per event for the same reason.
- MQTT and hotkey settings are read but not applied - both are already bound to something
  live. `config::deferred_changes` names what changed and the reload asks for a restart
  instead of pretending. Pure function, so it is unit tested.
- Menu rebuild reads the live pause state; hardcoding it would have silently un-ticked
  do-not-disturb on every reload.
- Verified end to end: clicked in the tray against the running app under live detection
  traffic, logged `config reloaded from disk cameras=5 deferred=0`, no menu-rebuild
  warnings, process still responding. The restart-required dialog is unit tested but has
  not been seen on screen.
- 68 unit tests, up from 64.

## 2026-08-21 - MQTT reconnect caveat retired

- A live mid-session reconnect has now been observed twice against real hardware, both
  sleep/resume rather than broker failure, correlated against Windows Kernel-Power events
  (Id 42 sleep, 130/131 resume). Reconnect and resubscribe 1.08s after a 1s backoff and
  2.02s after a 2s one.
- An apparent two-hour outage in the log was the machine asleep, not a stalled client.

## 2026-08-21 - Prepared for public release

- Audited all 6 commits of history before going public: no `config.toml`, no credentials,
  no real email (commits use a GitHub noreply address). No history rewrite was needed.
- Sanitised identifying detail: LAN addresses to `192.168.1.x` placeholders, indoor camera
  names replaced, and the README section documenting the owner's real network topology,
  camera-to-zone map and port scan replaced with generic field notes.
- Dropped a roadmap TODO naming the camera vendor and an unresolved credential exposure.
- Added MIT `LICENSE`, package metadata, and GitHub Actions CI on `windows-latest`
  (fmt, clippy `-D warnings`, tests, release build). Verified green locally first.
- README rewritten for a portfolio audience, led by a demo GIF and the `WS_EX_NOACTIVATE`
  focus-suppression problem. AI scaffolding documented deliberately rather than hidden.
- Untracked `.vscode/settings.json`.

## 2026-08-21 - Autostart actually wired up

- No code change. The tray toggle was never clicked, so nothing was ever in
  `HKCU\...\CurrentVersion\Run` and the app did not come back after a reboot.
- Only a debug build existed, so enabling the toggle would have registered a path under
  `target/` that `cargo clean` silently breaks. Built release and copied it to
  `%LOCALAPPDATA%\frigate-popup\` instead, then registered that path.
- Verified by launching the registry command line: `portable=false`, config loaded,
  MQTT connected to 192.168.1.11:1883, hotkey F20 registered.
- README gained a "binary that runs at boot" section - rebuilds must re-copy the exe or
  boot keeps running the old build.

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
