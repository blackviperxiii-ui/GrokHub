//! The running-model rows in Settings → AI & chat, for both shells: which
//! model is loaded, where it runs, how fast it answered, its draft model and
//! when it unloads. [`snapshot`] reads the live state; [`lines`] words it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use grokhub_core::local_draft::Decision;
use grokhub_core::local_setup;

use super::local_autoconfig::LaunchArgs;
use super::local_scale::Mode;
use super::local_scale::{Source, SpeedQuality};
use super::local_sidecar;
use super::local_spec;
use super::local_swap::{tier_word, AskOutcome, Refused, StandIn};

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
                "Draft model: {drafter} is on; its speed test is pending and runs on its own when the computer is idle."
            ),
            Decision::NoRoom => format!(
                "Draft model: {drafter} doesn't fit beside the model, so a smaller model runs."
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

/// What runs, in the words of the line under a reply, when no ask decides it.
fn running_why(stand_in: Option<StandIn>) -> String {
    let why = match stand_in {
        None => "your pick",
        Some(StandIn::LowBattery) => "a smaller model while the battery is low",
        Some(StandIn::OutOfMemory) => "a smaller model because your pick ran out of memory",
        Some(StandIn::Gaming) => "the lightest model while a game runs",
        Some(StandIn::Lighter) => "a smaller model while the computer saves power",
        Some(StandIn::Tight) => "a smaller model because less memory is free than your pick needs",
        Some(StandIn::Performance) => "a bigger model for a hard task while the computer is free",
        Some(StandIn::BestFor(kind)) => {
            return format!("a model stronger at {} than your pick", kind.words());
        }
        Some(StandIn::Asked(tier, _)) => {
            return format!("the model an earlier {} ask loaded", tier_word(tier));
        }
    };
    why.to_string()
}

/// The ask got its tier: what runs is the tier's model. A launch that
/// stepped down (or landed on the pick) didn't, unless the ask was Balance.
fn honored(tier: SpeedQuality, stand_in: Option<StandIn>) -> bool {
    match stand_in {
        None => tier == SpeedQuality::Balanced,
        Some(StandIn::Asked(..) | StandIn::Performance | StandIn::BestFor(_)) => {
            tier != SpeedQuality::Balanced
        }
        Some(_) => false,
    }
}

/// The one line under an on-device reply: which model answered, with its
/// draft or not, and why. A tier word (Speed, Balance, Performance) shows
/// only when a chat's ask or setting chose or wanted it.
pub fn chat_line(
    name: &str,
    draft: bool,
    stand_in: Option<StandIn>,
    ask: Option<AskOutcome>,
) -> String {
    let by = if draft {
        format!("Answered by {name} with its draft")
    } else {
        format!("Answered by {name}")
    };
    let base = running_why(stand_in);
    let Some(ask) = ask else {
        return format!("{by}, {base}.");
    };
    let tier = match ask {
        AskOutcome::Got(t, _) | AskOutcome::Refused(t, _) => t,
    };
    let word = tier_word(tier);
    match ask {
        AskOutcome::Got(t, source) if honored(t, stand_in) => {
            let because = match (source, t) {
                (Source::Pin, _) | (Source::Words, SpeedQuality::Balanced) => {
                    "this chat is set to it"
                }
                (Source::Words, SpeedQuality::Speed) => "you asked for a quick answer",
                (Source::Words, SpeedQuality::Quality) => "you asked it to think harder",
            };
            format!("{by}: {word}, because {because}.")
        }
        AskOutcome::Got(..) | AskOutcome::Refused(_, Refused::NoRoom) => {
            let none = match tier {
                SpeedQuality::Speed => "no faster model fits",
                SpeedQuality::Quality => "no bigger model fits",
                SpeedQuality::Balanced => "your pick doesn't fit",
            };
            format!("{by}, {base}; {none} for {word} right now.")
        }
        AskOutcome::Refused(_, Refused::Busy) => {
            format!(
                "{by}, {base}; another chat was answering, so {word} loads with the next reply."
            )
        }
        AskOutcome::Refused(_, Refused::KeptLoaded) => {
            format!("{by}, {base}; Keep loaded is on, so {word} doesn't swap the model.")
        }
        AskOutcome::Refused(_, Refused::Forced) => {
            format!("{by}, {base}; a model file is set for this app, so {word} doesn't swap it.")
        }
    }
}

/// The line for each chat's last on-device reply, until the chat takes it.
static LAST_LINE: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Keep the line for the reply that just ran on device for `conversation_id`
/// (a later step of the same turn replaces it). With nothing loaded there
/// is no line.
pub fn note_reply(conversation_id: &str, ask: Option<AskOutcome>) {
    let Some(l) = local_sidecar::loaded() else {
        forget_line(conversation_id);
        return;
    };
    let name = super::local_swap::catalog_for(&l.model).map_or_else(
        || {
            l.model
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        },
        |m| m.name.to_string(),
    );
    let draft = local_spec::draft_status()
        .is_some_and(|(_, d)| matches!(d, Decision::On | Decision::NeedsTest));
    let line = chat_line(&name, draft, l.stand_in, ask);
    if let Ok(mut lines) = LAST_LINE.lock() {
        lines.insert(conversation_id.to_string(), line);
    }
}

/// Drop `conversation_id`'s line: its last step didn't answer on device.
pub fn forget_line(conversation_id: &str) {
    if let Ok(mut lines) = LAST_LINE.lock() {
        lines.remove(conversation_id);
    }
}

/// The line for `conversation_id`'s turn that just ended, once: it clears,
/// so a later turn that ran elsewhere shows none.
pub fn take_line(conversation_id: &str) -> Option<String> {
    LAST_LINE.lock().ok()?.remove(conversation_id)
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
                Decision::Off("the speed test found the draft model didn't help"),
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
        assert_eq!(got[3], "Draft model: Gemma 4 31B draft model is off because the speed test found the draft model didn't help.");
        assert_eq!(
            got[4],
            "Stays loaded until you close the app or a game needs the memory."
        );
        let needs = LocalLive {
            draft: Some(("X".into(), Decision::NeedsTest, None)),
            ..loaded()
        };
        assert_eq!(
            lines(&needs)[3],
            "Draft model: X is on; its speed test is pending and runs on its own when the computer is idle."
        );
        let no_room = LocalLive {
            draft: Some(("X".into(), Decision::NoRoom, None)),
            ..loaded()
        };
        assert_eq!(
            lines(&no_room)[3],
            "Draft model: X doesn't fit beside the model, so a smaller model runs."
        );
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

    #[test]
    fn the_line_under_a_reply_says_which_model_answered_and_why() {
        use grokhub_core::local_skills::TaskKind;
        use SpeedQuality::{Balanced, Quality, Speed};
        let line = |draft, stand_in, ask| chat_line("Gemma 4 E2B", draft, stand_in, ask);
        assert_eq!(
            line(true, None, None),
            "Answered by Gemma 4 E2B with its draft, your pick."
        );
        assert_eq!(
            line(false, None, None),
            "Answered by Gemma 4 E2B, your pick."
        );
        let asked = |t, s| Some(StandIn::Asked(t, s));
        assert_eq!(
            line(
                true,
                asked(Speed, Source::Words),
                Some(AskOutcome::Got(Speed, Source::Words))
            ),
            "Answered by Gemma 4 E2B with its draft: Speed, because you asked for a quick answer."
        );
        assert_eq!(
            line(
                true,
                asked(Speed, Source::Pin),
                Some(AskOutcome::Got(Speed, Source::Pin))
            ),
            "Answered by Gemma 4 E2B with its draft: Speed, because this chat is set to it."
        );
        assert_eq!(
            line(
                true,
                asked(Quality, Source::Words),
                Some(AskOutcome::Got(Quality, Source::Words))
            ),
            "Answered by Gemma 4 E2B with its draft: Performance, because you asked it to think harder."
        );
        assert_eq!(
            line(
                false,
                asked(Quality, Source::Pin),
                Some(AskOutcome::Got(Quality, Source::Pin))
            ),
            "Answered by Gemma 4 E2B: Performance, because this chat is set to it."
        );
        // The model the auto pick already ran for hard work serves the ask.
        assert_eq!(
            line(
                true,
                Some(StandIn::Performance),
                Some(AskOutcome::Got(Quality, Source::Words))
            ),
            "Answered by Gemma 4 E2B with its draft: Performance, because you asked it to think harder."
        );
        assert_eq!(
            line(true, None, Some(AskOutcome::Got(Balanced, Source::Pin))),
            "Answered by Gemma 4 E2B with its draft: Balance, because this chat is set to it."
        );
        // The launch landed on the pick, or stepped down: the ask didn't get it.
        assert_eq!(
            line(true, None, Some(AskOutcome::Got(Quality, Source::Words))),
            "Answered by Gemma 4 E2B with its draft, your pick; no bigger model fits for Performance right now."
        );
        assert_eq!(
            line(
                true,
                Some(StandIn::Tight),
                Some(AskOutcome::Got(Quality, Source::Pin))
            ),
            "Answered by Gemma 4 E2B with its draft, a smaller model because less memory is free than your pick needs; no bigger model fits for Performance right now."
        );
        assert_eq!(
            line(
                true,
                None,
                Some(AskOutcome::Refused(Speed, Refused::NoRoom))
            ),
            "Answered by Gemma 4 E2B with its draft, your pick; no faster model fits for Speed right now."
        );
        assert_eq!(
            line(
                true,
                Some(StandIn::Tight),
                Some(AskOutcome::Got(Balanced, Source::Pin))
            ),
            "Answered by Gemma 4 E2B with its draft, a smaller model because less memory is free than your pick needs; your pick doesn't fit for Balance right now."
        );
        assert_eq!(
            line(
                true,
                Some(StandIn::Performance),
                Some(AskOutcome::Refused(Speed, Refused::Busy))
            ),
            "Answered by Gemma 4 E2B with its draft, a bigger model for a hard task while the computer is free; another chat was answering, so Speed loads with the next reply."
        );
        assert_eq!(
            line(
                false,
                None,
                Some(AskOutcome::Refused(Quality, Refused::KeptLoaded))
            ),
            "Answered by Gemma 4 E2B, your pick; Keep loaded is on, so Performance doesn't swap the model."
        );
        assert_eq!(
            line(
                false,
                None,
                Some(AskOutcome::Refused(Speed, Refused::Forced))
            ),
            "Answered by Gemma 4 E2B, your pick; a model file is set for this app, so Speed doesn't swap it."
        );
        // The stand-ins, in short.
        let why = |s| line(false, Some(s), None);
        assert_eq!(
            why(StandIn::LowBattery),
            "Answered by Gemma 4 E2B, a smaller model while the battery is low."
        );
        assert_eq!(
            why(StandIn::OutOfMemory),
            "Answered by Gemma 4 E2B, a smaller model because your pick ran out of memory."
        );
        assert_eq!(
            why(StandIn::Gaming),
            "Answered by Gemma 4 E2B, the lightest model while a game runs."
        );
        assert_eq!(
            why(StandIn::Lighter),
            "Answered by Gemma 4 E2B, a smaller model while the computer saves power."
        );
        assert_eq!(
            why(StandIn::Performance),
            "Answered by Gemma 4 E2B, a bigger model for a hard task while the computer is free."
        );
        assert_eq!(
            why(StandIn::BestFor(TaskKind::Reason)),
            "Answered by Gemma 4 E2B, a model stronger at reasoning and planning than your pick."
        );
        assert_eq!(
            why(StandIn::Asked(Speed, Source::Words)),
            "Answered by Gemma 4 E2B, the model an earlier Speed ask loaded."
        );
    }

    #[test]
    fn a_chats_line_is_taken_once_and_a_failed_step_drops_it() {
        let id = "shell-line-test";
        if let Ok(mut lines) = LAST_LINE.lock() {
            lines.insert(id.to_string(), "Answered by Gemma 4 E2B, your pick.".into());
        }
        assert_eq!(
            take_line(id).as_deref(),
            Some("Answered by Gemma 4 E2B, your pick.")
        );
        assert_eq!(take_line(id), None);
        if let Ok(mut lines) = LAST_LINE.lock() {
            lines.insert(id.to_string(), "stale".into());
        }
        forget_line(id);
        assert_eq!(take_line(id), None);
    }
}
