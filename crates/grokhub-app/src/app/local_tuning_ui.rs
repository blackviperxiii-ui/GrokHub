//! Settings → AI models → Local AI: how the on-device model runs.
//!
//! - "Local AI for chats" picks when the device answers while you're online:
//!   only as a fallback (default), first for your chats, or for every step it
//!   can run. The router reads it as [`LocalFirst`](local::LocalFirst).
//! - Context size, GPU offload and CPU threads are launch overrides for
//!   `llama-server` ([`local_autoconfig::Overrides`]); a change restarts a
//!   running model as soon as no request is in flight.
//! - "Keep the model loaded" turns the idle unload off.
//! - The governor rows (games, battery, idle) write `{config}/local/governor.json`,
//!   which the sampler re-reads on its own.
//!
//! Everything lives in `app.json` `localTuning` except the governor rules,
//! which keep their own file. The tick re-applies the saved values once.

use std::sync::Mutex;

use grokhub_agent::route::local::{self, LocalFirst};
use grokhub_agent::route::{local_autoconfig, local_gov, local_perf, local_sidecar};
use grokhub_core::local_governor::Rules;

use super::*;
use crate::config::LocalTuning;

const FIRST_ROW: &str = "Local AI for chats";
const FIRST_HINT: &str = "When the on-device model answers while you're online. Offline, or with no sign-in or key, it answers anyway.";
const FIRST_LABELS: [&str; 3] = [
    "Only when nothing else can answer",
    "First for your chats",
    "For everything it can run",
];
const CTX_ROW: &str = "Context size";
const CTX_HINT: &str = "How much of the chat the local model keeps in view. Auto fits your memory; a bigger size loads slower and may move layers off the GPU.";
const CTX_CHOICES: [(&str, u32); 6] = [
    ("Auto", 0),
    ("4k tokens", 4_096),
    ("8k tokens", 8_192),
    ("16k tokens", 16_384),
    ("32k tokens", 32_768),
    ("64k tokens", 65_536),
];
const GPU_ROW: &str = "GPU offload";
const GPU_HINT: &str = "Auto puts as many layers on the GPU as fit. CPU only frees the GPU for games and is much slower.";
const GPU_CHOICES: [(&str, &str); 3] = [
    ("Auto", ""),
    ("All layers on the GPU", "all"),
    ("CPU only", "cpu"),
];
const THREADS_ROW: &str = "CPU threads";
const THREADS_HINT: &str =
    "Auto follows the power state (fewer threads while you game or on battery).";
const THREADS_CHOICES: [(&str, &str); 3] = [
    ("Auto", ""),
    ("All cores", "all"),
    ("Half the cores", "half"),
];
const KEEP_ROW: &str = "Keep the model loaded";
const KEEP_HINT: &str = "Stays in memory between chats instead of unloading after 10 idle minutes. Answers start faster; the memory stays in use.";
const GAMES_ROW: &str = "When a game or fullscreen app runs";
const GAMES_HINT: &str = "Unload frees the GPU and RAM for the game; the model loads again on the next ask once the game is gone. Share keeps it loaded but sized to the GPU memory, RAM and cores the game leaves free, and re-sizes it when the game takes more. When the model is the only one that can answer (offline, Local AI first), Unload shares instead.";
const GAMES_LABELS: [&str; 3] = [
    "Unload the model (the game gets everything)",
    "Share with the game (fit into what it leaves free)",
    "Ignore games",
];
const BATTERY_ROW: &str = "Slow down on battery";
const BATTERY_HINT: &str = "Fewer threads and GPU layers while unplugged.";
const IDLE_ROW: &str = "Speed up when the PC is idle";
const IDLE_HINT: &str = "After 5 idle minutes on AC power the model gets every core.";
const RESTART_NOTE: &str =
    "Saved. The local model restarts with the new launch settings once it's idle.";
const GOV_ROW_ID: &str = "local-governor-rules";

/// The last tuning the tick applied, so Settings and startup apply each once.
static APPLIED: Mutex<Option<LocalTuning>> = Mutex::new(None);

/// What a tuning means for `llama-server`'s launch. `cores` is the physical
/// core count when known.
pub(super) fn overrides_for(t: &LocalTuning, cores: Option<u32>) -> local_autoconfig::Overrides {
    local_autoconfig::Overrides {
        ngl: match t.gpu.as_str() {
            "all" => Some(u32::MAX),
            "cpu" => Some(0),
            _ => None,
        },
        ctx: (t.ctx > 0).then_some(t.ctx),
        threads: match t.threads.as_str() {
            "all" => Some(u32::MAX),
            "half" => Some((cores.unwrap_or(8) / 2).max(1)),
            _ => None,
        },
        kv: None,
    }
}

/// Context, GPU or threads changed: the running model needs a new launch.
pub(super) fn launch_changed(a: &LocalTuning, b: &LocalTuning) -> bool {
    a.ctx != b.ctx || a.gpu != b.gpu || a.threads != b.threads
}

/// Hand a tuning to the router and the launcher. `restart` asks a running
/// model to relaunch when the launch knobs changed from `prev`.
fn apply(t: &LocalTuning, prev: Option<&LocalTuning>) {
    local::set_first(LocalFirst::parse(&t.first));
    local_autoconfig::set_overrides(overrides_for(t, local_perf::physical_cores()));
    local_perf::set_ctx_override((t.ctx > 0).then(|| u64::from(t.ctx)));
    if t.ctx > 0 {
        local::set_ctx_cap(u64::from(t.ctx));
    } else if prev.is_some_and(|p| p.ctx > 0) {
        // Back to Auto: the hardware probe's cap again. Never touch it
        // otherwise, so startup keeps the probe's value, not the 4k floor.
        local::set_ctx_cap(local_perf::current_defaults().ctx);
    }
    let idle = if t.keep_loaded {
        u64::MAX
    } else {
        local_perf::IDLE_UNLOAD_SECS
    };
    local_perf::set_idle_unload_secs(t.keep_loaded.then_some(u64::MAX));
    local_sidecar::set_idle_unload_secs(idle);
    if prev.is_some_and(|p| launch_changed(p, t)) {
        local_gov::request_restart();
    }
}

/// The games dropdown index for a rules file: unload, slow down, or ignore.
pub(super) fn games_choice(r: &Rules) -> usize {
    match (r.pause_for_games, r.unload_for_games) {
        (true, true) => 0,
        (true, false) => 1,
        (false, _) => 2,
    }
}

/// The rules a games dropdown pick means.
pub(super) fn set_games_choice(r: &mut Rules, i: usize) {
    let (pause, unload) = match i {
        0 => (true, true),
        1 => (true, false),
        _ => (false, false),
    };
    r.pause_for_games = pause;
    r.unload_for_games = unload;
}

/// The governor rows between frames: the rules file as last read or written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GovRow {
    rules: Option<Rules>,
    error: Option<String>,
}

fn rules_path() -> std::path::PathBuf {
    crate::config::config_dir()
        .join("local")
        .join("governor.json")
}

fn read_rules() -> Rules {
    std::fs::read_to_string(rules_path())
        .map(|t| Rules::from_json(&t))
        .unwrap_or_default()
}

/// Write the governor's choices from these rows into `governor.json`, read
/// fresh from disk so everything else in it (the hand-edited allow and deny
/// lists, keys this build doesn't know) stays as it is. A file that isn't a
/// JSON object is left alone and named in the error.
fn write_rules(rules: &Rules) -> Result<(), String> {
    let path = rules_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = match std::fs::read_to_string(&path) {
        Ok(t) if t.trim().is_empty() => serde_json::json!({}),
        Ok(t) => serde_json::from_str::<serde_json::Value>(&t).map_err(|e| {
            format!(
                "{} isn't valid JSON ({e}); fix or remove it",
                path.display()
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => {
            return Err(format!(
                "{} couldn't be read ({e}); fix or remove it",
                path.display()
            ))
        }
    };
    let obj = file
        .as_object_mut()
        .ok_or_else(|| format!("{} isn't a JSON object; fix or remove it", path.display()))?;
    for (key, on) in [
        ("pauseForGames", rules.pause_for_games),
        ("unloadForGames", rules.unload_for_games),
        ("throttleOnBattery", rules.throttle_on_battery),
        ("useIdleTime", rules.use_idle_time),
    ] {
        obj.insert(key.into(), serde_json::Value::Bool(on));
    }
    // A hand edit the governor can't read (`"deny": "game.exe"`) would turn
    // every rule back to its default: name it instead of saving over it.
    serde_json::from_value::<Rules>(file.clone()).map_err(|e| {
        format!(
            "{} has a value Loadout can't read ({e}); fix or remove it",
            path.display()
        )
    })?;
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

impl Cabin {
    /// The tick: apply `app.json` `localTuning` once, and again after a change.
    pub(super) fn sync_local_tuning(&mut self) {
        let mut applied = APPLIED.lock().unwrap_or_else(|e| e.into_inner());
        if applied.as_ref() == Some(&self.cfg.local_tuning) {
            return;
        }
        apply(&self.cfg.local_tuning, applied.as_ref());
        *applied = Some(self.cfg.local_tuning.clone());
    }

    /// Settings → AI models → Local AI, under the model rows.
    pub(super) fn ui_local_tuning_rows(&mut self, ui: &mut egui::Ui) {
        let before = self.cfg.local_tuning.clone();
        let mut t = before.clone();

        let first = LocalFirst::parse(&t.first);
        let labels: Vec<String> = FIRST_LABELS.iter().map(|l| l.to_string()).collect();
        let picked = labels[first as usize].clone();
        if let Some(i) =
            crate::cards::settings_dropdown(ui, FIRST_ROW, FIRST_HINT, &picked, &labels)
        {
            t.first = [LocalFirst::Fallback, LocalFirst::Chat, LocalFirst::All][i.min(2)]
                .as_str()
                .to_string();
        }

        let labels: Vec<String> = CTX_CHOICES.iter().map(|(l, _)| l.to_string()).collect();
        let picked = CTX_CHOICES
            .iter()
            .find(|(_, v)| *v == t.ctx)
            .map_or(labels[0].clone(), |(l, _)| l.to_string());
        if let Some(i) = crate::cards::settings_dropdown(ui, CTX_ROW, CTX_HINT, &picked, &labels) {
            t.ctx = CTX_CHOICES.get(i).map_or(0, |(_, v)| *v);
        }

        let labels: Vec<String> = GPU_CHOICES.iter().map(|(l, _)| l.to_string()).collect();
        let picked = GPU_CHOICES
            .iter()
            .find(|(_, v)| *v == t.gpu)
            .map_or(labels[0].clone(), |(l, _)| l.to_string());
        if let Some(i) = crate::cards::settings_dropdown(ui, GPU_ROW, GPU_HINT, &picked, &labels) {
            t.gpu = GPU_CHOICES.get(i).map_or("", |(_, v)| *v).to_string();
        }

        let labels: Vec<String> = THREADS_CHOICES.iter().map(|(l, _)| l.to_string()).collect();
        let picked = THREADS_CHOICES
            .iter()
            .find(|(_, v)| *v == t.threads)
            .map_or(labels[0].clone(), |(l, _)| l.to_string());
        if let Some(i) =
            crate::cards::settings_dropdown(ui, THREADS_ROW, THREADS_HINT, &picked, &labels)
        {
            t.threads = THREADS_CHOICES.get(i).map_or("", |(_, v)| *v).to_string();
        }

        let mut keep = t.keep_loaded;
        if crate::cards::settings_toggle(ui, KEEP_ROW, KEEP_HINT, &mut keep) {
            t.keep_loaded = keep;
        }

        if t != before {
            let relaunch = launch_changed(&before, &t);
            self.cfg.local_tuning = t;
            self.persist_cfg();
            self.sync_local_tuning();
            self.status = if relaunch {
                RESTART_NOTE.into()
            } else {
                "Saved".into()
            };
        }

        self.ui_governor_rows(ui);
    }

    /// The governor's three choices, kept in `{config}/local/governor.json`.
    fn ui_governor_rows(&mut self, ui: &mut egui::Ui) {
        let id = egui::Id::new(GOV_ROW_ID);
        let mut row: GovRow = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let mut rules = row.rules.clone().unwrap_or_else(read_rules);
        let before = rules.clone();
        let labels: Vec<String> = GAMES_LABELS.iter().map(|l| l.to_string()).collect();
        let picked = labels[games_choice(&rules)].clone();
        if let Some(i) =
            crate::cards::settings_dropdown(ui, GAMES_ROW, GAMES_HINT, &picked, &labels)
        {
            set_games_choice(&mut rules, i);
        }
        let mut battery = rules.throttle_on_battery;
        if crate::cards::settings_toggle(ui, BATTERY_ROW, BATTERY_HINT, &mut battery) {
            rules.throttle_on_battery = battery;
        }
        let mut idle = rules.use_idle_time;
        if crate::cards::settings_toggle(ui, IDLE_ROW, IDLE_HINT, &mut idle) {
            rules.use_idle_time = idle;
        }
        if rules != before {
            match write_rules(&rules) {
                Ok(()) => {
                    row.error = None;
                    self.status = "Saved".into();
                    // What the file holds now, hand edits included.
                    rules = read_rules();
                }
                Err(e) => row.error = Some(format!("Could not save the governor rules: {e}")),
            }
        }
        if let Some(err) = &row.error {
            crate::cards::settings_note(ui, err);
        }
        row.rules = Some(rules);
        ui.ctx().data_mut(|d| d.insert_temp(id, row));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning(first: &str, ctx: u32, gpu: &str, threads: &str, keep: bool) -> LocalTuning {
        LocalTuning {
            first: first.into(),
            ctx,
            gpu: gpu.into(),
            threads: threads.into(),
            keep_loaded: keep,
        }
    }

    #[test]
    fn tuning_maps_to_launch_overrides_and_the_router_mode() {
        let auto = overrides_for(&LocalTuning::default(), Some(16));
        assert_eq!(auto, local_autoconfig::Overrides::default());
        let manual = overrides_for(&tuning("chat", 16_384, "all", "half", true), Some(16));
        assert_eq!(
            (manual.ngl, manual.ctx, manual.threads, manual.kv),
            (Some(u32::MAX), Some(16_384), Some(8), None)
        );
        let cpu = overrides_for(&tuning("all", 0, "cpu", "all", false), None);
        assert_eq!(
            (cpu.ngl, cpu.ctx, cpu.threads),
            (Some(0), None, Some(u32::MAX))
        );
        // Half of an unknown core count still gives a sane positive number.
        assert_eq!(
            overrides_for(&tuning("", 0, "", "half", false), None).threads,
            Some(4)
        );
        assert_eq!(LocalFirst::parse("chat"), LocalFirst::Chat);
        assert_eq!(LocalFirst::parse("all"), LocalFirst::All);
        assert_eq!(LocalFirst::parse("anything else"), LocalFirst::Fallback);
        assert_eq!(LocalFirst::All.as_str(), "all");
    }

    #[test]
    fn the_games_dropdown_round_trips_the_two_governor_flags() {
        let mut r = Rules::default();
        assert_eq!(games_choice(&r), 0, "default: unload for games");
        set_games_choice(&mut r, 1);
        assert_eq!((r.pause_for_games, r.unload_for_games), (true, false));
        assert_eq!(games_choice(&r), 1);
        set_games_choice(&mut r, 2);
        assert_eq!((r.pause_for_games, r.unload_for_games), (false, false));
        assert_eq!(games_choice(&r), 2);
        set_games_choice(&mut r, 0);
        assert_eq!((r.pause_for_games, r.unload_for_games), (true, true));
        assert_eq!(
            GAMES_LABELS[0],
            "Unload the model (the game gets everything)"
        );
    }

    #[test]
    fn saving_a_governor_choice_keeps_the_hand_edited_lists_in_the_file() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("governor-merge");
        std::fs::create_dir_all(root.join("local")).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let file = root.join("local").join("governor.json");
        std::fs::write(
            &file,
            r#"{"allow":["blender.exe"],"deny":["game.exe"],"futureKey":7,"throttleOnBattery":true}"#,
        )
        .unwrap();
        let mut r = read_rules();
        r.throttle_on_battery = false;
        set_games_choice(&mut r, 1);
        write_rules(&r).unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(back["allow"], serde_json::json!(["blender.exe"]));
        assert_eq!(back["deny"], serde_json::json!(["game.exe"]));
        assert_eq!(back["futureKey"], serde_json::json!(7));
        assert_eq!(back["throttleOnBattery"], serde_json::json!(false));
        assert_eq!(back["pauseForGames"], serde_json::json!(true));
        assert_eq!(back["unloadForGames"], serde_json::json!(false));
        // A file that isn't JSON is left alone and named.
        std::fs::write(&file, "{ not json").unwrap();
        let err = write_rules(&r).unwrap_err();
        assert!(err.contains("isn't valid JSON"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
        // A value the governor can't read is named, and the file stays.
        std::fs::write(&file, r#"{"deny":"game.exe"}"#).unwrap();
        let err = write_rules(&r).unwrap_err();
        assert!(err.contains("can't read"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            r#"{"deny":"game.exe"}"#
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_launch_knobs_ask_for_a_relaunch() {
        let base = LocalTuning::default();
        assert!(!launch_changed(&base, &tuning("all", 0, "", "", true)));
        assert!(launch_changed(&base, &tuning("", 8_192, "", "", false)));
        assert!(launch_changed(&base, &tuning("", 0, "cpu", "", false)));
        assert!(launch_changed(&base, &tuning("", 0, "", "all", false)));
    }

    #[test]
    fn tuning_rows_paint_every_knob_and_the_governor_rules() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("local-tuning-rows");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let out = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            app.ui_local_tuning_rows(ui);
        });
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        for want in [
            FIRST_ROW,
            FIRST_LABELS[0],
            CTX_ROW,
            "Auto",
            GPU_ROW,
            THREADS_ROW,
            KEEP_ROW,
            GAMES_ROW,
            GAMES_LABELS[0],
            BATTERY_ROW,
            IDLE_ROW,
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "missing {want:?} in {texts:?}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
