//! Spike-4a trust floor in the cabin: `/privacy`, the `/sync` egress gate, and
//! the hub grant row under Settings → Permissions. The per-scope rows live in
//! `scope_ui.rs` (Spike-4b). The ledger and the send log are sealed at rest;
//! when the keyring can't open them, no grant applies and `/privacy` says so.
//!
//! A grant is written only here, from a Settings click (`UserClick::from_click`).
//! `/privacy` only reads. `/sync` asks `harness::decide` (`Step::Egress`) first:
//! no hub grant ⇒ a hard Send card (Approve sends once). D1: Grok Build's own
//! traffic is outside the cabin and is not watched here.

use super::*;
use grokhub_agent::harness::{self as hx, GateOutcome, Step};

/// Settings → Permissions headings (SY-06): the trust rows, then the rule editor.
pub(super) const LEAVING_HEAD: &str = "Leaving this computer";
pub(super) const RULES_HEAD: &str = "Command rules";
/// Under "Leaving this computer" in Settings → Permissions.
pub(super) const PRIVACY_NOTE: &str = "Nothing new leaves this computer without your OK. xAI model hosts stay allowed for chats and memory. Grok Build's own traffic is outside GrokHub. /privacy shows what left.";
pub(super) const HUB_ROW: &str = "Sync to paired computers";
/// The one user-facing name for the hub sync data (SY-02). Logs and files keep
/// the scope ids (`chat`, `personal`).
pub(super) const HUB_SCOPE_LABEL: &str = "chats, memory";
/// The same scope inside a sentence (SY-09): prose says "chats and memory";
/// compact spots (the card command, `/privacy` grant and egress lines, row
/// hints) keep [`HUB_SCOPE_LABEL`].
pub(super) const HUB_SCOPE_PROSE: &str = "chats and memory";
const HUB_OFF: &str = "Off. /sync asks each time. Sends chats, memory.";
/// The hard card for an ungranted `/sync` with at least one paired computer.
pub(super) const HUB_CARD_ACTION: &str = "/sync → paired computers (chats, memory)";
pub(super) const HUB_CARD_NOTE: &str =
    "Sends chats and memory to your paired computers. Approve sends once. Esc denies. Settings → Permissions can allow it every time.";
/// `/sync` with no paired computer: nothing is sent and nothing is logged (SY-03).
pub(super) const SYNC_NO_PEERS: &str = "Nothing paired yet. Start share to pair a computer.";
/// First line of the `/privacy` report; the chat pane finds the bubble by it.
pub(super) const PRIVACY_HEAD: &str = "/privacy — what leaves this computer";
/// What `/privacy` calls the hub destination (SY-03: name it plainly).
const HUB_DEST_LABEL: &str = "paired computers";
/// Days of `egress.jsonl` that `/privacy` sums.
const PRIVACY_DAYS: u64 = 7;

/// How a `/sync` was allowed: a standing grant, or one hard-card click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HubSend {
    Grant(String),
    Once,
}

impl HubSend {
    pub(super) fn consent_ref(&self) -> &str {
        match self {
            Self::Grant(id) => id,
            Self::Once => "approved-once",
        }
    }
}

/// One destination + basis in the `/privacy` egress summary.
struct EgressRow {
    dest: String,
    times: usize,
    data: Vec<hx::DataClass>,
    basis: String,
    last: u64,
}

/// The words a user reads for a data class. The ids stay in logs and files.
fn class_label(c: hx::DataClass) -> &'static str {
    c.label()
}

/// User words for a set of classes: the hub's pair reads "chats, memory".
pub(super) fn classes(data: &[hx::DataClass]) -> String {
    data.iter().map(|c| class_label(*c)).collect::<Vec<_>>().join(", ")
}

/// A destination as `/privacy` names it.
fn dest_label(dest: &str) -> &str {
    if dest.eq_ignore_ascii_case(hx::HUB_DEST) {
        HUB_DEST_LABEL
    } else {
        dest
    }
}

/// How `/privacy` names an active grant (no id: SY-08).
pub(super) fn grant_label(g: &hx::Grant) -> String {
    if g.destination == hx::HUB_DEST {
        HUB_ROW.to_string()
    } else if !g.destination.is_empty() {
        format!("Send to {}", g.destination)
    } else {
        super::scope_ui::scope_label(&g.source)
    }
}

/// `/sync` result line once the share has landed (SY-03).
pub(super) fn sync_result_line(peers: usize) -> String {
    match peers {
        0 => SYNC_NO_PEERS.to_string(),
        1 => format!("Synced {HUB_SCOPE_PROSE} to 1 computer."),
        n => format!("Synced {HUB_SCOPE_PROSE} to {n} computers."),
    }
}

/// The Settings button that asks the keyring again (SB-02).
pub(super) const TRY_AGAIN: &str = "Try again";

/// SB-01: the hover on every disabled pill while private data is locked.
pub(super) fn lock_hover(why: &hx::Locked) -> &'static str {
    match why {
        hx::Locked::Unavailable => "Locked: keyring unavailable",
        hx::Locked::Missing => "Locked: key missing from your keyring",
        hx::Locked::WrongKey => "Locked: your keyring holds a different key",
        hx::Locked::Busy => "Locked: another window is setting up the key",
        hx::Locked::Unwritable => "Locked: the config folder can't be written",
    }
}

/// SB-02: the one next step under a lock in Settings → Permissions, next to
/// Try again. Per OS: Windows names Credential Manager and macOS its
/// Keychain; only Linux talks about GNOME Keyring or KWallet.
pub(super) fn lock_next_step(why: &hx::Locked, os: hx::KeyringOs) -> String {
    let name = hx::keyring_name_for(os);
    match why {
        hx::Locked::Unavailable => match os {
            hx::KeyringOs::Linux => "Unlock or start your keyring (GNOME Keyring or KWallet), then Try again.".into(),
            hx::KeyringOs::Windows => {
                "Turn on Windows Credential Manager (Services → Credential Manager), then Try again.".into()
            }
            hx::KeyringOs::MacOs => "Unlock your macOS Keychain (Keychain Access → login), then Try again.".into(),
        },
        hx::Locked::Missing | hx::Locked::WrongKey => {
            format!("Put this data's GrokHub key back in your {name}, then Try again.")
        }
        hx::Locked::Busy => "Wait a moment for the other window, then Try again.".into(),
        hx::Locked::Unwritable => "Check that GrokHub's config folder can be written, then Try again.".into(),
    }
}

/// The same next step, short, for the `/sync` and `/privacy` lines.
pub(super) fn lock_next_step_short(why: &hx::Locked, os: hx::KeyringOs) -> String {
    let step = match why {
        hx::Locked::Unavailable => match os {
            hx::KeyringOs::Linux => "unlock or start GNOME Keyring or KWallet".to_string(),
            hx::KeyringOs::Windows => "turn on Windows Credential Manager".to_string(),
            hx::KeyringOs::MacOs => "unlock your macOS Keychain".to_string(),
        },
        hx::Locked::Missing | hx::Locked::WrongKey => {
            format!("put this data's GrokHub key back in your {}", hx::keyring_name_for(os))
        }
        hx::Locked::Busy => "wait a moment".to_string(),
        hx::Locked::Unwritable => "check that GrokHub's config folder can be written".to_string(),
    };
    format!("Next: {step}, then Try again in Settings → Permissions.")
}

/// `/privacy` text: grants, scopes (all off), and recent egress by destination.
/// No content, no secrets: the ledger and the log hold neither. `lock` is why
/// private data can't be opened or saved right now, if it can't (Spike-4b).
pub(super) fn privacy_report(
    ledger: &hx::ConsentLedger,
    log: &hx::EgressRead,
    lock: Option<&hx::Locked>,
    desktop_control: bool,
    now_ms: u64,
) -> String {
    let egress = &log.lines;
    let ago = |at: u64| grokhub_core::pulse::ago_label(at, now_ms);
    let mut out = vec![
        PRIVACY_HEAD.to_string(),
        String::new(),
        format!(
            "Allowed by default: {} ({HUB_SCOPE_PROSE} in model prompts). Sending {HUB_SCOPE_PROSE} anywhere else waits for your OK: a hard card, or a grant in Settings → Permissions.",
            grokhub_core::DEFAULT_CONNECTOR_HOSTS.join(", ")
        ),
        String::new(),
    ];
    if let Some(why) = ledger.locked().or(lock) {
        out.push(why.message());
        out.push(lock_next_step_short(why, hx::KeyringOs::current()));
        out.push(String::new());
    }
    // SB-05: one Grants list, each grant once. What is on gets a bullet with
    // its since-time (the bubble's Revoke rows sit under it); everything off
    // shares one line. The grant id stays out of the text (SY-08).
    out.push("Grants".to_string());
    let mut off: Vec<String> = Vec::new();
    if ledger.destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA).is_none() {
        off.push(format!("{HUB_ROW} (/sync asks each time)"));
    }
    for g in ledger.active().filter(|g| !g.destination.is_empty()) {
        out.push(format!(
            "- {}: on since {} · {}",
            grant_label(g),
            ago(g.granted_at),
            classes(&g.data_classes)
        ));
    }
    let read: Vec<&hx::Grant> = ledger.active().filter(|g| g.destination.is_empty()).collect();
    for g in &read {
        out.push(format!("- {}: on since {}", grant_label(g), ago(g.granted_at)));
    }
    for (kind, label) in hx::SCOPE_KINDS {
        if !read.iter().any(|g| g.source == *kind || g.source.starts_with(&format!("{kind}:"))) {
            off.push((*label).to_string());
        }
    }
    if !off.is_empty() {
        out.push(format!("- Off: {}", off.join(" · ")));
    }
    out.push(format!(
        "- Screen: \"Let Grok control the desktop\" in Settings → Cabin defaults ({})",
        if desktop_control { "on" } else { "off" }
    ));
    if ledger.unreadable() > 0 {
        out.push(format!("- {} ledger lines didn't open (damaged or edited) and count for nothing.", ledger.unreadable()));
    }
    out.push("Nothing reads the folder, app, browser, calendar, mail or system grants yet.".to_string());
    out.push(String::new());
    out.push(format!("Sent in the last {PRIVACY_DAYS} days (no content stored)"));
    let since = now_ms.saturating_sub(PRIVACY_DAYS * 86_400_000);
    let mut rows: Vec<EgressRow> = Vec::new();
    for line in egress.iter().filter(|l| l.ts_ms >= since) {
        let basis = match line.basis.as_str() {
            "approved_once" => "approved once",
            "grant" => "your grant",
            "model_host" => "default",
            "chat" => "allowed",
            other => other,
        };
        match rows.iter_mut().find(|r| r.dest == line.dest && r.basis == basis) {
            Some(r) => {
                r.times += 1;
                for c in &line.data_classes {
                    if !r.data.contains(c) {
                        r.data.push(*c);
                    }
                }
                r.data.sort();
                r.last = r.last.max(line.ts_ms);
            }
            None => rows.push(EgressRow {
                dest: line.dest.clone(),
                times: 1,
                data: line.data_classes.clone(),
                basis: basis.to_string(),
                last: line.ts_ms,
            }),
        }
    }
    if let Some(why) = &log.locked {
        out.push(format!("- The send log is locked ({}). Nothing new is logged, and grant sends wait, until it opens.", why.short()));
    } else if rows.is_empty() {
        out.push("- Nothing logged yet.".into());
    }
    if log.unreadable > 0 {
        out.push(format!("- {} log lines didn't open (damaged or edited) and were skipped.", log.unreadable));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.last));
    for r in rows {
        let times = if r.times == 1 { "1 time".to_string() } else { format!("{} times", r.times) };
        let data = if r.data.is_empty() { "no user data".to_string() } else { classes(&r.data) };
        out.push(format!("- {} · {times} · {data} · {} · last {}", dest_label(&r.dest), r.basis, ago(r.last)));
    }
    out.push(String::new());
    out.push("Not watched here: Grok Build's own traffic (its model calls, connectors, and web tools) and the commands an Update runs. Everything else GrokHub sends is logged above: model calls, Imagine, Labs web fetch, MCP servers, Pulse previews, update checks, and sign-in.".into());
    out.join("\n")
}

/// How long Settings reuses a lock answer before asking again.
const LOCK_RECHECK: std::time::Duration = std::time::Duration::from_secs(1);

/// Why private data can't be opened or saved here, if it can't: the ledger's
/// own lock, else the keyring's answer (a fresh config with no keyring can't
/// save a grant either). `wait = false` never blocks (UI thread).
pub(super) fn private_lock(dir: &std::path::Path, ledger: &hx::ConsentLedger, wait: bool) -> Option<hx::Locked> {
    lock_answer(dir, ledger, wait).flatten()
}

/// [`private_lock`], but `None` while the keyring hasn't answered yet (only
/// with `wait = false` on a keyring that can block).
fn lock_answer(dir: &std::path::Path, ledger: &hx::ConsentLedger, wait: bool) -> Option<Option<hx::Locked>> {
    if let Some(why) = ledger.locked() {
        return Some(Some(why.clone()));
    }
    match hx::read_key(dir, wait)? {
        Err(why) => Some(Some(why)),
        Ok(_) => Some(None),
    }
}

impl Cabin {
    /// The consent ledger, read once and kept current by the clicks below.
    /// Never waits on the keyring (UI thread): while it hasn't answered, or
    /// while private data is locked, it is read again on the next call.
    pub(super) fn consent(&mut self) -> &hx::ConsentLedger {
        let now = std::time::Instant::now();
        let stale = match self.harness.consent.as_ref() {
            None => true,
            Some(c) if c.pending() => true,
            Some(c) => {
                c.locked().is_some()
                    && self.harness.consent_at.is_none_or(|at| now.duration_since(at) >= LOCK_RECHECK)
            }
        };
        if stale {
            self.harness.consent = Some(hx::ConsentLedger::load_now(&crate::config::config_dir()));
            self.harness.consent_at = Some(now);
        }
        self.harness.consent.get_or_insert_with(hx::ConsentLedger::empty)
    }

    /// [`private_lock`] for painting Settings: never waits, and asks the
    /// keyring (and scans for sealed files) at most once per `LOCK_RECHECK`.
    pub(super) fn private_lock_for_paint(&mut self) -> Option<hx::Locked> {
        if let Some(why) = self.consent().locked() {
            return Some(why.clone());
        }
        if let Some((at, why)) = &self.harness.lock_seen {
            if at.elapsed() < LOCK_RECHECK {
                return why.clone();
            }
        }
        let ledger = self.consent().clone();
        let why = match lock_answer(&crate::config::config_dir(), &ledger, false) {
            Some(why) => {
                self.harness.lock_pending = false;
                why
            }
            // Not answered yet (Try again, or a slow keyring): keep the last
            // lock on screen until it has, so nothing unlocks early.
            None => {
                self.harness.lock_pending = true;
                self.harness.lock_seen.as_ref().and_then(|(_, w)| w.clone())
            }
        };
        if !self.harness.lock_pending {
            self.harness.lock_seen = Some((std::time::Instant::now(), why.clone()));
        }
        why
    }

    /// Settings → Permissions → Try again (SB-02): drop the cached keyring
    /// answer and read the ledger again, so the next paint asks the keyring
    /// now instead of up to 30 s later. It never grants anything; a lock
    /// stays until the keyring answers with the right key.
    pub(super) fn retry_keyring(&mut self) {
        hx::recheck_keyring(&crate::config::config_dir());
        self.harness.consent = None;
        self.harness.consent_at = None;
        if let Some((at, _)) = self.harness.lock_seen.as_mut() {
            *at = std::time::Instant::now().checked_sub(LOCK_RECHECK).unwrap_or(*at);
        }
        self.harness.scope_note = None;
        self.harness.lock_retry_at = Some(now_ms());
        self.status = "Checking your keyring…".into();
    }

    /// Paired computers the hub would serve a share to.
    pub(super) fn hub_peer_count(&self) -> usize {
        self.hub.lock().map(|st| st.peers.len()).unwrap_or(0)
    }

    /// One `/sync` result line in the chat, kept out of the next model kick.
    pub(super) fn post_sync_result(&mut self, line: &str) {
        self.status = line.to_string();
        self.live_mut().push(("assistant".into(), mark_slash_result(line)));
        self.stamp_current_access();
        self.persist();
    }

    /// `/sync` publishes chats and memory to paired computers, so it asks the
    /// EgressGuard first. No paired computer ⇒ nothing to send: one result
    /// line, no card, no egress line (SY-03). Granted ⇒ sync; not granted ⇒
    /// hard Send card.
    pub(super) fn gate_hub_sync(&mut self) {
        if self.hub_peer_count() == 0 {
            self.post_sync_result(SYNC_NO_PEERS);
            return;
        }
        let ledger = self.consent().clone();
        // Spike-4b: a send that can't be logged doesn't go, so a locked or
        // still-opening ledger stops here with one plain line.
        if ledger.pending() {
            self.status = "Sync waits for your keyring. Try /sync again in a moment.".into();
            return;
        }
        if let Some(why) = private_lock(&crate::config::config_dir(), &ledger, false) {
            let next = lock_next_step_short(&why, hx::KeyringOs::current());
            self.post_sync_result(&format!("Not synced. {} {next}", why.message()));
            return;
        }
        let step = Step::Egress { dest: hx::HUB_DEST, data: hx::HUB_SYNC_DATA, ledger: &ledger };
        match hx::decide(step) {
            GateOutcome::Allow => {
                let id = ledger
                    .destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA)
                    .map(|g| g.id.clone())
                    .unwrap_or_default();
                self.run_hub_sync(HubSend::Grant(id));
            }
            GateOutcome::Park { hard: Some(class), .. } => self.park_egress(hx::HUB_DEST, class),
            GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } => {
                self.status = format!("Sync not sent: {reason}");
            }
        }
    }

    /// `/privacy`: read the ledger and the log tail off the UI thread.
    pub(super) fn run_privacy(&mut self) {
        if self.harness.privacy_rx.is_some() {
            return;
        }
        let dir = crate::config::config_dir();
        let desktop = self.cfg.desktop_control;
        let (tx, rx) = mpsc::channel();
        self.harness.privacy_rx = Some(rx);
        std::thread::spawn(move || {
            let ledger = hx::ConsentLedger::load(&dir);
            let egress = hx::read_egress_report(&dir);
            let lock = private_lock(&dir, &ledger, true);
            let _ = tx.send(privacy_report(&ledger, &egress, lock.as_ref(), desktop, now_ms()));
        });
    }

    pub(super) fn poll_privacy(&mut self) {
        let Some(rx) = self.harness.privacy_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(body) => {
                self.live_mut().push(("assistant".into(), mark_slash_result(&body)));
                self.stamp_current_access();
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.privacy_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    /// Settings → Permissions, "Leaving this computer": the trust-floor note
    /// and the hub grant row. The Allow click is the only place a grant is
    /// written (rule 4). Revoke is a ghost: it only takes access away (SY-05).
    pub(super) fn ui_privacy_rows(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_heading(ui, LEAVING_HEAD);
        crate::cards::settings_note(ui, PRIVACY_NOTE);
        // Spike-4b: said once, at the top, because it holds every row below.
        // SB-02: with one next step for this OS and a Try again that asks the
        // keyring again.
        let ledger = self.consent().clone();
        let lock = self.private_lock_for_paint();
        let checking = ledger.pending() || self.harness.lock_pending;
        if checking {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        }
        if let Some(why) = &lock {
            ui.label(RichText::new(why.message()).size(13.0).color(crate::theme::fg()));
            ui.add_space(4.0);
            let mut retry = false;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(lock_next_step(why, hx::KeyringOs::current()))
                        .size(13.0)
                        .color(crate::theme::fg()),
                );
                ui.add_enabled_ui(!checking, |ui| retry = crate::cards::ghost_pill(ui, TRY_AGAIN));
            });
            if checking {
                crate::cards::settings_note(ui, "Checking your keyring…");
            } else if let Some(at) = self.harness.lock_retry_at {
                let ago = grokhub_core::pulse::ago_label(at, now_ms());
                crate::cards::settings_note(ui, &format!("Still locked. Checked {ago}."));
            } else {
                ui.add_space(8.0);
            }
            if retry {
                self.retry_keyring();
            }
        } else if checking {
            crate::cards::settings_note(ui, "Checking your keyring…");
        } else if self.harness.lock_retry_at.take().is_some() {
            self.status = "Your keyring answered. Private data is open again.".into();
        }
        let locked = lock.as_ref().map(lock_hover);
        let granted = self
            .consent()
            .destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA)
            .map(|g| (g.id.clone(), g.granted_at));
        let dir = crate::config::config_dir();
        match granted {
            None => {
                let allow = crate::cards::GrantPill::Allow;
                if crate::cards::settings_grant_row(ui, HUB_ROW, HUB_OFF, None, allow, locked, false, |_| {}) && locked.is_none() {
                    match hx::grant_destination(&dir, hx::HUB_DEST, hx::HUB_SYNC_DATA, hx::UserClick::from_click()) {
                        Ok(_) => self.status = "Sync to paired computers allowed. Revoke it here any time.".into(),
                        Err(e) => self.status = format!("Could not save the grant: {e}"),
                    }
                    self.harness.consent = None;
                }
            }
            Some((id, at)) => {
                let hint = format!(
                    "On since {}. Sends {HUB_SCOPE_LABEL}.",
                    grokhub_core::pulse::ago_label(at, now_ms())
                );
                let revoke = crate::cards::GrantPill::Revoke;
                if crate::cards::settings_grant_row(ui, HUB_ROW, &hint, None, revoke, locked, false, |_| {}) && locked.is_none() {
                    self.revoke_hub_grant(&id);
                }
            }
        }
        ui.add_space(8.0);
    }

    /// Active grants as the `/privacy` bubble lists them. The id only rides
    /// along for the click; it is never painted (SY-08).
    pub(super) fn privacy_revoke_rows(&mut self) -> Vec<PrivacyRevoke> {
        self.consent()
            .active()
            .map(|g| PrivacyRevoke {
                id: g.id.clone(),
                label: grant_label(g),
                detail: super::scope_ui::scope_detail(&g.source).filter(|_| g.destination.is_empty()).map(str::to_string),
            })
            .collect()
    }

    /// Revoke from the `/privacy` bubble (SY-04). Only `paint_privacy_revokes`
    /// returns an id, and only for a pointer click: no slash, chip, or model
    /// text reaches this. It only takes access away; granting stays in Settings.
    pub(super) fn revoke_from_privacy(&mut self, id: &str) {
        let hub = self
            .consent()
            .active()
            .find(|g| g.id == id)
            .map(|g| g.destination == hx::HUB_DEST);
        match hub {
            Some(true) => self.revoke_hub_grant(id),
            Some(false) => self.revoke_other_grant(id),
            None => return,
        }
        let line = self.status.clone();
        self.live_mut().push(("assistant".into(), mark_slash_result(&line)));
        self.stamp_current_access();
        self.persist();
    }

    /// Revoke stops sharing: the hub also drops the snapshot it was serving.
    pub(super) fn revoke_hub_grant(&mut self, id: &str) {
        match hx::revoke_grant(&crate::config::config_dir(), id) {
            Ok(_) => {
                if let Ok(mut st) = self.hub.lock() {
                    st.snapshot = None;
                }
                self.persist_hub();
                self.status = "Sync to paired computers revoked. /sync asks again.".into();
            }
            Err(e) => self.status = format!("Could not revoke: {e}"),
        }
        self.harness.consent = None;
    }
}

/// One Revoke row under the `/privacy` bubble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PrivacyRevoke {
    /// The grant id: for the click only, never painted (SY-08).
    pub id: String,
    /// The same short label the report uses ("Files in notes").
    pub label: String,
    /// A folder grant's whole path, shown on hover (SB-03).
    pub detail: Option<String>,
}

/// Height of one Revoke row: the pill's own height.
const REVOKE_ROW_H: f32 = 28.0;

/// Under the newest `/privacy` bubble: one row per active grant with a ghost
/// Revoke. Returns the grant id whose Revoke was clicked. Pointer clicks only;
/// Enter or any typed text never answers it. The rows start at the bubble's
/// text inset (SB-11); `locked` disables every Revoke, with why on hover
/// (SB-01: a revoke is a ledger write, and a locked ledger takes none).
pub(super) fn paint_privacy_revokes(ui: &mut egui::Ui, grants: &[PrivacyRevoke], locked: Option<&str>) -> Option<String> {
    let mut hit = None;
    // The pills share one column: every label takes the widest label's room.
    let labels: Vec<&str> = grants.iter().map(|g| g.label.as_str()).collect();
    let pads = super::chat_ui::result_row_label_pads(ui, &labels);
    for (g, pad) in grants.iter().zip(pads) {
        ui.push_id(("privacy-revoke", g.id.as_str()), |ui| {
            // One pill-high row so the label centers on the Revoke pill.
            let row = egui::vec2(ui.available_width(), REVOKE_ROW_H);
            ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(super::chat_ui::RESULT_TEXT_INSET);
                let r = ui.label(
                    RichText::new(&g.label).size(super::chat_ui::RESULT_ROW_LABEL_SIZE).color(crate::theme::muted()),
                );
                if let Some(full) = &g.detail {
                    r.on_hover_text(full);
                }
                ui.add_space(pad);
                let clicked = ui
                    .add_enabled_ui(locked.is_none(), |ui| {
                        let resp = crate::theme::felt_label_button(
                            ui,
                            "Revoke",
                            egui::Color32::TRANSPARENT,
                            crate::theme::muted(),
                            8.0,
                            egui::vec2(0.0, REVOKE_ROW_H),
                            Some(egui::Stroke::new(1.0_f32, crate::theme::border())),
                            false,
                        );
                        let resp = match locked {
                            Some(why) => resp.on_disabled_hover_text(why),
                            None => resp,
                        };
                        resp.clicked()
                    })
                    .inner;
                if clicked && locked.is_none() {
                    hit = Some(g.id.clone());
                }
            });
        });
    }
    if !grants.is_empty() {
        ui.add_space(4.0);
    }
    hit
}

/// The newest `/privacy` report among the painted rows, if any.
pub(super) fn newest_privacy_row(views: &[grokhub_core::ChatView]) -> Option<usize> {
    views
        .iter()
        .rposition(|v| v.kind == grokhub_core::ChatKind::Result && v.body.starts_with(PRIVACY_HEAD))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report for a plain list of log lines and no lock.
    fn report(ledger: &hx::ConsentLedger, lines: &[hx::EgressLine], desktop: bool, now: u64) -> String {
        let log = hx::EgressRead { lines: lines.to_vec(), ..hx::EgressRead::default() };
        privacy_report(ledger, &log, None, desktop, now)
    }

    fn line(dest: &str, basis: &str, at: u64, data: &[hx::DataClass]) -> hx::EgressLine {
        hx::EgressLine {
            ts_ms: at,
            dest: dest.into(),
            data_classes: data.to_vec(),
            node_ids: vec![],
            redactions: 0,
            grant_id: String::new(),
            span_id: String::new(),
            basis: basis.into(),
            origin: hx::Origin::User,
        }
    }

    /// Rule 4: the agent never widens its own permissions. Outside tests, a
    /// grant is built only from a Settings click: the hub row in this file and
    /// the scope rows in `scope_ui.rs`.
    #[test]
    fn only_a_settings_click_writes_a_grant() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates dir")
            .to_path_buf();
        let mut hits = Vec::new();
        let mut stack = vec![crates.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if path.is_dir() {
                    if name != "target" && name != "tests" {
                        stack.push(path);
                    }
                    continue;
                }
                if !name.ends_with(".rs") || name == "tests.rs" || name.ends_with("_tests.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let live = text.split("#[cfg(test)]").next().unwrap_or("");
                for needle in ["UserClick::from_click(", "grant_destination(", "grant_scope("] {
                    for _ in live.matches(needle) {
                        let rel = path.strip_prefix(&crates).unwrap_or(&path);
                        let rel = rel.display().to_string().replace('\\', "/");
                        hits.push(format!("{rel}: {needle}"));
                    }
                }
            }
        }
        hits.sort();
        assert_eq!(
            hits,
            vec![
                "grokhub-agent/src/harness/consent.rs: grant_destination(",
                "grokhub-agent/src/harness/consent.rs: grant_scope(",
                "grokhub-app/src/app/privacy_ui.rs: UserClick::from_click(",
                "grokhub-app/src/app/privacy_ui.rs: grant_destination(",
                "grokhub-app/src/app/scope_ui.rs: UserClick::from_click(",
                "grokhub-app/src/app/scope_ui.rs: grant_scope(",
            ]
        );
    }

    #[test]
    fn privacy_report_on_a_fresh_config() {
        let now = 1_000 * 86_400_000;
        let got = report(&hx::ConsentLedger::empty(), &[], false, now);
        assert_eq!(
            got,
            [
                "/privacy — what leaves this computer",
                "",
                "Allowed by default: grok.com, x.ai, api.x.ai (chats and memory in model prompts). Sending chats and memory anywhere else waits for your OK: a hard card, or a grant in Settings → Permissions.",
                "",
                "Grants",
                "- Off: Sync to paired computers (/sync asks each time) · Files in a folder · Installed apps · Browser history · Calendar · Mail · System state",
                "- Screen: \"Let Grok control the desktop\" in Settings → Cabin defaults (off)",
                "Nothing reads the folder, app, browser, calendar, mail or system grants yet.",
                "",
                "Sent in the last 7 days (no content stored)",
                "- Nothing logged yet.",
                "",
                "Not watched here: Grok Build's own traffic (its model calls, connectors, and web tools) and the commands an Update runs. Everything else GrokHub sends is logged above: model calls, Imagine, Labs web fetch, MCP servers, Pulse previews, update checks, and sign-in.",
            ]
            .join("\n")
        );
    }

    #[test]
    fn privacy_report_groups_egress_by_destination() {
        let now = 1_000 * 86_400_000;
        let chat = [hx::DataClass::Chat, hx::DataClass::Personal];
        let log = vec![
            line("api.x.ai", "model_host", now - 3_600_000, &chat),
            line("api.x.ai", "model_host", now - 120_000, &[hx::DataClass::Chat]),
            line("hub", "approved_once", now - 7_200_000, &chat),
            line("api.x.ai", "model_host", now - 8 * 86_400_000, &chat),
        ];
        let got = report(&hx::ConsentLedger::empty(), &log, true, now);
        assert!(got.contains("- api.x.ai · 2 times · chats, memory · default · last 2m ago\n- paired computers · 1 time · chats, memory · approved once · last 2h ago\n"), "{got}");
        assert!(!got.contains("hub ·"), "the hub is named plainly: {got}");
        assert!(got.contains("- Screen: \"Let Grok control the desktop\" in Settings → Cabin defaults (on)"), "{got}");
        assert!(!got.contains("3 times"), "lines older than 7 days are not summed: {got}");
        // Spike-4c: the newly guarded calls are new rows in the same grouping.
        let log = vec![
            line("example.org", "chat", now - 60_000, &[hx::DataClass::Chat]),
            line("example.org", "chat", now - 30_000, &[hx::DataClass::Chat]),
            line("api.github.com", "public", now - 600_000, &[]),
            line("mcp.example.test", "approved_once", now - 3_600_000, &chat),
        ];
        let got = report(&hx::ConsentLedger::empty(), &log, false, now);
        assert!(
            got.contains("- example.org · 2 times · chats · allowed · last just now\n- api.github.com · 1 time · no user data · public · last 10m ago\n- mcp.example.test · 1 time · chats, memory · approved once · last 1h ago\n"),
            "{got}"
        );
    }

    /// SY-02: one user-facing name for the hub data, "chats, memory", on the
    /// card command, the card body, the Settings hint and `/privacy`. The scope
    /// ids stay in the span args and the logs.
    #[test]
    fn hub_scope_label_is_chats_memory_everywhere_the_user_reads_it() {
        assert_eq!(HUB_SCOPE_LABEL, "chats, memory");
        assert_eq!(HUB_CARD_ACTION, "/sync → paired computers (chats, memory)");
        assert_eq!(HUB_OFF, "Off. /sync asks each time. Sends chats, memory.");
        assert_eq!(classes(hx::HUB_SYNC_DATA), HUB_SCOPE_LABEL);
        // Compact spots keep the label form; sentences use the prose form (SY-09).
        for compact in [HUB_CARD_ACTION, HUB_OFF] {
            assert!(compact.contains(HUB_SCOPE_LABEL), "{compact}");
            assert!(!compact.contains(HUB_SCOPE_PROSE), "{compact}");
        }
        for user_text in [HUB_CARD_ACTION, HUB_CARD_NOTE, HUB_OFF, PRIVACY_NOTE] {
            assert!(!user_text.contains("chat, personal"), "{user_text}");
        }
        let src = include_str!("privacy_ui.rs");
        let rows = src
            .split("fn ui_privacy_rows(")
            .nth(1)
            .and_then(|s| s.split("fn privacy_revoke_rows(").next())
            .expect("ui_privacy_rows");
        assert!(rows.contains("Sends {HUB_SCOPE_LABEL}."), "the On hint names the scope the same way: {rows}");
        // Logs keep the ids.
        assert_eq!(
            hx::HUB_SYNC_DATA.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            vec!["chat", "personal"]
        );
    }

    /// SY-09: sentences say "chats and memory": the `/sync` card body, the
    /// Settings note and the `/privacy` intro (both places). Compact spots
    /// (`/privacy` grant and egress lines) keep "chats, memory", and the scope
    /// ids in logs stay `chat` / `personal`.
    #[test]
    fn prose_says_chats_and_memory() {
        assert_eq!(HUB_SCOPE_PROSE, "chats and memory");
        assert_eq!(
            HUB_CARD_NOTE,
            "Sends chats and memory to your paired computers. Approve sends once. Esc denies. Settings → Permissions can allow it every time."
        );
        assert!(PRIVACY_NOTE.contains("allowed for chats and memory."), "{PRIVACY_NOTE}");
        assert!(!PRIVACY_NOTE.contains("chats, memory"), "{PRIVACY_NOTE}");
        assert!(!HUB_CARD_NOTE.contains("chats, memory"), "{HUB_CARD_NOTE}");
        let dir = crate::config::test_config_root("privacy-prose");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = hx::grant_destination(&dir, hx::HUB_DEST, hx::HUB_SYNC_DATA, hx::UserClick::from_click()).unwrap();
        let ledger = hx::ConsentLedger::load(&dir);
        let log = vec![line("hub", "grant", g.granted_at, &[hx::DataClass::Chat, hx::DataClass::Personal])];
        let got = report(&ledger, &log, false, g.granted_at + 60_000);
        let intro = got.lines().nth(2).unwrap_or_default();
        assert_eq!(
            intro,
            "Allowed by default: grok.com, x.ai, api.x.ai (chats and memory in model prompts). Sending chats and memory anywhere else waits for your OK: a hard card, or a grant in Settings → Permissions."
        );
        assert!(got.contains("- Sync to paired computers: on since 1m ago · chats, memory\n"), "{got}");
        assert!(got.contains("- paired computers · 1 time · chats, memory · your grant"), "{got}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SY-08: the grant id never shows in `/privacy`, in the report or the
    /// painted bubble with its Revoke row.
    #[test]
    fn privacy_report_hides_the_grant_id() {
        let dir = crate::config::test_config_root("privacy-id");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = hx::grant_destination(&dir, hx::HUB_DEST, hx::HUB_SYNC_DATA, hx::UserClick::from_click()).unwrap();
        let ledger = hx::ConsentLedger::load(&dir);
        let now = g.granted_at + 60_000;
        let got = report(&ledger, &[], false, now);
        assert!(!got.contains(&g.id), "{got}");
        assert!(got.contains("- Sync to paired computers: on since 1m ago · chats, memory\n"), "{got}");
        assert!(!got.contains("No grants yet"), "{got}");
        assert!(!got.contains("egress.jsonl"), "{got}");
        let rows: Vec<PrivacyRevoke> = ledger
            .active()
            .map(|g| PrivacyRevoke { id: g.id.clone(), label: grant_label(g), detail: None })
            .collect();
        let view = grokhub_core::ChatView {
            kind: grokhub_core::ChatKind::Result,
            title: String::new(),
            body: got.clone(),
        };
        assert_eq!(newest_privacy_row(std::slice::from_ref(&view)), Some(0));
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1400.0))),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = super::super::chat_ui::paint_chat_block_with(
                    ui,
                    &view,
                    true,
                    true,
                    grokhub_core::ThoughtFold::Expanded,
                    false,
                );
                let _ = paint_privacy_revokes(ui, &rows, None);
            });
        });
        let mut texts = String::new();
        for clipped in &out.shapes {
            collect_text(&clipped.shape, &mut texts);
        }
        assert!(texts.contains("Revoke"), "{texts}");
        assert!(texts.contains("Sent in the last 7 days (no content stored)"), "{texts}");
        assert!(!texts.contains(&g.id), "the grant id is not painted: {texts}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn collect_text(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(t) => {
                out.push_str(t.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(v) => v.iter().for_each(|c| collect_text(c, out)),
            _ => {}
        }
    }

    /// SB-02: the next step names this OS's keyring. Windows says Windows
    /// Credential Manager and never "Secret Service"; macOS says its Keychain;
    /// only Linux names GNOME Keyring or KWallet.
    #[test]
    fn lock_copy_is_per_os_and_windows_never_says_secret_service() {
        use hx::KeyringOs::{Linux, MacOs, Windows};
        let all = [
            hx::Locked::Unavailable,
            hx::Locked::Missing,
            hx::Locked::WrongKey,
            hx::Locked::Busy,
            hx::Locked::Unwritable,
        ];
        let u = hx::Locked::Unavailable;
        assert_eq!(lock_next_step(&u, Linux), "Unlock or start your keyring (GNOME Keyring or KWallet), then Try again.");
        assert_eq!(
            lock_next_step(&u, Windows),
            "Turn on Windows Credential Manager (Services → Credential Manager), then Try again."
        );
        assert_eq!(lock_next_step(&u, MacOs), "Unlock your macOS Keychain (Keychain Access → login), then Try again.");
        assert_eq!(
            lock_next_step_short(&u, Linux),
            "Next: unlock or start GNOME Keyring or KWallet, then Try again in Settings → Permissions."
        );
        assert_eq!(hx::keyring_name_for(Windows), "Windows Credential Manager");
        assert_eq!(hx::keyring_name_for(MacOs), "macOS Keychain");
        for why in &all {
            for os in [Windows, MacOs] {
                for text in [why.message_for(os), lock_next_step(why, os), lock_next_step_short(why, os)] {
                    assert!(!text.contains("Secret Service"), "{os:?}: {text}");
                    assert!(!text.contains("GNOME") && !text.contains("KWallet"), "{os:?}: {text}");
                }
            }
            for os in [Linux, Windows, MacOs] {
                assert!(lock_next_step(why, os).ends_with("then Try again."), "{os:?}: {why:?}");
                assert!(lock_next_step_short(why, os).ends_with("then Try again in Settings → Permissions."));
            }
        }
        assert!(hx::Locked::Unavailable.message_for(Windows).contains("Windows Credential Manager"));
        assert!(lock_next_step(&u, Windows).contains("Windows Credential Manager"));
        assert!(lock_next_step_short(&u, Windows).contains("Windows Credential Manager"));
        assert!(lock_next_step(&u, MacOs).contains("macOS Keychain"));
        assert!(lock_next_step_short(&u, MacOs).contains("macOS Keychain"));
        // This build picks its own OS's words with cfg.
        let here = hx::KeyringOs::current();
        assert_eq!(here, if cfg!(windows) { Windows } else if cfg!(target_os = "macos") { MacOs } else { Linux });
        assert_eq!(u.message(), u.message_for(here));
        // The folder placeholder follows the OS too (SB-04).
        assert_eq!(super::super::scope_ui::folder_hint_for(Windows), "C:\\Users\\you\\Notes");
        assert_eq!(super::super::scope_ui::folder_hint_for(Linux), "/home/you/Notes");
        assert_eq!(super::super::scope_ui::folder_hint_for(MacOs), "/Users/you/Notes");
        assert_eq!(super::super::scope_ui::FOLDER_HINT, super::super::scope_ui::folder_hint_for(here));
    }

    /// SB-11: the Revoke rows under the `/privacy` bubble start at the
    /// bubble's text inset, not at the row edge.
    #[test]
    fn privacy_revoke_rows_line_up_with_the_bubble_text() {
        let view = grokhub_core::ChatView {
            kind: grokhub_core::ChatKind::Result,
            title: String::new(),
            body: format!("{PRIVACY_HEAD}\n\nGrants"),
        };
        let rows = vec![PrivacyRevoke {
            id: "g1".into(),
            label: "Files in notes".into(),
            detail: Some("/home/you/Documents/notes".into()),
        }];
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = super::super::chat_ui::paint_chat_block_with(
                    ui,
                    &view,
                    true,
                    true,
                    grokhub_core::ThoughtFold::Expanded,
                    false,
                );
                let _ = paint_privacy_revokes(ui, &rows, None);
            });
        });
        let mut lefts = std::collections::HashMap::new();
        fn walk(shape: &egui::Shape, lefts: &mut std::collections::HashMap<String, f32>) {
            match shape {
                egui::Shape::Text(t) => {
                    lefts.insert(t.galley.text().to_string(), t.pos.x + t.galley.rect.min.x);
                }
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, lefts)),
                _ => {}
            }
        }
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut lefts);
        }
        let head = lefts.get(PRIVACY_HEAD).copied().expect("bubble head");
        let row = lefts.get("Files in notes").copied().expect("revoke row label");
        assert!((head - row).abs() < 1.0, "bubble text at {head}, Revoke row at {row}");
    }

    #[test]
    fn sync_result_line_counts_computers() {
        assert_eq!(sync_result_line(0), "Nothing paired yet. Start share to pair a computer.");
        assert_eq!(sync_result_line(1), "Synced chats and memory to 1 computer.");
        assert_eq!(sync_result_line(3), "Synced chats and memory to 3 computers.");
    }
}
