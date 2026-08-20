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

    // Centred on the monitor under the cursor, not the primary one: on a multi-monitor
    // desk the picker should appear where the user is already looking.
    let monitor = match app.cursor_position() {
        Ok(pos) => app.monitor_from_point(pos.x, pos.y).ok().flatten(),
        Err(_) => None,
    };
    let monitor = match monitor {
        Some(m) => m,
        None => app
            .primary_monitor()
            .context("querying the primary monitor")?
            .ok_or_else(|| anyhow!("no monitors reported by the system"))?,
    };

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

    // Slightly above centre: a launcher sitting dead centre feels low on screen.
    let position = PhysicalPosition::new(
        area.x + (area.w - size.width as i32) / 2,
        area.y + (area.h - size.height as i32) / 3,
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

    // Clicking away should dismiss it, the way any launcher does. Without this the
    // window would sit there always-on-top with no obvious way to get rid of it.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            debug!("camera picker lost focus; closing");
            close(&handle);
        }
    });

    window.show().context("showing the picker")?;
    let _ = window.set_focus();
    Ok(())
}

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
