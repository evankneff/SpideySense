//! The macOS half of `windows.rs::suppress_activation`.
//!
//! `WS_EX_NOACTIVATE` on Windows is two independent things on macOS, at two different
//! scopes:
//!
//! 1. **"never becomes the keyboard-focus window."** `.focusable(false)` on the
//!    `WebviewWindowBuilder` (see `windows.rs::open`) is the whole answer to this half —
//!    tao implements it as `-canBecomeKeyWindow` reading back an ivar the flag sets. No
//!    unsafe code, no crate, and it maps to `WS_EX_NOACTIVATE` on Windows for free.
//! 2. **"a click on it does not activate the process."** There is no flag for this.
//!    AppKit grants it to exactly one kind of window: an `NSPanel` whose style mask
//!    contains `NSWindowStyleMaskNonactivatingPanel`. Setting that bit on a plain
//!    `NSWindow` does not merely do nothing — it does not stick. No error, no log, no
//!    exception; `setStyleMask:` accepts it and `styleMask` reads back without it. See
//!    the macOS window spike's `FINDINGS.md` §1 for the measurement.
//!
//! [`convert_to_nonactivating_panel`] buys (2), by changing the class of the live
//! window. `WS_EX_TOOLWINDOW` (no Dock icon, no Cmd-Tab entry) has no per-window
//! equivalent either — it is `NSApplicationActivationPolicy::Accessory`, set once for the
//! whole app in `lib.rs`, next to `app.build()`.

use std::ffi::c_void;

use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSPanel, NSResponder, NSWindow, NSWindowCollectionBehavior, NSWindowLevel,
    NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::NSObject;
use tauri::{Runtime, WebviewWindow};
use tracing::warn;

/// `NSStatusWindowLevel`. Where the menu-bar extras live, and the closest macOS
/// equivalent of `HWND_TOPMOST` for a notification-shaped popup.
const POPUP_LEVEL: NSWindowLevel = 25;

/// Added to whatever tao put in the mask, never replacing it — the same shape as the
/// Windows `current | WS_EX_NOACTIVATE`.
const WANTED_MASK: NSWindowStyleMask = NSWindowStyleMask::NonactivatingPanel;

/// Spaces and full-screen behaviour: follow the user between Spaces, be allowed over
/// another app's full-screen window, do not slide with Mission Control, stay out of
/// Cmd-` cycling.
const WANTED_BEHAVIOR: NSWindowCollectionBehavior = NSWindowCollectionBehavior(
    NSWindowCollectionBehavior::CanJoinAllSpaces.0
        | NSWindowCollectionBehavior::FullScreenAuxiliary.0
        | NSWindowCollectionBehavior::Stationary.0
        | NSWindowCollectionBehavior::IgnoresCycle.0,
);

define_class!(
    /// An `NSPanel` that refuses to become key or main, whatever anyone asks of it.
    ///
    /// Adds **no instance variables**, which is the precondition
    /// [`convert_to_nonactivating_panel`] relies on.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SpideySenseNonactivatingPanel"]
    #[derive(Debug)]
    struct NonactivatingPanel;

    impl NonactivatingPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            false
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

/// Borrow the `NSWindow` behind a Tauri window. Tauri hands back the window itself
/// rather than the content view, so this is a cast and nothing more.
///
/// # Safety
/// Must be called on the main thread — `mtm` is the witness. The returned reference
/// borrows `window`; the `NSWindow` is owned by the Tauri runtime and outlives it.
unsafe fn ns_window<R: Runtime>(window: &WebviewWindow<R>, _mtm: MainThreadMarker) -> Option<&NSWindow> {
    let ptr: *mut c_void = window.ns_window().ok()?;
    (!ptr.is_null()).then(|| unsafe { &*ptr.cast::<NSWindow>() })
}

/// Change the window's class so AppKit will treat it as a non-activating panel.
///
/// Call once, **before the window is ever shown** — the same ordering constraint this
/// crate already documents on Windows in `windows.rs::open` ("must happen before the
/// window is shown, or the first show still activates it").
///
/// # Costs, on tao specifically
///
/// tao's window class (`TaoWindow`) declares an ivar (`focusable`, backing
/// `-canBecomeKeyWindow`/`-canBecomeMainWindow`) and a method (`-sendEvent:`, which makes
/// `with_movable_by_window_background` work) that this swap discards. Our override
/// answers the focus question the same way, so behaviour survives; storage does not —
/// **never call `WebviewWindow::set_focusable` after this**, it reads that ivar and will
/// throw. Dragging a borderless window by its background also stops working, which is
/// irrelevant for a popup pinned to a screen corner. See the macOS window spike's
/// `FINDINGS.md` §3.3/§4.5 for the full accounting, including why this bypasses
/// `objc2`'s own `AnyObject::set_class` (its size-equality debug-assert is stricter than
/// the actual safety requirement, which only needs the new class to be no larger).
pub fn convert_to_nonactivating_panel<R: Runtime>(window: &WebviewWindow<R>, mtm: MainThreadMarker) -> bool {
    // SAFETY: `object_setClass` is a plain runtime function; the preconditions are
    // argued above and the size one is enforced immediately below.
    unsafe extern "C" {
        fn object_setClass(obj: *mut AnyObject, cls: *const AnyClass) -> *const AnyClass;
    }

    let Some(ns_window) = (unsafe { ns_window(window, mtm) }) else {
        warn!("no NSWindow behind the Tauri window; leaving it alone");
        return false;
    };

    let panel = NonactivatingPanel::class();
    let current = ns_window.class();
    if panel.instance_size() > current.instance_size() {
        warn!(
            new = %panel.name().to_string_lossy(),
            new_bytes = panel.instance_size(),
            old = %current.name().to_string_lossy(),
            old_bytes = current.instance_size(),
            "refusing the class swap: AppKit's window layout changed"
        );
        return false;
    }

    let object: *const AnyObject = (ns_window as *const NSWindow).cast();
    // SAFETY: see the doc comment. `panel` outlives the process and the size
    // precondition was just checked.
    unsafe { object_setClass(object.cast_mut(), panel) };

    // Properties tao never rewrites, so they are set once here.
    unsafe {
        // Without this the popup vanishes whenever the user clicks another app — which,
        // for a window that deliberately never activates, is always.
        ns_window.setHidesOnDeactivate(false);
        ns_window.setExcludedFromWindowsMenu(true);

        // `NSPanel`-only, and legal now that the class swap has happened.
        let as_panel: &NSPanel = &*(ns_window as *const NSWindow).cast::<NSPanel>();
        as_panel.setFloatingPanel(true);
        as_panel.setBecomesKeyOnlyIfNeeded(true);
    }

    enforce(window, mtm);
    true
}

/// Re-assert the properties tao rebuilds behind our back. Returns `true` if it had to
/// correct one.
///
/// The macOS counterpart of `MonitorApp::enforce_styles` (in actions-monitor) /
/// `win::configure` — tao's `set_style_mask_{sync,async}` **replaces** the mask and is
/// reached from `set_decorations`, `set_resizable`, `set_maximized`, `set_fullscreen`,
/// `set_simple_fullscreen` and `set_minimizable`; `setLevel:` is likewise overwritten by
/// `set_always_on_top` and both fullscreen paths. Unlike the Windows code this does not
/// need to run every tick — a popup that is only ever shown, hidden and moved touches
/// none of those setters (measured: zero corrections across every converted-window run
/// in the spike). Called once right after the swap; safe to call again after anything
/// that touches decorations/resizability/always-on-top/fullscreen.
pub fn enforce<R: Runtime>(window: &WebviewWindow<R>, mtm: MainThreadMarker) -> bool {
    let Some(ns_window) = (unsafe { ns_window(window, mtm) }) else {
        return false;
    };
    let mut corrected = false;

    let mask = ns_window.styleMask();
    if !mask.contains(WANTED_MASK) {
        ns_window.setStyleMask(mask | WANTED_MASK);
        corrected = true;
    }
    if ns_window.level() != POPUP_LEVEL {
        ns_window.setLevel(POPUP_LEVEL);
        corrected = true;
    }
    if ns_window.collectionBehavior() != WANTED_BEHAVIOR {
        ns_window.setCollectionBehavior(WANTED_BEHAVIOR);
        corrected = true;
    }
    corrected
}

/// Show without taking focus — the counterpart of `SW_SHOWNOACTIVATE`.
///
/// **Not `WebviewWindow::show()`.** That reaches tao's `set_visible(true)`, which is
/// `makeKeyAndOrderFront:` — harmless on a panel that cannot become key, but the wrong
/// call to reach for. `orderFront:` is worse: it is a no-op while the application is
/// inactive, which for this app is always, so the popup would simply never appear.
/// "Regardless" means "regardless of whether the app is active".
pub fn show<R: Runtime>(window: &WebviewWindow<R>, mtm: MainThreadMarker) {
    if let Some(ns_window) = unsafe { ns_window(window, mtm) } {
        ns_window.orderFrontRegardless();
    }
}

/// Everything an automated check can observe about focus configuration, in one line.
///
/// This is *configuration*, not *behaviour* — see this crate's macOS runbook. It cannot
/// observe whether a click actually failed to steal focus; it can only confirm the class
/// swap and style bits took effect, which is the thing AppKit silently refuses to do on
/// a plain `NSWindow`.
pub fn diagnostics<R: Runtime>(window: &WebviewWindow<R>, mtm: MainThreadMarker) -> String {
    let app = NSApplication::sharedApplication(mtm);
    let frontmost = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|a| a.localizedName())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "<unknown>".into());

    let Some(ns_window) = (unsafe { ns_window(window, mtm) }) else {
        return format!("<no NSWindow> app_active={} frontmost={frontmost:?}", app.isActive());
    };

    format!(
        "class={} mask={:#06x} level={} key={} app_active={} policy={:?} frontmost={frontmost:?}",
        ns_window.class().name().to_string_lossy(),
        ns_window.styleMask().0,
        ns_window.level(),
        ns_window.isKeyWindow(),
        app.isActive(),
        app.activationPolicy(),
    )
}
