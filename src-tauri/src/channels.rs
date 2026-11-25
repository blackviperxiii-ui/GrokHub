//! Connectors → Phone and chat apps (messaging channels, part 6): connect a
//! Telegram or Discord bot, approve the pairing codes strangers were sent,
//! unpair senders, and add or remove signed webhooks. Each connected bot has one background worker; its
//! token stays in the OS keychain, and what paired senders write is queued
//! in `channels/inbox/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::channels_net::{
    self, DiscordLink, TelegramLink, POLL_WAIT_S, TELEGRAM_TOKEN_REFUSED,
};
use grokhub_agent::harness::DataClass;
use grokhub_agent::keyring_blob::Backend;
use grokhub_core::msg_channels::{self, ChannelKind, DmTrust};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub kind: ChannelKind,
    /// A token is saved.
    pub connected: bool,
    /// Its worker is listening now.
    pub running: bool,
    /// Why it stopped or what failed last.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    pub channel: ChannelKind,
    pub code: String,
    /// `@username` or the name the sender shows.
    pub label: String,
    pub expires_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedView {
    pub channel: ChannelKind,
    pub sender: String,
    /// The user turned approval asks on for this sender.
    pub asks: bool,
    /// They have written to the bot, so asks have a chat to go to.
    pub reachable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsView {
    pub channels: Vec<ChannelView>,
    pub pending: Vec<PendingView>,
    pub paired: Vec<PairedView>,
    pub webhooks: Vec<WebhookView>,
    /// The newest answered messages and webhooks.
    pub recent: Vec<RecentView>,
    /// A webhook's secret, only in the answer that made it.
    pub new_secret: Option<String>,
    /// What the last step did, for the page's status line.
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentView {
    /// "Telegram", "Discord" or "Webhook".
    pub source: String,
    /// The sender's label or the webhook's name.
    pub from: String,
    /// What they wrote; empty for a webhook.
    pub text: String,
    pub answer: String,
    pub problem: Option<String>,
    pub done_ms: u64,
}

/// How many answered records the page lists.
const RECENT_SHOWN: usize = 10;

fn recent(config_dir: &Path) -> Vec<RecentView> {
    crate::channel_inbox::recent(config_dir, RECENT_SHOWN)
        .into_iter()
        .map(|r| {
            let field = |k: &str| {
                r.get(k)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let source = match field("channel").as_str() {
                "telegram" => "Telegram",
                "discord" => "Discord",
                "webhook" => "Webhook",
                _ => "Unknown",
            };
            RecentView {
                source: source.into(),
                from: if source == "Webhook" {
                    field("hook")
                } else {
                    field("label")
                },
                text: field("text"),
                answer: field("answer").trim().to_string(),
                problem: r
                    .get("problem")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                done_ms: r.get("doneMs").and_then(|v| v.as_u64()).unwrap_or(0),
            }
        })
        .collect()
}

/// Send an answer back to a paired chat with the bot's saved token, only
/// while the sender is still paired. The text is chat the assistant wrote.
pub fn send_answer(
    config_dir: &Path,
    store: &dyn Backend,
    to: &crate::channel_inbox::ReplyTo,
    text: &str,
) -> Result<(), String> {
    if !DmTrust::load(config_dir).is_paired(to.channel, &to.sender) {
        return Err("The sender was unpaired, so the answer wasn't sent.".into());
    }
    let token = channels_net::load_token(store, to.channel)?
        .ok_or_else(|| format!("{} isn't connected any more.", name(to.channel)))?;
    match to.channel {
        ChannelKind::Telegram => {
            let chat: i64 = to
                .to
                .parse()
                .map_err(|_| "That isn't a Telegram chat id.".to_string())?;
            TelegramLink::new(&token)?.send(config_dir, chat, text, &[DataClass::Chat])
        }
        ChannelKind::Discord => {
            DiscordLink::new(&token)?.send(config_dir, &to.to, text, &[DataClass::Chat])
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookView {
    pub name: String,
    pub enabled: bool,
    pub filters: usize,
    /// Where the sender posts, on the hub this computer shares.
    pub path: String,
}

const KINDS: [ChannelKind; 2] = [ChannelKind::Telegram, ChannelKind::Discord];

fn name(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Telegram => "Telegram",
        ChannelKind::Discord => "Discord",
    }
}

struct Worker {
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    problem: Arc<Mutex<Option<String>>>,
}

static WORKERS: Mutex<BTreeMap<ChannelKind, Worker>> = Mutex::new(BTreeMap::new());

fn workers() -> std::sync::MutexGuard<'static, BTreeMap<ChannelKind, Worker>> {
    WORKERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The page as it is now. `store` says which bots have a token.
pub fn view(config_dir: &Path, store: &dyn Backend, status: Option<String>) -> ChannelsView {
    let running = workers();
    let channels = KINDS
        .iter()
        .map(|&kind| {
            let worker = running.get(&kind);
            ChannelView {
                kind,
                connected: matches!(channels_net::load_token(store, kind), Ok(Some(_))),
                running: worker.is_some_and(|w| w.running.load(Ordering::SeqCst)),
                problem: worker
                    .and_then(|w| w.problem.lock().unwrap_or_else(|e| e.into_inner()).clone()),
            }
        })
        .collect();
    drop(running);
    let trust = DmTrust::load(config_dir);
    let now = grokhub_core::now_ms();
    let pending = trust
        .pending
        .iter()
        .filter(|p| p.expires_ms > now)
        .map(|p| PendingView {
            channel: p.channel,
            code: p.code.clone(),
            label: p.label.clone(),
            expires_ms: p.expires_ms,
        })
        .collect();
    let paired = trust
        .paired
        .iter()
        .flat_map(|(&channel, senders)| {
            senders.iter().map(move |sender| PairedView {
                channel,
                sender: sender.clone(),
                asks: crate::channel_asks::is_approver(config_dir, channel, sender),
                reachable: crate::channel_asks::reachable(config_dir, channel, sender),
            })
        })
        .collect();
    let webhooks = msg_channels::load_webhooks(config_dir)
        .into_iter()
        .map(|h| WebhookView {
            path: format!("/v1/hook/{}", h.name),
            name: h.name,
            enabled: h.enabled,
            filters: h.filters.len(),
        })
        .collect();
    ChannelsView {
        channels,
        pending,
        paired,
        webhooks,
        recent: recent(config_dir),
        new_secret: None,
        status,
    }
}

/// A webhook with a fresh secret, shown this once.
pub fn add_webhook(
    config_dir: &Path,
    store: &dyn Backend,
    name: &str,
) -> Result<ChannelsView, String> {
    let hook = msg_channels::add_webhook(config_dir, name)?;
    let mut next = view(
        config_dir,
        store,
        Some(format!(
            "Added {}. Copy the secret now: it isn't shown again.",
            hook.name
        )),
    );
    next.new_secret = Some(hook.secret);
    Ok(next)
}

pub fn remove_webhook(
    config_dir: &Path,
    store: &dyn Backend,
    name: &str,
) -> Result<ChannelsView, String> {
    if !msg_channels::remove_webhook(config_dir, name)? {
        return Err(format!("There's no webhook named {name}."));
    }
    Ok(view(
        config_dir,
        store,
        Some(format!("Removed {name}. Its secret no longer works.")),
    ))
}

/// Waits `total` in short steps, so a stop is noticed soon.
fn nap(stop: &AtomicBool, total: Duration) {
    let until = std::time::Instant::now() + total;
    while !stop.load(Ordering::SeqCst) && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Telegram's loop: long polls back to back, waiting longer after each
/// failure; a refused token ends it.
fn telegram_loop(
    config_dir: &Path,
    mut link: TelegramLink,
    stop: &AtomicBool,
    problem: &Mutex<Option<String>>,
) {
    let mut backoff = Duration::from_secs(1);
    while !stop.load(Ordering::SeqCst) {
        match link.poll(config_dir, POLL_WAIT_S, grokhub_core::now_ms()) {
            Ok(polled) => {
                *problem.lock().unwrap_or_else(|e| e.into_inner()) = polled.unsent.last().cloned();
                backoff = Duration::from_secs(1);
            }
            Err(why) => {
                let fatal = why == TELEGRAM_TOKEN_REFUSED;
                *problem.lock().unwrap_or_else(|e| e.into_inner()) = Some(why);
                if fatal {
                    return;
                }
                nap(stop, backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

/// Start `kind`'s worker with `token`, after any running one stops.
fn start(config_dir: &Path, kind: ChannelKind, token: &str) -> Result<(), String> {
    stop(kind);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(true));
    let problem = Arc::new(Mutex::new(None));
    let dir: PathBuf = config_dir.to_path_buf();
    let (s, r, p) = (stop_flag.clone(), running.clone(), problem.clone());
    match kind {
        ChannelKind::Telegram => {
            let link = TelegramLink::new(token)?;
            std::thread::Builder::new()
                .name("channel-telegram".into())
                .spawn(move || {
                    telegram_loop(&dir, link, &s, &p);
                    r.store(false, Ordering::SeqCst);
                })
                .map_err(|e| e.to_string())?;
        }
        ChannelKind::Discord => {
            let mut link = DiscordLink::new(token)?;
            std::thread::Builder::new()
                .name("channel-discord".into())
                .spawn(move || {
                    if let Err(why) = link.run(&dir, &|| s.load(Ordering::SeqCst)) {
                        *p.lock().unwrap_or_else(|e| e.into_inner()) = Some(why);
                    }
                    r.store(false, Ordering::SeqCst);
                })
                .map_err(|e| e.to_string())?;
        }
    }
    workers().insert(
        kind,
        Worker {
            stop: stop_flag,
            running,
            problem,
        },
    );
    Ok(())
}

/// Ask `kind`'s worker to stop. A long poll in flight ends on its own.
fn stop(kind: ChannelKind) {
    if let Some(worker) = workers().remove(&kind) {
        worker.stop.store(true, Ordering::SeqCst);
    }
}

/// A quit step: every worker is asked to stop.
pub fn stop_all() {
    for kind in KINDS {
        stop(kind);
    }
}

/// At launch: start a worker for each bot with a saved token.
pub fn start_saved(config_dir: &Path, store: &dyn Backend) {
    for kind in KINDS {
        if let Ok(Some(token)) = channels_net::load_token(store, kind) {
            let _ = start(config_dir, kind, &token);
        }
    }
}

/// Check `token` with the service, save it, and start listening.
pub fn connect(
    config_dir: &Path,
    store: &dyn Backend,
    kind: ChannelKind,
    token: &str,
) -> Result<ChannelsView, String> {
    let token = token.trim();
    let bot = match kind {
        ChannelKind::Telegram => TelegramLink::new(token)?.check(config_dir)?,
        ChannelKind::Discord => DiscordLink::new(token)?.check(config_dir)?,
    };
    channels_net::save_token(store, kind, token)?;
    start(config_dir, kind, token)?;
    Ok(view(
        config_dir,
        store,
        Some(format!(
            "Connected {} as {bot}. Strangers who message it get a pairing code.",
            name(kind)
        )),
    ))
}

/// Stop listening and forget the token. Paired senders stay paired.
pub fn disconnect(
    config_dir: &Path,
    store: &dyn Backend,
    kind: ChannelKind,
) -> Result<ChannelsView, String> {
    stop(kind);
    channels_net::forget_token(store, kind)?;
    Ok(view(
        config_dir,
        store,
        Some(format!("Disconnected {}.", name(kind))),
    ))
}

/// Approve the code a stranger was sent: from now on their messages count.
pub fn approve(config_dir: &Path, store: &dyn Backend, code: &str) -> Result<ChannelsView, String> {
    let mut trust = DmTrust::load(config_dir);
    let approved = trust.approve(code, grokhub_core::now_ms());
    // A wrong code still counts toward the limit, so save either way.
    trust.save(config_dir)?;
    let pending = approved?;
    let who = if pending.label.is_empty() {
        pending.sender.clone()
    } else {
        pending.label.clone()
    };
    Ok(view(
        config_dir,
        store,
        Some(format!("Paired {who} on {}.", name(pending.channel))),
    ))
}

/// Unpair a sender: their next message gets a new pairing code.
pub fn unpair(
    config_dir: &Path,
    store: &dyn Backend,
    kind: ChannelKind,
    sender: &str,
) -> Result<ChannelsView, String> {
    let mut trust = DmTrust::load(config_dir);
    if !trust.revoke(kind, sender) {
        return Err("That sender isn't paired.".into());
    }
    trust.save(config_dir)?;
    crate::channel_asks::set_approver(config_dir, kind, sender, false)?;
    Ok(view(
        config_dir,
        store,
        Some(format!("Unpaired {sender} on {}.", name(kind))),
    ))
}

/// Turn approval asks on or off for one paired sender.
pub fn set_asks(
    config_dir: &Path,
    store: &dyn Backend,
    kind: ChannelKind,
    sender: &str,
    on: bool,
) -> Result<ChannelsView, String> {
    if on && !DmTrust::load(config_dir).is_paired(kind, sender) {
        return Err("That sender isn't paired.".into());
    }
    crate::channel_asks::set_approver(config_dir, kind, sender, on)?;
    let status = if !on {
        format!("{sender} on {} won't get approval asks.", name(kind))
    } else if crate::channel_asks::reachable(config_dir, kind, sender) {
        format!("Approval asks go to {sender} on {} now.", name(kind))
    } else {
        format!(
            "Approval asks for {sender} on {} start once they write to the bot.",
            name(kind)
        )
    };
    Ok(view(config_dir, store, Some(status)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MemoryStore(Mutex<BTreeMap<String, String>>);

    impl Backend for MemoryStore {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(account.into(), value.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn a_code_is_approved_listed_and_unpaired() {
        let dir =
            std::env::temp_dir().join(format!("grokhub-shell-channels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = MemoryStore::default();
        let now = grokhub_core::now_ms();
        let mut trust = DmTrust::default();
        trust.inbound(ChannelKind::Telegram, "777", "@user", now);
        trust.save(&dir).unwrap();
        let code = trust.pending[0].code.clone();

        let before = view(&dir, &store, None);
        assert_eq!(
            before.channels,
            vec![
                ChannelView {
                    kind: ChannelKind::Telegram,
                    connected: false,
                    running: false,
                    problem: None
                },
                ChannelView {
                    kind: ChannelKind::Discord,
                    connected: false,
                    running: false,
                    problem: None
                },
            ]
        );
        assert_eq!(
            before.pending,
            vec![PendingView {
                channel: ChannelKind::Telegram,
                code: code.clone(),
                label: "@user".into(),
                expires_ms: trust.pending[0].expires_ms,
            }]
        );

        assert_eq!(
            approve(&dir, &store, "WRONG-1").unwrap_err(),
            "That code doesn't match anyone waiting. Codes last an hour."
        );
        // The wrong try was saved, and a right one clears the count.
        assert_eq!(DmTrust::load(&dir).wrong, 1);
        let approved = approve(&dir, &store, &code).unwrap();
        assert_eq!(
            approved.status.as_deref(),
            Some("Paired @user on Telegram.")
        );
        assert_eq!(approved.pending, vec![]);
        assert_eq!(
            approved.paired,
            vec![PairedView {
                channel: ChannelKind::Telegram,
                sender: "777".into(),
                asks: false,
                reachable: false,
            }]
        );

        let on = set_asks(&dir, &store, ChannelKind::Telegram, "777", true).unwrap();
        assert!(on.paired[0].asks);
        assert_eq!(
            on.status.as_deref(),
            Some("Approval asks for 777 on Telegram start once they write to the bot.")
        );

        let unpaired = unpair(&dir, &store, ChannelKind::Telegram, "777").unwrap();
        assert_eq!(unpaired.paired, vec![]);
        assert!(!crate::channel_asks::is_approver(
            &dir,
            ChannelKind::Telegram,
            "777"
        ));
        assert_eq!(
            set_asks(&dir, &store, ChannelKind::Telegram, "777", true).unwrap_err(),
            "That sender isn't paired."
        );
        assert_eq!(
            unpaired.status.as_deref(),
            Some("Unpaired 777 on Telegram.")
        );
        assert_eq!(
            unpair(&dir, &store, ChannelKind::Telegram, "777").unwrap_err(),
            "That sender isn't paired."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_token_is_refused_before_anything_is_saved() {
        let dir =
            std::env::temp_dir().join(format!("grokhub-shell-channels-bad-{}", std::process::id()));
        let store = MemoryStore::default();
        assert_eq!(
            connect(&dir, &store, ChannelKind::Telegram, "nope").unwrap_err(),
            "That isn't a Telegram bot token."
        );
        assert_eq!(
            connect(&dir, &store, ChannelKind::Discord, "nope").unwrap_err(),
            "That isn't a Discord bot token."
        );
        assert!(store.0.lock().unwrap().is_empty());
        assert!(workers().is_empty());
    }

    #[test]
    fn a_webhook_secret_is_shown_once() {
        let dir = std::env::temp_dir().join(format!("grokhub-shell-hooks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = MemoryStore::default();
        let added = add_webhook(&dir, &store, "deploys").unwrap();
        let secret = added.new_secret.clone().unwrap();
        assert_eq!(secret.len(), 64);
        assert_eq!(
            added.webhooks,
            vec![WebhookView {
                name: "deploys".into(),
                enabled: true,
                filters: 0,
                path: "/v1/hook/deploys".into(),
            }]
        );
        assert_eq!(
            added.status.as_deref(),
            Some("Added deploys. Copy the secret now: it isn't shown again.")
        );
        let later = view(&dir, &store, None);
        assert!(later.new_secret.is_none());
        assert!(!serde_json::to_string(&later).unwrap().contains(&secret));
        let removed = remove_webhook(&dir, &store, "deploys").unwrap();
        assert_eq!(removed.webhooks, vec![]);
        assert_eq!(
            remove_webhook(&dir, &store, "deploys").unwrap_err(),
            "There's no webhook named deploys."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_answer_goes_only_to_a_still_paired_sender_with_a_saved_token() {
        let dir = std::env::temp_dir().join(format!("grokhub-shell-answer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = MemoryStore::default();
        let to = crate::channel_inbox::ReplyTo {
            channel: ChannelKind::Telegram,
            sender: "777".into(),
            to: "5001".into(),
        };
        assert_eq!(
            send_answer(&dir, &store, &to, "hi").unwrap_err(),
            "The sender was unpaired, so the answer wasn't sent."
        );
        let mut trust = DmTrust::default();
        trust
            .paired
            .entry(ChannelKind::Telegram)
            .or_default()
            .insert("777".into());
        trust.save(&dir).unwrap();
        assert_eq!(
            send_answer(&dir, &store, &to, "hi").unwrap_err(),
            "Telegram isn't connected any more."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn answered_records_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("grokhub-shell-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let done = crate::channel_inbox::done_dir(&dir);
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(
            done.join("1-telegram-a.json"),
            serde_json::json!({ "channel": "telegram", "label": "@user", "text": "hi", "answer": " Hello. ", "problem": null, "doneMs": 5 }).to_string(),
        )
        .unwrap();
        std::fs::write(
            done.join("2-deploys-b.json"),
            serde_json::json!({ "channel": "webhook", "hook": "deploys", "answer": "", "problem": "No model is set up.", "doneMs": 6 }).to_string(),
        )
        .unwrap();
        let store = MemoryStore::default();
        assert_eq!(
            view(&dir, &store, None).recent,
            vec![
                RecentView {
                    source: "Webhook".into(),
                    from: "deploys".into(),
                    text: String::new(),
                    answer: String::new(),
                    problem: Some("No model is set up.".into()),
                    done_ms: 6,
                },
                RecentView {
                    source: "Telegram".into(),
                    from: "@user".into(),
                    text: "hi".into(),
                    answer: "Hello.".into(),
                    problem: None,
                    done_ms: 5,
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
