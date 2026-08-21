//! Popup window creation, placement and focus suppression.

use crate::config::{Camera, Config, Corner, MonitorSelect, Popup};
use anyhow::{anyhow, Context, Result};
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
use tracing::{debug, info, warn};

/// Why a popup exists. A detection popup is an alert and says what was detected; a live
/// popup is one you asked for yourself, and must not claim anything was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Detection,
    Live,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Detection => "detection",
            Kind::Live => "live",
        }
    }
}

/// Window labels are derived from the camera name, so a second event for the same
/// camera finds the existing popup instead of stacking a duplicate on top of it.
pub fn label_for(camera: &str) -> String {
    format!("popup-{camera}")
}

/// A monitor's usable area in physical pixels, i.e. excluding the taskbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Top-left corner for the popup occupying `slot`, in physical pixels.
///
/// Config sizes and offsets are *logical* pixels so a 480x270 popup looks the same on a
/// 100% and a 150% display; everything is scaled to physical here because that is what
/// `set_position`/`set_size` take. Slots stack away from the chosen corner: downward from
/// a top corner, upward from a bottom one, so slot 0 always hugs the corner.
pub fn slot_position(area: Rect, scale: f64, popup: &Popup, slot: usize) -> PhysicalPosition<i32> {
    let phys = |logical: i32| (f64::from(logical) * scale).round() as i32;

    let w = phys(popup.width as i32);
    let h = phys(popup.height as i32);
    let off_x = phys(popup.offset_x);
    let off_y = phys(popup.offset_y);
    let stack = slot as i32 * (h + phys(popup.stack_gap));

    let left = area.x + off_x;
    let right = area.x + area.w - w - off_x;
    let top = area.y + off_y + stack;
    let bottom = area.y + area.h - h - off_y - stack;

    match popup.corner {
        Corner::TopLeft => PhysicalPosition::new(left, top),
        Corner::TopRight => PhysicalPosition::new(right, top),
        Corner::BottomLeft => PhysicalPosition::new(left, bottom),
        Corner::BottomRight => PhysicalPosition::new(right, bottom),
    }
}

/// CSS injected into go2rtc's `stream.html`.
///
/// That page is built for a full browser tab: it letterboxes any stream whose aspect ratio
/// does not match the window, and paints a mode badge ("RTC" / "MSE") in the corner. Neither
/// belongs in a glanceable notification, and the doorbell is 4:3 (896x672) against a 16:9
/// popup, so the letterboxing is very visible.
///
/// Runs before the document is parsed, hence the DOMContentLoaded fallback - `document.head`
/// does not exist yet on the first pass.
fn overlay_script(popup: &Popup) -> String {
    let mut css = format!(
        "html,body{{background:#000!important;margin:0!important;overflow:hidden!important}}\
         video-stream video{{object-fit:{}!important;width:100%!important;height:100%!important}}",
        popup.video_fit
    );
    if !popup.show_stream_badge {
        css.push_str("video-stream .info{display:none!important}");
    }

    // Serialised as a JSON string so quotes and braces in the CSS cannot break out.
    let literal = serde_json::to_string(&css).unwrap_or_else(|_| String::from("\"\""));

    // Ordering matters. This runs before the document is parsed, so `document.head` and
    // `document.documentElement` are both null on the first pass. Registering the
    // listeners *first* and guarding the eager attempt means a throw there cannot take
    // the fallbacks down with it - which is exactly what happened before.
    format!(
        "(function(){{var css={literal};\
         function inject(){{\
           try{{\
             if(document.getElementById('frigate-popup-style'))return true;\
             var root=document.head||document.documentElement;\
             if(!root)return false;\
             var s=document.createElement('style');\
             s.id='frigate-popup-style';s.textContent=css;root.appendChild(s);\
             return true;\
           }}catch(e){{return false;}}\
         }}\
         document.addEventListener('DOMContentLoaded',inject);\
         window.addEventListener('load',inject);\
         if(!inject()){{\
           var t=setInterval(function(){{if(inject())clearInterval(t);}},16);\
           setTimeout(function(){{clearInterval(t);}},5000);\
         }}}})();"
    )
}

/// Injects `window.__POPUP__` for the bundled page.
///
/// Passed this way rather than through the URL: `WebviewUrl::App` takes a path, and
/// smuggling a query string through it would mean escaping camera names by hand.
fn popup_context_script(config: &Config, camera: &Camera, kind: Kind) -> String {
    // The label is only meaningful for a detection. Sending it for a live view would put
    // a "PERSON" badge on a window opened by hand, which reads as a false alert.
    let label = match kind {
        Kind::Detection => config
            .labels_for(camera)
            .first()
            .cloned()
            .unwrap_or_default(),
        Kind::Live => String::new(),
    };
    let context = serde_json::json!({
        "camera": camera.name,
        "stream": camera.stream,
        "go2rtc": config.frigate.go2rtc_url,
        "fit": config.popup.video_fit,
        "kind": kind.as_str(),
        "label": label,
    });
    let literal = serde_json::to_string(&context).unwrap_or_else(|_| String::from("{}"));
    format!("window.__POPUP__={literal};")
}

fn pick_monitor<R: Runtime>(app: &AppHandle<R>, select: &MonitorSelect) -> Result<Monitor> {
    let fallback = || -> Result<Monitor> {
        app.primary_monitor()
            .context("querying the primary monitor")?
            .ok_or_else(|| anyhow!("no monitors reported by the system"))
    };

    match select {
        MonitorSelect::Named(name) if name == "cursor" => {
            let pos = app
                .cursor_position()
                .context("querying the cursor position")?;
            match app
                .monitor_from_point(pos.x, pos.y)
                .context("querying the monitor under the cursor")?
            {
                Some(monitor) => Ok(monitor),
                None => {
                    warn!("no monitor contains the cursor; falling back to the primary monitor");
                    fallback()
                }
            }
        }
        MonitorSelect::Index(i) => {
            let monitors = app.available_monitors().context("listing monitors")?;
            let count = monitors.len();
            match monitors.into_iter().nth(*i) {
                Some(monitor) => Ok(monitor),
                None => {
                    warn!("popup.monitor = {i} but only {count} monitor(s) present; using the primary monitor");
                    fallback()
                }
            }
        }
        // "primary", and anything else validation already accepted.
        MonitorSelect::Named(_) => fallback(),
    }
}

/// The chosen monitor's usable area, its scale factor, and a name for logging.
fn geometry<R: Runtime>(app: &AppHandle<R>, config: &Config) -> Result<(Rect, f64, String)> {
    let monitor = pick_monitor(app, &config.popup.monitor)?;
    let work = monitor.work_area();
    let area = Rect {
        x: work.position.x,
        y: work.position.y,
        w: work.size.width as i32,
        h: work.size.height as i32,
    };
    let name = monitor
        .name()
        .map(String::as_str)
        .unwrap_or("<unnamed>")
        .to_string();
    Ok((area, monitor.scale_factor(), name))
}

/// Slides an already-open popup into a different stack slot.
///
/// Used when a popup below it closes, so the survivors close up the gap instead of
/// leaving a hole in the corner.
pub fn move_to_slot<R: Runtime>(
    app: &AppHandle<R>,
    config: &Config,
    camera: &str,
    slot: usize,
) -> Result<()> {
    let Some(window) = app.get_webview_window(&label_for(camera)) else {
        return Ok(());
    };
    let (area, scale, _) = geometry(app, config)?;
    let position = slot_position(area, scale, &config.popup, slot);
    debug!(
        camera,
        slot,
        x = position.x,
        y = position.y,
        "restacking popup"
    );
    window
        .set_position(position)
        .context("repositioning the popup")?;
    Ok(())
}

/// Opens a popup for `camera` in the given stack slot, or returns the existing window
/// if one is already open for that camera.
pub fn open<R: Runtime>(
    app: &AppHandle<R>,
    config: &Config,
    camera: &Camera,
    slot: usize,
    kind: Kind,
) -> Result<WebviewWindow<R>> {
    let label = label_for(&camera.name);
    if let Some(existing) = app.get_webview_window(&label) {
        debug!(camera = %camera.name, "popup already open; reusing it");
        return Ok(existing);
    }

    let (area, scale, monitor_name) = geometry(app, config)?;
    let position = slot_position(area, scale, &config.popup, slot);
    let size = PhysicalSize::new(
        (f64::from(config.popup.width) * scale).round() as u32,
        (f64::from(config.popup.height) * scale).round() as u32,
    );

    // "local" serves our own page from the bundle and does its own WebRTC signalling;
    // "go2rtc" points straight at stream.html and restyles it from the outside.
    let local = config.popup.page == "local";
    let (webview_url, init_script) = if local {
        (
            WebviewUrl::App("popup.html".into()),
            popup_context_script(config, camera, kind),
        )
    } else {
        let url = config.stream_url(&camera.stream);
        let parsed = url
            .parse::<tauri::Url>()
            .with_context(|| format!("building a stream URL for camera {}: {url}", camera.name))?;
        (WebviewUrl::External(parsed), overlay_script(&config.popup))
    };

    info!(
        camera = %camera.name,
        stream = %camera.stream,
        page = if local { "local" } else { "go2rtc" },
        kind = kind.as_str(),
        slot,
        monitor = %monitor_name,
        scale,
        x = position.x, y = position.y, w = size.width, h = size.height,
        "opening popup"
    );

    // Built hidden so it can be positioned before it ever paints - otherwise it flashes
    // at the default location first.
    let window = WebviewWindowBuilder::new(app, &label, webview_url)
        .title(format!("{} - frigate-popup", camera.name))
        .inner_size(
            f64::from(config.popup.width),
            f64::from(config.popup.height),
        )
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .shadow(true)
        .visible(false)
        .initialization_script(init_script)
        .build()
        .with_context(|| format!("creating the popup window for {}", camera.name))?;

    window.set_size(size).context("sizing the popup")?;
    window
        .set_position(position)
        .context("positioning the popup")?;

    // Must happen before the window is shown, or the first show still activates it.
    if let Err(e) = suppress_activation(&window) {
        warn!("could not apply the no-activate window style: {e:#}");
    }

    window.show().context("showing the popup")?;
    Ok(window)
}

/// Adds `WS_EX_NOACTIVATE` so the popup can never take keyboard focus.
///
/// `focused(false)` alone is not enough on Windows: WebView2 can still pull focus when the
/// webview finishes initialising, which would swallow keystrokes mid-typing.
#[cfg(windows)]
fn suppress_activation<R: Runtime>(window: &WebviewWindow<R>) -> Result<()> {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    };

    // Tauri and this crate may link different `windows` versions; HWND is a newtype over
    // the same raw pointer either way.
    let hwnd = HWND(window.hwnd().context("retrieving the popup HWND")?.0);

    // SAFETY: `hwnd` is a live window handle owned by the Tauri runtime for as long as
    // this window exists, and both calls are plain style reads/writes on it.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, current | WS_EX_NOACTIVATE.0 as isize);
    }
    Ok(())
}

#[cfg(not(windows))]
fn suppress_activation<R: Runtime>(_window: &WebviewWindow<R>) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1920x1080 with a 40px taskbar along the bottom, at 100% scaling.
    fn area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1040,
        }
    }

    fn popup(corner: Corner) -> Popup {
        Popup {
            corner,
            ..Popup::default()
        }
    }

    #[test]
    fn slot_zero_hugs_each_corner() {
        let p = popup(Corner::BottomRight);
        let pos = slot_position(area(), 1.0, &p, 0);
        assert_eq!((pos.x, pos.y), (1920 - 480 - 24, 1040 - 270 - 24));

        let pos = slot_position(area(), 1.0, &popup(Corner::TopLeft), 0);
        assert_eq!((pos.x, pos.y), (24, 24));

        let pos = slot_position(area(), 1.0, &popup(Corner::TopRight), 0);
        assert_eq!((pos.x, pos.y), (1920 - 480 - 24, 24));

        let pos = slot_position(area(), 1.0, &popup(Corner::BottomLeft), 0);
        assert_eq!((pos.x, pos.y), (24, 1040 - 270 - 24));
    }

    #[test]
    fn stacking_moves_away_from_the_chosen_corner() {
        // Bottom corners stack upward...
        let bottom = popup(Corner::BottomRight);
        let slot0 = slot_position(area(), 1.0, &bottom, 0);
        let slot1 = slot_position(area(), 1.0, &bottom, 1);
        assert_eq!(slot1.x, slot0.x, "stacking must not move horizontally");
        assert_eq!(slot0.y - slot1.y, 270 + 12, "one popup height plus the gap");

        // ...top corners stack downward.
        let top = popup(Corner::TopLeft);
        let slot0 = slot_position(area(), 1.0, &top, 0);
        let slot1 = slot_position(area(), 1.0, &top, 1);
        assert_eq!(slot1.y - slot0.y, 270 + 12);
    }

    #[test]
    fn logical_sizes_are_scaled_to_physical_pixels() {
        // Same 1920x1040 panel at 150%: the work area is already physical, but the popup
        // and its offsets must grow so it occupies the same apparent size.
        let p = popup(Corner::BottomRight);
        let pos = slot_position(area(), 1.5, &p, 0);
        assert_eq!(pos.x, 1920 - 720 - 36);
        assert_eq!(pos.y, 1040 - 405 - 36);
    }

    use crate::testutil::config_from;

    #[test]
    fn the_popup_context_carries_everything_the_page_needs() {
        let mut config = config_from(crate::testutil::BASE_CONFIG);
        config.popup.video_fit = "contain".into();
        let camera = config.camera("front_doorbell").expect("camera").clone();

        let script = popup_context_script(&config, &camera, Kind::Detection);
        assert!(script.starts_with("window.__POPUP__="));
        for expected in [
            r#""camera":"front_doorbell""#,
            r#""stream":"doorbell_sub""#,
            r#""go2rtc":"http://192.168.1.10:1984""#,
            r#""fit":"contain""#,
            r#""label":"person""#,
            r#""kind":"detection""#,
        ] {
            assert!(script.contains(expected), "missing {expected} in {script}");
        }
    }

    #[test]
    fn a_live_popup_never_claims_something_was_detected() {
        let config = config_from(crate::testutil::BASE_CONFIG);
        let camera = config.camera("front_doorbell").expect("camera").clone();
        let script = popup_context_script(&config, &camera, Kind::Live);
        assert!(script.contains(r#""kind":"live""#));
        assert!(
            script.contains(r#""label":"""#),
            "a hand-opened window must not carry a detection label: {script}"
        );
        assert!(!script.contains("person"));
    }

    #[test]
    fn overlay_hides_the_badge_and_applies_the_configured_fit() {
        let script = overlay_script(&Popup::default());
        assert!(script.contains("object-fit:cover"));
        assert!(script.contains(".info{display:none!important}"));
    }

    #[test]
    fn overlay_registers_its_fallbacks_before_attempting_to_inject() {
        // Regression: the script runs before the document exists, so the eager inject()
        // threw on a null root and took the DOMContentLoaded registration down with it,
        // meaning the CSS never landed at all.
        let script = overlay_script(&Popup::default());
        let listener = script
            .find("DOMContentLoaded")
            .expect("listener registered");
        let eager = script
            .rfind("if(!inject())")
            .expect("eager attempt present");
        assert!(
            listener < eager,
            "listeners must be registered before the first inject() attempt"
        );
        assert!(script.contains("catch"), "inject must not be able to throw");
        assert!(
            script.contains("setInterval"),
            "needs a poll for when both load events have already fired"
        );
    }

    #[test]
    fn overlay_keeps_the_badge_when_asked() {
        let popup = Popup {
            show_stream_badge: true,
            video_fit: "contain".into(),
            ..Popup::default()
        };
        let script = overlay_script(&popup);
        assert!(script.contains("object-fit:contain"));
        assert!(!script.contains("display:none"));
    }

    #[test]
    fn monitor_offsets_are_respected_on_a_secondary_display() {
        // A second monitor placed to the left of the primary has a negative origin.
        let left_monitor = Rect {
            x: -1920,
            y: 0,
            w: 1920,
            h: 1040,
        };
        let pos = slot_position(left_monitor, 1.0, &popup(Corner::BottomRight), 0);
        assert_eq!((pos.x, pos.y), (-1920 + 1920 - 480 - 24, 1040 - 270 - 24));
    }
}
