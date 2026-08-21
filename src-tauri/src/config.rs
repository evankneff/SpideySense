//! Config file loading, defaults and validation.
//!
//! Everything the app needs at runtime lives in one TOML file. Missing sections fall
//! back to defaults; anything genuinely ambiguous is reported as a validation error
//! listing *every* problem at once, so a broken config takes one edit to fix, not five.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const EXAMPLE_CONFIG: &str = include_str!("../../config.example.toml");

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no config file found, so a commented starter config was written to:\n\n  {0}\n\nEdit it (at minimum the MQTT password and the camera list), then start frigate-popup again.")]
    Created(PathBuf),

    #[error("{path}\n\ncould not be parsed as TOML:\n\n{source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("{path}\n\nhas {count} problem(s):\n\n{details}")]
    Invalid {
        path: PathBuf,
        count: usize,
        details: String,
    },
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub mqtt: Mqtt,
    pub frigate: Frigate,
    #[serde(default)]
    pub popup: Popup,
    #[serde(default)]
    pub detection: Detection,
    #[serde(default)]
    pub hotkey: Hotkey,
    #[serde(default)]
    pub cameras: Vec<Camera>,
}

/// Global hotkey for the keyboard camera picker.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Hotkey {
    pub enabled: bool,
    /// Accelerator string, e.g. "F20", "CmdOrCtrl+Shift+C". Parsed by the
    /// global-shortcut plugin, so anything it accepts works here.
    pub binding: String,
}

impl Default for Hotkey {
    fn default() -> Self {
        Self {
            enabled: true,
            // F20 has no default binding on Windows and no physical key on most
            // keyboards, which makes it a safe target for a macro key.
            binding: "F20".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mqtt {
    pub host: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    /// Literal password. Mutually exclusive with `password_env`.
    #[serde(default)]
    pub password: Option<String>,
    /// Name of an environment variable holding the password. Preferred.
    #[serde(default)]
    pub password_env: Option<String>,
    #[serde(default = "default_client_id")]
    pub client_id: String,
    #[serde(default = "default_topic_prefix")]
    pub topic_prefix: String,
    #[serde(default = "default_keepalive")]
    pub keepalive_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frigate {
    /// Base URL of the Frigate web UI, used for click-through. e.g. https://192.168.1.10:8971
    pub ui_url: String,
    /// Base URL of the go2rtc API. e.g. http://192.168.1.10:1984
    pub go2rtc_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Popup {
    pub width: u32,
    pub height: u32,
    pub corner: Corner,
    pub offset_x: i32,
    pub offset_y: i32,
    /// Vertical gap between stacked popups, in logical pixels.
    pub stack_gap: i32,
    pub monitor: MonitorSelect,
    /// Floor on on-screen time so a popup never blinks away when `end` arrives immediately.
    pub min_display_seconds: u64,
    /// Grace period after the object ends or goes stationary.
    pub linger_seconds: u64,
    /// Close if no `update` arrives within this long. Covers a dropped `end` event.
    pub watchdog_seconds: u64,
    /// Absolute ceiling on time on screen, whatever the events say.
    pub max_display_seconds: u64,
    pub max_popups: usize,
    /// go2rtc player mode string, e.g. "webrtc" or "webrtc,mse" for fallback.
    pub stream_mode: String,
    /// How the video fills the popup: "cover", "contain" or "fill".
    pub video_fit: String,
    /// Show go2rtc's own mode badge (RTC / MSE). Useful when diagnosing stream fallback.
    pub show_stream_badge: bool,
    /// Which page a popup loads: "local" (bundled, snapshot-first) or "go2rtc"
    /// (go2rtc's own stream.html, kept as a fallback).
    pub page: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Detection {
    pub labels: Vec<String>,
    pub cooldown_seconds: u64,
    pub ignore_false_positives: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    /// Frigate camera name, exactly as it appears in the MQTT event's `after.camera`.
    pub name: String,
    /// go2rtc stream name used for the video feed.
    pub stream: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Overrides `detection.labels` for this camera.
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    /// If set, the object must have entered at least one of these zones.
    #[serde(default)]
    pub required_zones: Option<Vec<String>>,
    /// Overrides `detection.cooldown_seconds` for this camera.
    #[serde(default)]
    pub cooldown_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// `monitor = "primary"` / `monitor = "cursor"` / `monitor = 1`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum MonitorSelect {
    Index(usize),
    Named(String),
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

fn default_mqtt_port() -> u16 {
    1883
}
fn default_client_id() -> String {
    "frigate-popup".into()
}
fn default_topic_prefix() -> String {
    "frigate".into()
}
fn default_keepalive() -> u64 {
    30
}
fn default_true() -> bool {
    true
}

impl Default for Popup {
    fn default() -> Self {
        Self {
            width: 480,
            height: 270,
            corner: Corner::BottomRight,
            offset_x: 24,
            offset_y: 24,
            stack_gap: 12,
            monitor: MonitorSelect::Named("primary".into()),
            min_display_seconds: 8,
            linger_seconds: 15,
            watchdog_seconds: 20,
            max_display_seconds: 120,
            max_popups: 2,
            stream_mode: "webrtc".into(),
            video_fit: "cover".into(),
            show_stream_badge: false,
            page: "local".into(),
        }
    }
}

impl Default for Detection {
    fn default() -> Self {
        Self {
            labels: vec!["person".into()],
            cooldown_seconds: 60,
            ignore_false_positives: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl Config {
    /// Loads and validates the config, writing a starter file if none exists yet.
    pub fn load() -> Result<Self> {
        let path = crate::paths::config_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating config directory {}", parent.display()))?;
            }
            std::fs::write(path, EXAMPLE_CONFIG)
                .with_context(|| format!("writing starter config to {}", path.display()))?;
            return Err(ConfigError::Created(path.to_path_buf()).into());
        }

        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;

        let mut config: Config = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        config.normalize();

        let problems = config.problems();
        if !problems.is_empty() {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                count: problems.len(),
                details: problems
                    .iter()
                    .map(|p| format!("  - {p}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }
            .into());
        }

        Ok(config)
    }

    /// Trailing slashes would double up when we build URLs later.
    fn normalize(&mut self) {
        self.frigate.ui_url = self.frigate.ui_url.trim_end_matches('/').to_string();
        self.frigate.go2rtc_url = self.frigate.go2rtc_url.trim_end_matches('/').to_string();
        self.mqtt.topic_prefix = self.mqtt.topic_prefix.trim_matches('/').to_string();
    }

    /// Every validation failure, gathered so the user sees all of them in one pass.
    fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();

        if self.mqtt.host.trim().is_empty() {
            p.push("mqtt.host is empty".into());
        }
        if self.mqtt.port == 0 {
            p.push("mqtt.port must be 1-65535".into());
        }
        if self.mqtt.password.is_some() && self.mqtt.password_env.is_some() {
            p.push("mqtt.password and mqtt.password_env are both set - pick one".into());
        }
        if let Some(var) = &self.mqtt.password_env {
            if std::env::var(var).is_err() {
                p.push(format!(
                    "mqtt.password_env points at environment variable `{var}`, which is not set"
                ));
            }
        }
        if self.mqtt.topic_prefix.is_empty() {
            p.push("mqtt.topic_prefix is empty (the Frigate default is \"frigate\")".into());
        }

        for (field, url) in [
            ("frigate.ui_url", &self.frigate.ui_url),
            ("frigate.go2rtc_url", &self.frigate.go2rtc_url),
        ] {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                p.push(format!(
                    "{field} = \"{url}\" must start with http:// or https://"
                ));
            }
        }

        if self.popup.width == 0 || self.popup.height == 0 {
            p.push("popup.width and popup.height must both be greater than 0".into());
        }
        if self.popup.max_popups == 0 {
            p.push("popup.max_popups must be at least 1".into());
        }
        if self.popup.watchdog_seconds == 0 {
            p.push("popup.watchdog_seconds must be greater than 0".into());
        }
        if self.popup.max_display_seconds == 0 {
            p.push("popup.max_display_seconds must be greater than 0".into());
        }
        if self.popup.min_display_seconds > self.popup.max_display_seconds {
            p.push(format!(
                "popup.min_display_seconds ({}) cannot exceed popup.max_display_seconds ({})",
                self.popup.min_display_seconds, self.popup.max_display_seconds
            ));
        }
        if self.popup.linger_seconds > self.popup.max_display_seconds {
            p.push(format!(
                "popup.linger_seconds ({}) cannot exceed popup.max_display_seconds ({}), so the linger would always be cut short",
                self.popup.linger_seconds, self.popup.max_display_seconds
            ));
        }
        if let MonitorSelect::Named(name) = &self.popup.monitor {
            if !matches!(name.as_str(), "primary" | "cursor") {
                p.push(format!(
                    "popup.monitor = \"{name}\" is not recognised - use \"primary\", \"cursor\", or a 0-based monitor index"
                ));
            }
        }
        const MODES: [&str; 5] = ["webrtc", "mse", "hls", "mjpeg", "mp4"];
        for mode in self.popup.stream_mode.split(',').map(str::trim) {
            if !MODES.contains(&mode) {
                p.push(format!(
                    "popup.stream_mode contains unknown mode \"{mode}\" - valid modes are {}",
                    MODES.join(", ")
                ));
            }
        }

        const PAGES: [&str; 2] = ["local", "go2rtc"];
        if !PAGES.contains(&self.popup.page.as_str()) {
            p.push(format!(
                "popup.page = \"{}\" is not recognised - use {}",
                self.popup.page,
                PAGES.join(", ")
            ));
        }

        const FITS: [&str; 3] = ["cover", "contain", "fill"];
        if !FITS.contains(&self.popup.video_fit.as_str()) {
            p.push(format!(
                "popup.video_fit = \"{}\" is not recognised - use {}",
                self.popup.video_fit,
                FITS.join(", ")
            ));
        }

        if self.hotkey.enabled && self.hotkey.binding.trim().is_empty() {
            p.push("hotkey.binding is empty; set a key or use hotkey.enabled = false".into());
        }

        if self.detection.labels.is_empty() {
            p.push("detection.labels is empty, so nothing would ever trigger".into());
        }

        if self.cameras.is_empty() {
            p.push("no [[cameras]] entries defined".into());
        }
        let mut seen = BTreeSet::new();
        for (i, cam) in self.cameras.iter().enumerate() {
            let at = format!("cameras[{i}]");
            if cam.name.trim().is_empty() {
                p.push(format!("{at}.name is empty"));
            } else if !seen.insert(cam.name.as_str()) {
                p.push(format!("{at}.name = \"{}\" is defined twice", cam.name));
            }
            if cam.stream.trim().is_empty() {
                p.push(format!("{at}.stream is empty (the go2rtc stream name)"));
            }
            if cam.labels.as_ref().is_some_and(|l| l.is_empty()) {
                p.push(format!(
                    "{at}.labels is an empty list - remove it to inherit detection.labels"
                ));
            }
        }
        if !self.cameras.is_empty() && !self.cameras.iter().any(|c| c.enabled) {
            p.push("every camera has enabled = false, so nothing would ever trigger".into());
        }

        p
    }

    pub fn camera(&self, name: &str) -> Option<&Camera> {
        self.cameras.iter().find(|c| c.name == name)
    }

    /// Labels that should trigger for this camera, honouring the per-camera override.
    pub fn labels_for<'a>(&'a self, camera: &'a Camera) -> &'a [String] {
        camera
            .labels
            .as_deref()
            .unwrap_or(self.detection.labels.as_slice())
    }

    pub fn cooldown_for(&self, camera: &Camera) -> u64 {
        camera
            .cooldown_seconds
            .unwrap_or(self.detection.cooldown_seconds)
    }

    /// The MQTT password, resolved from either the literal or the environment variable.
    pub fn mqtt_password(&self) -> Result<Option<String>> {
        match (&self.mqtt.password, &self.mqtt.password_env) {
            (Some(p), _) => Ok(Some(p.clone())),
            (None, Some(var)) => std::env::var(var).map(Some).with_context(|| {
                format!("reading MQTT password from environment variable `{var}`")
            }),
            (None, None) => Ok(None),
        }
    }

    /// Frigate UI deep link for a camera, used when a popup is clicked.
    pub fn camera_ui_url(&self, camera: &str) -> String {
        format!("{}/cameras/{camera}", self.frigate.ui_url)
    }

    /// go2rtc player page for a stream.
    pub fn stream_url(&self, stream: &str) -> String {
        format!(
            "{}/stream.html?src={stream}&mode={}",
            self.frigate.go2rtc_url, self.popup.stream_mode
        )
    }

    /// go2rtc single-frame JPEG, used for the snapshot-first paint.
    pub fn frame_url(&self, stream: &str) -> String {
        format!("{}/api/frame.jpeg?src={stream}", self.frigate.go2rtc_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> &'static str {
        r#"
[mqtt]
host = "192.168.1.11"

[frigate]
ui_url = "https://192.168.1.10:8971/"
go2rtc_url = "http://192.168.1.10:1984"

[[cameras]]
name = "doorbell"
stream = "doorbell_sub"
"#
    }

    /// Writes `text` to its own temp directory so parallel tests never collide.
    fn parse_named(name: &str, text: &str) -> Result<Config> {
        let dir = std::env::temp_dir().join(format!("frigate-popup-test-{name}"));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        std::fs::write(&path, text)?;
        let result = Config::load_from(&path);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn minimal_config_fills_in_defaults() -> Result<()> {
        let c = parse_named("defaults", minimal())?;
        assert_eq!(c.mqtt.port, 1883);
        assert_eq!(c.mqtt.topic_prefix, "frigate");
        assert_eq!(c.popup.width, 480);
        assert_eq!(c.popup.corner, Corner::BottomRight);
        assert_eq!(c.popup.max_popups, 2);
        assert_eq!(c.detection.labels, vec!["person".to_string()]);
        assert_eq!(c.detection.cooldown_seconds, 60);
        assert!(c.cameras[0].enabled);
        Ok(())
    }

    #[test]
    fn trailing_slashes_are_stripped_from_urls() -> Result<()> {
        let c = parse_named("urls", minimal())?;
        assert_eq!(c.frigate.ui_url, "https://192.168.1.10:8971");
        assert_eq!(
            c.camera_ui_url("doorbell"),
            "https://192.168.1.10:8971/cameras/doorbell"
        );
        Ok(())
    }

    #[test]
    fn per_camera_overrides_win_over_globals() -> Result<()> {
        let text = format!("{}\nlabels = [\"dog\"]\ncooldown_seconds = 5\n", minimal());
        let c = parse_named("overrides", &text)?;
        let cam = c.camera("doorbell").expect("camera present");
        assert_eq!(c.labels_for(cam), ["dog".to_string()]);
        assert_eq!(c.cooldown_for(cam), 5);
        Ok(())
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let text = r#"
[mqtt]
host = ""
port = 0

[frigate]
ui_url = "192.168.1.10:8971"
go2rtc_url = "ftp://nope"

[popup]
max_display_seconds = 5
min_display_seconds = 30
max_popups = 0
monitor = "leftmost"
stream_mode = "webrtc,telepathy"

[[cameras]]
name = "doorbell"
stream = "a"

[[cameras]]
name = "doorbell"
stream = ""
"#;
        let err = parse_named("problems", text).expect_err("config should be rejected");
        let msg = format!("{err}");
        for expected in [
            "mqtt.host is empty",
            "mqtt.port must be",
            "frigate.ui_url",
            "frigate.go2rtc_url",
            "min_display_seconds",
            "max_popups",
            "leftmost",
            "telepathy",
            "defined twice",
            "stream is empty",
        ] {
            assert!(msg.contains(expected), "missing {expected:?} in:\n{msg}");
        }
    }

    #[test]
    fn unknown_keys_are_rejected_rather_than_silently_ignored() {
        let text = format!("{}\n[detection]\nlabel = [\"person\"]\n", minimal());
        let err = parse_named("typo", &text).expect_err("typo should be rejected");
        assert!(format!("{err}").contains("label"));
    }

    #[test]
    fn missing_file_writes_the_starter_config() -> Result<()> {
        let dir = std::env::temp_dir().join("fp-test-starter");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        let err = Config::load_from(&path).expect_err("first run should not succeed");
        assert!(format!("{err}").contains("starter config"));
        assert!(path.is_file(), "starter config should have been written");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn the_shipped_example_config_is_itself_valid() -> Result<()> {
        // Guards against the example drifting away from the schema.
        let c = parse_named("example", EXAMPLE_CONFIG)?;
        assert!(!c.cameras.is_empty());
        Ok(())
    }
}
