//! MQTT client: subscribes to `<prefix>/events` and turns payloads into decisions.
//!
//! The task never returns. Connection failures are logged and retried with exponential
//! backoff; malformed payloads are logged and dropped. Neither can take the app down.

use crate::config::ConfigSlot;
use crate::events::{Decision, EventType, FrigateEvent, SkipReason, Triggers};
use crate::lifecycle::{Popups, Signal};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, Publish, QoS};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Runtime};
use tracing::{debug, error, info, trace, warn};

/// Everything the MQTT task needs. Bundled so `run` and `dispatch` keep short signatures.
pub struct Context<R: Runtime> {
    pub app: AppHandle<R>,
    pub config: ConfigSlot,
    pub triggers: Arc<Mutex<Triggers>>,
    pub popups: Arc<Mutex<Popups>>,
}

/// Recovers the guard rather than propagating a panic from another thread.
fn lock<'a, T>(what: &str, m: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|poisoned| {
        warn!("{what} lock was poisoned; recovering");
        poisoned.into_inner()
    })
}

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// rumqttc defaults to a 10 KB incoming limit, and anything over it kills the connection
/// rather than dropping the one message - which shows up as an endless reconnect loop.
/// Frigate's retained snapshot JPEGs alone are ~14 KB, and event payloads grow with the
/// number of zones, so the default is not enough headroom.
const MAX_INCOMING_PACKET: usize = 1024 * 1024;
const MAX_OUTGOING_PACKET: usize = 64 * 1024;
/// Cap on the cadence map so a flood of events cannot grow it without bound.
const MAX_TRACKED_EVENTS: usize = 256;

/// When a tracked object was first seen and last heard from.
///
/// Both are needed: `last` gives the inter-message gap, `first` gives the total lifetime.
/// Keeping only one conflates them.
#[derive(Debug, Clone, Copy)]
struct EventTiming {
    first: Instant,
    last: Instant,
}

pub async fn run<R: Runtime>(ctx: Context<R>) {
    let config = ctx.config.load();
    let topic = format!("{}/events", config.mqtt.topic_prefix);

    let mut options = MqttOptions::new(&config.mqtt.client_id, &config.mqtt.host, config.mqtt.port);
    options.set_keep_alive(Duration::from_secs(config.mqtt.keepalive_seconds));
    options.set_clean_session(true);
    options.set_max_packet_size(MAX_INCOMING_PACKET, MAX_OUTGOING_PACKET);

    if let Some(username) = &config.mqtt.username {
        match config.mqtt_password() {
            Ok(Some(password)) => {
                options.set_credentials(username, password);
            }
            Ok(None) => {
                warn!("mqtt.username is set but no password was supplied; connecting without one")
            }
            Err(e) => {
                // Validation already checked the variable exists, so this is close to
                // unreachable - but failing loudly beats connecting as nobody.
                error!("could not resolve the MQTT password: {e:#}");
                return;
            }
        }
    }

    info!(
        broker = %format!("{}:{}", config.mqtt.host, config.mqtt.port),
        client_id = %config.mqtt.client_id,
        %topic,
        "starting MQTT client"
    );

    let (client, mut eventloop) = AsyncClient::new(options, 32);
    let mut backoff = MIN_BACKOFF;
    let mut cadence: HashMap<String, EventTiming> = HashMap::new();

    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                info!(code = ?ack.code, "connected to the MQTT broker");
                backoff = MIN_BACKOFF;
                // Subscribing on every ConnAck rather than once at startup: the session is
                // clean, so a reconnect starts with no subscriptions at all.
                if let Err(e) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                    error!("could not subscribe to {topic}: {e}");
                } else {
                    info!("subscribed to {topic}");
                }
            }
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                handle_publish(&ctx, &publish, &mut cadence);
            }
            Ok(other) => trace!(?other, "mqtt event"),
            Err(e) => {
                warn!(
                    "MQTT connection error: {e}; retrying in {}s",
                    backoff.as_secs()
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

fn handle_publish<R: Runtime>(
    ctx: &Context<R>,
    publish: &Publish,
    cadence: &mut HashMap<String, EventTiming>,
) {
    let event: FrigateEvent = match serde_json::from_slice(&publish.payload) {
        Ok(event) => event,
        Err(e) => {
            // Truncated so a huge or binary payload cannot flood the log.
            let preview = String::from_utf8_lossy(&publish.payload);
            let preview: String = preview.chars().take(200).collect();
            warn!(
                topic = %publish.topic,
                "ignoring malformed event payload: {e} | {preview}"
            );
            return;
        }
    };

    record_cadence(&event, cadence);
    dispatch(ctx, &event);
}

/// Measures the gap between messages for a single tracked object.
///
/// Frigate publishes `update` on meaningful changes rather than on a timer, so the real
/// cadence has to be observed before the milestone 4 watchdog can be tuned to it.
fn record_cadence(event: &FrigateEvent, cadence: &mut HashMap<String, EventTiming>) {
    let now = Instant::now();
    let id = &event.after.id;

    match event.event_type {
        EventType::New => {
            if cadence.len() >= MAX_TRACKED_EVENTS {
                cadence.clear();
                warn!("cadence map hit its cap and was reset");
            }
            cadence.insert(
                id.clone(),
                EventTiming {
                    first: now,
                    last: now,
                },
            );
        }
        EventType::Update => {
            if let Some(timing) = cadence.get_mut(id) {
                let gap = now.saturating_duration_since(timing.last);
                timing.last = now;
                debug!(
                    camera = %event.after.camera,
                    event_id = %id,
                    gap_ms = gap.as_millis(),
                    age_ms = now.saturating_duration_since(timing.first).as_millis(),
                    stationary = event.after.stationary,
                    zones = ?event.after.current_zones,
                    "update cadence"
                );
            }
        }
        EventType::End => {
            if let Some(timing) = cadence.remove(id) {
                debug!(
                    camera = %event.after.camera,
                    event_id = %id,
                    lifetime_ms = now.saturating_duration_since(timing.first).as_millis(),
                    since_last_ms = now.saturating_duration_since(timing.last).as_millis(),
                    "event ended"
                );
            }
        }
        EventType::Unknown => {}
    }
}

/// Evaluates one event, logs the outcome with its reason, and drives the popup lifecycle.
///
/// Trigger evaluation and lifecycle are deliberately separate: only a `new` event can
/// *open* a popup, but `update` and `end` still have to reach an already-open one. An
/// event skipped for cooldown, for instance, must still keep its window alive.
pub fn dispatch<R: Runtime>(ctx: &Context<R>, event: &FrigateEvent) {
    let now = Instant::now();
    // One snapshot for the whole decision: a reload landing mid-dispatch must not
    // change the rules between evaluating the event and opening its window.
    let config = ctx.config.load();
    let config = config.as_ref();
    let camera = &event.after.camera;

    let decision = {
        let mut triggers = lock("trigger state", &ctx.triggers);
        let decision = triggers.evaluate(config, event, now);
        if let Decision::Fire { camera, .. } = &decision {
            triggers.record_fired(camera, now);
        }
        decision
    };

    match &decision {
        Decision::Fire {
            camera,
            stream,
            event_id,
            label,
        } => {
            info!(
                %camera, %stream, %label, %event_id,
                score = event.after.score,
                "TRIGGER FIRED"
            );
            match config.camera(camera.as_str()) {
                Some(cfg) => {
                    let mut popups = lock("popup state", &ctx.popups);
                    if let Err(e) = popups.open(&ctx.app, config, cfg, now) {
                        error!(%camera, "could not open the popup: {e:#}");
                    }
                }
                // evaluate() already proved the camera exists, so this is unreachable.
                None => error!(%camera, "fired for a camera that vanished from the config"),
            }
        }
        Decision::Skip(SkipReason::NotANewEvent(kind)) => {
            trace!(%camera, %kind, "skipped");
        }
        Decision::Skip(reason) => {
            info!(
                %camera,
                label = %event.after.label,
                event_id = %event.after.id,
                "skipped: {reason}"
            );
        }
    }

    // Regardless of the trigger decision, keep any open popup for this camera in step
    // with what its object is doing.
    let signal = match event.event_type {
        EventType::End => Some(Signal::Ended),
        EventType::Update if event.after.stationary => Some(Signal::Stationary),
        EventType::Update => Some(Signal::Active),
        EventType::New | EventType::Unknown => None,
    };
    if let Some(signal) = signal {
        let mut popups = lock("popup state", &ctx.popups);
        if popups.has(camera) {
            debug!(%camera, ?signal, "lifecycle signal");
            popups.note(camera, signal, &config.popup, now);
        }
    }
}

/// Builds a realistic `new` event for `--simulate`, so the whole decision path can be
/// exercised without waiting for someone to walk past a camera.
pub fn simulated_event(camera: &str, label: &str) -> FrigateEvent {
    let json = serde_json::json!({
        "type": "new",
        "before": null,
        "after": {
            "id": format!("simulated-{camera}"),
            "camera": camera,
            "label": label,
            "sub_label": null,
            "score": 0.87,
            "top_score": 0.87,
            "false_positive": false,
            "stationary": false,
            "current_zones": [],
            "entered_zones": [],
            "has_clip": false,
            "has_snapshot": false,
            "start_time": 0.0,
            "end_time": null
        }
    });
    // Constructed from a literal above, so this cannot realistically fail; the fallback
    // keeps the no-unwrap rule intact regardless.
    serde_json::from_value(json).unwrap_or_else(|_| FrigateEvent {
        event_type: EventType::Unknown,
        before: None,
        after: crate::events::TrackedObject {
            id: String::new(),
            camera: camera.to_string(),
            label: label.to_string(),
            sub_label: None,
            score: None,
            top_score: None,
            current_zones: Vec::new(),
            entered_zones: Vec::new(),
            has_snapshot: false,
            has_clip: false,
            false_positive: false,
            stationary: false,
            start_time: None,
            end_time: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulated_event_looks_like_a_real_new_event() {
        let event = simulated_event("doorbell", "person");
        assert_eq!(event.event_type, EventType::New);
        assert_eq!(event.after.camera, "doorbell");
        assert_eq!(event.after.label, "person");
        assert!(!event.after.false_positive);
    }

    #[test]
    fn cadence_tracking_pairs_updates_with_their_new_event() {
        let mut cadence = HashMap::new();
        let new = simulated_event("doorbell", "person");
        record_cadence(&new, &mut cadence);
        assert_eq!(cadence.len(), 1);

        let mut update = simulated_event("doorbell", "person");
        update.event_type = EventType::Update;
        record_cadence(&update, &mut cadence);
        assert_eq!(cadence.len(), 1, "an update replaces rather than adds");

        let mut end = simulated_event("doorbell", "person");
        end.event_type = EventType::End;
        record_cadence(&end, &mut cadence);
        assert!(cadence.is_empty(), "end should release the entry");
    }

    #[test]
    fn an_update_does_not_reset_the_events_start_time() {
        // Regression: overwriting `first` on every update made the `end` log report the
        // gap since the last message instead of the object's real lifetime.
        let mut cadence = HashMap::new();
        let new = simulated_event("doorbell", "person");
        record_cadence(&new, &mut cadence);
        let started = cadence
            .get(&new.after.id)
            .map(|t| t.first)
            .expect("tracked");

        std::thread::sleep(Duration::from_millis(20));
        let mut update = simulated_event("doorbell", "person");
        update.event_type = EventType::Update;
        record_cadence(&update, &mut cadence);

        let timing = cadence.get(&new.after.id).copied().expect("still tracked");
        assert_eq!(timing.first, started, "start time must survive an update");
        assert!(timing.last > started, "last-seen must advance");
    }

    #[test]
    fn the_cadence_map_cannot_grow_without_bound() {
        let mut cadence = HashMap::new();
        for i in 0..MAX_TRACKED_EVENTS + 10 {
            let mut event = simulated_event(&format!("cam{i}"), "person");
            event.after.id = format!("id-{i}");
            record_cadence(&event, &mut cadence);
        }
        assert!(cadence.len() <= MAX_TRACKED_EVENTS);
    }
}
