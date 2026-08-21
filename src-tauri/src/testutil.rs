//! Shared test helpers.
//!
//! Exists because the "write a config to a temp dir, load it, delete the dir" dance was
//! duplicated per module, and every copy used a fixed directory name. Tests run in
//! parallel, so each copy deleted the others' config out from under them - an intermittent
//! failure that looked like a logic bug twice before the cause was obvious.

use crate::config::Config;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Loads `text` as a config through the real loader, in a directory unique to this call.
pub fn config_from(text: &str) -> Config {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("frigate-popup-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("config.toml");
    std::fs::write(&path, text).expect("write config");

    let config = Config::load_from(&path).expect("config should be valid");
    let _ = std::fs::remove_dir_all(&dir);
    config
}

/// A minimal valid config with the cameras the tests use.
pub const BASE_CONFIG: &str = r#"
[mqtt]
host = "192.168.1.11"

[frigate]
ui_url = "https://192.168.1.10:8971"
go2rtc_url = "http://192.168.1.10:1984"

[[cameras]]
name = "front_doorbell"
stream = "doorbell_sub"

[[cameras]]
name = "shed"
stream = "shed_sub"
enabled = false
"#;
