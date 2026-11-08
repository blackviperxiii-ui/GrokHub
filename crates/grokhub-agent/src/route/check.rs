//! The connection check: "is my Claude key working?" answered from what the
//! wire says right now, never from a guess. For each online provider you
//! added it reads whether the key is saved and allowed, asks the provider's
//! model listing with that key (nothing of yours goes out), and, when the
//! provider is allowed for chats, asks its cheapest model for one word. The
//! report also says whether Grok is signed in and, for an 18+ chat, why
//! nothing in it goes online.
//!
//! Two ways in: the `models_check` tool, and a turn that asks about the
//! online models or a key ([`connection_question`]), which gets the report
//! as a note on the call ([`note`]) so the answering model can't guess.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use grokhub_core::model_registry::discover::split_provider_model;
use grokhub_core::model_registry::store::{load_quota, load_registry};
use grokhub_core::model_registry::{Credential, Registry};

use super::providers::{self, ProviderKind, ProviderState, TrialProbe};
use crate::client::ClientError;
use crate::harness::ConsentLedger;

/// Opens the note a connection question gets on its call.
pub const NOTE_OPENS: &str = "[Connections] ";

/// What one live step came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    /// The listing answered with this many models.
    Listed(usize),
    /// The test reply came back in this many ms, and said the word.
    Replied(u128),
    /// The test reply came back but didn't say the word.
    Odd(u128),
    /// The provider refused the key.
    KeyRejected,
    /// Out of credit or over a spend limit.
    NoCredit,
    /// Rate limited right now.
    Limited,
    /// Couldn't get an answer at all, and why.
    Unreachable(String),
}

/// One provider's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCheck {
    pub name: String,
    pub state: ProviderState,
    /// The quota ledger rests it (spend limit or out of credit).
    pub benched: bool,
    /// The listing read, when it ran.
    pub listing: Option<Wire>,
    /// The one-word reply, when it ran.
    pub reply: Option<Wire>,
}

/// The whole check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub grok: Credential,
    pub providers: Vec<ProviderCheck>,
    /// Whether the live steps ran (a status-only read skips them).
    pub live: bool,
}

/// How a provider reads in the report.
pub fn display_name(kind: ProviderKind, id: &str, dest: &str) -> String {
    match (kind, id) {
        (ProviderKind::Anthropic, _) => "Claude (Anthropic)".to_string(),
        (_, "openrouter") => "OpenRouter".to_string(),
        _ => dest.to_string(),
    }
}

fn wire_of(err: &ClientError) -> Wire {
    match err {
        ClientError::Auth { .. } | ClientError::KeyOffer { .. } => Wire::KeyRejected,
        ClientError::PaymentRequired { .. } => Wire::NoCredit,
        ClientError::RateLimited { .. } => Wire::Limited,
        ClientError::Server { status, .. } => {
            Wire::Unreachable(format!("it answered HTTP {status}"))
        }
        ClientError::Transport(e) => Wire::Unreachable(grokhub_core::redact_secrets(e)),
        ClientError::IdleTimeout | ClientError::Disconnect => {
            Wire::Unreachable("the connection dropped".into())
        }
        ClientError::Protocol(e) => Wire::Unreachable(grokhub_core::redact_secrets(e)),
        other => Wire::Unreachable(grokhub_core::redact_secrets(&other.to_string())),
    }
}

/// The provider's cheapest model Auto could route to: the one the test
/// reply goes to. Free first, then the lowest prompt price.
pub fn test_model(reg: &Registry, provider_id: &str) -> Option<String> {
    reg.models
        .iter()
        .filter(|(id, r)| {
            r.state.routable()
                && !super::policy::utility_model(id)
                && split_provider_model(id).is_some_and(|(p, _)| p == provider_id)
        })
        .min_by_key(|(id, r)| {
            (
                !providers::is_free_model(id),
                r.meta.prices.prompt.unwrap_or(u64::MAX),
                (*id).clone(),
            )
        })
        .map(|(id, _)| id.clone())
}

/// Run the check. `live` false reads only what's saved (no network).
pub fn run(config_dir: &Path, live: bool, now_ms: u64) -> Check {
    let reg = load_registry(config_dir);
    let quota = load_quota(config_dir);
    let ledger = ConsentLedger::load(config_dir);
    let providers = providers::load_providers(config_dir)
        .into_iter()
        .map(|p| {
            let state = providers::provider_state(config_dir, &ledger, &p);
            let benched = p.kind == ProviderKind::Anthropic && quota.anthropic_benched(now_ms);
            // Nothing goes to a destination you haven't allowed, not even
            // the listing (the egress gate parks it), so a key that isn't
            // allowed yet can't be tested.
            let listing = (live && state == ProviderState::Ready).then(|| {
                match providers::live_listing(config_dir, &p.id) {
                    Ok(n) => Wire::Listed(n),
                    Err(e) => wire_of(&e),
                }
            });
            let key_ok = matches!(listing, Some(Wire::Listed(_)));
            let reply = (live && key_ok)
                .then(|| test_model(&reg, &p.id))
                .flatten()
                .map(|model| {
                    let began = Instant::now();
                    let span = format!("check:{now_ms}:{}", p.id);
                    match providers::probe_model(config_dir, &model, TrialProbe::Text, &span) {
                        Ok(true) => Wire::Replied(began.elapsed().as_millis()),
                        Ok(false) => Wire::Odd(began.elapsed().as_millis()),
                        Err(e) => wire_of(&e),
                    }
                });
            ProviderCheck {
                name: display_name(p.kind, &p.id, &p.dest()),
                state,
                benched,
                listing,
                reply,
            }
        })
        .collect();
    Check {
        grok: reg.entitlement.credential,
        providers,
        live,
    }
}

fn wire_words(w: &Wire) -> String {
    match w {
        Wire::Listed(n) => format!("the key works (it lists {n} models)"),
        Wire::Replied(ms) => format!("a one-word test reply came back in {ms} ms"),
        Wire::Odd(ms) => {
            format!("a test reply came back in {ms} ms but didn't say the word it was asked for")
        }
        Wire::KeyRejected => "the provider rejected the saved key".into(),
        Wire::NoCredit => "the account is out of credit or over its spend limit".into(),
        Wire::Limited => "it's rate limited right now".into(),
        Wire::Unreachable(why) => format!("it couldn't be reached ({why})"),
    }
}

fn provider_line(p: &ProviderCheck, live: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    match p.state {
        ProviderState::NoKey => {
            return format!(
                "- {}: added, but its key is gone from the keyring, so Auto can't use it until the key is added again.",
                p.name
            );
        }
        ProviderState::NotAllowed => {
            parts.push("key saved but not allowed yet, so nothing goes to it (not even a test) until the user allows it once with the Allow button on its row in Settings".into());
        }
        ProviderState::Ready => parts.push("key saved and allowed for chats".into()),
    }
    if let Some(w) = &p.listing {
        parts.push(wire_words(w));
    }
    if let Some(w) = &p.reply {
        parts.push(wire_words(w));
    }
    if p.benched {
        parts.push("Auto is resting it for now (spend limit or out of credit)".into());
    }
    let usable = p.state == ProviderState::Ready
        && !p.benched
        && (!live || matches!(p.reply, Some(Wire::Replied(_)) | None))
        && !matches!(
            p.listing,
            Some(Wire::KeyRejected | Wire::NoCredit | Wire::Unreachable(_))
        );
    let verdict = if usable && live && p.reply.is_some() {
        " Auto can use it."
    } else if usable && !live {
        " Auto can use it (not tested live just now)."
    } else {
        ""
    };
    format!("- {}: {}.{verdict}", p.name, parts.join("; "))
}

/// The report the user (or the answering model) reads.
pub fn report(check: &Check, adult_chat: bool) -> String {
    let mut out = vec![if check.live {
        "Connection check, run live just now:".to_string()
    } else {
        "Connection status (saved settings, not tested live):".to_string()
    }];
    out.push(match check.grok {
        Credential::Plan => "- Grok (xAI): signed in with the Grok plan.".into(),
        Credential::ApiKey => "- Grok (xAI): signed in with an xAI API key.".into(),
        Credential::None => "- Grok (xAI): not signed in.".into(),
    });
    if check.providers.is_empty() {
        out.push("- No other online providers (Claude, OpenRouter) have been added.".into());
    }
    out.extend(check.providers.iter().map(|p| provider_line(p, check.live)));
    out.push("The app picks which model answers each turn on its own; nothing said in a chat switches or \"triggers\" one.".into());
    if adult_chat {
        out.push("This chat is 18+, so by the user's own rule every turn in it runs on this computer's model and nothing in it goes online, Claude included. The check above sent no chat text.".into());
    }
    out.join("\n")
}

/// Does this ask want to know about the online models: a key, a provider
/// being reachable, or which models can answer? Then it gets the check.
pub fn connection_question(text: &str) -> bool {
    let t = super::ask::own_words(text).to_lowercase();
    const LIST: &[&str] = &[
        "what models",
        "which models",
        "what ai models",
        "which ai models",
        "models you have access",
        "models do you have",
        "models can you use",
        "models are available",
        "models you can use",
    ];
    const WHO: &[&str] = &[
        "claude",
        "anthropic",
        "openrouter",
        "open router",
        "api key",
        "xai key",
    ];
    const ASK: &[&str] = &[
        "test",
        "check",
        "reach",
        "connect",
        "working",
        "work?",
        "access",
        "available",
        "live",
        "hooked up",
        "set up",
        "added",
        "use it",
    ];
    LIST.iter().any(|q| t.contains(q))
        || (WHO.iter().any(|w| t.contains(w)) && ASK.iter().any(|a| t.contains(a)))
}

/// How long one turn's check stands: every call of a turn (each tool
/// round) asks the same question, and one live check covers them all.
const TURN_REUSE_MS: u64 = 2 * 60 * 1000;

/// The report for one turn's connection question: run live once, then
/// reused for the rest of that turn's calls.
pub fn report_for_turn(
    config_dir: &Path,
    conversation: &str,
    text: &str,
    adult_chat: bool,
) -> String {
    static LAST: Mutex<Option<(String, u64, String)>> = Mutex::new(None);
    let key = format!("{conversation}\u{0}{text}\u{0}{adult_chat}");
    let now = grokhub_core::now_ms();
    let lock = || LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, at, r)) = lock().as_ref() {
        if *k == key && now.saturating_sub(*at) < TURN_REUSE_MS {
            return r.clone();
        }
    }
    let r = report(&run(config_dir, true, now), adult_chat);
    *lock() = Some((key, now, r.clone()));
    r
}

/// The note a connection question's call carries: the live report and how
/// to use it.
pub fn note(report: &str) -> String {
    format!(
        "{NOTE_OPENS}The user asked about the online models or a key. The app just ran this check:\n{report}\nTell the user what it found, plainly and in your own voice. This check is the test: don't ask them to update, restart, sign in again or send a harder question to test anything, and don't claim any result it doesn't show. Don't mention this note."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(state: ProviderState, listing: Option<Wire>, reply: Option<Wire>) -> String {
        provider_line(
            &ProviderCheck {
                name: "Claude (Anthropic)".into(),
                state,
                benched: false,
                listing,
                reply,
            },
            true,
        )
    }

    #[test]
    fn a_working_key_reads_as_usable_and_a_bad_one_says_why() {
        let ok = line(
            ProviderState::Ready,
            Some(Wire::Listed(9)),
            Some(Wire::Replied(840)),
        );
        assert_eq!(
            ok,
            "- Claude (Anthropic): key saved and allowed for chats; the key works (it lists 9 models); a one-word test reply came back in 840 ms. Auto can use it."
        );
        let bad = line(ProviderState::Ready, Some(Wire::KeyRejected), None);
        assert!(bad.contains("rejected the saved key"), "{bad}");
        assert!(!bad.contains("Auto can use it"), "{bad}");
        let held = line(ProviderState::NotAllowed, None, None);
        assert!(held.contains("not allowed yet"), "{held}");
        assert!(!held.contains("Auto can use it"), "{held}");
        let gone = line(ProviderState::NoKey, None, None);
        assert!(gone.contains("key is gone"), "{gone}");
    }

    #[test]
    fn the_report_names_grok_and_says_why_an_adult_chat_stays_local() {
        let check = Check {
            grok: Credential::Plan,
            providers: Vec::new(),
            live: true,
        };
        let r = report(&check, true);
        assert!(r.starts_with("Connection check, run live just now:"), "{r}");
        assert!(
            r.contains("Grok (xAI): signed in with the Grok plan."),
            "{r}"
        );
        assert!(r.contains("No other online providers"), "{r}");
        assert!(r.contains("This chat is 18+"), "{r}");
        assert!(!report(&check, false).contains("18+"));
    }

    #[test]
    fn connection_questions_are_the_ones_about_keys_providers_and_models() {
        for ask in [
            "ok its done can you test the claude api now?",
            "no i want you to test that api key for claude first to make sure its connected",
            "ok check to make sure you can reach claude throught the api i added",
            "can you give me a list of all the ai models you have access too",
            "is openrouter working?",
        ] {
            assert!(connection_question(ask), "{ask}");
        }
        for ask in [
            "write me a poem about the sea",
            "what's the weather tomorrow",
            "check my steam library",
        ] {
            assert!(!connection_question(ask), "{ask}");
        }
    }

    #[test]
    fn the_note_rides_beside_the_route_note_and_leaves_the_words_alone() {
        use crate::client::{ContentPart, InputItem};
        let ask = "can you give me a list of all the ai models you have access too";
        let mut input = vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(ask.into())],
        }];
        let report = report(
            &Check {
                grok: Credential::None,
                providers: Vec::new(),
                live: true,
            },
            false,
        );
        // Both go in one write, as the call path sends them; a later route
        // note (a fallback's correction) replaces only its own line.
        let both = format!("[Route] the local model\n{}", note(&report));
        super::super::ask::add_route_note(&mut input, &both);
        super::super::ask::add_route_note(&mut input, "[Route] the fallback");
        let InputItem::Message { content, .. } = &input[0] else {
            panic!("a message");
        };
        let ContentPart::InputText(text) = &content[0] else {
            panic!("text");
        };
        assert!(text.contains("[Route] the fallback"), "{text}");
        assert!(!text.contains("[Route] the local model"), "{text}");
        assert!(text.contains("[Connections] The user asked"), "{text}");
        assert!(text.contains("Grok (xAI): not signed in."), "{text}");
        assert_eq!(crate::prefix_cache::visible_user_text(text), ask);
    }

    #[test]
    fn the_test_reply_goes_to_the_cheapest_model_of_that_provider() {
        use grokhub_core::model_registry::{Listing, ModelMeta, SourceKind};
        let mut reg = Registry::default();
        let meta = |id: &str, prompt: u64| {
            let mut m = ModelMeta {
                id: id.into(),
                ..ModelMeta::default()
            };
            m.prices.prompt = Some(prompt);
            m
        };
        reg.apply_refresh(
            &[Listing::new(
                SourceKind::Anthropic,
                vec![
                    meta("anthropic/claude-opus-5.5", 15),
                    meta("anthropic/claude-haiku-5.5", 1),
                ],
            )],
            &[],
            1,
        );
        assert_eq!(
            test_model(&reg, "anthropic").as_deref(),
            Some("anthropic/claude-haiku-5.5")
        );
        assert_eq!(test_model(&reg, "openrouter"), None);
    }
}
