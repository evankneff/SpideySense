//! Commands the popup page invokes.
//!
//! All three are fire-and-forget from the page's point of view: it never waits on a
//! result, so a failure here degrades the interaction rather than breaking the popup.

use crate::windows;
use crate::AppState;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_opener::OpenerExt;
use tracing::{debug, info, warn};

/// Opens this camera in the Frigate web UI, in the default browser.
#[tauri::command]
pub fn popup_open_ui<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, camera: String) {
    let url = state.config.camera_ui_url(&camera);
    info!(%camera, %url, "opening the Frigate UI");
    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
        warn!(%camera, "could not open {url}: {e:#}");
    }
}

/// Closes the popup immediately, from its own close button.
#[tauri::command]
pub fn popup_dismiss<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, camera: String) {
    info!(%camera, "popup dismissed from its close button");

    // Forget it first: if the window close raced the sweeper, the sweeper would
    // otherwise try to restack a popup that no longer exists.
    {
        let mut popups = state.popups();
        popups.forget(&camera);
    }

    if let Some(window) = app.get_webview_window(&windows::label_for(&camera)) {
        if let Err(e) = window.close() {
            warn!(%camera, "could not close the popup: {e:#}");
        }
    }
    restack(&app, &state);
}

/// Pauses or resumes the auto-close while the pointer is over the popup.
#[tauri::command]
pub fn popup_hover(state: State<'_, AppState>, camera: String, hovered: bool) {
    debug!(%camera, hovered, "popup hover");
    state.popups().set_hovered(&camera, hovered);
}

/// Closes up the stack after a manual dismissal.
fn restack<R: Runtime>(app: &AppHandle<R>, state: &State<'_, AppState>) {
    let slots = state.popups().slots();
    for (camera, slot) in slots {
        if let Err(e) = windows::move_to_slot(app, &state.config, &camera, slot) {
            warn!(camera, slot, "could not restack after dismissal: {e:#}");
        }
    }
}
