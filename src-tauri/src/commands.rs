//! Commands the popup page invokes.
//!
//! All three are fire-and-forget from the page's point of view: it never waits on a
//! result, so a failure here degrades the interaction rather than breaking the popup.

use crate::{lifecycle, picker, windows, AppState};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_opener::OpenerExt;
use tracing::{debug, error, info, warn};

/// Opens this camera in the Frigate web UI, in the default browser.
#[tauri::command]
pub fn popup_open_ui<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, camera: String) {
    let url = state.config.load().camera_ui_url(&camera);
    info!(%camera, %url, "opening the Frigate UI");
    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
        warn!(%camera, "could not open {url}: {e:#}");
    }
}

/// Closes the popup immediately, from its own close button.
#[tauri::command]
pub fn popup_dismiss<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, camera: String) {
    info!(%camera, "popup dismissed from its close button");
    dismiss(&app, &state, &camera);
}

/// Closes a popup the way its own close button would, from somewhere that has only an
/// `AppHandle` - the global hotkey, for instance.
pub fn dismiss_pinned<R: Runtime>(app: &AppHandle<R>, camera: &str) {
    let state = app.state::<AppState>();
    dismiss(app, &state, camera);
}

fn dismiss<R: Runtime>(app: &AppHandle<R>, state: &State<'_, AppState>, camera: &str) {
    // Forget it first: if the window close raced the sweeper, the sweeper would
    // otherwise try to restack a popup that no longer exists.
    {
        let mut popups = state.popups();
        popups.forget(camera);
    }

    if let Some(window) = app.get_webview_window(&windows::label_for(camera)) {
        if let Err(e) = window.close() {
            warn!(%camera, "could not close the popup: {e:#}");
        }
    }
    restack(app, state);
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
    // One snapshot for the whole restack, so every window is placed by the same rules.
    let config = state.config.load();
    for (camera, slot) in slots {
        if let Err(e) = windows::move_to_slot(app, &config, &camera, slot) {
            warn!(camera, slot, "could not restack after dismissal: {e:#}");
        }
    }
}

/// Opens the chosen camera as a pinned live view and dismisses the picker.
#[tauri::command]
pub fn picker_choose<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, camera: String) {
    // Close first: the picker holds focus, and opening the popup underneath it would
    // leave the picker covering the thing the user just asked to see.
    picker::close(&app);

    let config = state.config.load();
    let Some(config_camera) = config.camera(&camera).cloned() else {
        error!(%camera, "picker chose a camera that is not in the config");
        return;
    };

    info!(%camera, "opening a pinned popup from the picker");
    // Off this thread: creating a window inside a synchronous command deadlocks on
    // Windows, which froze the whole app.
    lifecycle::spawn_open_pinned(app.clone(), config, state.popups.clone(), config_camera);
}

/// Dismisses the picker without choosing anything.
#[tauri::command]
pub fn picker_close<R: Runtime>(app: AppHandle<R>) {
    picker::close(&app);
}
