//! Spike-4a trust floor in the cabin: `/privacy`, the `/sync` egress gate, and
//! the one grant row under Settings → Permissions.
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
    match c {
        hx::DataClass::Chat => "chats",
        hx::DataClass::Personal => "memory",
        hx::DataClass::Sensitive => "sensitive data",
    }
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
        format!("Read {}", g.source)
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

/// `/privacy` text: grants, scopes (all off), and recent egress by destination.
/// No content, no secrets: the ledger and the log hold neither.
pub(super) fn privacy_report(
    ledger: &hx::ConsentLedger,
    egress: &[hx::EgressLine],
    desktop_control: bool,
    now_ms: u64,
) -> String {
    let ago = |at: u64| grokhub_core::pulse::ago_label(at, now_ms);
    let mut out = vec![
        PRIVACY_HEAD.to_string(),
        String::new(),
        format!(
            "Allowed by default: {} ({HUB_SCOPE_PROSE} in model prompts). Sending {HUB_SCOPE_PROSE} anywhere else waits for your OK: a hard card, or a grant in Settings → Permissions.",
            grokhub_core::DEFAULT_CONNECTOR_HOSTS.join(", ")
        ),
        String::new(),
        "Grants".to_string(),
    ];
    // The "off" line already says there is no hub grant (SY-07), and the
    // grant id stays out of the text (SY-08).
    if ledger.destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA).is_none() {
        out.push(format!("- {HUB_ROW}: off. /sync asks each time."));
    }
    for g in ledger.active() {
        out.push(format!(
            "- {}: on since {} · {}",
            grant_label(g),
            ago(g.granted_at),
            classes(&g.data_classes)
        ));
    }
    out.push(String::new());
    out.push("Learning scopes: all off. Nothing reads them yet.".into());
    let scopes: Vec<String> = hx::SCOPE_KINDS
        .iter()
        .map(|(kind, label)| {
            let on = ledger.active().any(|g| g.source == *kind || g.source.starts_with(&format!("{kind}:")));
            format!("{label} {}", if on { "on" } else { "off" })
        })
        .collect();
    out.push(format!("- {}", scopes.join(" · ")));
    out.push(format!(
        "- Screen: Settings → Let Grok control the desktop ({})",
        if desktop_control { "on" } else { "off" }
    ));
    out.push(String::new());
    out.push(format!("Sent in the last {PRIVACY_DAYS} days (no content stored)"));
    let since = now_ms.saturating_sub(PRIVACY_DAYS * 86_400_000);
    let mut rows: Vec<EgressRow> = Vec::new();
    for line in egress.iter().filter(|l| l.ts_ms >= since) {
        let basis = match line.basis.as_str() {
            "approved_once" => "approved once",
            "grant" => "your grant",
            "model_host" => "default",
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
    if rows.is_empty() {
        out.push("- Nothing logged yet.".into());
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.last));
    for r in rows {
        let times = if r.times == 1 { "1 time".to_string() } else { format!("{} times", r.times) };
        let data = if r.data.is_empty() { "no user data".to_string() } else { classes(&r.data) };
        out.push(format!("- {} · {times} · {data} · {} · last {}", dest_label(&r.dest), r.basis, ago(r.last)));
    }
    out.push(String::new());
    out.push("Not watched here: Grok Build's own traffic (its model calls, connectors, and web tools). GrokHub's feed reads, update checks, and Labs web fetch and MCP servers are not logged yet.".into());
    out.join("\n")
}

impl Cabin {
    /// The consent ledger, read once and kept current by the clicks below.
    pub(super) fn consent(&mut self) -> &hx::ConsentLedger {
        if self.harness.consent.is_none() {
            self.harness.consent = Some(hx::ConsentLedger::load(&crate::config::config_dir()));
        }
        self.harness.consent.get_or_insert_with(hx::ConsentLedger::empty)
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
            let egress = hx::read_egress(&dir);
            let _ = tx.send(privacy_report(&ledger, &egress, desktop, now_ms()));
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
        let granted = self
            .consent()
            .destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA)
            .map(|g| (g.id.clone(), g.granted_at));
        let dir = crate::config::config_dir();
        match granted {
            None => {
                if crate::cards::settings_action(ui, HUB_ROW, HUB_OFF, "Allow") {
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
                if crate::cards::settings_action_ghost(ui, HUB_ROW, &hint, "Revoke") {
                    self.revoke_hub_grant(&id);
                }
            }
        }
        ui.add_space(8.0);
    }

    /// Active grants as the `/privacy` bubble lists them: (id, label). The id
    /// only rides along for the click; it is never painted (SY-08).
    pub(super) fn privacy_revoke_rows(&mut self) -> Vec<(String, String)> {
        self.consent().active().map(|g| (g.id.clone(), grant_label(g))).collect()
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
            Some(false) => {
                self.status = match hx::revoke_grant(&crate::config::config_dir(), id) {
                    Ok(_) => "Revoked. It asks again next time.".into(),
                    Err(e) => format!("Could not revoke: {e}"),
                };
                self.harness.consent = None;
            }
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

/// Height of one Revoke row: the pill's own height.
const REVOKE_ROW_H: f32 = 28.0;

/// Under the newest `/privacy` bubble: one row per active grant with a ghost
/// Revoke. Returns the grant id whose Revoke was clicked. Pointer clicks only;
/// Enter or any typed text never answers it.
pub(super) fn paint_privacy_revokes(ui: &mut egui::Ui, grants: &[(String, String)]) -> Option<String> {
    let mut hit = None;
    for (id, label) in grants {
        ui.push_id(("privacy-revoke", id.as_str()), |ui| {
            // One pill-high row so the label centers on the Revoke pill.
            let row = egui::vec2(ui.available_width(), REVOKE_ROW_H);
            ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(label).size(13.0).color(crate::theme::muted()));
                if crate::cards::ghost_pill(ui, "Revoke") {
                    hit = Some(id.clone());
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
    /// grant is built only from the Settings click in this file.
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
            ]
        );
    }

    #[test]
    fn privacy_report_on_a_fresh_config() {
        let now = 1_000 * 86_400_000;
        let got = privacy_report(&hx::ConsentLedger::empty(), &[], false, now);
        assert_eq!(
            got,
            [
                "/privacy — what leaves this computer",
                "",
                "Allowed by default: grok.com, x.ai, api.x.ai (chats and memory in model prompts). Sending chats and memory anywhere else waits for your OK: a hard card, or a grant in Settings → Permissions.",
                "",
                "Grants",
                "- Sync to paired computers: off. /sync asks each time.",
                "",
                "Learning scopes: all off. Nothing reads them yet.",
                "- Files in one folder off · Installed apps off · Browser history off · Calendar off · Mail off · System state off",
                "- Screen: Settings → Let Grok control the desktop (off)",
                "",
                "Sent in the last 7 days (no content stored)",
                "- Nothing logged yet.",
                "",
                "Not watched here: Grok Build's own traffic (its model calls, connectors, and web tools). GrokHub's feed reads, update checks, and Labs web fetch and MCP servers are not logged yet.",
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
        let got = privacy_report(&hx::ConsentLedger::empty(), &log, true, now);
        assert!(got.contains("- api.x.ai · 2 times · chats, memory · default · last 2m ago\n- paired computers · 1 time · chats, memory · approved once · last 2h ago\n"), "{got}");
        assert!(!got.contains("hub ·"), "the hub is named plainly: {got}");
        assert!(got.contains("- Screen: Settings → Let Grok control the desktop (on)"), "{got}");
        assert!(!got.contains("3 times"), "lines older than 7 days are not summed: {got}");
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
        let got = privacy_report(&ledger, &log, false, g.granted_at + 60_000);
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
        let got = privacy_report(&ledger, &[], false, now);
        assert!(!got.contains(&g.id), "{got}");
        assert!(got.contains("- Sync to paired computers: on since 1m ago · chats, memory\n"), "{got}");
        assert!(!got.contains("No grants yet"), "{got}");
        assert!(!got.contains("egress.jsonl"), "{got}");
        let rows: Vec<(String, String)> = ledger.active().map(|g| (g.id.clone(), grant_label(g))).collect();
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
                let _ = paint_privacy_revokes(ui, &rows);
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

    #[test]
    fn sync_result_line_counts_computers() {
        assert_eq!(sync_result_line(0), "Nothing paired yet. Start share to pair a computer.");
        assert_eq!(sync_result_line(1), "Synced chats and memory to 1 computer.");
        assert_eq!(sync_result_line(3), "Synced chats and memory to 3 computers.");
    }
}
