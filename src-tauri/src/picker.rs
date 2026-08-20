//! The global-hotkey camera picker.
//!
//! Deliberately the one window in this app that **does** take keyboard focus. Everything
//! else is built around never interrupting the user; this window exists only because the
//! user just pressed a key asking for it, and it cannot read arrow keys without focus.
//! It is a separate window type from the popups precisely so that exception cannot leak
//! into them.

use crate::config::Config;
use crate::windows::Rect;
use anyhow::{anyhow, Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tracing::{debug, info, warn};

pub const LABEL: &str = "camera-picker";

/// Logical pixel geometry. Height is derived from the camera count so the window is
/// exactly as tall as its contents - no scrollbar, no empty space.
const WIDTH: f64 = 320.0;
const HEADER: f64 = 32.0;
const ROW: f64 = 34.0;
const PADDING: f64 = 12.0;
/// Gap between the picker and the corner of the work area.
const MARGIN: f64 = 8.0;

fn height_for(count: usize) -> f64 {
    HEADER + PADDING + (count.max(1) as f64 * ROW)
}

pub fn is_open<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(LABEL).is_some()
}

/// Closes the picker if it is open. Safe to call when it is not.
pub fn close<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        debug!("closing the camera picker");
        if let Err(e) = window.close() {
            warn!("could not close the camera picker: {e:#}");
        }
    }
}

/// Opens the picker, or closes it if already open. Bound to the global hotkey.
pub fn toggle<R: Runtime>(app: &AppHandle<R>, config: &Config) {
    if is_open(app) {
        close(app);
        return;
    }
    if let Err(e) = open(app, config) {
        warn!("could not open the camera picker: {e:#}");
    }
}

fn open<R: Runtime>(app: &AppHandle<R>, config: &Config) -> Result<()> {
    if config.cameras.is_empty() {
        return Err(anyhow!("no cameras configured"));
    }

    // The primary monitor, because that is where the notification area lives. The
    // picker should appear where a tray context menu would, not wherever the cursor is.
    let monitor = app
        .primary_monitor()
        .context("querying the primary monitor")?
        .ok_or_else(|| anyhow!("no monitors reported by the system"))?;

    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let area = Rect {
        x: work.position.x,
        y: work.position.y,
        w: work.size.width as i32,
        h: work.size.height as i32,
    };

    let logical_height = height_for(config.cameras.len());
    let size = PhysicalSize::new(
        (WIDTH * scale).round() as u32,
        (logical_height * scale).round() as u32,
    );

    // Bottom-right of the *work area*, which already excludes the taskbar - so this
    // lands exactly where a tray context menu opens, hugging the notification area.
    let margin = (MARGIN * scale).round() as i32;
    let position = PhysicalPosition::new(
        area.x + area.w - size.width as i32 - margin,
        area.y + area.h - size.height as i32 - margin,
    );

    info!(
        cameras = config.cameras.len(),
        x = position.x,
        y = position.y,
        "opening the camera picker"
    );

    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("picker.html".into()))
        .title("Show camera")
        .inner_size(WIDTH, logical_height)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(true)
        .visible(false)
        // The one deliberate exception to the no-focus rule. No WS_EX_NOACTIVATE here.
        .focused(true)
        .initialization_script(context_script(config))
        .build()
        .context("creating the camera picker window")?;

    window.set_size(size).context("sizing the picker")?;
    window
        .set_position(position)
        .context("positioning the picker")?;

    // Clicking away should dismiss it, the way any launcher does. But a blur arriving
    // *before* the window has ever held focus is not the user clicking away - it is the
    // window failing to reach the foreground, and closing on it makes the picker flash
    // and vanish. Only honour a blur once focus has genuinely been held at least once.
    let had_focus = Arc::new(AtomicBool::new(false));
    let handle = app.clone();
    let seen = had_focus.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(focused) = event {
            if *focused {
                seen.store(true, Ordering::Relaxed);
            } else if seen.load(Ordering::Relaxed) {
                debug!("camera picker lost focus; closing");
                close(&handle);
            } else {
                debug!("ignoring blur before the picker ever held focus");
            }
        }
    });

    window.show().context("showing the picker")?;
    let _ = window.set_focus();
    force_foreground(&window);
    Ok(())
}

/// Drags the picker to the foreground and gives it keyboard focus.
///
/// Windows refuses `SetForegroundWindow` from a process that did not just receive input,
/// and a global hotkey does not count - so the window appears without focus and the very
/// first key press goes to whatever was focused before. The standard workaround is to
/// attach this thread's input queue to the foreground window's thread for the duration
/// of the call, which makes Windows treat them as the same input context.
#[cfg(windows)]
fn force_foreground<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    let Ok(handle) = window.hwnd() else {
        warn!("could not retrieve the picker HWND; focus may not land");
        return;
    };
    let hwnd = HWND(handle.0);

    // SAFETY: `hwnd` is a live window owned by the Tauri runtime. Every call below is a
    // read or a focus change on it, and the thread attachment is undone on both paths.
    unsafe {
        let foreground = GetForegroundWindow();
        let other = GetWindowThreadProcessId(foreground, None);
        let current = GetCurrentThreadId();

        let attached =
            other != 0 && other != current && AttachThreadInput(other, current, true).as_bool();

        let _ = SetForegroundWindow(hwnd);
        let _ = SetActiveWindow(hwnd);
        let _ = SetFocus(Some(hwnd));

        if attached {
            let _ = AttachThreadInput(other, current, false);
        }
    }
}

#[cfg(not(windows))]
fn force_foreground<R: Runtime>(_window: &tauri::WebviewWindow<R>) {}

/// Injects the camera list for the picker page.
fn context_script(config: &Config) -> String {
    let cameras: Vec<_> = config
        .cameras
        .iter()
        .map(|c| serde_json::json!({ "name": c.name, "enabled": c.enabled }))
        .collect();
    let literal = serde_json::to_string(&serde_json::json!({ "cameras": cameras }))
        .unwrap_or_else(|_| String::from("{\"cameras\":[]}"));
    format!("window.__PICKER__={literal};")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{config_from, BASE_CONFIG};

    #[test]
    fn the_picker_hugs_the_tray_corner() {
        // 1920x1040 work area (40px taskbar), 5 cameras.
        let area = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1040,
        };
        let h = height_for(5);
        let x = area.x + area.w - WIDTH as i32 - MARGIN as i32;
        let y = area.y + area.h - h as i32 - MARGIN as i32;
        assert_eq!(x, 1920 - 320 - 8);
        assert_eq!(y, 1040 - h as i32 - 8);
        // Must sit inside the work area, i.e. above the taskbar.
        assert!(y + h as i32 <= area.h);
    }

    #[test]
    fn height_grows_with_the_camera_count() {
        assert!(height_for(5) > height_for(2));
        assert_eq!(height_for(3) - height_for(2), ROW);
    }

    #[test]
    fn an_empty_list_still_has_a_usable_height() {
        // Guards against a zero-height window if the config somehow has no cameras.
        assert!(height_for(0) >= HEADER + ROW);
    }

    #[test]
    fn the_context_carries_every_camera_with_its_enabled_state() {
        let config = config_from(BASE_CONFIG);
        let script = context_script(&config);
        assert!(script.starts_with("window.__PICKER__="));
        assert!(script.contains(r#""name":"front_doorbell""#));
        assert!(script.contains(r#""enabled":true"#));
        // Disabled cameras are listed too - asking for a feed by hand is a different
        // intent from wanting alerts from it.
        assert!(script.contains(r#""name":"indoor_garage""#));
        assert!(script.contains(r#""enabled":false"#));
    }
}
