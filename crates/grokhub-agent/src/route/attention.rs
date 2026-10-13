//! Request-aware attention for Auto: a **built-in Rust logic system** that
//! understands the ask (intent, risk, ambiguity, blast radius) and moves
//! **model + effort** off the class baseline when the work needs more — or
//! less — care.
//!
//! **No model-in-the-loop for Auto picks.** No judge LLM, no meta-prompt, no
//! routing model. Decisions are local, deterministic rules + capability
//! catalog + learned weights from product feedback (corrections, repair
//! failures, redos). Token savings apply only to the *task* model chosen.
//!
//! Invariants: `repair:diagnose` / `prepare:hard` always [`Attention::ForceTop`].

use grokhub_core::model_registry::Registry;

use super::capabilities::{self, class_need, Caps, ClassNeed};
use super::ladder::{self, Steer};
use super::policy;
use super::table::RoutingTable;

/// What attention wants relative to the class baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// Keep the class table / capability baseline.
    Stay,
    /// Prefer a stronger reasoning model (and a higher effort bias).
    Escalate,
    /// Prefer a cheaper/faster *task* model when the ask truly needs light care.
    DeEscalate,
    /// Always the strongest reasoning model (host repair / hard prepare).
    ForceTop,
}

impl Attention {
    pub fn breaks_episode_sticky(self) -> bool {
        matches!(self, Self::Escalate | Self::ForceTop)
    }

    pub fn raises_effort(self) -> bool {
        matches!(self, Self::Escalate | Self::ForceTop)
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Stay => "baseline",
            Self::Escalate => "escalated",
            Self::DeEscalate => "de-escalated",
            Self::ForceTop => "top model",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionDecision {
    pub kind: Attention,
    /// Stable rule id for route logs (`attn:escalate:host`).
    pub rule: &'static str,
    /// Short plain clause for `/why` and the Auto chip.
    pub why: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct AttentionInput<'a> {
    pub class: &'a str,
    pub text: &'a str,
    pub d: f64,
    pub needs_tools: bool,
    pub ctx_tokens: u64,
    pub hard_tool: bool,
    pub steer: Steer,
    /// Learned escalate bias for this class (−2..=+2) from [`super::feedback`].
    pub escalate_bias: i8,
    /// This episode already escalated once (sticky stronger model).
    pub prior_escalated: bool,
}

/// Structured read of the request — used to pick attention with full context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestRead {
    pub host_risk: bool,
    pub blast_radius: bool,
    pub multi_file: bool,
    pub ambiguous: bool,
    pub light_follow: bool,
    pub code_heavy: bool,
    pub unclear_diagnosis: bool,
}

/// Host / machine risk — wrong action can damage the user's system.
const HOST_RISK: &[&str] = &[
    "sudo",
    "rm -rf",
    "/etc/",
    "bootloader",
    "grub",
    "partition",
    "disk wipe",
    "systemd",
    "~/.ssh",
    "/.ssh/",
    "iptables",
    "firewalld",
    "chmod 777",
    "mkfs",
    "dd if=",
    "chown -r",
    "systemctl",
    "kernel panic",
    "won't boot",
    "broken boot",
];

/// Wide blast radius outside a single file edit.
const BLAST: &[&str] = &[
    "production",
    "drop table",
    "force push",
    "migrate database",
    "destructive",
    "all users",
    "entire fleet",
    "wipe",
    "factory reset",
];

const MULTI_FILE: &[&str] = &[
    "multi-file",
    "across files",
    "several files",
    "many files",
    "rename across",
    "refactor across",
    "move between",
    "split into modules",
];

const AMBIGUOUS: &[&str] = &[
    "not sure",
    "maybe",
    "might be",
    "unclear",
    "don't know why",
    "no idea",
    "weird",
    "intermittent",
    "flaky",
    "sometimes",
];

const DIAGNOSIS: &[&str] = &[
    "diagnose",
    "what's wrong",
    "root cause",
    "why is it broken",
    "keeps failing",
    "can't figure out",
];

const CODE_HEAVY: &[&str] = &[
    "implement",
    "refactor",
    "compile error",
    "type error",
    "borrow checker",
    "race condition",
    "deadlock",
    "api break",
];

const LIGHT_FOLLOW: &[&str] = &[
    "thanks",
    "thank you",
    "thx",
    "ok",
    "okay",
    "got it",
    "lgtm",
    "sounds good",
    "perfect",
    "that worked",
];

/// Phrase (`contains`) or whole-token match. Short tokens like `ok` must not
/// fire inside unrelated words (`book`, `token`, `okay` is listed separately).
fn has_signal(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|w| {
        if w.contains(' ') || w.contains('/') || w.contains('.') || w.len() > 4 {
            text.contains(w)
        } else {
            text.split(|c: char| !c.is_alphanumeric())
                .any(|tok| tok == *w)
        }
    })
}

/// Full local read of the request text + structural signals.
pub fn read_request(inp: &AttentionInput<'_>) -> RequestRead {
    let t = inp.text.to_ascii_lowercase();
    let host_risk = inp.hard_tool || has_signal(&t, HOST_RISK);
    let blast_radius = has_signal(&t, BLAST);
    let multi_file = has_signal(&t, MULTI_FILE)
        || t.matches(".rs").count() + t.matches(".py").count() + t.matches(".ts").count() >= 3;
    let ambiguous = has_signal(&t, AMBIGUOUS) || (inp.d > 0.45 && inp.d < 0.7 && t.contains('?'));
    let unclear_diagnosis = has_signal(&t, DIAGNOSIS) && (ambiguous || inp.d > 0.4);
    let code_heavy = has_signal(&t, CODE_HEAVY) || (inp.needs_tools && t.contains("```"));
    let light_follow = t.chars().count() < 96 && has_signal(&t, LIGHT_FOLLOW);
    RequestRead {
        host_risk,
        blast_radius,
        multi_file,
        ambiguous,
        light_follow,
        code_heavy,
        unclear_diagnosis,
    }
}

/// Pure, low-latency decision from class + a full request read + learned bias.
/// No I/O and **no model call**.
pub fn decide(inp: &AttentionInput<'_>) -> AttentionDecision {
    let class = policy::class_row(inp.class)
        .map(|r| r.class)
        .unwrap_or(inp.class.trim());
    let read = read_request(inp);

    // Hard invariants: host repair / hard prepare never take a cheap path.
    if class == "repair:diagnose" {
        return AttentionDecision {
            kind: Attention::ForceTop,
            rule: "attn:force_top:repair",
            why: "host repair always uses the strongest model",
        };
    }
    if class == "prepare:hard" {
        return AttentionDecision {
            kind: Attention::ForceTop,
            rule: "attn:force_top:prepare",
            why: "preparing a hard action stays on the strongest model",
        };
    }

    // Background / non-user-facing classes keep their baseline task model and
    // effort. Escalation is for live user work where a wrong weak model hurts.
    if !ladder::user_facing(class) {
        return AttentionDecision {
            kind: Attention::Stay,
            rule: "attn:stay:light_task",
            why: "background task — baseline keeps the task model cheap",
        };
    }

    if inp.steer == Steer::Harder {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:steer",
            why: "you asked it to think hard",
        };
    }

    // Risk and blast radius win over class baseline — task model must be strong.
    if read.host_risk {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:host",
            why: "host or high-risk actions need more care",
        };
    }
    if read.blast_radius {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:blast",
            why: "wide blast radius needs more care",
        };
    }
    if read.unclear_diagnosis {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:diagnosis",
            why: "unclear diagnosis needs a stronger model",
        };
    }
    if read.multi_file || (read.code_heavy && inp.d > 0.5) {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:multi",
            why: "messy multi-file or heavy code work needs a stronger model",
        };
    }
    if read.ambiguous && inp.d > 0.45 {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:ambiguous",
            why: "ambiguous ask needs more care",
        };
    }
    if inp.d > 0.7 || (inp.d > 0.55 && inp.ctx_tokens > 80_000) {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:hard",
            why: "this ask looks hard",
        };
    }

    // Learned bias: repeated under-care (corrections after Stay) nudges escalate.
    if inp.escalate_bias >= 2 && inp.d > 0.35 {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:learned",
            why: "learned from earlier corrections on this kind of step",
        };
    }
    if inp.prior_escalated && inp.d > 0.4 {
        return AttentionDecision {
            kind: Attention::Escalate,
            rule: "attn:escalate:episode",
            why: "this episode already needed more care",
        };
    }

    // De-escalate only when understanding is confident the *task* is light.
    let may_deesc = matches!(
        class,
        "chat:default" | "chat:quick" | "code:edit-small" | "desktop:soft"
    );
    if may_deesc
        && !read.host_risk
        && !read.blast_radius
        && inp.escalate_bias <= 0
        && (inp.steer == Steer::Quicker || (inp.d < 0.25 && read.light_follow))
    {
        return AttentionDecision {
            kind: Attention::DeEscalate,
            rule: "attn:deescalate:light",
            why: "a light follow-up can use a cheaper task model",
        };
    }

    AttentionDecision {
        kind: Attention::Stay,
        rule: "attn:stay",
        why: "class baseline fits this ask",
    }
}

/// Need vector when escalating / forcing top (ignore cost and speed for the task model).
fn top_need() -> ClassNeed {
    ClassNeed {
        reasoning: 10,
        code: 7,
        speed: 0,
        cost: 0,
        vision: 1,
        long_context: 5,
        tools: 7,
        require_reasoning: true,
    }
}

fn cheap_task_need() -> ClassNeed {
    ClassNeed {
        reasoning: 2,
        code: 1,
        speed: 10,
        cost: 10,
        vision: 0,
        long_context: 1,
        tools: 2,
        require_reasoning: false,
    }
}

fn caps_of(reg: &Registry, id: &str) -> Caps {
    let mut caps = capabilities::derive_caps(
        &reg.get(id)
            .map(|r| r.meta.clone())
            .unwrap_or_else(|| grokhub_core::model_registry::ModelMeta::bare(id)),
    );
    capabilities::apply_seed(id, &mut caps);
    caps
}

/// Re-order candidates for the attention decision. Table order is the Stay
/// baseline; escalate/force-top score against max-reasoning; de-escalate picks
/// a cheaper *task* model when attention is confident care can be light.
pub fn rank_models(
    attn: Attention,
    class: &str,
    candidates: &[&str],
    table: &RoutingTable,
    reg: &Registry,
    current: &str,
    ctx_tokens: u64,
) -> Vec<String> {
    match attn {
        Attention::Stay => policy::rank(class, candidates, table, reg, current, ctx_tokens),
        Attention::Escalate | Attention::ForceTop => {
            let need = top_need();
            let mut out: Vec<&str> = candidates.to_vec();
            out.retain(|id| !id.to_ascii_lowercase().contains("non-reasoning"));
            if out.is_empty() {
                return policy::rank(class, candidates, table, reg, current, ctx_tokens);
            }
            out.sort_by(|a, b| {
                let fa = capabilities::fit_score(&caps_of(reg, a), &need);
                let fb = capabilities::fit_score(&caps_of(reg, b), &need);
                fb.total_cmp(&fa).then(a.cmp(b))
            });
            out.into_iter().map(str::to_string).collect()
        }
        Attention::DeEscalate => {
            let need = if class_need(class).require_reasoning {
                ClassNeed {
                    require_reasoning: true,
                    reasoning: 4,
                    ..cheap_task_need()
                }
            } else {
                cheap_task_need()
            };
            let mut out: Vec<&str> = candidates.to_vec();
            if need.require_reasoning {
                out.retain(|id| !id.to_ascii_lowercase().contains("non-reasoning"));
            }
            out.sort_by(|a, b| {
                let fa = capabilities::fit_score(&caps_of(reg, a), &need);
                let fb = capabilities::fit_score(&caps_of(reg, b), &need);
                fb.total_cmp(&fa).then(a.cmp(b))
            });
            out.into_iter().map(str::to_string).collect()
        }
    }
}

/// Note under Settings / `/why table` baseline rows.
pub const TABLE_ATTENTION_NOTE: &str = "Baselines: Cheap / Workhorse / Code / Flagship (flagship reserved for plan/design/debug/prepare/repair). Each turn can escalate or de-escalate from built-in signals + learned feedback (no model-in-the-loop). Token savings apply to the task model.";

#[cfg(test)]
mod tests {
    use super::*;

    fn inp<'a>(class: &'a str, text: &'a str, d: f64) -> AttentionInput<'a> {
        AttentionInput {
            class,
            text,
            d,
            needs_tools: false,
            ctx_tokens: 1_000,
            hard_tool: false,
            steer: Steer::None,
            escalate_bias: 0,
            prior_escalated: false,
        }
    }

    #[test]
    fn repair_diagnose_is_always_force_top() {
        let d = decide(&inp("repair:diagnose", "keep it quick, just a peek", 0.0));
        assert_eq!(d.kind, Attention::ForceTop);
        assert_eq!(d.rule, "attn:force_top:repair");
        let d2 = decide(&AttentionInput {
            steer: Steer::Quicker,
            ..inp("repair:diagnose", "thanks", 0.0)
        });
        assert_eq!(d2.kind, Attention::ForceTop);
    }

    #[test]
    fn host_risk_escalates_chat_and_light_background_stays_on_cheap_task_model() {
        let esc = decide(&AttentionInput {
            needs_tools: true,
            ..inp(
                "chat:default",
                "Please sudo rm -rf /etc/old-config after backing up",
                0.4,
            )
        });
        assert_eq!(esc.kind, Attention::Escalate);
        assert_eq!(esc.rule, "attn:escalate:host");

        let stay = decide(&inp(
            "background:triage",
            "Label this line as bug or feature",
            0.1,
        ));
        assert_eq!(stay.kind, Attention::Stay);
        assert_eq!(stay.rule, "attn:stay:light_task");
    }

    #[test]
    fn light_follow_up_deescalates_chat_not_repair() {
        let d = decide(&inp("chat:default", "thanks, that fixed it", 0.0));
        assert_eq!(d.kind, Attention::DeEscalate);
        let r = decide(&inp("repair:diagnose", "thanks", 0.0));
        assert_eq!(r.kind, Attention::ForceTop);
    }

    #[test]
    fn ambiguous_diagnosis_escalates() {
        let d = decide(&inp(
            "chat:default",
            "Not sure what's wrong — keeps failing intermittently, can you diagnose?",
            0.5,
        ));
        assert_eq!(d.kind, Attention::Escalate);
        assert!(
            d.rule == "attn:escalate:diagnosis" || d.rule == "attn:escalate:ambiguous",
            "{}",
            d.rule
        );
    }

    #[test]
    fn book_it_is_not_a_light_ok_follow_up() {
        // Regression: substring `ok` inside `book` must not de-escalate.
        let read = read_request(&inp("chat:default", "now book it", 0.0));
        assert!(!read.light_follow);
        let d = decide(&inp("chat:default", "now book it", 0.0));
        assert_eq!(d.kind, Attention::Stay, "{}", d.rule);
        let thanks = decide(&inp("chat:default", "ok, thanks", 0.0));
        assert_eq!(thanks.kind, Attention::DeEscalate);
    }
}
