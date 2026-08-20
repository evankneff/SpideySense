//! Lists the camera names Frigate actually uses, by reading its retained MQTT topics.
//!
//! Frigate publishes retained state under `<prefix>/<camera>/...`, so subscribing to
//! `<prefix>/#` gets the full set delivered immediately on connect - no Frigate API auth
//! needed, which matters here because the HTTP API sits behind the authenticated 8971 port.
//!
//! These names are what `after.camera` carries in an event, so they are exactly what the
//! `[[cameras]] name` entries in config.toml must match.
//!
//!     cargo run --example discover_cameras

use anyhow::{Context, Result};
use frigate_popup_lib::config::Config;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// Second-level segments that are Frigate's own topics rather than camera names.
const NOT_CAMERAS: [&str; 8] = [
    "available",
    "events",
    "stats",
    "reviews",
    "notifications",
    "restart",
    "tracked_object_update",
    "onConnect",
];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let config = Config::load().context("loading the frigate-popup config")?;
    let prefix = config.mqtt.topic_prefix.clone();

    let mut options = MqttOptions::new(
        format!("{}-discover", config.mqtt.client_id),
        &config.mqtt.host,
        config.mqtt.port,
    );
    options.set_keep_alive(Duration::from_secs(5));
    // frigate/# includes retained snapshot JPEGs, well over rumqttc's 10 KB default.
    options.set_max_packet_size(4 * 1024 * 1024, 64 * 1024);
    if let Some(username) = &config.mqtt.username {
        if let Some(password) = config.mqtt_password()? {
            options.set_credentials(username, password);
        }
    }

    let (client, mut eventloop) = AsyncClient::new(options, 100);

    let mut cameras: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut other: BTreeSet<String> = BTreeSet::new();

    println!("listening on {prefix}/# for 5s...\n");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, eventloop.poll()).await {
            Err(_) => break, // deadline reached
            Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                client
                    .subscribe(format!("{prefix}/#"), QoS::AtMostOnce)
                    .await
                    .context("subscribing")?;
            }
            Ok(Ok(Event::Incoming(Packet::Publish(publish)))) => {
                let rest = publish.topic.strip_prefix(&format!("{prefix}/"));
                if let Some(rest) = rest {
                    let mut parts = rest.splitn(2, '/');
                    if let Some(first) = parts.next() {
                        if NOT_CAMERAS.contains(&first) {
                            other.insert(first.to_string());
                        } else {
                            cameras
                                .entry(first.to_string())
                                .or_default()
                                .insert(parts.next().unwrap_or("").to_string());
                        }
                    }
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                eprintln!("connection error: {e}");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    if cameras.is_empty() {
        println!("no camera topics seen. Is the topic prefix `{prefix}` correct?");
        return Ok(());
    }

    println!("Frigate cameras ({}):\n", cameras.len());
    for (camera, topics) in &cameras {
        println!("  {camera}  ({} topics)", topics.len());
    }

    println!("\nConfig block to paste into config.toml (check each `stream` against");
    println!("`curl {}/api/streams`):\n", config.frigate.go2rtc_url);
    for camera in cameras.keys() {
        println!("[[cameras]]");
        println!("name = \"{camera}\"");
        println!("stream = \"CHECK_ME_sub\"");
        println!();
    }

    if !other.is_empty() {
        let list: Vec<_> = other.iter().map(String::as_str).collect();
        println!("(non-camera topics seen: {})", list.join(", "));
    }
    Ok(())
}
