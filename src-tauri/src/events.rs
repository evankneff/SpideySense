//! Frigate event schema and the trigger decision.
//!
//! Deliberately free of I/O, Tauri and MQTT so the whole decision path can be tested
//! against canned payloads. `Triggers::evaluate` takes `&self` and an explicit `now`;
//! recording a fire is a separate call, which keeps the interesting logic pure.

use crate::config::Config;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

/// One message from `frigate/events`.
///
/// Intentionally lenient: Frigate adds fields between releases, and a payload we do not
/// fully understand is still worth acting on.
#[derive(Debug, Clone, Deserialize)]
pub struct FrigateEvent {
    #[serde(rename = "type")]
    pub event_type: EventType,
    #[serde(default)]
    pub before: Option<TrackedObject>,
    pub after: TrackedObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    New,
    Update,
    End,
    /// Anything a future Frigate version introduces. Logged and ignored, never fatal.
    #[serde(other)]
    Unknown,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EventType::New => "new",
            EventType::Update => "update",
            EventType::End => "end",
            EventType::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackedObject {
    pub id: String,
    pub camera: String,
    pub label: String,
    /// Frigate has shipped this as a string, as `[name, score]` and as null across
    /// versions, so it stays untyped rather than breaking the whole parse.
    #[serde(default)]
    pub sub_label: Option<serde_json::Value>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub top_score: Option<f64>,
    #[serde(default)]
    pub current_zones: Vec<String>,
    #[serde(default)]
    pub entered_zones: Vec<String>,
    #[serde(default)]
    pub has_snapshot: bool,
    #[serde(default)]
    pub has_clip: bool,
    #[serde(default)]
    pub false_positive: bool,
    #[serde(default)]
    pub stationary: bool,
    #[serde(default)]
    pub start_time: Option<f64>,
    #[serde(default)]
    pub end_time: Option<f64>,
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Fire {
        camera: String,
        stream: String,
        event_id: String,
        label: String,
    },
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    NotificationsPaused,
    NotANewEvent(EventType),
    UnknownCamera(String),
    CameraDisabled(String),
    FalsePositive,
    LabelNotWatched {
        label: String,
        watched: Vec<String>,
    },
    ZoneNotEntered {
        required: Vec<String>,
        entered: Vec<String>,
    },
    Cooldown {
        remaining: Duration,
    },
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkipReason::NotificationsPaused => write!(f, "notifications are paused"),
            SkipReason::NotANewEvent(t) => {
                write!(f, "event type is `{t}`, only `new` opens a popup")
            }
            SkipReason::UnknownCamera(c) => {
                write!(f, "camera `{c}` is not in the config")
            }
            SkipReason::CameraDisabled(c) => write!(f, "camera `{c}` is disabled"),
            SkipReason::FalsePositive => write!(f, "Frigate marked it a false positive"),
            SkipReason::LabelNotWatched { label, watched } => write!(
                f,
                "label `{label}` is not watched (watching: {})",
                watched.join(", ")
            ),
            SkipReason::ZoneNotEntered { required, entered } => write!(
                f,
                "entered zones [{}] do not include any required zone [{}]",
                entered.join(", "),
                required.join(", ")
            ),
            SkipReason::Cooldown { remaining } => write!(
                f,
                "camera is in cooldown for another {:.1}s",
                remaining.as_secs_f64()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Trigger state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Triggers {
    /// Last time a popup fired, per camera. Drives the cooldown.
    last_fired: HashMap<String, Instant>,
    paused: bool,
}

impl Triggers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Notes that a popup fired, starting this camera's cooldown.
    pub fn record_fired(&mut self, camera: &str, now: Instant) {
        self.last_fired.insert(camera.to_string(), now);
    }

    /// Decides what to do with an event. Checks run cheapest-and-most-common first, and
    /// the order determines which reason gets reported when several apply.
    pub fn evaluate(&self, config: &Config, event: &FrigateEvent, now: Instant) -> Decision {
        use Decision::Skip;

        if self.paused {
            return Skip(SkipReason::NotificationsPaused);
        }

        if event.event_type != EventType::New {
            return Skip(SkipReason::NotANewEvent(event.event_type));
        }

        let after = &event.after;

        let Some(camera) = config.camera(&after.camera) else {
            return Skip(SkipReason::UnknownCamera(after.camera.clone()));
        };

        if !camera.enabled {
            return Skip(SkipReason::CameraDisabled(after.camera.clone()));
        }

        if after.false_positive && config.detection.ignore_false_positives {
            return Skip(SkipReason::FalsePositive);
        }

        let watched = config.labels_for(camera);
        if !watched.iter().any(|l| l == &after.label) {
            return Skip(SkipReason::LabelNotWatched {
                label: after.label.clone(),
                watched: watched.to_vec(),
            });
        }

        if let Some(required) = &camera.required_zones {
            // A `new` event often has no zones yet, so this legitimately filters out the
            // first sighting of an object that later walks into the zone. Zones are opt-in
            // per camera for exactly that reason.
            if !required.iter().any(|z| after.entered_zones.contains(z)) {
                return Skip(SkipReason::ZoneNotEntered {
                    required: required.clone(),
                    entered: after.entered_zones.clone(),
                });
            }
        }

        let cooldown = Duration::from_secs(config.cooldown_for(camera));
        if let Some(last) = self.last_fired.get(&after.camera) {
            let elapsed = now.saturating_duration_since(*last);
            if elapsed < cooldown {
                return Skip(SkipReason::Cooldown {
                    remaining: cooldown - elapsed,
                });
            }
        }

        Decision::Fire {
            camera: after.camera.clone(),
            stream: camera.stream.clone(),
            event_id: after.id.clone(),
            label: after.label.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real Frigate payload; every field the app reads is present.
    const NEW_PERSON: &str = r#"{
      "type": "new",
      "before": null,
      "after": {
        "id": "1755705123.456789-ab12cd",
        "camera": "front_doorbell",
        "frame_time": 1755705123.456789,
        "snapshot_time": 1755705123.456789,
        "label": "person",
        "sub_label": null,
        "top_score": 0.83,
        "false_positive": false,
        "start_time": 1755705123.456789,
        "end_time": null,
        "score": 0.81,
        "box": [421, 322, 546, 597],
        "area": 34375,
        "ratio": 0.4545,
        "region": [370, 250, 640, 640],
        "stationary": false,
        "motionless_count": 0,
        "position_changes": 1,
        "current_zones": [],
        "entered_zones": [],
        "has_clip": false,
        "has_snapshot": false
      }
    }"#;

    use crate::testutil::{config_from, BASE_CONFIG};

    fn config() -> Config {
        config_from(BASE_CONFIG)
    }

    fn parse(json: &str) -> FrigateEvent {
        serde_json::from_str(json).expect("event should parse")
    }

    /// Rewrites one top-level field of `after` so tests stay close to the real payload.
    fn with_after(json: &str, key: &str, value: serde_json::Value) -> String {
        let mut v: serde_json::Value = serde_json::from_str(json).expect("valid json");
        v["after"][key] = value;
        v.to_string()
    }

    #[test]
    fn a_real_payload_parses() {
        let e = parse(NEW_PERSON);
        assert_eq!(e.event_type, EventType::New);
        assert_eq!(e.after.camera, "front_doorbell");
        assert_eq!(e.after.label, "person");
        assert_eq!(e.after.id, "1755705123.456789-ab12cd");
        assert!(!e.after.stationary);
        assert!(e.before.is_none());
    }

    #[test]
    fn a_watched_person_on_an_enabled_camera_fires() {
        let decision = Triggers::new().evaluate(&config(), &parse(NEW_PERSON), Instant::now());
        assert_eq!(
            decision,
            Decision::Fire {
                camera: "front_doorbell".into(),
                stream: "doorbell_sub".into(),
                event_id: "1755705123.456789-ab12cd".into(),
                label: "person".into(),
            }
        );
    }

    #[test]
    fn update_and_end_events_never_open_a_popup() {
        for kind in ["update", "end"] {
            let json = NEW_PERSON.replace(r#""type": "new""#, &format!(r#""type": "{kind}""#));
            let decision = Triggers::new().evaluate(&config(), &parse(&json), Instant::now());
            assert!(
                matches!(decision, Decision::Skip(SkipReason::NotANewEvent(_))),
                "{kind} should be skipped, got {decision:?}"
            );
        }
    }

    #[test]
    fn an_unrecognised_event_type_is_skipped_rather_than_failing_to_parse() {
        let json = NEW_PERSON.replace(r#""type": "new""#, r#""type": "reindex""#);
        let event = parse(&json);
        assert_eq!(event.event_type, EventType::Unknown);
        let decision = Triggers::new().evaluate(&config(), &event, Instant::now());
        assert!(matches!(
            decision,
            Decision::Skip(SkipReason::NotANewEvent(EventType::Unknown))
        ));
    }

    #[test]
    fn unwatched_labels_are_skipped() {
        let json = with_after(NEW_PERSON, "label", "car".into());
        let decision = Triggers::new().evaluate(&config(), &parse(&json), Instant::now());
        match decision {
            Decision::Skip(SkipReason::LabelNotWatched { label, watched }) => {
                assert_eq!(label, "car");
                assert_eq!(watched, vec!["person".to_string()]);
            }
            other => panic!("expected LabelNotWatched, got {other:?}"),
        }
    }

    #[test]
    fn cameras_missing_from_the_config_are_skipped() {
        let json = with_after(NEW_PERSON, "camera", "back_yard".into());
        let decision = Triggers::new().evaluate(&config(), &parse(&json), Instant::now());
        assert_eq!(
            decision,
            Decision::Skip(SkipReason::UnknownCamera("back_yard".into()))
        );
    }

    #[test]
    fn disabled_cameras_are_skipped() {
        let json = with_after(NEW_PERSON, "camera", "indoor_garage".into());
        let decision = Triggers::new().evaluate(&config(), &parse(&json), Instant::now());
        assert_eq!(
            decision,
            Decision::Skip(SkipReason::CameraDisabled("indoor_garage".into()))
        );
    }

    #[test]
    fn false_positives_are_skipped() {
        let json = with_after(NEW_PERSON, "false_positive", true.into());
        let decision = Triggers::new().evaluate(&config(), &parse(&json), Instant::now());
        assert_eq!(decision, Decision::Skip(SkipReason::FalsePositive));
    }

    #[test]
    fn cooldown_blocks_a_second_event_then_expires() {
        let config = config();
        let event = parse(NEW_PERSON);
        let mut triggers = Triggers::new();
        let t0 = Instant::now();

        assert!(matches!(
            triggers.evaluate(&config, &event, t0),
            Decision::Fire { .. }
        ));
        triggers.record_fired("front_doorbell", t0);

        // Default cooldown is 60s.
        match triggers.evaluate(&config, &event, t0 + Duration::from_secs(30)) {
            Decision::Skip(SkipReason::Cooldown { remaining }) => {
                assert_eq!(remaining.as_secs(), 30);
            }
            other => panic!("expected Cooldown, got {other:?}"),
        }

        assert!(matches!(
            triggers.evaluate(&config, &event, t0 + Duration::from_secs(61)),
            Decision::Fire { .. }
        ));
    }

    #[test]
    fn cooldown_is_tracked_per_camera() {
        let config = config();
        let mut triggers = Triggers::new();
        let t0 = Instant::now();
        triggers.record_fired("front_doorbell", t0);

        // A different camera is unaffected by the doorbell's cooldown.
        let json = with_after(NEW_PERSON, "camera", "indoor_garage".into());
        let decision = triggers.evaluate(&config, &parse(&json), t0 + Duration::from_secs(1));
        assert_eq!(
            decision,
            Decision::Skip(SkipReason::CameraDisabled("indoor_garage".into())),
            "should fail on being disabled, not on the doorbell's cooldown"
        );
    }

    #[test]
    fn pausing_suppresses_everything() {
        let mut triggers = Triggers::new();
        triggers.set_paused(true);
        assert_eq!(
            triggers.evaluate(&config(), &parse(NEW_PERSON), Instant::now()),
            Decision::Skip(SkipReason::NotificationsPaused)
        );
    }

    #[test]
    fn required_zones_gate_the_trigger() {
        let text = r#"
[mqtt]
host = "10.0.0.213"

[frigate]
ui_url = "https://10.0.0.193:8971"
go2rtc_url = "http://10.0.0.193:1984"

[[cameras]]
name = "front_doorbell"
stream = "doorbell_sub"
required_zones = ["walkway"]
"#;
        let config = config_from(text);

        // No zones entered yet -> skipped.
        let decision = Triggers::new().evaluate(&config, &parse(NEW_PERSON), Instant::now());
        assert!(matches!(
            decision,
            Decision::Skip(SkipReason::ZoneNotEntered { .. })
        ));

        // Once the object has entered the zone it fires.
        let json = with_after(
            NEW_PERSON,
            "entered_zones",
            serde_json::json!(["walkway", "porch"]),
        );
        assert!(matches!(
            Triggers::new().evaluate(&config, &parse(&json), Instant::now()),
            Decision::Fire { .. }
        ));
    }

    #[test]
    fn sub_label_survives_every_shape_frigate_has_shipped() {
        for shape in ["null", r#""Evan""#, r#"["Evan", 0.92]"#] {
            let json =
                NEW_PERSON.replace(r#""sub_label": null"#, &format!(r#""sub_label": {shape}"#));
            let event: Result<FrigateEvent, _> = serde_json::from_str(&json);
            assert!(event.is_ok(), "sub_label {shape} should parse");
        }
    }

    #[test]
    fn unknown_fields_from_a_newer_frigate_are_ignored() {
        let json = with_after(NEW_PERSON, "some_future_field", serde_json::json!({"a": 1}));
        assert!(serde_json::from_str::<FrigateEvent>(&json).is_ok());
    }
}
