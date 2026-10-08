//! Router R2b in the cabin: the spend settings the router reads, the premium
//! hard money card and its revocable grant rows under Settings → Permissions,
//! and the budget cards (80%, 100%, the upgrade nudge) on the approval stack.
//!
//! A premium grant is written only here, from the hard card's Approve click
//! (`UserClick::from_click`). Enter never approves it, Esc and the timeout
//! deny, and there is no Always. GrokHub has no path to credits, top-ups or
//! plans: the nudge's details name grok.com and open nothing.

use std::sync::mpsc;
use std::time::Instant;

use grokhub_agent::harness::{self as hx, GateOutcome, Step};
use grokhub_agent::route::budget::{self, Budget, BudgetNotes, NudgeFacts, NudgeGate};
use grokhub_agent::route::spend::{self, SpendSettings, PREMIUM_PREFIX};
use grokhub_core::model_registry::cost_class::usd_per_m;
use grokhub_core::model_registry::store::load_registry;

use super::*;

/// Settings → Cabin defaults rows.
pub(super) const FAST_ROW: &str = "Use Grok 4.7 Fast when you're waiting";
pub(super) const FAST_HINT: &str =
    "Auto may use the faster Grok 4.7 Fast (about twice the usage) only while you wait on a reply. Never for background work. Off means never.";
pub(super) const CAP_ROW: &str = "Weekly spend cap";
const CAP_HINT: &str = "API key only. At 80% background work moves to cheaper settings. At 100% it pauses and chats ask first.";
pub(super) const CEILING_ROW: &str = "Price limit";
const CEILING_HINT: &str = "A model over this price per million output tokens needs your OK once.";
/// Weekly caps offered, USD. 0 is off.
pub(super) const CAPS: &[u32] = &[0, 5, 10, 25, 50, 100];
/// Price limits offered, USD per million output tokens.
pub(super) const CEILINGS: &[u32] = &[5, 15, 30, 60];

/// Settings → Permissions: the premium grant rows.
pub(super) const PREMIUM_HEAD: &str = "Paid upgrades";
const PREMIUM_NONE: &str = "None. A model over your price limit asks once before Auto uses it.";
/// The hard money card under its action line.
pub(super) const PREMIUM_CARD_NOTE: &str =
    "Costs more than your price limit. Approve allows it until you revoke it in Settings → Permissions. Esc denies.";
pub(super) const PREMIUM_TOOL: &str = "model.route";

/// Budget card eyebrows and buttons.
const BUDGET_EYEBROW: &str = "This week's usage";
const NUDGE_EYEBROW: &str = "Your plan";

pub(super) fn cap_label(usd: u32) -> String {
    if usd == 0 {
        "Off".into()
    } else {
        format!("${usd} a week")
    }
}

pub(super) fn ceiling_label(usd: u32) -> String {
    format!("${usd} per million")
}

/// The model a premium grant key names (`premium:grok-heavy+priority` → `grok-heavy`).
pub(super) fn premium_model(key: &str) -> &str {
    let rest = key.strip_prefix(PREMIUM_PREFIX).unwrap_or(key);
    rest.split('+').next().unwrap_or(rest)
}

/// How a premium grant reads in Settings and `/privacy`.
pub(super) fn premium_label(key: &str) -> String {
    let mut label = format!("Use {}", premium_model(key));
    for (tag, words) in [("+priority", " at priority speed"), ("+us", " on the US endpoint"), ("+agents16", " with 16 agents")] {
        if key.contains(tag) {
            label.push_str(words);
        }
    }
    label
}

/// One card on the approval stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BudgetCard {
    /// 80% of the week used (once a week).
    Eighty,
    /// 100%: user-facing work waits for this answer.
    Ask,
    /// The upgrade nudge, with its words.
    Nudge(String),
}

#[derive(Debug, Default)]
pub(super) struct BudgetUi {
    pub rx: Option<mpsc::Receiver<(Budget, NudgeFacts)>>,
    pub last: Option<Instant>,
    pub card: Option<BudgetCard>,
    pub facts: NudgeFacts,
    /// The settings the router last heard.
    pub sent: Option<SpendSettings>,
    /// Premium routes already asked about this launch (approved or not).
    pub asked: Vec<String>,
}

impl Cabin {
    fn spend_settings(&self) -> SpendSettings {
        SpendSettings {
            fast_when_waiting: self.cfg.fast_when_waiting,
            weekly_cap_usd: self.cfg.weekly_spend_cap_usd as f64,
            ceiling_usd_per_m: self.cfg.price_ceiling_usd_per_m as f64,
        }
    }

    /// The router reads Settings; it never writes them.
    pub(super) fn sync_spend(&mut self) {
        let now = self.spend_settings();
        if self.harness.router.budget.sent != Some(now) {
            spend::set_spend(now);
            budget::forget_week();
            self.harness.router.budget.sent = Some(now);
        }
    }

    /// A call wanted a premium route: park the hard money card once per route
    /// this launch. `harness::decide` is the gate; a grant lets it through.
    pub(super) fn poll_route_asks(&mut self) {
        if let Some(key) = grokhub_agent::route::live::take_premium_ask() {
            if !self.harness.router.budget.asked.contains(&key) {
                self.harness.router.budget.asked.push(key.clone());
                self.ask_premium(&key);
            }
        }
        if grokhub_agent::route::live::take_budget_ask() {
            self.harness.router.budget.card = Some(BudgetCard::Ask);
        }
    }

    pub(super) fn ask_premium(&mut self, key: &str) {
        let ledger = self.consent().clone();
        if let GateOutcome::Park { hard: Some(class), .. } = hx::decide(Step::Premium { route_key: key, ledger: &ledger }) {
            let action = self.premium_action(key);
            let args = serde_json::json!({ "route": key }).to_string();
            self.write_span(hx::Span::hard_park(&self.trace_id(), PREMIUM_TOOL, &args, class), "route");
            self.park_hard(super::harness_ui::ParkSource::Premium(key.to_string()), class, "route", PREMIUM_TOOL.into(), action);
        }
    }

    /// "Let Auto use grok-heavy ($60 per million, over your $15 limit)".
    fn premium_action(&self, key: &str) -> String {
        let model = premium_model(key);
        let (reg, _) = grokhub_agent::route::live::snapshot(&crate::config::config_dir());
        let price = reg.get(model).and_then(|r| r.meta.prices.completion).map(usd_per_m);
        let limit = self.cfg.price_ceiling_usd_per_m;
        let what = premium_label(key);
        match price {
            Some(p) => format!("Let Auto {} (${p:.0} per million, over your ${limit} limit)", what.replacen("Use ", "use ", 1)),
            None => format!("Let Auto {}", what.replacen("Use ", "use ", 1)),
        }
    }

    /// The hard money card's Approve click: one revocable grant for exactly this route.
    pub(super) fn grant_premium_click(&mut self, key: &str) {
        match hx::grant_premium(&crate::config::config_dir(), key, hx::UserClick::from_click()) {
            Ok(_) => self.status = format!("{} allowed. Revoke it in Settings → Permissions.", premium_label(key)),
            Err(e) => self.status = format!("Could not save the grant: {e}"),
        }
        self.harness.consent = None;
    }

    /// Every minute: this week's spend and the nudge's facts, off the UI thread.
    pub(super) fn tick_budget(&mut self, now: u64) {
        if let Some(rx) = self.harness.router.budget.rx.take() {
            match rx.try_recv() {
                Ok((b, facts)) => self.budget_landed(&b, facts, now),
                Err(mpsc::TryRecvError::Empty) => self.harness.router.budget.rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        let due = self.harness.router.budget.last.is_none_or(|t| t.elapsed().as_millis() as u64 >= budget::BUDGET_EVERY_MS);
        if !due || self.harness.router.budget.rx.is_some() {
            return;
        }
        self.harness.router.budget.last = Some(Instant::now());
        let dir = crate::config::config_dir();
        let cap = self.cfg.weekly_spend_cap_usd as f64;
        let (tx, rx) = mpsc::channel();
        self.harness.router.budget.rx = Some(rx);
        std::thread::spawn(move || {
            let reg = load_registry(&dir);
            let b = Budget::of(&budget::load_week(&dir, &reg, now), reg.entitlement.credential, cap);
            let facts = budget::nudge_facts(&budget::read_records(&dir), now);
            let _ = tx.send((b, facts));
        });
    }

    /// Which card goes up, if any: the 80% card once a week, else the nudge
    /// when its rule allows. One card at a time; no OS notification.
    pub(super) fn budget_landed(&mut self, b: &Budget, facts: NudgeFacts, now: u64) {
        self.harness.router.budget.facts = facts.clone();
        if self.harness.router.budget.card.is_some() {
            return;
        }
        let dir = crate::config::config_dir();
        let mut notes = BudgetNotes::load(&dir);
        if notes.eighty_due(b) {
            notes.eighty_week = b.week;
            notes.save(&dir);
            self.harness.router.budget.card = Some(BudgetCard::Eighty);
            return;
        }
        let gate = NudgeGate { in_task: self.running, quiet_hours: self.quiet_now(), throttled: self.proactive.room(now) == 0 };
        if budget::nudge_due(&facts, &notes, gate, now) {
            notes.nudge_shown_ms = now;
            notes.save(&dir);
            let (reg, _) = grokhub_agent::route::live::snapshot(&dir);
            let text = budget::nudge_text(&facts, reg.entitlement.tier.as_deref());
            self.harness.router.budget.card = Some(BudgetCard::Nudge(text));
        }
    }

    /// The card's words: (eyebrow, title, note, primary, secondary).
    pub(super) fn budget_card_text(card: &BudgetCard) -> (&'static str, String, &'static str, &'static str, &'static str) {
        match card {
            BudgetCard::Eighty => (BUDGET_EYEBROW, budget::EIGHTY_CARD.into(), "Settings → Cabin defaults has the cap.", "OK", "Change cap"),
            BudgetCard::Ask => (BUDGET_EYEBROW, budget::BUDGET_ASK.into(), "Background work stays paused until next week.", "Keep going", "Wait"),
            BudgetCard::Nudge(text) => (NUDGE_EYEBROW, text.clone(), "Nothing opens until you click.", "Details", "Not now"),
        }
    }

    /// A click on a budget card. Primary is OK / Keep going / Details.
    pub(super) fn answer_budget_card(&mut self, primary: bool) {
        let Some(card) = self.harness.router.budget.card.take() else {
            return;
        };
        let dir = crate::config::config_dir();
        let now = now_ms();
        match (card, primary) {
            (BudgetCard::Eighty, true) => self.status = "OK. Chats stay at normal quality.".into(),
            (BudgetCard::Eighty, false) => {
                self.nav = Nav::Settings;
                self.settings_sec = SettingsSec::Defaults;
            }
            (BudgetCard::Ask, true) => {
                let mut notes = BudgetNotes::load(&dir);
                notes.go_on_week = budget::week_of(now);
                notes.save(&dir);
                self.status = "Chats go on this week. Background work waits.".into();
            }
            (BudgetCard::Ask, false) => self.status = "Chats wait until next week or a higher cap.".into(),
            (BudgetCard::Nudge(_), true) => {
                let body = budget::nudge_details(&self.harness.router.budget.facts);
                self.live_mut().push(("assistant".into(), mark_slash_result(&body)));
                self.stamp_current_access();
                self.persist();
            }
            (BudgetCard::Nudge(_), false) => {
                let mut notes = BudgetNotes::load(&dir);
                notes.nudge_muted_until_ms = now + budget::NUDGE_EVERY_MS;
                notes.save(&dir);
                self.status = "Not now. GrokHub won't bring this up for 30 days.".into();
            }
        }
    }

    /// The budget card on the approval stack, same white-accent card as the others. Click only.
    pub(super) fn paint_budget_card(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(card) = self.harness.router.budget.card.clone() else {
            return;
        };
        let (eyebrow, title, note, primary, secondary) = Self::budget_card_text(&card);
        let text = super::harness_ui::CardText { eyebrow, title: &title, action: "", note, primary, secondary, hard: false };
        let key = match &card {
            BudgetCard::Eighty => "eighty",
            BudgetCard::Ask => "ask",
            BudgetCard::Nudge(_) => "nudge",
        };
        if let Some(primary) = super::harness_ui::harness_card(ui, ("budget", key.to_string()), text, running) {
            self.answer_budget_card(primary);
        }
    }

    /// Settings → Cabin defaults: the Fast toggle, the weekly cap, the price limit.
    pub(super) fn ui_spend_rows(&mut self, ui: &mut egui::Ui) {
        let mut changed = crate::cards::settings_toggle(ui, FAST_ROW, FAST_HINT, &mut self.cfg.fast_when_waiting);
        let caps: Vec<String> = CAPS.iter().map(|c| cap_label(*c)).collect();
        if let Some(i) = crate::cards::settings_dropdown(ui, CAP_ROW, CAP_HINT, &cap_label(self.cfg.weekly_spend_cap_usd), &caps) {
            changed |= CAPS.get(i).is_some_and(|c| std::mem::replace(&mut self.cfg.weekly_spend_cap_usd, *c) != *c);
        }
        let ceilings: Vec<String> = CEILINGS.iter().map(|c| ceiling_label(*c)).collect();
        let now = ceiling_label(self.cfg.price_ceiling_usd_per_m);
        if let Some(i) = crate::cards::settings_dropdown(ui, CEILING_ROW, CEILING_HINT, &now, &ceilings) {
            changed |= CEILINGS.get(i).is_some_and(|c| std::mem::replace(&mut self.cfg.price_ceiling_usd_per_m, *c) != *c);
        }
        if changed {
            self.persist_cfg();
            self.sync_spend();
            self.status = "Saved".into();
        }
    }

    /// Settings → Permissions, "Paid upgrades": one row per premium route you
    /// approved, each with Revoke. Granting happens only on the hard card.
    pub(super) fn ui_premium_rows(&mut self, ui: &mut egui::Ui, locked: Option<&str>) {
        crate::cards::section_heading(ui, PREMIUM_HEAD);
        let grants: Vec<(String, String, u64)> = self
            .consent()
            .active()
            .filter(|g| g.source.starts_with(PREMIUM_PREFIX))
            .map(|g| (g.id.clone(), g.source.clone(), g.granted_at))
            .collect();
        if grants.is_empty() {
            crate::cards::settings_note(ui, PREMIUM_NONE);
        }
        for (id, key, at) in grants {
            let hint = format!("Allowed {}. Revoke and Auto asks again.", grokhub_core::pulse::ago_label(at, now_ms()));
            ui.push_id(("premium-grant", id.as_str()), |ui| {
                if crate::cards::settings_grant_row(ui, &premium_label(&key), &hint, None, crate::cards::GrantPill::Revoke, locked, false, |_| {})
                    && locked.is_none()
                {
                    self.revoke_premium(&id, &key);
                }
            });
        }
        ui.add_space(8.0);
    }

    pub(super) fn revoke_premium(&mut self, id: &str, key: &str) {
        match hx::revoke_grant(&crate::config::config_dir(), id) {
            Ok(_) => {
                self.harness.router.budget.asked.retain(|k| k != key);
                self.status = format!("{} revoked. Auto asks again before using it.", premium_label(key));
            }
            Err(e) => self.status = format!("Could not revoke: {e}"),
        }
        self.harness.consent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::harness::HardClass;

    fn is_premium_card(class: HardClass, source: &super::super::harness_ui::ParkSource) -> bool {
        class == HardClass::Money && matches!(source, super::super::harness_ui::ParkSource::Premium(_))
    }

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        std::fs::create_dir_all(&root).unwrap();
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    #[test]
    fn premium_labels_name_the_model_and_its_options() {
        assert_eq!(premium_model("premium:grok-heavy+priority+us"), "grok-heavy");
        assert_eq!(premium_label("premium:grok-heavy"), "Use grok-heavy");
        assert_eq!(premium_label("premium:grok-4.7+priority+us+agents16"), "Use grok-4.7 at priority speed on the US endpoint with 16 agents");
        assert_eq!((cap_label(0), cap_label(25), ceiling_label(15)), ("Off".to_string(), "$25 a week".to_string(), "$15 per million".to_string()));
    }

    #[test]
    fn a_premium_ask_parks_a_hard_money_card_and_approve_writes_a_revocable_grant() {
        let (_pin, root) = pinned("r2b-premium-card");
        let mut app = Cabin::quiet_for_test();
        app.ask_premium("premium:grok-heavy");
        let park = app.harness.park.clone().expect("a hard card");
        assert!(is_premium_card(park.class, &park.source));
        assert_eq!(park.class.label(), "Purchase / money");
        // Enter never approves it; Esc denies it.
        assert_eq!(hx::hard_card_key(true, false, false), None);
        assert_eq!(hx::hard_card_key(false, true, false), Some(hx::HardAnswer::Deny));
        // Deny writes nothing.
        app.resolve_hard_park(false, "Jeremy denied (Esc)");
        assert!(app.consent().premium_grant("premium:grok-heavy").is_none());
        // Approve (a click) writes exactly one grant for exactly this route.
        app.ask_premium("premium:grok-heavy");
        app.resolve_hard_park(true, "");
        let g = app.consent().premium_grant("premium:grok-heavy").cloned().expect("granted");
        assert!(app.consent().premium_grant("premium:grok-heavy+priority").is_none());
        // Granted: decide lets it through, so no second card.
        app.ask_premium("premium:grok-heavy");
        assert!(app.harness.park.is_none());
        // Revoke in Settings → Permissions: the card comes back.
        app.revoke_premium(&g.id, "premium:grok-heavy");
        assert!(app.consent().premium_grant("premium:grok-heavy").is_none());
        app.ask_premium("premium:grok-heavy");
        assert!(app.harness.park.is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_eighty_card_shows_once_a_week_and_not_now_mutes_the_nudge_for_30_days() {
        let (_pin, root) = pinned("r2b-budget-cards");
        let mut app = Cabin::quiet_for_test();
        app.cfg.quiet_start = "00:00".into();
        app.cfg.quiet_end = "00:00".into();
        let now = 1_000 * budget::DAY_MS;
        let tight = Budget { week: budget::week_of(now), pct: Some(82), cap_usd: 10.0, ..Budget::default() };
        app.budget_landed(&tight, NudgeFacts::default(), now);
        assert_eq!(app.harness.router.budget.card, Some(BudgetCard::Eighty));
        let (_, title, _, primary, _) = Cabin::budget_card_text(&BudgetCard::Eighty);
        assert_eq!((title.as_str(), primary), (budget::EIGHTY_CARD, "OK"));
        app.answer_budget_card(true);
        app.budget_landed(&tight, NudgeFacts::default(), now + 60_000);
        assert_eq!(app.harness.router.budget.card, None, "once a week");
        // The nudge: thresholds met, nothing running.
        let facts = NudgeFacts { would_upgrade: 5, fails: 3, last_failed: 3, last_of: 5 };
        let calm = Budget { week: tight.week, pct: Some(40), ..Budget::default() };
        app.running = true;
        app.budget_landed(&calm, facts.clone(), now);
        assert_eq!(app.harness.router.budget.card, None, "never during a task");
        app.running = false;
        app.cfg.quiet_end = "23:59".into();
        app.budget_landed(&calm, facts.clone(), now);
        assert_eq!(app.harness.router.budget.card, None, "never in quiet hours");
        app.cfg.quiet_end = "00:00".into();
        app.budget_landed(&calm, facts.clone(), now);
        assert!(matches!(&app.harness.router.budget.card, Some(BudgetCard::Nudge(t)) if t.ends_with("Want details?")));
        app.answer_budget_card(false);
        let notes = BudgetNotes::load(&crate::config::config_dir());
        assert_eq!(notes.nudge_shown_ms, now);
        assert!(notes.nudge_muted_until_ms >= now_ms() + budget::NUDGE_EVERY_MS - 60_000);
        app.budget_landed(&calm, facts, now + budget::DAY_MS);
        assert_eq!(app.harness.router.budget.card, None, "Not now mutes it");
        // At 100% the ask: Keep going lets chat go on this week.
        app.harness.router.budget.card = Some(BudgetCard::Ask);
        app.answer_budget_card(true);
        assert_eq!(BudgetNotes::load(&crate::config::config_dir()).go_on_week, budget::week_of(now_ms()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn spend_settings_reach_the_router_and_fast_is_on_by_default() {
        let mut app = Cabin::quiet_for_test();
        assert!(app.cfg.fast_when_waiting);
        assert_eq!((app.cfg.weekly_spend_cap_usd, app.cfg.price_ceiling_usd_per_m), (0, 15));
        app.cfg.fast_when_waiting = false;
        app.cfg.weekly_spend_cap_usd = 25;
        app.sync_spend();
        assert_eq!(spend::spend_settings(), SpendSettings { fast_when_waiting: false, weekly_cap_usd: 25.0, ceiling_usd_per_m: 15.0 });
        app.cfg = crate::config::AppConfig::default();
        app.sync_spend();
        assert_eq!(spend::spend_settings(), SpendSettings::default());
    }

    /// GrokHub has no path to credits, top-ups or plans: no source line holds
    /// such a URL, and no opener is pointed at one. Reads files at run time
    /// (no `include_str!`), so CRLF checkouts read the same.
    #[test]
    fn no_code_path_opens_a_top_up_or_credits_url() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates dir").to_path_buf();
        let words = ["top-up", "topup", "top_up", "credits", "billing", "/subscribe", "/upgrade", "/plans", "/pricing", "auto-top", "autotopup"];
        let mut hits = Vec::new();
        let mut stack = vec![crates.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if path.is_dir() {
                    if name != "target" {
                        stack.push(path);
                    }
                    continue;
                }
                if !name.ends_with(".rs") || name == "budget_ui.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for line in text.lines() {
                    let l = line.to_ascii_lowercase();
                    if (l.contains("http://") || l.contains("https://")) && words.iter().any(|w| l.contains(w)) {
                        hits.push(format!("{}: {}", path.display(), line.trim()));
                    }
                }
            }
        }
        assert!(hits.is_empty(), "{hits:#?}");
    }
}
