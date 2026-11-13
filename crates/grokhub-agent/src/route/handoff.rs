//! Failover PR 7: the hand-off packet — what a model that takes over a step
//! partway through is told, and the record of every switch.
//!
//! The history itself is the lossless part of the packet: the ask, the tool
//! calls and their results, the plan (the session's todos ride the prompt's
//! volatile block every step) and any partial reply the chat already showed
//! all go out again on the next attempt, compacted into a summary when the
//! session outgrew the window. What was missing was the header over that
//! history: which model stopped, why, which models already failed this step,
//! and who wrote what. This module keeps that header per conversation while
//! a step's resume chain is live, renders it as one cabin note for the model
//! that takes over, and stamps each in-call switch onto the route record
//! (`RouteRecord::handoffs`) with a plain line for the chat.
//!
//! Only model ids and rule ids are ever kept here — never prompt text. The
//! packet also names the person's latest ask and the calls already tried
//! ([`goal_facts`]); those are read from the history at hand-off time and
//! only rendered, never stored.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::client::ClientError;

use super::log::HandoffLine;

/// Told to the model when its reply was cut off and the step goes again
/// (the packet's closing line when a partial reply was kept).
pub const RESUME_NOTE: &str = "Your last reply was cut off by a connection problem. Carry on from where it stopped: do not repeat what you already said, and do not redo steps whose tool results are above.";

/// The closing line when the step hands over with tool work but no reply text.
const CARRY_ON: &str = "Carry on from where the work stopped: do not repeat what was already said, and do not redo steps whose tool results are above.";

/// Failed tries one step's packet names at most; the rest fold into a count.
const FAILED_CAP: usize = 6;
/// Sessions whose hand-off state is kept at once; the oldest goes first.
const SESSION_CAP: usize = 128;
/// Model ids are rendered at most this long in a packet or record line.
const ID_CAP: usize = 80;

/// Why a model stopped, as a rule id for the route log.
pub fn stop_rule(err: &ClientError) -> &'static str {
    match err {
        ClientError::PaymentRequired { .. } => "handoff:credit",
        ClientError::RateLimited { message, .. } => {
            let lower = message.to_ascii_lowercase();
            if lower.contains("free-usage") || lower.contains("free_model") {
                "handoff:free_cap"
            } else {
                "handoff:limit"
            }
        }
        ClientError::Server { status, .. } => match *status {
            402 => "handoff:credit",
            429 => "handoff:limit",
            401 | 403 => "handoff:auth",
            _ => "handoff:outage",
        },
        ClientError::IdleTimeout => "handoff:stall",
        ClientError::Refused { .. } => "handoff:refused",
        ClientError::Disconnect | ClientError::CutOff(_) => "handoff:dropped",
        ClientError::Transport(m) if grokhub_core::offline::is_refusal(m) => "handoff:offline",
        ClientError::Transport(_) => "handoff:dropped",
        ClientError::Auth { .. } | ClientError::KeyOffer { .. } => "handoff:auth",
        _ => "handoff:error",
    }
}

/// The stop rule as a verb phrase that follows a model id:
/// "grok-4.7 hit its usage limit."
pub fn plain_reason(rule: &str) -> &'static str {
    match rule {
        "handoff:credit" => "ran out of credit",
        "handoff:limit" => "hit its usage limit",
        "handoff:free_cap" => "reached the free daily cap",
        "handoff:outage" => "stopped answering (a server error)",
        "handoff:stall" => "stopped responding",
        "handoff:refused" => "declined to answer",
        "handoff:dropped" => "dropped the connection partway",
        "handoff:offline" => "could not be reached (this computer is offline)",
        "handoff:auth" => "rejected the sign-in",
        _ => "hit a problem",
    }
}

/// The one plain line the chat shows when a turn changes models. Model names
/// stay out of it on purpose (the standing design); the route log names them.
pub fn chat_line(rule: &str) -> &'static str {
    match rule {
        "handoff:credit" => {
            "The model I was using ran out of credit, so I'm carrying on with another one — the work so far is kept."
        }
        "handoff:limit" | "handoff:free_cap" => {
            "The model I was using hit its limit, so I'm carrying on with another one — the work so far is kept."
        }
        "handoff:auth" => {
            "The model I was using wouldn't sign in, so I'm carrying on with another one — the work so far is kept."
        }
        "handoff:error" => {
            "The model I was using hit a problem, so I'm carrying on with another one — the work so far is kept."
        }
        _ => {
            "The model I was using stopped answering, so I'm carrying on with another one — the work so far is kept."
        }
    }
}

/// One failed try at the step: the model and why it stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FailedTry {
    model: String,
    rule: &'static str,
}

/// A conversation's live hand-off state: what failed at the step being
/// resumed, and which models answered the steps before it in this run.
#[derive(Debug, Default)]
struct Chain {
    failed: Vec<FailedTry>,
    served: Vec<String>,
}

static CHAINS: Mutex<BTreeMap<String, Chain>> = Mutex::new(BTreeMap::new());
/// Switches waiting for the route record of the call that made them.
static STASHED: Mutex<BTreeMap<String, Vec<HandoffLine>>> = Mutex::new(BTreeMap::new());

fn short_id(model: &str) -> String {
    if model.chars().count() > ID_CAP {
        model.chars().take(ID_CAP).collect()
    } else {
        model.to_string()
    }
}

fn with_chain<R>(session: &str, f: impl FnOnce(&mut Chain) -> R) -> R {
    let mut chains = CHAINS.lock().unwrap_or_else(|e| e.into_inner());
    if chains.len() >= SESSION_CAP && !chains.contains_key(session) {
        chains.pop_first();
    }
    f(chains.entry(session.to_string()).or_default())
}

/// A new run on this conversation: the last run's failures and attributions
/// are done with.
pub fn begin(session: &str) {
    if session.is_empty() {
        return;
    }
    let mut chains = CHAINS.lock().unwrap_or_else(|e| e.into_inner());
    chains.remove(session);
}

/// A step on this conversation finished: its failures stop shadowing the
/// next step's packet. What the run's models served stays for attribution.
pub fn clear_chain(session: &str) {
    if session.is_empty() {
        return;
    }
    with_chain(session, |c| c.failed.clear());
}

/// A model answered a step in this run (attribution for a later packet).
pub fn note_served(session: &str, model: &str) {
    if session.is_empty() || model.is_empty() {
        return;
    }
    with_chain(session, |c| {
        let model = short_id(model);
        if c.served.last() != Some(&model) {
            c.served.push(model);
        }
    });
}

/// The model that answered this conversation's latest call in this run.
pub fn last_served(session: &str) -> Option<String> {
    let chains = CHAINS.lock().unwrap_or_else(|e| e.into_inner());
    chains.get(session)?.served.last().cloned()
}

/// A model failed a call and nothing in that call took over: the try joins
/// the step's chain, so a resume's packet can name it.
pub fn note_failure(session: &str, model: &str, err: &ClientError) {
    if session.is_empty() || model.is_empty() {
        return;
    }
    with_chain(session, |c| push_failed(c, model, stop_rule(err)));
}

fn push_failed(chain: &mut Chain, model: &str, rule: &'static str) {
    let model = short_id(model);
    if chain
        .failed
        .iter()
        .any(|f| f.model == model && f.rule == rule)
    {
        return;
    }
    chain.failed.push(FailedTry { model, rule });
}

/// A model stopped inside a call and another took the same call over: the
/// try joins the step's chain AND the switch is stashed for the call's
/// route record ([`take_stashed`]).
pub fn note_switch(session: &str, from: &str, to: &str, err: &ClientError) {
    log_switch(session, from, to, err);
    if !session.is_empty() && !from.is_empty() {
        with_chain(session, |c| push_failed(c, from, stop_rule(err)));
    }
}

/// Only the route-record half of [`note_switch`], for call sites whose
/// callers never build a packet (the router's own non-streamed calls).
pub fn log_switch(session: &str, from: &str, to: &str, err: &ClientError) {
    if session.is_empty() || from.is_empty() || to.is_empty() {
        return;
    }
    let line = HandoffLine {
        from: short_id(from),
        to: short_id(to),
        rule: stop_rule(err).to_string(),
    };
    let mut stashed = STASHED.lock().unwrap_or_else(|e| e.into_inner());
    if stashed.len() >= SESSION_CAP && !stashed.contains_key(session) {
        stashed.pop_first();
    }
    stashed.entry(session.to_string()).or_default().push(line);
}

/// The switches stashed for a session's call, drained: `route_log` puts
/// them on that call's route record.
pub fn take_stashed(session: &str) -> Vec<HandoffLine> {
    if session.is_empty() {
        return Vec::new();
    }
    let mut stashed = STASHED.lock().unwrap_or_else(|e| e.into_inner());
    stashed.remove(session).unwrap_or_default()
}

/// What a step hands over with, for the packet's header.
#[derive(Debug, Default)]
pub struct PacketFacts {
    /// Part of a reply was shown and kept in the history above the note.
    pub partial_kept: bool,
    /// Tool results the history holds so far (this step's and earlier ones').
    pub tool_results: usize,
    /// The history above was compacted into a summary at some point.
    pub compacted: bool,
    /// Plan items still open in the session's todo list.
    pub open_items: usize,
    /// The person's latest ask, on one line and clipped ([`goal_facts`]).
    pub last_ask: String,
    /// Tool calls made since that ask, oldest first, as `name: short args`.
    pub attempts: Vec<String>,
}

/// Characters of the person's ask a packet repeats.
const ASK_CAP: usize = 400;
/// Characters of one attempt's arguments a packet repeats.
const ARG_CAP: usize = 80;
/// Distinct attempts a packet names; the newest are kept.
const ATTEMPT_CAP: usize = 6;

/// The goal half of a packet, read from the history the step resends: the
/// person's latest ask (a note the app wrote in their place doesn't count)
/// and the tool calls made since, so the model that takes over never has to
/// ask what it was doing. Rendered only into the packet, never kept here.
pub fn goal_facts(history: &[crate::client::InputItem]) -> (String, Vec<String>) {
    use crate::client::InputItem;
    let ask_at = history.iter().rposition(|item| {
        matches!(item, InputItem::Message { role, .. } if role == "user")
            && !super::live::last_user_text(std::slice::from_ref(item))
                .0
                .trim()
                .is_empty()
    });
    let last_ask = ask_at
        .map(|i| one_line(&super::live::last_user_text(&history[i..=i]).0, ASK_CAP))
        .unwrap_or_default();
    let mut tried: Vec<(String, usize)> = Vec::new();
    for item in &history[ask_at.map_or(0, |i| i + 1)..] {
        let InputItem::FunctionCall {
            name, arguments, ..
        } = item
        else {
            continue;
        };
        let line = format!("{name}: {}", short_args(arguments));
        match tried.iter_mut().find(|(seen, _)| *seen == line) {
            Some((_, n)) => *n += 1,
            None => tried.push((line, 1)),
        }
    }
    let skip = tried.len().saturating_sub(ATTEMPT_CAP);
    let attempts = tried
        .into_iter()
        .skip(skip)
        .map(|(line, n)| {
            if n > 1 {
                format!("{line} ({n} times)")
            } else {
                line
            }
        })
        .collect();
    (last_ask, attempts)
}

/// A call's main argument (its query, URL, command or path), else its JSON.
fn short_args(arguments: &str) -> String {
    let value: Option<serde_json::Value> = serde_json::from_str(arguments).ok();
    let main = value.as_ref().and_then(|v| {
        ["query", "url", "command", "target_file", "path", "pattern"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|s| s.as_str()))
    });
    one_line(main.unwrap_or(arguments), ARG_CAP)
}

/// `text` on one line, cut to `cap` characters with an ellipsis.
fn one_line(text: &str, cap: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > cap {
        let mut cut: String = flat.chars().take(cap).collect();
        cut.push('…');
        cut
    } else {
        flat
    }
}

/// The hand-off packet for a step that is about to be re-sent on its next
/// route: one cabin note naming who stopped and why, who already failed,
/// who wrote what is above, and what stands. `None` when the step produced
/// nothing at all — no partial reply, no tool results — because then the
/// resent request must be exactly the one that failed (a clean retry, no
/// "continue" the user never sent). The packet itself is only ever this
/// header: the work it points at already rides the history, so a long
/// session can't make it grow.
pub fn packet(session: &str, err: &ClientError, facts: PacketFacts) -> Option<String> {
    if !facts.partial_kept && facts.tool_results == 0 {
        return None;
    }
    let (failed, served) = if session.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let chains = CHAINS.lock().unwrap_or_else(|e| e.into_inner());
        match chains.get(session) {
            Some(c) => (c.failed.clone(), c.served.clone()),
            None => (Vec::new(), Vec::new()),
        }
    };
    let mut out =
        String::from("Hand-off: this step is moving to a different model partway through.");
    // Who stopped: the chain's last try is this failure; without a chain
    // (a caller that never recorded) the error still names the reason.
    let stopped = failed.last();
    match stopped {
        Some(f) => {
            out.push_str(&format!(" {} {}.", f.model, plain_reason(f.rule)));
        }
        None => {
            out.push_str(&format!(
                " The previous model {}.",
                plain_reason(stop_rule(err))
            ));
        }
    }
    if failed.len() > 1 {
        let shown: Vec<&str> = failed
            .iter()
            .take(FAILED_CAP)
            .map(|f| f.model.as_str())
            .collect();
        out.push_str(&format!(
            " Models that already failed this step: {}",
            shown.join(", ")
        ));
        if failed.len() > FAILED_CAP {
            out.push_str(&format!(" (and {} more)", failed.len() - FAILED_CAP));
        }
        out.push('.');
    }
    // Attribution: the models that answered this run's earlier steps.
    let earlier: Vec<&str> = served
        .iter()
        .filter(|m| stopped.is_none_or(|f| f.model != **m))
        .map(String::as_str)
        .collect();
    if !earlier.is_empty() {
        out.push_str(&format!(
            " Earlier steps in this run were answered by {}.",
            earlier.join(", then ")
        ));
    }
    if facts.compacted {
        out.push_str(" Earlier parts of this conversation were summarized to fit; the summary above stands in for them, and the plan and open items in it still stand.");
    }
    if facts.open_items > 0 {
        out.push_str(&format!(
            " The plan still has {} open item{}; they are listed above.",
            facts.open_items,
            if facts.open_items == 1 { "" } else { "s" }
        ));
    }
    if !facts.last_ask.is_empty() {
        out.push_str(&format!(
            " The person's latest ask: \"{}\". Keep working on that ask; do not ask the person what they wanted.",
            facts.last_ask
        ));
    }
    if !facts.attempts.is_empty() {
        out.push_str(&format!(
            " Already tried this step: {}.",
            facts.attempts.join("; ")
        ));
    }
    if facts.partial_kept {
        if let Some(f) = stopped {
            out.push_str(&format!(
                " The partial reply above was written by {}.",
                f.model
            ));
        }
        out.push(' ');
        out.push_str(RESUME_NOTE);
    } else {
        out.push(' ');
        out.push_str(CARRY_ON);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(status: u16) -> ClientError {
        ClientError::Server {
            status,
            message: "boom".into(),
            retry_after_secs: None,
        }
    }

    #[test]
    fn stop_rules_name_each_kind_of_stop() {
        assert_eq!(
            stop_rule(&ClientError::PaymentRequired {
                status: 402,
                message: "x".into()
            }),
            "handoff:credit"
        );
        assert_eq!(
            stop_rule(&ClientError::RateLimited {
                retry_after_secs: None,
                message: "slow down".into()
            }),
            "handoff:limit"
        );
        assert_eq!(
            stop_rule(&ClientError::RateLimited {
                retry_after_secs: None,
                message: "free-usage-exhausted".into()
            }),
            "handoff:free_cap"
        );
        assert_eq!(stop_rule(&server(503)), "handoff:outage");
        assert_eq!(stop_rule(&server(429)), "handoff:limit");
        assert_eq!(stop_rule(&ClientError::IdleTimeout), "handoff:stall");
        assert_eq!(stop_rule(&ClientError::Disconnect), "handoff:dropped");
        assert_eq!(
            stop_rule(&ClientError::Auth {
                status: 401,
                message: "bad".into()
            }),
            "handoff:auth"
        );
        // Every rule id has a plain reason and a chat line.
        for rule in [
            "handoff:credit",
            "handoff:limit",
            "handoff:free_cap",
            "handoff:outage",
            "handoff:stall",
            "handoff:dropped",
            "handoff:offline",
            "handoff:auth",
            "handoff:error",
        ] {
            assert!(!plain_reason(rule).is_empty(), "{rule}");
            assert!(chat_line(rule).contains("work so far is kept"), "{rule}");
            assert!(!chat_line(rule).contains("grok"), "{rule}");
        }
    }

    #[test]
    fn a_step_with_nothing_done_hands_over_nothing() {
        begin("s-nothing");
        note_failure("s-nothing", "grok-4.7", &server(503));
        let facts = PacketFacts {
            partial_kept: false,
            tool_results: 0,
            compacted: false,
            open_items: 0,
            ..PacketFacts::default()
        };
        assert_eq!(packet("s-nothing", &server(503), facts), None);
    }

    #[test]
    fn the_packet_names_who_stopped_who_failed_and_who_wrote_what() {
        begin("s-full");
        note_served("s-full", "grok-4.3");
        note_failure(
            "s-full",
            "openrouter/free-a",
            &ClientError::RateLimited {
                retry_after_secs: None,
                message: "free-usage-exhausted".into(),
            },
        );
        note_failure("s-full", "grok-4.7", &server(503));
        let facts = PacketFacts {
            partial_kept: true,
            tool_results: 3,
            compacted: true,
            open_items: 2,
            ..PacketFacts::default()
        };
        let note = packet("s-full", &server(503), facts).expect("packet");
        assert!(
            note.contains("grok-4.7 stopped answering (a server error)"),
            "{note}"
        );
        assert!(
            note.contains("Models that already failed this step: openrouter/free-a, grok-4.7"),
            "{note}"
        );
        assert!(note.contains("answered by grok-4.3"), "{note}");
        assert!(note.contains("summarized to fit"), "{note}");
        assert!(note.contains("plan still has 2 open items"), "{note}");
        assert!(
            note.contains("partial reply above was written by grok-4.7"),
            "{note}"
        );
        assert!(note.ends_with(RESUME_NOTE), "{note}");
    }

    #[test]
    fn a_tools_only_packet_carries_on_without_claiming_a_cut_reply() {
        begin("s-tools");
        note_failure("s-tools", "grok-4.7", &ClientError::IdleTimeout);
        let facts = PacketFacts {
            partial_kept: false,
            tool_results: 1,
            compacted: false,
            open_items: 0,
            ..PacketFacts::default()
        };
        let note = packet("s-tools", &ClientError::IdleTimeout, facts).expect("packet");
        assert!(note.contains("grok-4.7 stopped responding"), "{note}");
        assert!(
            note.contains("do not redo steps whose tool results are above"),
            "{note}"
        );
        assert!(!note.contains("cut off"), "{note}");
    }

    #[test]
    fn the_packet_carries_the_last_ask_and_what_was_already_tried() {
        use crate::client::{ContentPart, InputItem};
        let user = |t: &str| InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(t.into())],
        };
        let call = |id: &str, name: &str, args: &str| InputItem::FunctionCall {
            call_id: id.into(),
            name: name.into(),
            arguments: args.into(),
        };
        let out = |id: &str| InputItem::FunctionCallOutput {
            call_id: id.into(),
            output: "bash: web_search: command not found".into(),
        };
        let shell = r#"{"command":"web_search \"best budget ssd 2026\""}"#;
        let history = vec![
            user("hey what's up"),
            call("a0", "read_file", r#"{"target_file":"old.txt"}"#),
            out("a0"),
            user("what's the best budget ssd right now?"),
            call("c1", "run_terminal_command", shell),
            out("c1"),
            call("c2", "run_terminal_command", shell),
            out("c2"),
            call("c3", "web_search", r#"{"query":"budget nvme ssd"}"#),
            out("c3"),
            // A note the app wrote in the person's place is not their ask.
            user(&crate::compact::cabin_note_text(
                "Checkpoint: one item open",
            )),
        ];
        let (last_ask, attempts) = goal_facts(&history);
        assert_eq!(last_ask, "what's the best budget ssd right now?");
        assert_eq!(
            attempts,
            [
                "run_terminal_command: web_search \"best budget ssd 2026\" (2 times)".to_string(),
                "web_search: budget nvme ssd".to_string(),
            ]
        );
        begin("s-goal");
        note_failure("s-goal", "grok-4.7", &server(503));
        let facts = PacketFacts {
            tool_results: 3,
            last_ask,
            attempts,
            ..PacketFacts::default()
        };
        let note = packet("s-goal", &server(503), facts).expect("packet");
        assert!(
            note.contains("The person's latest ask: \"what's the best budget ssd right now?\""),
            "{note}"
        );
        assert!(
            note.contains("Already tried this step: run_terminal_command: web_search \"best budget ssd 2026\" (2 times); web_search: budget nvme ssd."),
            "{note}"
        );
        assert!(
            note.contains("do not ask the person what they wanted"),
            "{note}"
        );
        // A long ask is clipped on one line.
        let long = format!("find {}\nnow", "x".repeat(2_000));
        let (ask, _) = goal_facts(&[user(&long)]);
        assert_eq!(ask.chars().count(), ASK_CAP + 1);
        assert!(ask.starts_with("find xxx") && ask.ends_with('…') && !ask.contains('\n'));
    }

    #[test]
    fn switches_are_stashed_for_the_record_and_failures_dedupe() {
        begin("s-switch");
        let err = ClientError::PaymentRequired {
            status: 402,
            message: "no credit".into(),
        };
        note_switch("s-switch", "openrouter/paid-a", "grok-4.7", &err);
        note_switch("s-switch", "openrouter/paid-a", "grok-4.7", &err);
        let lines = take_stashed("s-switch");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].from, "openrouter/paid-a");
        assert_eq!(lines[0].to, "grok-4.7");
        assert_eq!(lines[0].rule, "handoff:credit");
        assert!(take_stashed("s-switch").is_empty());
        // The chain holds the failed try once, however it was recorded.
        let facts = PacketFacts {
            partial_kept: false,
            tool_results: 1,
            compacted: false,
            open_items: 0,
            ..PacketFacts::default()
        };
        let note = packet("s-switch", &err, facts).expect("packet");
        assert!(
            note.contains("openrouter/paid-a ran out of credit"),
            "{note}"
        );
        clear_chain("s-switch");
        let facts = PacketFacts {
            partial_kept: false,
            tool_results: 1,
            compacted: false,
            open_items: 0,
            ..PacketFacts::default()
        };
        let note = packet("s-switch", &err, facts).expect("packet");
        assert!(
            note.contains("The previous model ran out of credit"),
            "{note}"
        );
    }
}
