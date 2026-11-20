//! Connectors → Phone and chat apps (messaging channels, part 6a): connect a
//! Telegram or Discord bot, approve the pairing codes strangers were sent,
//! and unpair senders. Each connected bot has one background worker; its
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
use grokhub_agent::keyring_blob::Backend;
use grokhub_core::msg_channels::{ChannelKind, DmTrust};
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsView {
    pub channels: Vec<ChannelView>,
    pub pending: Vec<PendingView>,
    pub paired: Vec<PairedView>,
    /// What the last step did, for the page's status line.
    pub status: Option<String>,
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
            })
        })
        .collect();
    ChannelsView {
        channels,
        pending,
        paired,
        status,
    }
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
    Ok(view(
        config_dir,
        store,
        Some(format!("Unpaired {sender} on {}.", name(kind))),
    ))
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
                sender: "777".into()
            }]
        );

        let unpaired = unpair(&dir, &store, ChannelKind::Telegram, "777").unwrap();
        assert_eq!(unpaired.paired, vec![]);
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
}
