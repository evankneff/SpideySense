//! The tray icon and its menu. The app has no main window, so this is the entire UI
//! apart from the popups themselves.

use crate::AppState;
use anyhow::{Context, Result};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_opener::OpenerExt;
use tracing::{error, info, warn};

pub const TRAY_ID: &str = "frigate-popup-tray";

const ICON_ACTIVE: &[u8] = include_bytes!("../icons/tray-active.png");
/// Desaturated variant, so do-not-disturb is visible at a glance in the tray.
const ICON_PAUSED: &[u8] = include_bytes!("../icons/tray-paused.png");

mod ids {
    pub const RELOAD: &str = "reload_config";
    pub const OPEN_CONFIG: &str = "open_config";
    pub const OPEN_LOG: &str = "open_log";
    pub const QUIT: &str = "quit";
    pub const PAUSE: &str = "pause";
    pub const AUTOSTART: &str = "autostart";
    /// Prefix for the per-camera "show me this feed" items.
    pub const CAMERA: &str = "camera:";
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> Result<TrayIcon<R>> {
    let menu = build_menu(app)?;

    let icon = Image::from_bytes(ICON_ACTIVE).context("decoding the tray icon")?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("frigate-popup")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event)
        .build(app)
        .context("creating the tray icon")?;

    Ok(tray)
}

/// Builds the menu from the *current* config.
///
/// Separate from `create` so a config reload can swap in a fresh menu - the camera
/// list is built from config, so a reload that did not rebuild it would leave the menu
/// describing cameras that no longer exist.
fn build_menu<R: Runtime>(app: &AppHandle<R>) -> Result<Menu<R>> {
    // One entry per configured camera, including disabled ones - asking for a feed by
    // hand is a different intent from wanting detections from it.
    let state = app.state::<AppState>();
    let config = state.config.load();
    let camera_items: Vec<MenuItem<R>> = config
        .cameras
        .iter()
        .map(|camera| {
            let title = camera.name.replace('_', " ");
            let title = if camera.enabled {
                title
            } else {
                format!("{title}  (alerts off)")
            };
            MenuItem::with_id(
                app,
                format!("{}{}", ids::CAMERA, camera.name),
                title,
                true,
                None::<&str>,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let camera_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = camera_items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<R>)
        .collect();
    let cameras = Submenu::with_items(app, "Show camera", true, &camera_refs)?;

    // Reflects the real registry state rather than an assumption, so the tick is right
    // even if launch-at-login was changed outside the app.
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);

    // Read the live value rather than assuming false: a rebuild after a reload must not
    // silently un-tick do-not-disturb while the app is still paused.
    let paused = match state.triggers.lock() {
        Ok(guard) => guard.is_paused(),
        Err(poisoned) => poisoned.into_inner().is_paused(),
    };

    let pause = CheckMenuItem::with_id(
        app,
        ids::PAUSE,
        "Pause notifications",
        true,
        paused,
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        ids::AUTOSTART,
        "Launch at login",
        true,
        autostart_on,
        None::<&str>,
    )?;

    let reload = MenuItem::with_id(app, ids::RELOAD, "Reload config", true, None::<&str>)?;
    let open_config = MenuItem::with_id(
        app,
        ids::OPEN_CONFIG,
        "Open config file",
        true,
        None::<&str>,
    )?;
    let open_log = MenuItem::with_id(app, ids::OPEN_LOG, "Open log file", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, ids::QUIT, "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &cameras,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &reload,
            &open_config,
            &open_log,
            &separator,
            &quit,
        ],
    )?;

    Ok(menu)
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        ids::RELOAD => reload_config(app),
        ids::OPEN_CONFIG => match crate::paths::config_path() {
            Ok(path) => reveal(app, &path.to_string_lossy()),
            Err(e) => error!("could not resolve the config path: {e:#}"),
        },
        ids::OPEN_LOG => match crate::paths::newest_log_file() {
            // Fall back to the directory when nothing has been written yet.
            Ok(Some(path)) => reveal(app, &path.to_string_lossy()),
            Ok(None) => match crate::paths::log_dir() {
                Ok(dir) => reveal(app, &dir.to_string_lossy()),
                Err(e) => error!("could not resolve the log directory: {e:#}"),
            },
            Err(e) => error!("could not find the newest log file: {e:#}"),
        },
        ids::QUIT => {
            info!("quit requested from the tray menu");
            app.exit(0);
        }
        ids::PAUSE => toggle_pause(app),
        ids::AUTOSTART => toggle_autostart(app),
        id if id.starts_with(ids::CAMERA) => {
            show_camera(app, &id[ids::CAMERA.len()..]);
        }
        other => error!("unhandled tray menu id: {other}"),
    }
}

/// Re-reads the config from disk and swaps it in, then rebuilds the menu from it.
///
/// Two settings are deliberately *not* applied. The MQTT client is already connected and
/// subscribed, and the hotkey is already registered with the OS; swapping those values
/// would leave the running app quietly disagreeing with the file it just read. Rather
/// than pretend, the reload names them and asks for a restart - a reload that lies about
/// what it applied is worse than one that does less.
///
/// A config that fails to parse or validate leaves the running one untouched.
fn reload_config<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let current = state.config.load();

    let next = match crate::config::Config::load() {
        Ok(next) => next,
        Err(e) => {
            error!("config reload failed: {e:#}");
            crate::ui::error(
                "frigate-popup",
                &format!(
                    "The config was not reloaded, so the running settings are unchanged.

{e:#}"
                ),
            );
            return;
        }
    };

    let deferred = crate::config::deferred_changes(&current, &next);

    let cameras = next.cameras.len();
    state.config.store(next);

    // The camera list is built from config, so the menu has to follow the swap.
    match build_menu(app) {
        Ok(menu) => match app.tray_by_id(TRAY_ID) {
            Some(tray) => {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    warn!("could not swap in the rebuilt tray menu: {e:#}");
                }
            }
            None => warn!("tray icon not found while rebuilding the menu"),
        },
        Err(e) => warn!("could not rebuild the tray menu after reload: {e:#}"),
    }

    info!(
        cameras,
        deferred = deferred.len(),
        "config reloaded from disk"
    );

    if !deferred.is_empty() {
        crate::ui::info(
            "frigate-popup",
            &format!(
                "Config reloaded.

{} changed, which needs a restart to take effect.                  Everything else is already live.",
                deferred.join(" and ")
            ),
        );
    }
}

/// Flips do-not-disturb and repaints the tray icon to match.
///
/// The icon is the only always-visible surface the app has, so a paused state that is not
/// reflected there is a state you forget you are in.
fn toggle_pause<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();

    let paused = {
        let mut triggers = match state.triggers.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("trigger state lock was poisoned; recovering");
                poisoned.into_inner()
            }
        };
        let next = !triggers.is_paused();
        triggers.set_paused(next);
        next
    };

    info!(paused, "notifications toggled from the tray");
    set_paused_icon(app, paused);
}

/// Repaints the tray icon and tooltip for the current do-not-disturb state.
pub fn set_paused_icon<R: Runtime>(app: &AppHandle<R>, paused: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        warn!("tray icon not found while updating the paused state");
        return;
    };

    let bytes = if paused { ICON_PAUSED } else { ICON_ACTIVE };
    match Image::from_bytes(bytes) {
        Ok(icon) => {
            if let Err(e) = tray.set_icon(Some(icon)) {
                warn!("could not update the tray icon: {e:#}");
            }
        }
        Err(e) => warn!("could not decode the tray icon: {e:#}"),
    }

    let tooltip = if paused {
        "frigate-popup - notifications paused"
    } else {
        "frigate-popup"
    };
    if let Err(e) = tray.set_tooltip(Some(tooltip)) {
        warn!("could not update the tray tooltip: {e:#}");
    }
}

/// Toggles launch-at-login, then re-reads the real state so the tick cannot drift from
/// what the registry actually says.
fn toggle_autostart<R: Runtime>(app: &AppHandle<R>) {
    let manager = app.autolaunch();
    let enabled = manager.is_enabled().unwrap_or(false);

    let result = if enabled {
        manager.disable()
    } else {
        manager.enable()
    };

    match result {
        Ok(()) => info!(enabled = !enabled, "launch at login toggled"),
        Err(e) => {
            error!("could not change launch at login: {e:#}");
            crate::ui::error(
                "frigate-popup",
                &format!(
                    "Could not change the launch-at-login setting.

{e:#}"
                ),
            );
        }
    }
}

/// Opens a feed on request from the tray.
///
/// Pinned, so it stays until dismissed - a popup you deliberately asked for should not
/// time out the way a detection alert does. It also bypasses the cooldown, which exists
/// to stop *detections* spamming windows, not to stop you looking at a camera.
fn show_camera<R: Runtime>(app: &AppHandle<R>, camera: &str) {
    let state = app.state::<AppState>();
    let config = state.config.load();
    let Some(config_camera) = config.camera(camera).cloned() else {
        error!(camera, "tray asked for a camera that is not in the config");
        return;
    };

    info!(camera, "opening a pinned popup from the tray");
    // A tray menu handler is an event handler, which has the same window-creation
    // deadlock on Windows as a synchronous command.
    crate::lifecycle::spawn_open_pinned(app.clone(), config, state.popups.clone(), config_camera);
}

fn reveal<R: Runtime>(app: &AppHandle<R>, path: &str) {
    if let Err(e) = app.opener().open_path(path, None::<&str>) {
        error!("could not open {path}: {e:#}");
    }
}
