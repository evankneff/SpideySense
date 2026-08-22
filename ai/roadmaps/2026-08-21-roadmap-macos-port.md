# Roadmap — macOS Port: Non-activating Popups

Created 2026-08-21 · Status: Complete (terminal state) — visual confirm is Evan's
Plan: [2026-08-21-macos-port.md](2026-08-21-macos-port.md)

## Milestone 1 — Make it compile and keep the Windows path intact

- [x] Add `objc2`/`objc2-app-kit`/`objc2-foundation` under
      `[target.'cfg(target_os = "macos")'.dependencies]`
- [x] `cargo check` clean, no warnings
- [x] `cargo test` — 68/68, same as the pre-port baseline

## Milestone 2 — The class swap

- [x] `src-tauri/src/macos.rs`: `NonactivatingPanel` (`NSPanel` subclass, no ivars),
      `convert_to_nonactivating_panel`, `enforce`, `show`, `diagnostics`
- [x] Wired into `windows.rs::suppress_activation` and `windows.rs::show` behind
      `#[cfg(target_os = "macos")]`, ordered before the window's first show (existing
      Windows comment already documents this ordering requirement)
- [x] `.focusable(false)` added to the popup builder
- [x] `app.set_activation_policy(Accessory)` added once in `lib.rs`

## Milestone 3 — Evidence

- [x] `--preview` launch logs `class=SpideySenseNonactivatingPanel mask=0x8084 …
      policy=NSApplicationActivationPolicy(1)` — configuration confirmed
- [ ] Visual confirm against an unlocked screen — see `MACOS_RUNBOOK.md`. **Not run this
      session; needs Evan.**

## Parked, not blocking

- [ ] Tray + `Accessory` interaction, watch after the visual confirm
