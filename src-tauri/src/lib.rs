//! frigate-popup - borderless, always-on-top camera popups driven by Frigate NVR events.
//!
//! Tray-resident app: MQTT drives the trigger logic, which opens borderless popups.

mod cli;
mod commands;
pub mod config;
pub mod events;
mod lifecycle;
mod logging;
mod mqtt;
mod paths;
mod picker;
#[cfg(test)]
mod testutil;
mod tray;
mod ui;
mod windows;

use anyhow::{anyhow, Context, Result};
use config::{Camera, Config, ConfigSlot};
use events::Triggers;
use lifecycle::Popups;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent};
use tracing::{error, info, warn};

/// How often expired popups are swept. Fine enough that a close feels immediate, coarse
/// enough to be free; also means a machine waking from sleep tidies up within half a
/// second rather than leaving stale windows on screen.
const SWEEP_INTERVAL: Duration = Duration::from_millis(500);

/// Shared application state.
pub struct AppState {
    /// Swappable, so the tray's "Reload config" item can replace it at runtime.
    pub config: ConfigSlot,
    /// Cooldown bookkeeping and the do-not-disturb flag. Shared with the MQTT task.
    pub triggers: Arc<Mutex<Triggers>>,
    /// Which popups are open and when each one is due to close.
    pub popups: Arc<Mutex<Popups>>,
}

impl AppState {
    /// Locks the popup state, recovering rather than propagating a panic from a thread
    /// that died holding it.
    pub fn popups(&self) -> std::sync::MutexGuard<'_, Popups> {
        self.popups.lock().unwrap_or_else(|poisoned| {
            warn!("popup state lock was poisoned; recovering");
            poisoned.into_inner()
        })
    }
}

pub fn run() {
    let mode = match cli::parse(std::env::args()) {
        Ok(cli::Mode::Help) => {
            println!("{}", cli::USAGE);
            return;
        }
        Ok(mode) => mode,
        Err(message) => {
            // Release builds have no console, so the message box is the only way this
            // is ever seen.
            println!("{message}");
            ui::error("frigate-popup", &message);
            std::process::exit(2);
        }
    };

    // Logging first, so a config failure has somewhere to be recorded.
    let log_dir = match paths::log_dir() {
        Ok(dir) => dir,
        Err(e) => {
            ui::error(
                "frigate-popup could not start",
                &format!("Could not work out where to write logs.\n\n{e:#}"),
            );
            std::process::exit(1);
        }
    };

    let _log_guard = match logging::init(&log_dir) {
        Ok(guard) => guard,
        Err(e) => {
            ui::error(
                "frigate-popup could not start",
                &format!("Logging could not be initialised.\n\n{e:#}"),
            );
            std::process::exit(1);
        }
    };

    info!(
        version = env!("CARGO_PKG_VERSION"),
        portable = paths::is_portable(),
        log_dir = %log_dir.display(),
        "frigate-popup starting"
    );

    let config = match Config::load() {
        Ok(config) => Arc::new(config),
        Err(e) => {
            // A missing config is a first-run prompt, not a crash - the starter file
            // has just been written and the user needs to fill it in.
            let first_run = e
                .downcast_ref::<config::ConfigError>()
                .is_some_and(|e| matches!(e, config::ConfigError::Created(_)));
            let body = format!("{e:#}");
            if first_run {
                info!("first run: starter config written");
                ui::info("frigate-popup - first run", &body);
            } else {
                error!("configuration is not usable: {body}");
                ui::error("frigate-popup - configuration problem", &body);
            }
            std::process::exit(1);
        }
    };

    info!(
        cameras = config.cameras.len(),
        enabled = config.cameras.iter().filter(|c| c.enabled).count(),
        labels = ?config.detection.labels,
        broker = %format!("{}:{}", config.mqtt.host, config.mqtt.port),
        "configuration loaded"
    );

    if let Err(e) = start(config, mode) {
        error!("fatal: {e:#}");
        ui::error("frigate-popup stopped", &format!("{e:#}"));
        std::process::exit(1);
    }
}

/// Cameras a `--preview` run should open, in stack order.
///
/// An explicit name wins over `enabled = false`, since previewing a camera you have
/// switched off is exactly how you decide whether to switch it back on.
fn preview_targets<'a>(config: &'a Config, requested: &[String]) -> Result<Vec<&'a Camera>> {
    if requested.is_empty() {
        let defaults: Vec<_> = config
            .cameras
            .iter()
            .filter(|c| c.enabled)
            .take(config.popup.max_popups)
            .collect();
        if defaults.is_empty() {
            return Err(anyhow!(
                "--preview was given no camera names and no cameras are enabled in the config"
            ));
        }
        return Ok(defaults);
    }

    requested
        .iter()
        .map(|name| {
            config.camera(name).ok_or_else(|| {
                let known: Vec<_> = config.cameras.iter().map(|c| c.name.as_str()).collect();
                anyhow!(
                    "--preview names camera `{name}`, which is not in the config. Known cameras: {}",
                    known.join(", ")
                )
            })
        })
        .collect()
}

/// Registers the global hotkey that toggles the camera picker.
///
/// Registration fails if another application already owns the combination, which is why
/// it is reported rather than propagated - the app is still useful without it.
fn register_hotkey<R: tauri::Runtime>(app: &tauri::AppHandle<R>, config: ConfigSlot) -> Result<()> {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

    let snapshot = config.load();
    if !snapshot.hotkey.enabled {
        info!("camera picker hotkey disabled in config");
        return Ok(());
    }

    let binding = snapshot.hotkey.binding.trim().to_string();
    let shortcut = Shortcut::from_str(&binding)
        .with_context(|| format!("`{binding}` is not a valid shortcut"))?;

    // The slot, not a snapshot: a reloaded camera list shows up in the picker without
    // a restart, even though rebinding the key itself still needs one.
    let for_handler = config.clone();
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, _shortcut, event| {
                // Pressed only. Acting on Released too would toggle twice per keypress.
                if event.state() == ShortcutState::Pressed {
                    picker::hotkey(app, &for_handler.load());
                }
            })
            .build(),
    )
    .context("registering the global shortcut plugin")?;

    app.global_shortcut()
        .register(shortcut)
        .with_context(|| format!("registering `{binding}` (is another app using it?)"))?;

    info!(binding = %binding, "camera picker hotkey registered");
    Ok(())
}

/// Closes expired popups on a fixed tick.
///
/// A single sweeper rather than a timer per popup: extending a deadline is then just a
/// field write, with no task to cancel and reschedule, and a machine resuming from sleep
/// naturally finds everything already past its deadline and cleans up in one pass.
fn spawn_sweeper<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    config: ConfigSlot,
    popups: Arc<Mutex<Popups>>,
) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            ticker.tick().await;

            // The plan is computed under the lock and applied outside it, so a window
            // call can never block while another thread is waiting on popup state.
            let plan = {
                let mut guard = match popups.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("popup state lock was poisoned; recovering");
                        poisoned.into_inner()
                    }
                };
                lifecycle::plan(&mut guard, Instant::now())
            };

            if !plan.is_empty() {
                lifecycle::apply(&app, &config.load(), &plan);
            }
        }
    });
}

fn start(config: Arc<Config>, mode: cli::Mode) -> Result<()> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            // Desktop-only launcher; the argument is ignored on Windows but the plugin
            // requires it for the macOS code path.
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::popup_open_ui,
            commands::popup_dismiss,
            commands::popup_hover,
            commands::picker_choose,
            commands::picker_close,
        ])
        .setup(move |app| {
            let triggers = Arc::new(Mutex::new(Triggers::new()));
            let popups = Arc::new(Mutex::new(Popups::new()));
            // Everything long-lived reads through the slot, so the tray's reload is
            // visible to the MQTT task and the sweeper without restarting either.
            let slot = ConfigSlot::new(config.clone());
            app.manage(AppState {
                config: slot.clone(),
                triggers: triggers.clone(),
                popups: popups.clone(),
            });
            tray::create(app.handle())?;
            info!("tray icon ready");

            if let Err(e) = register_hotkey(app.handle(), slot.clone()) {
                // A hotkey conflict must not stop the app; detections still work.
                error!("camera picker hotkey unavailable: {e:#}");
            }

            let context = || mqtt::Context {
                app: app.handle().clone(),
                config: slot.clone(),
                triggers: triggers.clone(),
                popups: popups.clone(),
            };

            match &mode {
                cli::Mode::Preview { cameras } => {
                    // Preview is about how the windows look, so it stays off MQTT and
                    // out of the lifecycle - the popups are meant to stay put.
                    let targets = preview_targets(&config, cameras)?;
                    info!(count = targets.len(), "preview mode");
                    for (slot, camera) in targets.iter().enumerate() {
                        // Preview is a hand-opened look at the feed, not an alert.
                        windows::open(app.handle(), &config, camera, slot, windows::Kind::Live)?;
                    }
                }
                cli::Mode::Simulate { camera, label } => {
                    info!(%camera, %label, "injecting a simulated detection");
                    let ctx = context();
                    mqtt::dispatch(&ctx, &mqtt::simulated_event(camera, label));
                    spawn_sweeper(app.handle().clone(), slot.clone(), popups.clone());
                    tauri::async_runtime::spawn(mqtt::run(ctx));
                }
                cli::Mode::Normal => {
                    spawn_sweeper(app.handle().clone(), slot.clone(), popups.clone());
                    tauri::async_runtime::spawn(mqtt::run(context()));
                }
                cli::Mode::Help => {}
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .context("building the Tauri application")?;

    app.run(|_app, event| {
        // The app lives in the tray. Popups closing must never take the process with
        // them, so the only way out is the tray's Quit item calling app.exit().
        if let RunEvent::ExitRequested { api, code, .. } = &event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });

    Ok(())
}
