# macOS Port — Non-activating Popups

Created 2026-08-21 · Status: Complete (compiles + tests; visual confirm outstanding)
Roadmap: [2026-08-21-roadmap-macos-port.md](2026-08-21-roadmap-macos-port.md)

## Goal

Make the popup non-activating on macOS the way it already is on Windows
(`WS_EX_NOACTIVATE`): a click on it must never steal keyboard focus or the menu bar from
whatever the user is doing. The camera picker (`picker.rs`) is excluded on purpose — it
is the one window that deliberately takes focus.

## Why this needed more than `focused(false)`

`WS_EX_NOACTIVATE` is really two things on macOS, at two different levels:

1. Never becomes the key window — `WebviewWindowBuilder::focusable(false)`. Free, one
   line, no `unsafe`, and it also maps to `WS_EX_NOACTIVATE` on Windows.
2. A click doesn't activate the *process* — AppKit only grants this to an `NSPanel`
   whose style mask carries `NSWindowStyleMaskNonactivatingPanel`. Setting that bit on a
   plain `NSWindow` (what tao hands back) is silently discarded: no error, no log,
   `styleMask` reads back without it. The window's class has to actually change at
   runtime (`object_setClass`) to a small custom `NSPanel` subclass.

This was established by a standalone spike before any of this repo's code was touched
(`FINDINGS.md`, kept outside this repo) that built a throwaway Tauri app and measured the
class-swap requirement directly rather than assuming it from documentation.

## Approach

- `src-tauri/src/macos.rs`, `#[cfg(target_os = "macos")]`, mirrors the shape of
  `windows.rs`'s `#[cfg(windows)]` arm: `convert_to_nonactivating_panel` (the class swap,
  called once before first show), `enforce` (re-asserts style mask / level / collection
  behaviour, cheap to call defensively — tao's setters can silently rebuild the mask),
  `show` (`-orderFrontRegardless`, not `WebviewWindow::show()` — the latter is a no-op
  while the app is inactive, which for this window is always), `diagnostics` (logs class,
  mask, level, key state, activation policy — the only thing that can be verified without
  a screen).
- `windows.rs::suppress_activation` gains a `#[cfg(target_os = "macos")]` arm alongside
  the existing Windows one; the old `#[cfg(not(windows))]` no-op stub narrows to
  `#[cfg(not(any(windows, target_os = "macos")))]`.
- `.focusable(false)` added to the popup's `WebviewWindowBuilder` chain — portable, so it
  helps Windows too (redundant there, since `WS_EX_NOACTIVATE` already covers it).
- `app.set_activation_policy(Accessory)` once at startup in `lib.rs`, after `build()`.
  `Accessory` rather than `Prohibited`: a prohibited app cannot be activated at all,
  which would break the tray icon's own menu.
- No new crates: `objc2`/`objc2-app-kit`/`objc2-foundation` 0.6.4 / 0.3.2 were already
  transitive dependencies of `tao`; this only adds a direct edge to what was already in
  the lockfile (confirmed via `git diff Cargo.lock` — three lines).

## What is verified, and what is not

Verified (lock-independent, don't need a human): `cargo test` still 68/68, `cargo check`
clean, and a `--preview` launch logs
`class=SpideySenseNonactivatingPanel mask=0x8084 level=25 key=false app_active=false
policy=NSApplicationActivationPolicy(1)` — the class swap and style bits took effect.

**Not verified: that a click on the popup actually leaves focus and the menu bar alone.**
That is a screen-and-mouse check, not something this session can observe. See
`MACOS_RUNBOOK.md` for the two-minute walkthrough.

## Parked

- Tray icon coexistence with `ActivationPolicy::Accessory` — the standard configuration
  for a menu-bar app, and `tray-icon`/Tauri's tray both target it, but not exercised by
  the spike or this session. Watch for anything odd about the tray menu after the visual
  confirm.
- `tauri.conf.json` has no macOS `LSUIElement` bundle setting yet. Without it, a shipped
  `.app` briefly shows a Dock icon before `set_activation_policy(Accessory)` runs at
  startup (a plain `cargo build`/`cargo run` always shows one, bundled or not — that part
  is expected, not a bug). Packaging is out of scope for this port; flagging so it isn't
  forgotten before a macOS release build.
