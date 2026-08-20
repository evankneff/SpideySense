//! Command line arguments. Deliberately hand-rolled - there are two flags and adding a
//! parser crate for them would be more code than this.

pub const USAGE: &str = "\
frigate-popup - camera popups driven by Frigate NVR events

USAGE:
    frigate-popup [OPTIONS]

OPTIONS:
    --preview [CAMERAS]  Open popups immediately without waiting for a detection, so you
                         can see the size, position and stream quality. CAMERAS is a
                         comma-separated list of camera names from the config; omit it to
                         preview the first `popup.max_popups` enabled cameras. The windows
                         stay up until you quit from the tray.
    --simulate CAMERA    Inject a fake `new` detection for CAMERA once at startup and run
                         it through the real trigger logic, then carry on normally. Accepts
                         CAMERA:LABEL to simulate something other than a person.
    -h, --help           Show this message and exit.

Config and logs live in %APPDATA%\\frigate-popup\\, or next to the executable when a
config.toml sits beside it.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Normal operation: sit in the tray and wait for MQTT events.
    Normal,
    /// Open popups straight away. An empty list means "pick sensible defaults".
    Preview {
        cameras: Vec<String>,
    },
    /// Run normally, but inject one fake detection at startup.
    Simulate {
        camera: String,
        label: String,
    },
    Help,
}

pub const DEFAULT_SIMULATED_LABEL: &str = "person";

pub fn parse<I, S>(args: I) -> Result<Mode, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut mode = Mode::Normal;
    // Skip argv[0].
    let mut args = args.into_iter().skip(1).peekable();

    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "-h" | "--help" => return Ok(Mode::Help),
            "--preview" => {
                // The camera list is optional, so only consume the next token when it is
                // not itself a flag.
                let cameras = match args.peek() {
                    Some(next) if !next.as_ref().starts_with('-') => {
                        let list = args
                            .next()
                            .map(|s| s.as_ref().to_string())
                            .unwrap_or_default();
                        list.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect()
                    }
                    _ => Vec::new(),
                };
                mode = Mode::Preview { cameras };
            }
            "--simulate" => {
                let value = args
                    .next()
                    .map(|s| s.as_ref().to_string())
                    .filter(|s| !s.starts_with('-') && !s.is_empty())
                    .ok_or_else(|| {
                        format!(
                            "--simulate needs a camera name, e.g. --simulate doorbell\n\n{USAGE}"
                        )
                    })?;
                let (camera, label) = match value.split_once(':') {
                    Some((camera, label)) if !label.is_empty() => (camera, label),
                    _ => (value.as_str(), DEFAULT_SIMULATED_LABEL),
                };
                mode = Mode::Simulate {
                    camera: camera.to_string(),
                    label: label.to_string(),
                };
            }
            other => return Err(format!("unrecognised argument: {other}\n\n{USAGE}")),
        }
    }

    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Mode, String> {
        let mut all = vec!["frigate-popup.exe"];
        all.extend_from_slice(args);
        parse(all)
    }

    #[test]
    fn no_arguments_means_normal_operation() {
        assert_eq!(parse_args(&[]), Ok(Mode::Normal));
    }

    #[test]
    fn preview_without_a_camera_list_is_allowed() {
        assert_eq!(
            parse_args(&["--preview"]),
            Ok(Mode::Preview { cameras: vec![] })
        );
    }

    #[test]
    fn preview_accepts_a_comma_separated_list() {
        assert_eq!(
            parse_args(&["--preview", "doorbell, garage ,sidedoor"]),
            Ok(Mode::Preview {
                cameras: vec!["doorbell".into(), "garage".into(), "sidedoor".into()]
            })
        );
    }

    #[test]
    fn a_following_flag_is_not_swallowed_as_a_camera_name() {
        assert_eq!(parse_args(&["--preview", "--help"]), Ok(Mode::Help));
    }

    #[test]
    fn simulate_defaults_to_person() {
        assert_eq!(
            parse_args(&["--simulate", "doorbell"]),
            Ok(Mode::Simulate {
                camera: "doorbell".into(),
                label: "person".into()
            })
        );
    }

    #[test]
    fn simulate_accepts_an_explicit_label() {
        assert_eq!(
            parse_args(&["--simulate", "garage:dog"]),
            Ok(Mode::Simulate {
                camera: "garage".into(),
                label: "dog".into()
            })
        );
    }

    #[test]
    fn simulate_without_a_camera_is_an_error() {
        let err = parse_args(&["--simulate"]).expect_err("should be rejected");
        assert!(err.contains("needs a camera name"));
    }

    #[test]
    fn unknown_arguments_are_rejected_with_usage() {
        let err = parse_args(&["--nope"]).expect_err("should be rejected");
        assert!(err.contains("unrecognised argument: --nope"));
        assert!(err.contains("USAGE"));
    }
}
