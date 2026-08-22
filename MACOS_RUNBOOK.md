# macOS runbook — SpideySense (frigate-popup)

Two minutes, screen unlocked. Confirms the thing no automated check can: that the popup
does not steal focus or the menu bar. Everything else (class swap, style mask,
activation policy) was already confirmed at runtime by `cargo test` + a `--preview`
launch — see `ai/roadmaps/2026-08-21-macos-port.md`.

```sh
cd ~/dev/SpideySense/src-tauri
PATH=/opt/homebrew/opt/rustup/bin:$PATH cargo build
```

You need a `config.toml` with at least one camera (portable mode: put it next to the
binary at `src-tauri/target/debug/config.toml`, or use whatever config you already have
under `~/Library/Application Support/frigate-popup/`). The MQTT broker does not need to
be reachable for this check — `--preview` never touches MQTT.

```sh
open TextEdit  # or any app — this is what focus-stealing would interrupt
```

1. Click into a TextEdit document and start typing.
2. Without clicking away, run (from a terminal, backgrounded or in another Space):
   ```sh
   RUST_LOG=debug ./target/debug/frigate-popup --preview
   ```
3. A popup appears in the configured corner within a second or two.

| # | Check | Pass |
|---|-------|------|
| 1 | Keep typing while the popup appears | characters keep landing in TextEdit |
| 2 | Menu bar | still says "TextEdit", not "frigate-popup" |
| 3 | **Click inside the popup** (the video area, not a button) | menu bar still says "TextEdit" |
| 4 | Cmd-Tab | **no** entry for frigate-popup |
| 5 | Dock | no icon, no bounce, at any point |
| 6 | Position | in the configured corner, correctly inset, not clipped |
| 7 | Tray icon | still there, still opens its menu, still responds to clicks |

Then scan the log for the line starting `class=`:

```
class=SpideySenseNonactivatingPanel mask=0x8084 level=25 key=false app_active=false policy=NSApplicationActivationPolicy(1) frontmost="TextEdit"
```

`class=` must say `SpideySenseNonactivatingPanel`, not `NSWindow` or a tao class name —
that is the one line proving the swap took effect. `frontmost=` should read `"TextEdit"`
(or whatever app you were using) on every line, never `"frigate-popup"`.

Quit from the tray's Quit item when done — the picker/popup windows have no close
button reachable by keyboard, by design.

## If something looks wrong

| Symptom | Look at |
|---|---|
| popup never appears | log for `no NSWindow behind the Tauri window` |
| popup appears but steals focus on click | log's `class=` — if it says anything other than `SpideySenseNonactivatingPanel`, the swap did not happen or was refused (see `refusing the class swap` in the log) |
| no tray icon / tray menu broken | `ActivationPolicy::Accessory` interaction, not exercised before this session — see the "Parked" note in the roadmap doc |
| Dock icon appears briefly at launch | expected for `cargo run`/`cargo build` debug binaries — winit-style toolkits only suppress this for a *bundled* `.app` with `LSUIElement` set; ship one before judging this |
