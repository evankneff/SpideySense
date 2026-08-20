# frigate-popup — Architecture

Version 1.0 · Last updated 2026-08-20 · Package versions verified 2026-08-20

## Platform & Build

| Layer | Decision | Notes |
|---|---|---|
| Language | Rust 2021, MSRV 1.85 | Built with 1.97.1 |
| Shell | Tauri v2 | Tray-only; `app.windows` is empty in `tauri.conf.json` |
| Webview | WebView2 (Chromium) | Ships with Windows 11; gives WebRTC for free |
| Frontend | Vanilla HTML/JS, single file | No framework, no bundler, no build step |
| Async | tokio, via `tauri::async_runtime::spawn` | Avoids managing a second runtime |
| Platform | Windows 11 only | Focus suppression is Win32-specific |
| Packaging | NSIS installer via `cargo tauri build` | Also runs portable |

## Dependencies

### Core

| Package | Version | Purpose |
|---|---|---|
| tauri | 2.11.5 | Shell, windows, tray. Features `tray-icon`, `image-png` |
| tauri-plugin-opener | 2.5.4 | Open Frigate UI / config / log in the default handler |
| tauri-plugin-autostart | 2.5.1 | Launch at login |
| rumqttc | 0.25.1 | MQTT client, `default-features = false` (no TLS; broker is plaintext 1883) |
| tokio | 1.53 | `rt`, `macros`, `time`, `sync` |
| serde / serde_json | 1.x | Event parsing, popup context injection |
| toml | 1.0.7 | Config parsing |
| anyhow / thiserror | 1.x / 2.x | Error handling; thiserror for typed config errors |
| tracing + tracing-subscriber + tracing-appender | 0.1 / 0.3 / 0.2 | Rotating daily logs |
| dirs | 6.0 | Locating `%APPDATA%` |
| tauri-plugin-global-shortcut | 2.3.2 | The camera picker hotkey |
| windows | 0.61.3 | Win32 only, under `[target.'cfg(windows)'.dependencies]` |

### Deliberately excluded

| Package | Why not |
|---|---|
| tauri-plugin-log | Aimed at forwarding frontend logs. All logic is in Rust; `tracing-appender` writes the rotating file directly |
| clap | Two flags. Hand-rolled parsing in `cli.rs` is less code than the dependency |
| Any JS framework | One page, roughly 200 lines. A build step would cost more than it saves |
| rustls / TLS features | Broker is plaintext on the LAN. Re-enable if the broker ever moves |

## Key API Decisions

### Tauri

- `WebviewWindowBuilder` supports every option needed: `decorations(false)`,
  `always_on_top(true)`, `skip_taskbar(true)`, `focused(false)`, `resizable(false)`,
  `shadow`, `visible`.
- **Monitor APIs are on `AppHandle`, not `Manager`**: `primary_monitor()`,
  `available_monitors()`, `monitor_from_point()`, `cursor_position()`.
- **`Monitor::work_area()`** returns the area excluding the taskbar. Always use this for
  placement, never `size()`, or bottom-corner popups sit under the taskbar.
- Windows are built with `.visible(false)`, positioned, then `.show()` — otherwise they
  flash at the default location first.
- `initialization_script` runs **after the global object exists but before the document
  is parsed**. `document.head` and `document.documentElement` are both null on that first
  pass. Register load listeners *before* attempting any DOM work, and guard the eager
  attempt, or a throw silently kills the rest of the script.
- `withGlobalTauri: true` is required for the vanilla page to reach
  `window.__TAURI__.core.invoke` without a bundler.

### Focus suppression (the load-bearing detail)

`.focused(false)` is not sufficient on Windows — WebView2 can still pull focus when the
webview finishes initialising. `windows.rs::suppress_activation` sets `WS_EX_NOACTIVATE`
on the HWND via `SetWindowLongPtrW`, **before** the window is shown. Tauri and this crate
may link different `windows` crate versions, so the handle is rebuilt as
`HWND(window.hwnd()?.0)` — a newtype over the same raw pointer either way.

### Global hotkey focus (Windows)

A global hotkey does not count as input to the receiving process, so Windows refuses
`SetForegroundWindow` and the picker appears unfocused. `picker.rs::force_foreground`
uses the standard workaround: `AttachThreadInput` (in `Win32::System::Threading`, not
`WindowsAndMessaging`) to join the foreground window's input queue for the duration of
the activation, then detach.

Independently, the blur-to-close handler ignores any `Focused(false)` that arrives before
a `Focused(true)` — a blur before the window ever held focus is a failed activation, not
the user clicking away, and closing on it makes the picker flash and disappear.

### rumqttc

- **`set_max_packet_size(1 MB, 64 KB)` is mandatory.** The default incoming limit is
  10 KB and an oversized message is treated as a *connection* error, not a dropped
  message — which presents as an endless reconnect loop. Frigate's retained snapshot
  JPEGs alone are ~14 KB.
- **Subscribe on every `ConnAck`, not once at startup.** The session is clean, so a
  reconnect returns subscribed to nothing. This is also what makes sleep/resume work.
- On `Err` from `poll()`, sleep with exponential backoff and continue; the event loop
  reconnects itself on the next poll.

### go2rtc signalling

The popup page speaks go2rtc's protocol directly over
`ws://<host>:1984/api/ws?src=<stream>`:

- Send `{type: "webrtc/offer", value: <sdp>}` after `createOffer` + `setLocalDescription`
- Receive `{type: "webrtc/answer", value: <sdp>}` → `setRemoteDescription`
- Exchange `{type: "webrtc/candidate", value: <candidate>}` both ways; send a final
  empty-string candidate to signal end of gathering
- `addIceCandidate` needs `sdpMid: "0"`

Only a video transceiver is added (`recvonly`). No audio is negotiated at all.

This avoids needing `stream.html`, `video-rtc.js` or `video-stream.js`, which reduces the
go2rtc `api.allow_paths` whitelist to `/api/ws` and `/api/frame.jpeg`.

## Data Models

### Config (`config.toml`)

Sections: `[mqtt]`, `[frigate]`, `[popup]`, `[detection]`, and repeated `[[cameras]]`.
Uses `deny_unknown_fields` so a typo like `label = [...]` fails loudly rather than
silently disabling a filter. Validation collects **every** problem and reports them
together.

Key `[[cameras]]` fields:

| Field | Type | Notes |
|---|---|---|
| name | String | The **Frigate** camera name, matched against `after.camera` |
| stream | String | The **go2rtc** stream name. These differ on this deployment |
| enabled | bool | Default true |
| labels | Option<Vec<String>> | Overrides `detection.labels` |
| required_zones | Option<Vec<String>> | Must intersect `entered_zones` |
| cooldown_seconds | Option<u64> | Overrides `detection.cooldown_seconds` |

### FrigateEvent (wire format)

Deliberately lenient — Frigate adds fields between releases:

- Unknown `type` values deserialize to `EventType::Unknown` rather than failing
- No `deny_unknown_fields`; new Frigate fields are ignored
- `sub_label` is `Option<serde_json::Value>` because Frigate has shipped it as null, as a
  string, and as `[name, score]`

### Runtime state (in memory only, never persisted)

| Type | Holds |
|---|---|
| `Triggers` | `HashMap<camera, Instant>` of last fire (cooldown) + `paused` flag |
| `Popups` | `Vec<Entry>` ordered oldest-first; index **is** the stack slot |
| `Entry` | camera, `Timing`, `hovered`, `pinned` |
| `Timing` | `opened_at`, `deadline` |

## Folder Structure

```
CLAUDE.md                      agent entry point
config.example.toml            commented reference config, compiled into the binary
aiDocs/                        project documentation
ai/                            roadmaps, guides, notes
assets/gen-icon.js             generates icon PNGs from scratch, no image deps
src/
  popup.html                   snapshot, WebRTC, header, controls
  picker.html                  keyboard camera picker
  index.html                   placeholder; Tauri requires a frontendDist
src-tauri/
  examples/
    discover_cameras.rs        lists real Frigate camera names from retained MQTT topics
    publish_test_event.rs      publishes a new/update/end sequence to the real broker
  src/
    main.rs                    entry point; no console in release
    lib.rs                     startup sequence, AppState, sweeper task, Tauri wiring
    cli.rs                     --preview / --simulate / --help
    config.rs                  schema, defaults, validation
    paths.rs                   portable vs installed config and log locations
    logging.rs                 rotating daily file logging
    events.rs                  Frigate schema + trigger decision (pure, unit-tested)
    mqtt.rs                    client, backoff, cadence measurement, dispatch
    lifecycle.rs               popup deadlines, stacking, sweep (pure timing)
    picker.rs                  global-hotkey camera picker (the one focused window)
    windows.rs                 window creation, placement math, focus suppression
    tray.rs                    tray icon, camera picker, DND, autostart
    commands.rs                the three commands popup.html invokes
    ui.rs                      native message boxes for fatal startup errors
    testutil.rs                shared test config helper (test-only)
```

## Concurrency Model

```
main thread (Tauri event loop)
  ├── tray menu events        → open pinned popups, toggle DND/autostart
  ├── invoke handlers         → popup_open_ui / popup_dismiss / popup_hover
  ├── tokio task: mqtt::run   → poll loop, parse, decide, open popups
  └── tokio task: sweeper     → every 500ms, close expired popups and restack
```

Two `Arc<Mutex<..>>`. Rules:

1. **Never hold a lock across a window call.** `lifecycle::plan()` computes what to do
   under the lock; `lifecycle::apply()` performs the window work outside it.
2. **Recover poisoned locks**, never propagate the panic — a dead thread must not take
   the MQTT client down with it.

A single sweeper rather than a timer per popup: extending a deadline is then just a field
write with no task to cancel, and a machine resuming from sleep finds everything past its
deadline and cleans up in one pass.

## Popup Lifecycle

Pure function of `(opened_at, signal, config, now)` in `lifecycle.rs`:

| Signal | Source | Proposed deadline |
|---|---|---|
| `Opened` | trigger fired | now + `watchdog_seconds` |
| `Active` | `update`, moving | now + `watchdog_seconds` |
| `Stationary` | `update` with `stationary: true` | now + `linger_seconds` |
| `Ended` | `end` | now + `linger_seconds` |

Then clamped: floor at `opened_at + min_display_seconds`, ceiling at
`opened_at + max_display_seconds`. A linger only ever **pulls the deadline in**, never
pushes it out, so a late `end` cannot resurrect a popup that was about to close.

## Hard Constraints

- `WS_EX_NOACTIVATE` before `show()`. Never call `set_focus()` on a popup.
- `set_max_packet_size` on the MQTT client. The default will break it.
- Subscribe on `ConnAck`, not at startup.
- Never hold the popups lock across a window call.
- No `unwrap()`/`expect()` after startup.
- Placement uses `work_area()`, never `size()`.
- Restack survivors **before** opening a new popup after eviction, or the new window
  lands on top of one that never moved.
- `initialization_script`: register listeners before attempting DOM work.
- Detection labels are suppressed in Rust for `Kind::Live`, not hidden in CSS.
