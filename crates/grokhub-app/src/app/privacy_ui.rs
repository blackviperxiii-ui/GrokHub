//! Spike-4a trust floor in the cabin: `/privacy`, the `/sync` egress gate, and
//! the one grant row under Settings → Permissions.
//!
//! A grant is written only here, from a Settings click (`UserClick::from_click`).
//! `/privacy` only reads. `/sync` asks `harness::decide` (`Step::Egress`) first:
//! no hub grant ⇒ a hard Send card (Approve sends once). D1: Grok Build's own
//! traffic is outside the cabin and is not watched here.

use super::*;
use grokhub_agent::harness::{self as hx, GateOutcome, Step};

/// Top of Settings → Permissions.
pub(super) const PRIVACY_NOTE: &str = "Nothing new leaves this computer without your OK. xAI model hosts stay allowed for chat. Grok Build's own traffic is outside GrokHub. /privacy shows what left.";
pub(super) const HUB_ROW: &str = "Sync to paired computers";
const HUB_OFF: &str = "Off. /sync asks each time. Sends chats and memory.";
/// The hard card for an ungranted `/sync`.
pub(super) const HUB_CARD_ACTION: &str = "/sync → paired computers (chat, personal)";
pub(super) const HUB_CARD_NOTE: &str =
    "Your chats and memory go to your paired computers. Approve sends once. Esc denies. Settings → Permissions can allow it every time.";
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

fn classes(data: &[hx::DataClass]) -> String {
    data.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
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
        "/privacy — what leaves this computer".to_string(),
        String::new(),
        format!(
            "Allowed by default: {} (chat and memory in model prompts). Anywhere else, chat or memory waits for your OK: a hard card, or a grant in Settings → Permissions.",
            grokhub_core::DEFAULT_CONNECTOR_HOSTS.join(", ")
        ),
        String::new(),
        "Grants".to_string(),
    ];
    let mut any = false;
    if ledger.destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA).is_none() {
        out.push(format!("- {HUB_ROW}: off. /sync asks each time."));
    }
    for g in ledger.active() {
        any = true;
        let what = if g.destination == hx::HUB_DEST {
            HUB_ROW.to_string()
        } else if !g.destination.is_empty() {
            format!("Send to {}", g.destination)
        } else {
            format!("Read {}", g.source)
        };
        out.push(format!(
            "- {what}: on since {} · {} · {}",
            ago(g.granted_at),
            classes(&g.data_classes),
            g.id
        ));
    }
    if !any && ledger.all().is_empty() {
        out.push("- No grants yet.".into());
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
    out.push(format!("Sent in the last {PRIVACY_DAYS} days (egress.jsonl, no content)"));
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
        out.push(format!("- {} · {times} · {data} · {} · last {}", r.dest, r.basis, ago(r.last)));
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

    /// `/sync` publishes chats and memory to paired computers, so it asks the
    /// EgressGuard first. Granted ⇒ sync; not granted ⇒ hard Send card.
    pub(super) fn gate_hub_sync(&mut self) {
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

    /// Settings → Permissions: the trust-floor note and the hub grant row.
    /// The Allow click is the only place a grant is written (rule 4).
    pub(super) fn ui_privacy_rows(&mut self, ui: &mut egui::Ui) {
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
                    "On since {}. Chats and memory.",
                    grokhub_core::pulse::ago_label(at, now_ms())
                );
                if crate::cards::settings_action(ui, HUB_ROW, &hint, "Revoke") {
                    self.revoke_hub_grant(&id);
                }
            }
        }
        ui.add_space(8.0);
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
                "Allowed by default: grok.com, x.ai, api.x.ai (chat and memory in model prompts). Anywhere else, chat or memory waits for your OK: a hard card, or a grant in Settings → Permissions.",
                "",
                "Grants",
                "- Sync to paired computers: off. /sync asks each time.",
                "- No grants yet.",
                "",
                "Learning scopes: all off. Nothing reads them yet.",
                "- Files in one folder off · Installed apps off · Browser history off · Calendar off · Mail off · System state off",
                "- Screen: Settings → Let Grok control the desktop (off)",
                "",
                "Sent in the last 7 days (egress.jsonl, no content)",
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
        assert!(got.contains("- api.x.ai · 2 times · chat, personal · default · last 2m ago\n- hub · 1 time · chat, personal · approved once · last 2h ago\n"), "{got}");
        assert!(got.contains("- Screen: Settings → Let Grok control the desktop (on)"), "{got}");
        assert!(!got.contains("3 times"), "lines older than 7 days are not summed: {got}");
    }
}
