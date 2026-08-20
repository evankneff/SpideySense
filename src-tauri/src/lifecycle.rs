//! Popup lifetimes: when a window opens, how long it stays, and when it closes.
//!
//! The timing rules are a pure function of (opened_at, signal, config, now), so every
//! rule below is unit-tested without a window, a clock or a broker.
//!
//! A popup lives as long as its camera is doing something. `new` opens it, `update`
//! pushes the deadline out, and `end` — or the object going stationary — starts a linger
//! countdown. Two bounds sit on top: it never closes before `min_display_seconds`, and
//! never survives past `max_display_seconds`.

use crate::config::{Camera, Config, Popup};
use crate::windows;
use anyhow::Result;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime};
use tracing::{debug, info, warn};

/// What just happened to the object a popup is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The popup just opened.
    Opened,
    /// The object moved. Frigate is still tracking it.
    Active,
    /// Frigate still tracks it but it has stopped moving.
    Stationary,
    /// Frigate finished the event.
    Ended,
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub opened_at: Instant,
    pub deadline: Instant,
}

impl Timing {
    pub fn new(cfg: &Popup, now: Instant) -> Self {
        let mut timing = Self {
            opened_at: now,
            deadline: now,
        };
        timing.apply(Signal::Opened, cfg, now);
        timing
    }

    /// Recomputes the deadline for `signal`, then clamps it to the floor and ceiling.
    ///
    /// Clamping is what makes the rules safe to combine: the floor stops an immediate
    /// `end` from making the popup blink, and the ceiling stops an object that never
    /// stops generating updates from pinning the window forever.
    pub fn apply(&mut self, signal: Signal, cfg: &Popup, now: Instant) {
        let secs = Duration::from_secs;

        let proposed = match signal {
            // A fresh popup gets a full watchdog window; the first `update` will extend it.
            Signal::Opened | Signal::Active => now + secs(cfg.watchdog_seconds),
            Signal::Stationary | Signal::Ended => now + secs(cfg.linger_seconds),
        };

        let floor = self.opened_at + secs(cfg.min_display_seconds);
        let ceiling = self.opened_at + secs(cfg.max_display_seconds);

        // An `end` must never *extend* a popup, so a linger only ever pulls the deadline
        // in. Activity, on the other hand, is allowed to push it out.
        let proposed = match signal {
            Signal::Stationary | Signal::Ended => proposed.min(self.deadline),
            Signal::Opened | Signal::Active => proposed,
        };

        self.deadline = proposed.max(floor).min(ceiling);
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
}

/// One popup currently on screen.
#[derive(Debug)]
struct Entry {
    camera: String,
    timing: Timing,
    /// Suppresses expiry while the pointer is over the window.
    hovered: bool,
    /// Opened deliberately from the tray rather than by a detection, so it stays until
    /// it is dismissed rather than timing out.
    pinned: bool,
}

/// Tracks which popups are open and in which stack slot.
#[derive(Debug, Default)]
pub struct Popups {
    /// Oldest first. Index is the stack slot, so slot 0 always hugs the corner.
    entries: Vec<Entry>,
}

impl Popups {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn has(&self, camera: &str) -> bool {
        self.entries.iter().any(|e| e.camera == camera)
    }

    fn index_of(&self, camera: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.camera == camera)
    }

    /// Opens a popup for `camera`, or refreshes the existing one.
    ///
    /// At `max_popups`, the oldest is closed to make room — a new detection is more
    /// interesting than one that has been sitting there for a while.
    pub fn open<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        config: &Config,
        camera: &Camera,
        now: Instant,
    ) -> Result<()> {
        self.open_inner(app, config, camera, now, false)
    }

    /// Opens a popup that never times out, for the tray's camera picker.
    pub fn open_pinned<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        config: &Config,
        camera: &Camera,
        now: Instant,
    ) -> Result<()> {
        self.open_inner(app, config, camera, now, true)
    }

    fn open_inner<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        config: &Config,
        camera: &Camera,
        now: Instant,
        pinned: bool,
    ) -> Result<()> {
        if let Some(i) = self.index_of(&camera.name) {
            debug!(camera = %camera.name, "popup already open; extending instead");
            self.entries[i]
                .timing
                .apply(Signal::Active, &config.popup, now);
            // Asking for it from the tray pins one that was opened by a detection.
            self.entries[i].pinned |= pinned;
            return Ok(());
        }

        let evicted = self.make_room(config.popup.max_popups);
        for camera in &evicted {
            info!(camera = %camera, "closing the oldest popup to make room");
            close_window(app, camera);
        }
        // Survivors have to slide down into the freed slots before the new popup takes
        // the next one, or it opens on top of a window that never moved.
        if !evicted.is_empty() {
            for (camera, slot) in self.slots() {
                if let Err(e) = windows::move_to_slot(app, config, &camera, slot) {
                    warn!(camera, slot, "could not restack after eviction: {e:#}");
                }
            }
        }

        let slot = self.entries.len();
        let kind = if pinned {
            windows::Kind::Live
        } else {
            windows::Kind::Detection
        };
        windows::open(app, config, camera, slot, kind)?;
        self.entries.push(Entry {
            camera: camera.name.clone(),
            timing: Timing::new(&config.popup, now),
            hovered: false,
            pinned,
        });
        Ok(())
    }

    /// Drops the oldest popups until there is room for one more, returning what was
    /// evicted. Pure bookkeeping - the caller closes the windows and restacks.
    fn make_room(&mut self, max: usize) -> Vec<String> {
        let mut evicted = Vec::new();
        while self.entries.len() + 1 > max.max(1) {
            let oldest = self.entries.remove(0);
            evicted.push(oldest.camera);
        }
        evicted
    }

    /// Feeds an event signal to the popup for `camera`, if one is open.
    pub fn note(&mut self, camera: &str, signal: Signal, cfg: &Popup, now: Instant) {
        if let Some(i) = self.index_of(camera) {
            self.entries[i].timing.apply(signal, cfg, now);
        }
    }

    pub fn set_hovered(&mut self, camera: &str, hovered: bool) {
        if let Some(i) = self.index_of(camera) {
            self.entries[i].hovered = hovered;
        }
    }

    /// Cameras whose popups are due to close. Hovered popups are held back.
    pub fn expired(&self, now: Instant) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| !e.hovered && !e.pinned && e.timing.is_expired(now))
            .map(|e| e.camera.clone())
            .collect()
    }

    /// Drops `camera` from the stack and returns whether anything was removed.
    pub fn forget(&mut self, camera: &str) -> bool {
        match self.index_of(camera) {
            Some(i) => {
                self.entries.remove(i);
                true
            }
            None => false,
        }
    }

    /// Slot each remaining popup should now occupy, after something closed above it.
    pub fn slots(&self) -> Vec<(String, usize)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.camera.clone(), i))
            .collect()
    }
}

fn close_window<R: Runtime>(app: &AppHandle<R>, camera: &str) {
    let label = windows::label_for(camera);
    if let Some(window) = app.get_webview_window(&label) {
        if let Err(e) = window.close() {
            warn!(camera, "could not close the popup: {e:#}");
        }
    }
}

/// What a sweep decided to do. Computed under the lock, applied without it, so a window
/// call can never block while the popup state is held.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub close: Vec<String>,
    pub restack: Vec<(String, usize)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.close.is_empty() && self.restack.is_empty()
    }
}

/// Decides which popups have expired and where the survivors belong.
pub fn plan(popups: &mut Popups, now: Instant) -> Plan {
    let close = popups.expired(now);
    if close.is_empty() {
        return Plan::default();
    }
    for camera in &close {
        popups.forget(camera);
    }
    Plan {
        close,
        restack: popups.slots(),
    }
}

pub fn apply<R: Runtime>(app: &AppHandle<R>, config: &Config, plan: &Plan) {
    for camera in &plan.close {
        info!(camera = %camera, "popup expired; closing");
        close_window(app, camera);
    }
    for (camera, slot) in &plan.restack {
        if let Err(e) = windows::move_to_slot(app, config, camera, *slot) {
            warn!(camera, slot, "could not restack the popup: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Popup {
        Popup {
            min_display_seconds: 8,
            linger_seconds: 15,
            watchdog_seconds: 20,
            max_display_seconds: 120,
            ..Popup::default()
        }
    }

    fn secs(t: Instant, base: Instant) -> u64 {
        t.saturating_duration_since(base).as_secs()
    }

    #[test]
    fn a_new_popup_gets_the_full_watchdog_window() {
        let t0 = Instant::now();
        let timing = Timing::new(&cfg(), t0);
        assert_eq!(secs(timing.deadline, t0), 20);
    }

    #[test]
    fn activity_pushes_the_deadline_out() {
        let t0 = Instant::now();
        let mut timing = Timing::new(&cfg(), t0);

        timing.apply(Signal::Active, &cfg(), t0 + Duration::from_secs(10));
        assert_eq!(secs(timing.deadline, t0), 30, "10s in, plus a 20s watchdog");
    }

    #[test]
    fn an_end_event_pulls_the_deadline_in_to_the_linger() {
        // Realistic shape: updates keep the popup alive, then the object leaves.
        let t0 = Instant::now();
        let mut timing = Timing::new(&cfg(), t0);

        timing.apply(Signal::Active, &cfg(), t0 + Duration::from_secs(30));
        assert_eq!(secs(timing.deadline, t0), 50, "30s in, plus a 20s watchdog");

        timing.apply(Signal::Ended, &cfg(), t0 + Duration::from_secs(31));
        assert_eq!(
            secs(timing.deadline, t0),
            46,
            "the 15s linger is shorter than the watchdog, so it pulls the close in"
        );
    }

    #[test]
    fn going_stationary_behaves_like_ending() {
        let t0 = Instant::now();
        let mut a = Timing::new(&cfg(), t0);
        let mut b = Timing::new(&cfg(), t0);
        let at = t0 + Duration::from_secs(30);

        a.apply(Signal::Stationary, &cfg(), at);
        b.apply(Signal::Ended, &cfg(), at);
        assert_eq!(a.deadline, b.deadline);
    }

    #[test]
    fn an_immediate_end_lingers_rather_than_blinking() {
        // The blink case: `end` arrives half a second after the popup opened. With the
        // default 15s linger it stays up ~15s, which is the whole point of the linger.
        let t0 = Instant::now();
        let mut timing = Timing::new(&cfg(), t0);

        timing.apply(Signal::Ended, &cfg(), t0 + Duration::from_millis(500));
        assert_eq!(secs(timing.deadline, t0), 15);
        assert!(!timing.is_expired(t0 + Duration::from_secs(15)));
    }

    #[test]
    fn the_minimum_display_time_is_a_floor_under_a_short_linger() {
        // min_display only bites when linger is configured below it.
        let short = Popup {
            linger_seconds: 2,
            ..cfg()
        };
        let t0 = Instant::now();
        let mut timing = Timing::new(&short, t0);

        timing.apply(Signal::Ended, &short, t0 + Duration::from_millis(500));
        assert_eq!(
            secs(timing.deadline, t0),
            8,
            "a 2s linger would close at 2.5s; the floor holds it to 8s"
        );
        assert!(!timing.is_expired(t0 + Duration::from_secs(7)));
        assert!(timing.is_expired(t0 + Duration::from_secs(8)));
    }

    #[test]
    fn an_end_never_extends_a_popup_that_was_about_to_close() {
        // Deadline is already only 2s away; a 15s linger must not resurrect it.
        let t0 = Instant::now();
        let mut timing = Timing::new(&cfg(), t0);
        timing.deadline = t0 + Duration::from_secs(12);

        timing.apply(Signal::Ended, &cfg(), t0 + Duration::from_secs(10));
        assert_eq!(
            secs(timing.deadline, t0),
            12,
            "linger pulls in, never pushes out"
        );
    }

    #[test]
    fn constant_activity_cannot_exceed_the_hard_ceiling() {
        let t0 = Instant::now();
        let mut timing = Timing::new(&cfg(), t0);

        // Someone loitering: an update every 5s for ten minutes.
        for i in 1..=120 {
            timing.apply(Signal::Active, &cfg(), t0 + Duration::from_secs(i * 5));
        }
        assert_eq!(
            secs(timing.deadline, t0),
            120,
            "capped at max_display_seconds"
        );
    }

    #[test]
    fn expiry_is_inclusive_of_the_deadline() {
        let t0 = Instant::now();
        let timing = Timing::new(&cfg(), t0);
        assert!(!timing.is_expired(t0 + Duration::from_secs(19)));
        assert!(timing.is_expired(t0 + Duration::from_secs(20)));
    }

    #[test]
    fn hovered_popups_are_held_back_from_expiry() {
        let t0 = Instant::now();
        let mut popups = Popups::new();
        popups.entries.push(Entry {
            camera: "front_doorbell".into(),
            timing: Timing::new(&cfg(), t0),
            hovered: false,
            pinned: false,
        });

        let later = t0 + Duration::from_secs(60);
        assert_eq!(popups.expired(later), vec!["front_doorbell".to_string()]);

        popups.set_hovered("front_doorbell", true);
        assert!(
            popups.expired(later).is_empty(),
            "hovering must pause the auto-close"
        );
    }

    #[test]
    fn a_sweep_plan_closes_the_expired_and_restacks_the_rest() {
        let t0 = Instant::now();
        let mut popups = Popups::new();
        let mut stale = Timing::new(&cfg(), t0);
        stale.deadline = t0 + Duration::from_secs(1);
        popups.entries.push(Entry {
            camera: "front_doorbell".into(),
            timing: stale,
            hovered: false,
            pinned: false,
        });
        popups.entries.push(Entry {
            camera: "side_camera".into(),
            timing: Timing::new(&cfg(), t0),
            hovered: false,
            pinned: false,
        });

        let plan = plan(&mut popups, t0 + Duration::from_secs(10));
        assert_eq!(plan.close, vec!["front_doorbell".to_string()]);
        assert_eq!(plan.restack, vec![("side_camera".to_string(), 0)]);
        assert_eq!(popups.len(), 1);
    }

    #[test]
    fn a_sweep_with_nothing_expired_is_empty() {
        let t0 = Instant::now();
        let mut popups = Popups::new();
        popups.entries.push(Entry {
            camera: "front_doorbell".into(),
            timing: Timing::new(&cfg(), t0),
            hovered: false,
            pinned: false,
        });
        assert!(plan(&mut popups, t0 + Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn slots_close_up_when_a_popup_below_is_removed() {
        let t0 = Instant::now();
        let mut popups = Popups::new();
        for camera in ["front_doorbell", "garage_camera", "side_camera"] {
            popups.entries.push(Entry {
                camera: camera.into(),
                timing: Timing::new(&cfg(), t0),
                hovered: false,
                pinned: false,
            });
        }

        assert!(popups.forget("garage_camera"));
        assert_eq!(
            popups.slots(),
            vec![
                ("front_doorbell".to_string(), 0),
                ("side_camera".to_string(), 1)
            ],
            "the survivor above must slide down into the vacated slot"
        );
    }

    #[test]
    fn eviction_leaves_contiguous_slots_for_the_survivors() {
        // Regression: evicting slot 0 without restacking left the survivor sitting in
        // its old slot, so the incoming popup opened directly on top of it.
        let t0 = Instant::now();
        let mut popups = Popups::new();
        for camera in ["front_doorbell", "garage_camera"] {
            popups.entries.push(Entry {
                camera: camera.into(),
                timing: Timing::new(&cfg(), t0),
                hovered: false,
                pinned: false,
            });
        }

        let evicted = popups.make_room(2);
        assert_eq!(evicted, vec!["front_doorbell".to_string()]);
        assert_eq!(
            popups.slots(),
            vec![("garage_camera".to_string(), 0)],
            "the survivor must be restacked to slot 0 before the new popup takes slot 1"
        );
        assert_eq!(popups.len(), 1, "and there must be room for one more");
    }

    #[test]
    fn make_room_is_a_no_op_when_there_is_already_space() {
        let t0 = Instant::now();
        let mut popups = Popups::new();
        popups.entries.push(Entry {
            camera: "front_doorbell".into(),
            timing: Timing::new(&cfg(), t0),
            hovered: false,
            pinned: false,
        });
        assert!(popups.make_room(2).is_empty());
        assert_eq!(popups.len(), 1);
    }

    #[test]
    fn a_max_of_one_evicts_on_every_new_popup() {
        let t0 = Instant::now();
        let mut popups = Popups::new();
        popups.entries.push(Entry {
            camera: "front_doorbell".into(),
            timing: Timing::new(&cfg(), t0),
            hovered: false,
            pinned: false,
        });
        assert_eq!(popups.make_room(1), vec!["front_doorbell".to_string()]);
        assert!(popups.is_empty());
    }

    #[test]
    fn forgetting_an_unknown_camera_is_a_no_op() {
        let mut popups = Popups::new();
        assert!(!popups.forget("nope"));
    }

    #[test]
    fn signals_for_a_camera_with_no_popup_are_ignored() {
        let mut popups = Popups::new();
        popups.note("front_doorbell", Signal::Ended, &cfg(), Instant::now());
        assert!(popups.is_empty());
    }
}
