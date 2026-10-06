//! Launch decision for the composer glow. No GPU types.

use std::path::{Path, PathBuf};

pub(crate) const MARKER_NAME: &str = "renderer-trying";
pub(crate) const FALLBACK_NOTICE: &str =
    "Composer glow was turned off because the GPU renderer failed last time.";
pub(crate) const SETTINGS_CAPTION: &str =
    "Soft light around the composer while Grok replies. Uses the GPU renderer after restart.";
pub(crate) const GLOW_EXPAND: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    Glow,
    GlowAfterCrash,
    Wgpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnvOverride {
    None,
    Glow,
    Wgpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkerAction {
    Keep,
    Clear,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub launch: Launch,
    pub marker: MarkerAction,
    pub turn_setting_off: bool,
    pub show_notice: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Persisted {
    pub composer_glow: bool,
    pub marker: bool,
    pub notice: bool,
}

/// Armed while this launch is on wgpu. The first `logic` call runs before anything is presented,
/// so the crash marker is cleared on the second call: the first wgpu frame rendered and presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FirstFrame {
    armed: bool,
    seen: u8,
    cleared: bool,
}

impl FirstFrame {
    pub(crate) const fn idle() -> Self {
        Self {
            armed: false,
            seen: 0,
            cleared: false,
        }
    }

    pub(crate) const fn arm() -> Self {
        Self {
            armed: true,
            seen: 0,
            cleared: false,
        }
    }

    /// True exactly once while armed: on the frame after the first one was presented.
    pub(crate) fn on_frame(&mut self) -> bool {
        if !self.armed || self.cleared {
            return false;
        }
        self.seen = self.seen.saturating_add(1);
        if self.seen >= 2 {
            self.cleared = true;
            true
        } else {
            false
        }
    }
}

pub(crate) fn parse_env(raw: Option<&str>) -> EnvOverride {
    let Some(raw) = raw else {
        return EnvOverride::None;
    };
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("glow") {
        EnvOverride::Glow
    } else if raw.eq_ignore_ascii_case("wgpu") {
        EnvOverride::Wgpu
    } else {
        EnvOverride::None
    }
}

pub(crate) fn decide(setting_on: bool, marker_present: bool, env: EnvOverride) -> Decision {
    match env {
        EnvOverride::Glow => Decision {
            launch: Launch::Glow,
            marker: MarkerAction::Keep,
            turn_setting_off: false,
            show_notice: false,
        },
        EnvOverride::Wgpu => Decision {
            launch: Launch::Wgpu,
            marker: MarkerAction::Write,
            turn_setting_off: false,
            show_notice: false,
        },
        EnvOverride::None if setting_on && marker_present => Decision {
            launch: Launch::GlowAfterCrash,
            marker: MarkerAction::Clear,
            turn_setting_off: true,
            show_notice: true,
        },
        EnvOverride::None if setting_on => Decision {
            launch: Launch::Wgpu,
            marker: MarkerAction::Write,
            turn_setting_off: false,
            show_notice: false,
        },
        EnvOverride::None => Decision {
            launch: Launch::Glow,
            marker: if marker_present {
                MarkerAction::Clear
            } else {
                MarkerAction::Keep
            },
            turn_setting_off: false,
            show_notice: false,
        },
    }
}

#[cfg(test)]
pub(crate) fn pick(setting_on: bool, marker_present: bool) -> Launch {
    decide(setting_on, marker_present, EnvOverride::None).launch
}

pub(crate) fn apply_decision(mut state: Persisted, decision: &Decision) -> Persisted {
    match decision.marker {
        MarkerAction::Clear => state.marker = false,
        MarkerAction::Write => state.marker = true,
        MarkerAction::Keep => {}
    }
    if decision.turn_setting_off {
        state.composer_glow = false;
    }
    state.notice = decision.show_notice;
    state
}

/// `run_native` failed or the wgpu render state was missing. Switch off, marker gone, notice on.
pub(crate) fn fallback_after_error(mut state: Persisted) -> Persisted {
    state.composer_glow = false;
    state.marker = false;
    state.notice = true;
    state
}

/// Delete the marker once the first wgpu frame was presented. Later frames leave the file alone.
pub(crate) fn clear_on_first_frame(frame: &mut FirstFrame, dir: &Path) -> bool {
    if !frame.on_frame() {
        return false;
    }
    let _ = clear_marker(dir);
    true
}

pub(crate) fn marker_path(dir: &Path) -> PathBuf {
    dir.join(MARKER_NAME)
}

pub(crate) fn marker_exists(dir: &Path) -> bool {
    marker_path(dir).is_file()
}

pub(crate) fn write_marker(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(marker_path(dir), b"wgpu\n")
}

pub(crate) fn clear_marker(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(marker_path(dir)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_truth_table() {
        assert_eq!(pick(false, false), Launch::Glow);
        assert_eq!(pick(true, false), Launch::Wgpu);
        assert_eq!(pick(true, true), Launch::GlowAfterCrash);
        assert_eq!(pick(false, true), Launch::Glow);

        let off = decide(false, false, EnvOverride::None);
        assert_eq!(off.launch, Launch::Glow);
        assert_eq!(off.marker, MarkerAction::Keep);
        assert!(!off.turn_setting_off && !off.show_notice);

        let on = decide(true, false, EnvOverride::None);
        assert_eq!(on.marker, MarkerAction::Write);
        assert!(!on.turn_setting_off && !on.show_notice);
        let started = apply_decision(
            Persisted {
                composer_glow: true,
                marker: false,
                notice: false,
            },
            &on,
        );
        assert!(started.composer_glow && started.marker && !started.notice);

        let crash = decide(true, true, EnvOverride::None);
        assert_eq!(crash.marker, MarkerAction::Clear);
        assert!(crash.turn_setting_off && crash.show_notice);
        let recovered = apply_decision(
            Persisted {
                composer_glow: true,
                marker: true,
                notice: false,
            },
            &crash,
        );
        assert!(!recovered.composer_glow && !recovered.marker && recovered.notice);

        let stale = decide(false, true, EnvOverride::None);
        assert_eq!(stale.launch, Launch::Glow);
        assert_eq!(stale.marker, MarkerAction::Clear);
        assert!(!stale.turn_setting_off && !stale.show_notice);
        let cleaned = apply_decision(
            Persisted {
                composer_glow: false,
                marker: true,
                notice: false,
            },
            &stale,
        );
        assert!(!cleaned.composer_glow && !cleaned.marker && !cleaned.notice);
    }

    #[test]
    fn env_override_parse_and_side_effects() {
        assert_eq!(parse_env(None), EnvOverride::None);
        assert_eq!(parse_env(Some("")), EnvOverride::None);
        assert_eq!(parse_env(Some("  ")), EnvOverride::None);
        assert_eq!(parse_env(Some("opengl")), EnvOverride::None);
        assert_eq!(parse_env(Some("Glow")), EnvOverride::Glow);
        assert_eq!(parse_env(Some(" glow ")), EnvOverride::Glow);
        assert_eq!(parse_env(Some("WGPU")), EnvOverride::Wgpu);
        assert_eq!(parse_env(Some(" wgpu\n")), EnvOverride::Wgpu);

        let escape = decide(true, true, EnvOverride::Glow);
        assert_eq!(escape.launch, Launch::Glow);
        assert_eq!(escape.marker, MarkerAction::Keep);
        assert!(!escape.turn_setting_off && !escape.show_notice);

        let forced = decide(false, true, EnvOverride::Wgpu);
        assert_eq!(forced.launch, Launch::Wgpu);
        assert_eq!(forced.marker, MarkerAction::Write);
        assert!(!forced.turn_setting_off && !forced.show_notice);
    }

    #[test]
    fn marker_round_trip_in_test_config_dir() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("glow-marker");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let dir = crate::config::config_dir();
        assert!(!marker_exists(&dir));
        write_marker(&dir).unwrap();
        assert!(marker_exists(&dir));
        assert_eq!(std::fs::read(marker_path(&dir)).unwrap(), b"wgpu\n");
        clear_marker(&dir).unwrap();
        assert!(!marker_exists(&dir));
        clear_marker(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn first_frame_clears_once() {
        let mut idle = FirstFrame::idle();
        assert!(!idle.on_frame());
        assert!(!idle.on_frame());
        assert!(!idle.on_frame());

        let mut frame = FirstFrame::arm();
        assert!(
            !frame.on_frame(),
            "nothing is presented before the first logic call"
        );
        assert!(
            frame.on_frame(),
            "the second call means frame one was presented"
        );
        assert!(!frame.on_frame());
        assert!(!frame.on_frame());

        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("glow-frame");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let dir = crate::config::config_dir();
        write_marker(&dir).unwrap();
        let mut frame = FirstFrame::arm();
        assert!(!clear_on_first_frame(&mut frame, &dir));
        assert!(
            marker_exists(&dir),
            "marker survives until frame one was presented"
        );
        assert!(clear_on_first_frame(&mut frame, &dir));
        assert!(!marker_exists(&dir));
        write_marker(&dir).unwrap();
        assert!(!clear_on_first_frame(&mut frame, &dir));
        assert!(marker_exists(&dir));
        let mut idle = FirstFrame::idle();
        assert!(!clear_on_first_frame(&mut idle, &dir));
        assert!(!clear_on_first_frame(&mut idle, &dir));
        assert!(marker_exists(&dir));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fallback_after_error_turns_the_setting_off() {
        let next = fallback_after_error(Persisted {
            composer_glow: true,
            marker: true,
            notice: false,
        });
        assert!(!next.composer_glow);
        assert!(!next.marker);
        assert!(next.notice);
    }
}
