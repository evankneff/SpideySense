//! Publishes a realistic Frigate event sequence to the real broker, so the MQTT path can
//! be exercised without waiting for someone to walk past a camera.
//!
//! Credentials come from the app's own config file, so the password is never passed on the
//! command line or printed.
//!
//!     cargo run --example publish_test_event -- doorbell person 5
//!
//! Arguments: CAMERA (default doorbell), LABEL (default person), END_AFTER_SECS (default 5;
//! 0 sends only the `new` event). An `update` is sent halfway through.

use anyhow::{Context, Result};
use frigate_popup_lib::config::Config;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let camera = args.next().unwrap_or_else(|| "doorbell".into());
    let label = args.next().unwrap_or_else(|| "person".into());
    let end_after: u64 = args
        .next()
        .unwrap_or_else(|| "5".into())
        .parse()
        .context("END_AFTER_SECS must be a whole number of seconds")?;

    let config = Config::load().context("loading the frigate-popup config")?;
    let topic = format!("{}/events", config.mqtt.topic_prefix);

    let mut options = MqttOptions::new(
        format!("{}-testpub", config.mqtt.client_id),
        &config.mqtt.host,
        config.mqtt.port,
    );
    options.set_keep_alive(Duration::from_secs(5));
    if let Some(username) = &config.mqtt.username {
        if let Some(password) = config.mqtt_password()? {
            options.set_credentials(username, password);
        }
    }

    let (client, mut eventloop) = AsyncClient::new(options, 10);

    // The event loop has to be driven for anything to actually go out on the wire.
    let pump = tokio::spawn(async move {
        loop {
            if eventloop.poll().await.is_err() {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    });

    let id = format!("test-{camera}-{label}");
    let base = |kind: &str, stationary: bool| {
        serde_json::json!({
            "type": kind,
            "before": null,
            "after": {
                "id": id,
                "camera": camera,
                "label": label,
                "sub_label": null,
                "score": 0.81,
                "top_score": 0.83,
                "false_positive": false,
                "stationary": stationary,
                "current_zones": [],
                "entered_zones": [],
                "has_clip": false,
                "has_snapshot": false,
                "start_time": 1755705123.456789,
                "end_time": null
            }
        })
        .to_string()
    };

    println!("publishing `new` for {camera}/{label} to {topic}");
    client
        .publish(&topic, QoS::AtLeastOnce, false, base("new", false))
        .await
        .context("publishing the new event")?;

    if end_after > 0 {
        tokio::time::sleep(Duration::from_secs(end_after / 2)).await;
        println!("publishing `update`");
        client
            .publish(&topic, QoS::AtLeastOnce, false, base("update", false))
            .await
            .context("publishing the update event")?;

        tokio::time::sleep(Duration::from_secs(end_after - end_after / 2)).await;
        println!("publishing `end`");
        client
            .publish(&topic, QoS::AtLeastOnce, false, base("end", false))
            .await
            .context("publishing the end event")?;
    }

    // Give the event loop a moment to flush before the process exits.
    tokio::time::sleep(Duration::from_millis(500)).await;
    pump.abort();
    println!("done");
    Ok(())
}
