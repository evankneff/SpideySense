//! Prints raw `frigate/events` payloads, to answer questions the app's own log cannot.
//!
//! Every optional field in `events::After` carries `#[serde(default)]`, which is what keeps
//! a schema change from killing the MQTT client. The cost is that a field Frigate never
//! sends is indistinguishable from one it sends as `false` - `stationary=false` in the log
//! could mean either. This dumps what actually arrives on the wire so the difference is
//! visible.
//!
//!     cargo run --example dump_events
//!     cargo run --example dump_events -- --raw     # full JSON, not the summary
//!
//! Uses its own client id, so it can run alongside the app without either being
//! disconnected by the broker.

use anyhow::{Context, Result};
use frigate_popup_lib::config::Config;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::Value;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let raw = std::env::args().any(|a| a == "--raw");

    let config = Config::load().context("loading the frigate-popup config")?;
    let topic = format!("{}/events", config.mqtt.topic_prefix);

    let mut options = MqttOptions::new(
        format!("{}-dump", config.mqtt.client_id),
        &config.mqtt.host,
        config.mqtt.port,
    );
    options.set_keep_alive(Duration::from_secs(5));
    options.set_max_packet_size(1024 * 1024, 64 * 1024);
    if let Some(username) = &config.mqtt.username {
        if let Some(password) = config.mqtt_password()? {
            options.set_credentials(username, password);
        }
    }

    let (client, mut eventloop) = AsyncClient::new(options, 32);
    println!(
        "listening on {topic} at {}:{} - Ctrl+C to stop",
        config.mqtt.host, config.mqtt.port
    );

    let mut seen_keys = false;

    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                client.subscribe(&topic, QoS::AtLeastOnce).await?;
            }
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let value: Value = match serde_json::from_slice(&publish.payload) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("unparseable payload: {e}");
                        continue;
                    }
                };

                if raw {
                    println!("{}", serde_json::to_string_pretty(&value)?);
                    continue;
                }

                let kind = value.get("type").and_then(Value::as_str).unwrap_or("?");
                let after = value.get("after").unwrap_or(&Value::Null);

                // Once, so the answer to "does Frigate even send this field" is on record.
                if !seen_keys {
                    if let Some(map) = after.as_object() {
                        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
                        keys.sort_unstable();
                        println!("`after` keys: {}", keys.join(", "));
                        println!(
                            "stationary present: {}   motionless_count present: {}",
                            map.contains_key("stationary"),
                            map.contains_key("motionless_count")
                        );
                        println!("---");
                    }
                    seen_keys = true;
                }

                let field = |name: &str| -> String {
                    match after.get(name) {
                        None => "<absent>".to_string(),
                        Some(v) => v.to_string(),
                    }
                };

                println!(
                    "{kind:<6} camera={} id={} label={} stationary={} motionless={} zones={}",
                    field("camera"),
                    field("id"),
                    field("label"),
                    field("stationary"),
                    field("motionless_count"),
                    field("current_zones"),
                );
            }
            Ok(_) => {}
            Err(e) => {
                println!("connection error: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
