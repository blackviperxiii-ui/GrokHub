//! The running-model rows in Settings → AI & chat, for both shells: which
//! model is loaded, where it runs, how fast it answered, its draft model and
//! when it unloads. [`snapshot`] reads the live state; [`lines`] words it.

use std::path::Path;

use grokhub_core::local_draft::Decision;
use grokhub_core::local_setup;

use super::local_autoconfig::LaunchArgs;
use super::local_scale::Mode;
use super::local_sidecar;
use super::local_spec;
use super::local_swap::StandIn;

/// Where the loaded model's layers run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Gpu,
    /// Some layers (or a mixture of experts' experts) on the GPU, the rest in RAM.
    Split {
        on_gpu: u32,
        all: u32,
    },
    Cpu,
}

/// One look at the on-device model.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalLive {
    /// Nothing loaded: the rest is `None`.
    pub model: Option<String>,
    pub quant: Option<String>,
    pub place: Option<Place>,
    pub size_mb: Option<u64>,
    pub ctx: Option<u32>,
    pub busy: bool,
    pub tok_s: Option<f64>,
    /// The drafter's name, the launch's decision and the share of its
    /// guesses kept since it loaded.
    pub draft: Option<(String, Decision, Option<f64>)>,
    pub unload_in_secs: Option<u64>,
    pub kept_loaded: bool,
    /// Why a model other than the Settings pick runs.
    pub stand_in: Option<StandIn>,
    /// The scaling mode, as a read-only label.
    pub mode: Mode,
    /// The model's refusal check on this PC, once it has run.
    pub refusal: Option<String>,
}

pub fn place_of(a: Option<&LaunchArgs>, bare_cpu: bool) -> Place {
    match a {
        _ if bare_cpu => Place::Cpu,
        None => Place::Gpu,
        Some(a) if a.cpu_only || a.ngl == 0 => Place::Cpu,
        Some(a) if a.ngl >= a.all_layers && a.n_cpu_moe.is_none() => Place::Gpu,
        Some(a) => Place::Split {
            on_gpu: a.ngl.min(a.all_layers),
            all: a.all_layers,
        },
    }
}

/// The on-device model as it is now.
pub fn snapshot(config_dir: &Path) -> LocalLive {
    let kept_loaded = local_sidecar::keep_loaded();
    let dynamic = super::local_task::dynamic(config_dir);
    let scale = super::local_scale::scale_now(local_sidecar::unix_ms());
    let Some(l) = local_sidecar::loaded() else {
        return LocalLive {
            model: None,
            quant: None,
            place: None,
            size_mb: None,
            ctx: None,
            busy: false,
            tok_s: None,
            draft: None,
            unload_in_secs: None,
            kept_loaded,
            stand_in: None,
            mode: super::local_swap::mode(None, &scale, dynamic),
            refusal: None,
        };
    };
    let file = l
        .model
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let row = local_setup::local_models().iter().find(|m| m.file == file);
    let (tok_s, kept) = local_spec::reply_stats();
    let busy = local_sidecar::in_flight() > 0;
    let idle = local_sidecar::secs_since_use();
    LocalLive {
        model: Some(row.map_or(file.clone(), |m| m.name.to_string())),
        quant: row.map(|m| m.quant.to_string()),
        place: Some(place_of(l.launch.as_ref(), l.bare_cpu)),
        size_mb: std::fs::metadata(&l.model)
            .ok()
            .map(|m| m.len() / (1024 * 1024)),
        ctx: l.launch.as_ref().map(|a| a.ctx),
        busy,
        tok_s,
        draft: local_spec::draft_status().map(|(n, d)| (n, d, kept)),
        unload_in_secs: (!busy && !kept_loaded).then(|| l.idle_unload_secs.saturating_sub(idle)),
        kept_loaded,
        stand_in: l.stand_in,
        mode: super::local_swap::mode(l.stand_in, &scale, dynamic),
        refusal: local_spec::refusal_line(&l.model),
    }
}

fn minutes(secs: u64) -> String {
    match secs.div_ceil(60) {
        0 | 1 => "a minute".to_string(),
        m => format!("{m} minutes"),
    }
}

/// The running-model rows, plainest first.
pub fn lines(l: &LocalLive) -> Vec<String> {
    let Some(model) = &l.model else {
        let mut out = vec![if l.kept_loaded {
            "Running now: nothing. The model loads with the next reply and then stays loaded."
                .to_string()
        } else {
            "Running now: nothing. The model loads with the next reply.".to_string()
        }];
        if l.mode != Mode::Balanced {
            out.push(l.mode.line().to_string());
        }
        return out;
    };
    let name = match &l.quant {
        Some(q) => format!("{model} ({q})"),
        None => model.clone(),
    };
    let place = match l.place {
        Some(Place::Gpu) | None => "on the GPU".to_string(),
        Some(Place::Split { on_gpu, all }) => {
            format!("on the GPU and in RAM ({on_gpu} of {all} layers on the GPU)")
        }
        Some(Place::Cpu) => "on the CPU".to_string(),
    };
    let size = l
        .size_mb
        .map(|mb| format!(", {}", local_setup::size_label(mb)))
        .unwrap_or_default();
    let doing = if l.busy { "answering" } else { "loaded" };
    let mut out = vec![format!("Running now: {name}, {doing} {place}{size}.")];
    if let Some(why) = l.stand_in {
        out.push(why.line());
    }
    out.push(l.mode.line().to_string());
    let mut speed = match l.tok_s {
        Some(t) => format!("Last reply: {t:.0} tok/s"),
        None => "Last reply: none since it loaded".to_string(),
    };
    if let Some(c) = l.ctx {
        speed.push_str(&format!(", with room for {} tokens of chat", group(c)));
    }
    out.push(format!("{speed}."));
    if let Some((drafter, decision, kept)) = &l.draft {
        out.push(match decision {
            Decision::On => match kept {
                Some(k) => format!(
                    "Draft model: {drafter} is on and kept {:.0}% of its guesses so far.",
                    k * 100.0
                ),
                None => format!("Draft model: {drafter} is on."),
            },
            Decision::NeedsTest => format!(
                "Draft model: {drafter} is off until its speed test runs, which happens on its own when the computer is idle."
            ),
            Decision::Off(why) => format!("Draft model: {drafter} is off because {why}."),
        });
    }
    if let Some(r) = &l.refusal {
        out.push(r.clone());
    }
    out.push(if l.kept_loaded {
        "Stays loaded until you close the app or a game needs the memory.".to_string()
    } else {
        match l.unload_in_secs {
            Some(s) => format!("Unloads after {} without replies.", minutes(s)),
            None => "Unloads once it has been idle for a while.".to_string(),
        }
    });
    out
}

fn group(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> LocalLive {
        LocalLive {
            model: Some("Gemma 4 12B".into()),
            quant: Some("Q4_K".into()),
            place: Some(Place::Gpu),
            size_mb: Some(7_039),
            ctx: Some(16_384),
            busy: false,
            tok_s: Some(41.3),
            draft: Some(("Gemma 4 12B draft model".into(), Decision::On, Some(0.634))),
            unload_in_secs: Some(240),
            kept_loaded: false,
            stand_in: None,
            mode: Mode::Balanced,
            refusal: None,
        }
    }

    #[test]
    fn the_rows_say_what_runs_where_how_fast_its_draft_and_when_it_unloads() {
        assert_eq!(
            lines(&loaded()),
            [
                "Running now: Gemma 4 12B (Q4_K), loaded on the GPU, 6.9 GB.",
                "Mode: Balanced. The model is picked for each task from what's free right now.",
                "Last reply: 41 tok/s, with room for 16,384 tokens of chat.",
                "Draft model: Gemma 4 12B draft model is on and kept 63% of its guesses so far.",
                "Unloads after 4 minutes without replies.",
            ]
        );
    }

    #[test]
    fn the_rows_say_why_a_draft_model_is_off_and_when_it_stays_loaded() {
        let l = LocalLive {
            place: Some(Place::Split {
                on_gpu: 20,
                all: 49,
            }),
            busy: true,
            tok_s: None,
            draft: Some((
                "Gemma 4 31B draft model".into(),
                Decision::Off("there isn't room for the model and its draft model together"),
                None,
            )),
            unload_in_secs: None,
            kept_loaded: true,
            stand_in: None,
            ..loaded()
        };
        let got = lines(&l);
        assert_eq!(
            got[0],
            "Running now: Gemma 4 12B (Q4_K), answering on the GPU and in RAM (20 of 49 layers on the GPU), 6.9 GB."
        );
        assert_eq!(got[1], Mode::Balanced.line());
        assert_eq!(
            got[2],
            "Last reply: none since it loaded, with room for 16,384 tokens of chat."
        );
        assert_eq!(got[3], "Draft model: Gemma 4 31B draft model is off because there isn't room for the model and its draft model together.");
        assert_eq!(
            got[4],
            "Stays loaded until you close the app or a game needs the memory."
        );
        let needs = LocalLive {
            draft: Some(("X".into(), Decision::NeedsTest, None)),
            ..loaded()
        };
        assert!(lines(&needs)[3].contains("happens on its own when the computer is idle"));
    }

    #[test]
    fn nothing_loaded_says_so() {
        let l = snapshot_empty(false);
        assert_eq!(
            lines(&l),
            ["Running now: nothing. The model loads with the next reply."]
        );
        assert!(lines(&snapshot_empty(true))[0].ends_with("and then stays loaded."));
    }

    fn snapshot_empty(kept_loaded: bool) -> LocalLive {
        LocalLive {
            model: None,
            quant: None,
            place: None,
            size_mb: None,
            ctx: None,
            busy: false,
            tok_s: None,
            draft: None,
            unload_in_secs: None,
            kept_loaded,
            stand_in: None,
            mode: Mode::Balanced,
            refusal: None,
        }
    }

    #[test]
    fn where_it_runs_follows_the_launch() {
        assert_eq!(place_of(None, true), Place::Cpu);
        assert_eq!(place_of(None, false), Place::Gpu);
    }

    #[test]
    fn a_smaller_model_standing_in_for_the_pick_says_why() {
        let l = LocalLive {
            stand_in: Some(StandIn::LowBattery),
            mode: Mode::Balanced,
            ..loaded()
        };
        let rows = lines(&l);
        assert!(rows[0].starts_with("Running now: "));
        assert_eq!(rows[1], StandIn::LowBattery.line());
        assert_eq!(rows[2], Mode::Balanced.line());
        assert!(!lines(&loaded()).iter().any(|r| r.contains("smaller model")));
        let gaming = LocalLive {
            mode: Mode::Gaming,
            ..snapshot_empty(false)
        };
        assert_eq!(
            lines(&gaming),
            [
                "Running now: nothing. The model loads with the next reply.",
                "Mode: Gaming. A lighter model answers on the CPU, so your game keeps the GPU.",
            ],
            "with nothing loaded only Gaming is worth a row"
        );
    }

    #[test]
    fn a_finished_refusal_check_shows_under_the_draft_model() {
        let l = LocalLive {
            refusal: Some("Refusal check: answered 12 of 12 everyday asks on this PC.".into()),
            ..loaded()
        };
        let rows = lines(&l);
        let draft = rows
            .iter()
            .position(|r| r.starts_with("Draft model:"))
            .unwrap();
        assert_eq!(
            rows[draft + 1],
            "Refusal check: answered 12 of 12 everyday asks on this PC."
        );
        assert!(!lines(&loaded())
            .iter()
            .any(|r| r.starts_with("Refusal check")));
    }
}
