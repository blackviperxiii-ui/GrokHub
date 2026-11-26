//! The running-model rows in Settings → AI & chat, for both shells: which
//! model is loaded, where it runs, how fast it answered, its draft model and
//! when it unloads. [`snapshot`] reads the live state; [`lines`] words it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grokhub_core::local_draft::Decision;
use grokhub_core::local_setup;
use grokhub_core::local_skills::TaskKind;

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

/// What runs, in the words of the line under a reply, when no ask decides it:
/// the stand-in's own reason, the one Settings shows too.
fn running_why(stand_in: Option<StandIn>) -> String {
    stand_in.map_or_else(|| "your pick".to_string(), StandIn::reason)
}

/// What an ask came to, given whether what runs is the model its plan aimed
/// at (`met`). A plan that got its tier but whose launch stepped down, or
/// landed elsewhere, didn't get it: no room for it.
pub fn settle(ask: AskOutcome, met: bool) -> AskOutcome {
    match ask {
        AskOutcome::Got(tier, _) if !met => AskOutcome::Refused(tier, Refused::NoRoom),
        other => other,
    }
}

/// The one line under an on-device reply: which model answered, with its
/// draft or not, and why. A tier word (Speed, Balance, Performance) shows
/// only when a chat's ask or setting chose or wanted it. `ask` is what
/// [`settle`] made of the plan's outcome.
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
        AskOutcome::Got(t, source) => {
            let because = match (source, t) {
                (Source::Pin, _) | (Source::Words, SpeedQuality::Balanced) => {
                    "this chat is set to it"
                }
                (Source::Words, SpeedQuality::Speed) => "you asked for a quick answer",
                (Source::Words, SpeedQuality::Quality) => "you asked it to think harder",
            };
            format!("{by}: {word}, because {because}.")
        }
        AskOutcome::Refused(_, Refused::NoRoom) => {
            let none = match tier {
                SpeedQuality::Speed => "no faster model fits",
                SpeedQuality::Quality => "no bigger model fits",
                SpeedQuality::Balanced => "your pick doesn't fit",
            };
            format!("{by}, {base}; {none} for {word} right now.")
        }
        AskOutcome::Refused(_, Refused::Busy(Source::Pin)) => {
            format!(
                "{by}, {base}; another chat was answering, so {word} loads with the next reply."
            )
        }
        AskOutcome::Refused(_, Refused::Busy(Source::Words)) => {
            format!("{by}, {base}; another chat was answering, so this reply stayed on it.")
        }
        AskOutcome::Refused(_, Refused::LowBattery) => {
            format!("{by}, {base}; the battery is low, so {word} doesn't load a bigger model.")
        }
        AskOutcome::Refused(_, Refused::KeptLoaded) => {
            format!("{by}, {base}; Keep loaded is on, so {word} doesn't swap the model.")
        }
        AskOutcome::Refused(_, Refused::Forced) => {
            format!("{by}, {base}; a model file is set for this app, so {word} doesn't swap it.")
        }
    }
}

/// Formats the on-device chat line with an optional local routing reason.
pub fn chat_line_with_reason(
    name: &str,
    draft: bool,
    stand_in: Option<StandIn>,
    ask: Option<AskOutcome>,
    reason: Option<&str>,
) -> String {
    let base = chat_line(name, draft, stand_in, ask);
    match reason.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => {
            let trimmed = base.strip_suffix('.').unwrap_or(&base);
            format!("{trimmed} — routed locally because {r}.")
        }
        None => base,
    }
}

/// Derives a plain-English local routing reason from the router's rule ids and
/// any upstream server handoff rule.
pub fn routing_reason(rules: &[String], handoff_rule: Option<&str>) -> &'static str {
    let has = |id: &str| rules.iter().any(|r| r == id);
    if has("size:warm_local") {
        "already warm on this computer"
    } else if has("local:cloud_benched") {
        "cloud models are benched"
    } else if has("privacy:local_only") {
        "keeping sensitive content on this computer"
    } else if has("adult:local_only") {
        "keeping explicit content on this computer"
    } else if has("adult:voice_local") {
        "18+ voice stays on this computer"
    } else if has("offline:local") {
        "offline mode is on"
    } else if has("local:no_keys") {
        "no online sign-in or API key is configured"
    } else if has("voice:local_only") {
        "voice replies are set to local only"
    } else if has("code:local_only") {
        "Code tab is set to local models only"
    } else if let Some(h) = handoff_rule {
        match h {
            "handoff:limit" => "cloud model hit its rate limit",
            "handoff:credit" => "cloud account is out of credit",
            "handoff:auth" => "cloud sign-in or API key was refused",
            "handoff:free_cap" => "cloud free tier hit its limit",
            _ => "cloud model was unavailable",
        }
    } else if has("local:cloud_down") {
        "online models were unavailable"
    } else {
        "selected for on-device execution"
    }
}

/// Plain-English local routing reason for one turn. Only a turn whose own ask
/// is explicit shows the explicit-content reason; a clean turn kept local by an
/// 18+ voice says so directly instead of blaming explicit content.
pub fn local_reason(
    rules: &[String],
    handoff_rule: Option<&str>,
    forced_adult_local: bool,
    ask_adult: bool,
    must_stay: bool,
) -> &'static str {
    let adult_rule = forced_adult_local || rules.iter().any(|r| r == "adult:local_only");
    if adult_rule && ask_adult {
        "keeping explicit content on this computer"
    } else if adult_rule && must_stay {
        "18+ voice stays on this computer"
    } else if adult_rule {
        "selected for on-device execution"
    } else {
        routing_reason(rules, handoff_rule)
    }
}

/// Formats the chat loading note with an optional routing reason.
pub fn loading_note(reason: Option<&str>) -> String {
    match reason.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => format!("{} (Reason: {r}.)", super::local::LOCAL_LOADING_NOTE),
        None => super::local::LOCAL_LOADING_NOTE.to_string(),
    }
}

/// The line for each chat's last on-device reply, until the chat takes it.
static LAST_LINE: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// The task kind of each chat's last on-device reply, kept beside its line.
static LAST_KIND: Mutex<BTreeMap<String, TaskKind>> = Mutex::new(BTreeMap::new());
/// Why the chat's current step was routed locally, for display on the reply/loading note.
static ROUTE_REASON: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

pub fn note_route_reason(conversation_id: &str, reason: &str) {
    if let Ok(mut map) = ROUTE_REASON.lock() {
        map.insert(conversation_id.to_string(), reason.to_string());
    }
}

pub fn take_route_reason(conversation_id: &str) -> Option<String> {
    ROUTE_REASON.lock().ok()?.remove(conversation_id)
}

/// Keep the line for the reply that just ran on device for `conversation_id`
/// (a later step of the same turn replaces it). With nothing loaded there
/// is no line.
pub fn note_reply(conversation_id: &str, ask: Option<(AskOutcome, PathBuf)>) {
    let reason = take_route_reason(conversation_id);
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
    // What runs decides whether the ask got its tier, not the running
    // model's label: a plan that keeps the model the ask wanted leaves the
    // label of whatever loaded it.
    let ask = ask.map(|(a, target)| settle(a, target == l.model));
    keep_line(
        conversation_id,
        chat_line_with_reason(&name, draft, l.stand_in, ask, reason.as_deref()),
    );
    if let Ok(mut kinds) = LAST_KIND.lock() {
        match super::local_task::current_kind() {
            Some(kind) => kinds.insert(conversation_id.to_string(), kind),
            None => kinds.remove(conversation_id),
        };
    }
}

/// Keep `line` for `conversation_id`'s turn, until [`take_line`] takes it.
pub fn keep_line(conversation_id: &str, line: String) {
    if let Ok(mut lines) = LAST_LINE.lock() {
        lines.insert(conversation_id.to_string(), line);
    }
}

/// Said under a reply when the person's newest picture went to a model that
/// can't see it ([`super::local_vision::strip_images`]).
pub const BLIND_LINE: &str =
    "The picture wasn't read: the model that answered on this computer can't see pictures.";

/// `line` with [`BLIND_LINE`] after it.
pub fn blind_line(line: &str) -> String {
    format!("{line} {BLIND_LINE}")
}

/// Add [`BLIND_LINE`] to `conversation_id`'s kept line, or keep it alone
/// when the reply named no model: the device answered without the picture
/// either way.
pub fn note_blind(conversation_id: &str) {
    if let Ok(mut lines) = LAST_LINE.lock() {
        let line = lines
            .get(conversation_id)
            .map_or_else(|| BLIND_LINE.to_string(), |l| blind_line(l));
        lines.insert(conversation_id.to_string(), line);
    }
}

/// Drop `conversation_id`'s line: its last step didn't answer on device.
pub fn forget_line(conversation_id: &str) {
    if let Ok(mut lines) = LAST_LINE.lock() {
        lines.remove(conversation_id);
    }
    if let Ok(mut reasons) = ROUTE_REASON.lock() {
        reasons.remove(conversation_id);
    }
    if let Ok(mut kinds) = LAST_KIND.lock() {
        kinds.remove(conversation_id);
    }
}

/// The line for `conversation_id`'s turn that just ended, once: it clears,
/// so a later turn that ran elsewhere shows none.
pub fn take_line(conversation_id: &str) -> Option<String> {
    LAST_LINE.lock().ok()?.remove(conversation_id)
}

/// The task kind of `conversation_id`'s last on-device reply, once.
pub fn take_kind(conversation_id: &str) -> Option<TaskKind> {
    LAST_KIND.lock().ok()?.remove(conversation_id)
}

/// Where each chat's newest step ran, written when the step is decided so
/// the voice picks its engine before the first spoken piece. In memory only:
/// a chat with no step since launch reads as online.
static RUNS_HERE: Mutex<BTreeMap<String, Here>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Clone, Copy, Default)]
struct Here {
    here: bool,
    must_stay: bool,
}

/// `conversation_id`'s newest step runs on this computer (`here`), and the
/// chat must stay here (`must_stay`: an 18+ voice).
pub fn note_step_here(conversation_id: &str, here: bool, must_stay: bool) {
    if let Ok(mut runs) = RUNS_HERE.lock() {
        runs.insert(conversation_id.to_string(), Here { here, must_stay });
    }
}

fn here_of(conversation_id: &str) -> Here {
    RUNS_HERE
        .lock()
        .ok()
        .and_then(|runs| runs.get(conversation_id).copied())
        .unwrap_or_default()
}

/// `conversation_id`'s newest step ran on this computer. An unknown chat
/// reads as false.
pub fn runs_here(conversation_id: &str) -> bool {
    here_of(conversation_id).here
}

/// `conversation_id` must stay on this computer (its newest step carried an
/// 18+ voice). An unknown chat reads as false.
pub fn must_stay_here(conversation_id: &str) -> bool {
    here_of(conversation_id).must_stay
}

#[cfg(test)]
mod tests {
    #[test]
    fn routing_reason_and_chat_line_explain_why_local_was_used() {
        assert_eq!(
            super::routing_reason(&["size:warm_local".into()], None),
            "already warm on this computer"
        );
        assert_eq!(
            super::routing_reason(&["local:cloud_benched".into()], None),
            "cloud models are benched"
        );
        assert_eq!(
            super::routing_reason(&[], Some("handoff:limit")),
            "cloud model hit its rate limit"
        );
        assert_eq!(
            super::chat_line_with_reason(
                "Gemma 4 E4B",
                false,
                None,
                None,
                Some("cloud models are benched")
            ),
            "Answered by Gemma 4 E4B, your pick — routed locally because cloud models are benched."
        );
        assert_eq!(
            super::loading_note(Some("already warm on this computer")),
            format!(
                "{} (Reason: already warm on this computer.)",
                super::super::local::LOCAL_LOADING_NOTE
            )
        );
        assert_eq!(
            super::local_reason(&["adult:local_only".into()], None, false, true, false),
            "keeping explicit content on this computer"
        );
        assert_eq!(
            super::local_reason(&["adult:local_only".into()], None, false, false, true),
            "18+ voice stays on this computer"
        );
        assert_eq!(
            super::local_reason(&["local:no_keys".into()], None, false, false, false),
            "no online sign-in or API key is configured"
        );
        assert_eq!(
            super::local_reason(&["size:warm_local".into()], None, false, false, false),
            "already warm on this computer"
        );
    }

    #[test]
    fn a_blind_reply_says_the_picture_wasnt_read() {
        assert_eq!(
            super::blind_line("Answered by Gemma 4 E2B with its draft, your pick."),
            "Answered by Gemma 4 E2B with its draft, your pick. The picture wasn't read: \
             the model that answered on this computer can't see pictures."
        );
        super::keep_line("blind-c", "Answered by Gemma 4 E2B, your pick.".into());
        super::note_blind("blind-c");
        assert_eq!(
            super::take_line("blind-c").as_deref(),
            Some(
                "Answered by Gemma 4 E2B, your pick. The picture wasn't read: \
                 the model that answered on this computer can't see pictures."
            )
        );
        // No model named under the reply: the line is the blind line alone.
        super::note_blind("blind-none");
        assert_eq!(
            super::take_line("blind-none").as_deref(),
            Some(super::BLIND_LINE)
        );
    }

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
            line(
                true,
                None,
                Some(settle(AskOutcome::Got(Quality, Source::Words), false))
            ),
            "Answered by Gemma 4 E2B with its draft, your pick; no bigger model fits for Performance right now."
        );
        assert_eq!(
            line(
                true,
                Some(StandIn::Tight),
                Some(settle(AskOutcome::Got(Quality, Source::Pin), false))
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
                Some(settle(AskOutcome::Got(Balanced, Source::Pin), false))
            ),
            "Answered by Gemma 4 E2B with its draft, a smaller model because less memory is free than your pick needs; your pick doesn't fit for Balance right now."
        );
        assert_eq!(
            line(
                true,
                Some(StandIn::Performance),
                Some(AskOutcome::Refused(Speed, Refused::Busy(Source::Pin)))
            ),
            "Answered by Gemma 4 E2B with its draft, a bigger model than your pick for hard tasks while the computer is free; another chat was answering, so Speed loads with the next reply."
        );
        // Words ask for this reply only: the next one carries no ask.
        assert_eq!(
            line(
                true,
                None,
                Some(AskOutcome::Refused(Speed, Refused::Busy(Source::Words)))
            ),
            "Answered by Gemma 4 E2B with its draft, your pick; another chat was answering, so this reply stayed on it."
        );
        // A low battery keeps Performance off even with no smaller model.
        assert_eq!(
            line(
                true,
                None,
                Some(AskOutcome::Refused(Quality, Refused::LowBattery))
            ),
            "Answered by Gemma 4 E2B with its draft, your pick; the battery is low, so Performance doesn't load a bigger model."
        );
        // The ask's model was already running under an older label (a
        // walk down, a low battery since charged, a power-saving stand-in):
        // what runs is what the ask wanted, so the line says it got it.
        for old in [StandIn::Tight, StandIn::LowBattery, StandIn::Lighter] {
            assert_eq!(
                line(
                    true,
                    Some(old),
                    Some(settle(AskOutcome::Got(Speed, Source::Pin), true))
                ),
                "Answered by Gemma 4 E2B with its draft: Speed, because this chat is set to it.",
                "{old:?}"
            );
        }
        assert_eq!(
            line(
                true,
                Some(StandIn::Lighter),
                Some(settle(AskOutcome::Got(Speed, Source::Words), true))
            ),
            "Answered by Gemma 4 E2B with its draft: Speed, because you asked for a quick answer."
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
            "Answered by Gemma 4 E2B, a smaller model for light tasks while the computer saves power."
        );
        assert_eq!(
            why(StandIn::Performance),
            "Answered by Gemma 4 E2B, a bigger model than your pick for hard tasks while the computer is free."
        );
        assert_eq!(
            why(StandIn::BestFor(TaskKind::Reason)),
            "Answered by Gemma 4 E2B, a model that's stronger at reasoning and planning than your pick."
        );
        assert_eq!(
            why(StandIn::Asked(Speed, Source::Words)),
            "Answered by Gemma 4 E2B, a faster model than your pick because a chat asked for Speed."
        );
        // One source for each reason: Settings' row says the same thing.
        for s in [
            StandIn::LowBattery,
            StandIn::OutOfMemory,
            StandIn::Gaming,
            StandIn::Lighter,
            StandIn::Tight,
            StandIn::Performance,
            StandIn::BestFor(TaskKind::Reason),
            StandIn::Asked(Quality, Source::Pin),
        ] {
            let row = s.line();
            let clause = why(s)
                .strip_prefix("Answered by Gemma 4 E2B, ")
                .and_then(|c| c.strip_suffix('.'))
                .map(str::to_string);
            assert!(
                clause
                    .as_ref()
                    .is_some_and(|c| row.starts_with(&format!("Running {c}. "))),
                "{s:?}: {row} / {clause:?}"
            );
        }
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
        if let Ok(mut kinds) = LAST_KIND.lock() {
            kinds.insert(id.to_string(), TaskKind::Code);
        }
        assert_eq!(take_kind(id), Some(TaskKind::Code));
        assert_eq!(take_kind(id), None);
        if let Ok(mut lines) = LAST_LINE.lock() {
            lines.insert(id.to_string(), "stale".into());
        }
        if let Ok(mut kinds) = LAST_KIND.lock() {
            kinds.insert(id.to_string(), TaskKind::Code);
        }
        forget_line(id);
        assert_eq!(take_line(id), None);
        assert_eq!(take_kind(id), None);
    }

    #[test]
    fn a_chat_reads_as_here_only_after_a_step_here() {
        assert!(!runs_here("voice-c1"));
        assert!(!must_stay_here("voice-c1"));
        note_step_here("voice-c1", true, false);
        assert!(runs_here("voice-c1"));
        assert!(!must_stay_here("voice-c1"));
        assert!(!runs_here("voice-c2"));
        note_step_here("voice-c2", true, true);
        assert!(must_stay_here("voice-c2"));
        note_step_here("voice-c1", false, false);
        assert!(!runs_here("voice-c1"));
        assert!(runs_here("voice-c2"));
    }
}
