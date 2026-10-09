//! The router in the cabin: the model registry refresh on the heartbeat, the
//! passive-health fold, the onboarding and half-open probes, the routing table
//! rebuild (R2a), what the user hears when a model breaks, and `/why`. Network
//! and file work run off the UI thread.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use grokhub_agent::route::guard::{run_guard, GuardRun};
use grokhub_agent::route::learn::{self, TuneRun};
use grokhub_agent::route::local::LocalSource;
use grokhub_agent::route::signals::{LedgerOutcomeSource, SpanVerifySource};
use grokhub_agent::route::heal::{heal_messages, joined_messages, pause_msg, HealMsg, HealNotes, Heard, Tier};
use grokhub_agent::route::refresh::{fold_health, probe_next, run_refresh, RefreshDone, RefreshJob};
use grokhub_agent::route::sources::{probe_model, rebuild_table, GrokBuildSource, XaiApiSource};
use grokhub_core::model_registry::RegistryEvent;
use grokhub_agent::AuthKind;
use grokhub_core::model_registry::profile::read_profiles;
use grokhub_core::model_registry::store::load_registry;
use grokhub_core::model_registry::{CatalogSource, Credential, RefreshClock};

use super::*;

/// Passive health is folded into the registry this often.
pub(super) const HEALTH_FOLD_EVERY: Duration = Duration::from_secs(60);
/// The R1 accuracy guard reads route records and outcomes this often.
pub(super) const GUARD_EVERY: Duration = Duration::from_secs(60 * 60);
/// Router lines kept in the Work tree.
pub(super) const ROUTER_ROWS_MAX: usize = 4;

/// What a refresh or fold told the user, after each incident's once-only check.
#[derive(Debug, Default)]
pub(super) struct HealOut {
    pub msgs: Vec<HealMsg>,
    /// Incidents that ended: their Work-tree line goes.
    pub cleared: Vec<String>,
}

/// Messages for a batch of registry events, each incident told once.
fn heal_after(dir: &std::path::Path, events: &[RegistryEvent], joined: &[String], pin: &str) -> HealOut {
    let now = grokhub_core::now_ms();
    let mut heard = Heard::default();
    if !events.is_empty() {
        heard = heal_messages(events, &load_registry(dir), &read_profiles(dir), pin, now);
    }
    heard.msgs.extend(joined_messages(joined));
    if heard.msgs.is_empty() && heard.cleared.is_empty() {
        return HealOut::default();
    }
    let cleared = heard.cleared.clone();
    let mut notes = HealNotes::load(dir);
    let msgs = notes.admit(heard, now);
    notes.save(dir);
    HealOut { msgs, cleared }
}

#[derive(Debug, Default)]
pub(super) struct RouterUi {
    pub clock: Option<RefreshClock>,
    pub rx: Option<mpsc::Receiver<(RefreshDone, HealOut)>>,
    pub fold_rx: Option<mpsc::Receiver<(bool, HealOut)>>,
    pub last_fold: Option<Instant>,
    pub why_rx: Option<mpsc::Receiver<String>>,
    pub guard_rx: Option<mpsc::Receiver<GuardRun>>,
    pub last_guard: Option<Instant>,
    /// R3a self-tuning, on the guard's hourly slot.
    pub tune_rx: Option<mpsc::Receiver<Result<TuneRun, String>>>,
    /// Refresh models was clicked: the next refresh rebuilds the table even unchanged.
    pub force_table: bool,
    /// The pin the router last heard.
    pub pin: Option<String>,
    /// Quiet Work-tree lines: (incident key, text), newest first.
    pub rows: Vec<(String, String)>,
    /// Pauses already shown this launch (a model that answers again clears them).
    pub paused: Vec<String>,
    /// Router R2b: spend settings, premium asks, budget cards.
    pub budget: super::budget_ui::BudgetUi,
    /// Router R3b: the "Add a provider" fields and the provider pick the router last heard.
    pub providers: super::provider_ui::ProviderUi,
}

impl Cabin {
    /// Sign-in kind only (no token): a change asks for a refresh.
    fn router_auth_stamp(&self) -> String {
        let plan = self.secrets.oauth.is_some() || self.imagine_native.tokens.is_some();
        let key = !self.console_key().trim().is_empty();
        format!("plan={plan};key={key}")
    }

    /// Ids a pin or a default names (a ghost if no source lists them).
    fn router_named_models(&self) -> Vec<String> {
        let mut named: Vec<String> = vec![
            self.cfg.model.trim().to_string(),
            grokhub_core::CABIN_FAST_MODEL.to_string(),
            grokhub_core::CABIN_FAST_FALLBACK.to_string(),
            grokhub_core::DEFAULT_MODEL.to_string(),
            grokhub_agent::DEFAULT_MODEL.to_string(),
        ];
        named.retain(|m| !m.is_empty());
        named.sort();
        named.dedup();
        named
    }

    /// Heartbeat (Housekeep): refresh the registry when due, fold health every minute.
    pub(super) fn tick_model_registry(&mut self, halted: bool) {
        if let Some(rx) = self.harness.router.rx.take() {
            match rx.try_recv() {
                Ok((done, heal)) => {
                    if let Some(clock) = self.harness.router.clock.as_mut() {
                        clock.signal |= done.signal;
                        if done.gb_version.is_some() {
                            clock.gb_version = done.gb_version.clone();
                        }
                    }
                    self.deliver_heal(heal);
                    let added = grokhub_agent::route::providers::load_providers(&crate::config::config_dir());
                    if let Some(note) = super::provider_ui::provider_refresh_note(&done.events, &done.errors, &added) {
                        self.status = note;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => self.harness.router.rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(rx) = self.harness.router.fold_rx.take() {
            match rx.try_recv() {
                Ok((signal, heal)) => {
                    if let Some(clock) = self.harness.router.clock.as_mut() {
                        clock.signal |= signal;
                    }
                    self.deliver_heal(heal);
                }
                Err(mpsc::TryRecvError::Empty) => self.harness.router.fold_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        self.poll_guard();
        self.poll_tune();
        self.poll_openrouter_sign_in();
        self.sync_router_pin();
        self.sync_spend();
        self.poll_route_asks();
        if let Some((_, model, why)) = grokhub_agent::route::live::take_no_route() {
            self.deliver_heal(HealOut { msgs: vec![pause_msg(&model, &why, &[])], cleared: Vec::new() });
        }
        if halted || self.scratch() {
            return;
        }
        let now = now_ms();
        self.tick_budget(now);
        if self.harness.router.last_guard.is_none_or(|t| t.elapsed() >= GUARD_EVERY) && self.harness.router.guard_rx.is_none() {
            self.harness.router.last_guard = Some(Instant::now());
            let dir = crate::config::config_dir();
            let (tx, rx) = mpsc::channel();
            self.harness.router.guard_rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(run_guard(&dir, now));
            });
            if self.harness.router.tune_rx.is_none() {
                let dir = crate::config::config_dir();
                let (tx, rx) = mpsc::channel();
                self.harness.router.tune_rx = Some(rx);
                std::thread::spawn(move || {
                    let verify = SpanVerifySource { config_dir: dir.clone(), session: String::new() };
                    let _ = tx.send(learn::tick(&dir, &verify, &LedgerOutcomeSource::new(&dir), now));
                });
            }
        }
        let stamp = self.router_auth_stamp();
        let idle = !self.running && !self.heartbeat_busy();
        let gb_version = self.cli_installed.clone();
        let clock = self.harness.router.clock.get_or_insert_with(|| RefreshClock::new(now));
        if clock.due(now, idle, gb_version.as_deref(), &stamp).is_some() && self.harness.router.rx.is_none() {
            clock.ran(now, gb_version.as_deref(), &stamp);
            self.start_model_refresh(now);
            return;
        }
        let fold_due = self.harness.router.last_fold.is_none_or(|t| t.elapsed() >= HEALTH_FOLD_EVERY);
        if fold_due && self.harness.router.rx.is_none() && self.harness.router.fold_rx.is_none() {
            self.harness.router.last_fold = Some(Instant::now());
            let dir = crate::config::config_dir();
            let pin = self.cfg.model.trim().to_string();
            let (tx, rx) = mpsc::channel();
            self.harness.router.fold_rx = Some(rx);
            std::thread::spawn(move || {
                let (events, signal) = fold_health(&dir);
                let heal = heal_after(&dir, &events, &[], &pin);
                let _ = tx.send((signal, heal));
            });
        }
    }

    fn start_model_refresh(&mut self, now: u64) {
        let (bearer, credential) = match self.native_cred() {
            Ok((b, AuthKind::OAuth)) => (Some(b), Credential::Plan),
            Ok((b, AuthKind::ApiKey)) => (Some(b), Credential::ApiKey),
            Err(_) => (None, Credential::None),
        };
        let gb = grokhub_acp::find_grok().map(|bin| GrokBuildSource { bin, cwd: self.grok_cwd() });
        let named = self.router_named_models();
        let default_model = grokhub_agent::DEFAULT_MODEL.to_string();
        let pin = self.cfg.model.trim().to_string();
        let force = std::mem::take(&mut self.harness.router.force_table);
        let dir = crate::config::config_dir();
        let (tx, rx) = mpsc::channel();
        self.harness.router.rx = Some(rx);
        std::thread::spawn(move || {
            let api = bearer.clone().map(|b| XaiApiSource { config_dir: dir.clone(), bearer: b });
            let mut sources: Vec<&dyn CatalogSource> = Vec::new();
            if let Some(a) = api.as_ref() {
                sources.push(a);
            }
            if let Some(g) = gb.as_ref() {
                sources.push(g);
            }
            // Lists nothing while `localModel` is off (the default).
            sources.push(&LocalSource);
            // R3b: each provider you added lists only with its key and your grant.
            let added = grokhub_agent::route::providers::sources(&dir);
            for p in &added {
                sources.push(p);
            }
            let job = RefreshJob { config_dir: dir.clone(), sources, credential, named, default_model, now_ms: now };
            let done = run_refresh(&job);
            let plan = bearer.as_deref().filter(|_| credential == Credential::Plan);
            if let Some(b) = plan {
                while probe_next(&dir, credential, grokhub_core::now_ms(), &mut |m, c| probe_model(&dir, b, m, c)).is_some() {}
            }
            // Evals run only on a plan sign-in (included routes); a key rebuilds from metadata.
            let table = rebuild_table(&dir, plan, force, grokhub_core::now_ms());
            let heal = heal_after(&dir, &done.events, &table.joined, &pin);
            let _ = tx.send((done, heal));
        });
    }

    /// A class the accuracy guard reverted gets one Pulse Suggestion saying why.
    fn poll_guard(&mut self) {
        let Some(rx) = self.harness.router.guard_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(run) => {
                if run.reverted.is_empty() {
                    return;
                }
                let now = now_ms();
                for (class, text) in run.reverted {
                    let card = grokhub_core::suggestion_card(&format!("router-guard:{class}"), "Auto now thinks harder here", &text, now);
                    grokhub_core::post_update(&mut self.updates, card);
                }
                self.persist_updates();
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.router.guard_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    /// Self-tuning runs on its own and asks nothing. A promotion gets one
    /// info-only "why" card; a rollback is a ledger line and the weekly
    /// review's router line.
    fn poll_tune(&mut self) {
        let Some(rx) = self.harness.router.tune_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(run)) if !run.promoted.is_empty() => {
                let state = learn::load_state(&crate::config::config_dir());
                self.post_why_cards(&state, &run.promoted);
            }
            Ok(_) | Err(mpsc::TryRecvError::Disconnected) => {}
            Err(mpsc::TryRecvError::Empty) => self.harness.router.tune_rx = Some(rx),
        }
    }

    /// One "Why grok-4-fast for summaries" card per promoted candidate. No actions.
    pub(super) fn post_why_cards(&mut self, state: &grokhub_agent::route::tune::TuneState, promoted: &[String]) {
        for c in state.candidates.iter().filter(|c| promoted.contains(&c.id)) {
            let Some((title, body)) = grokhub_agent::route::tune::why_card_text(c) else {
                continue;
            };
            let at = c.promoted_at.unwrap_or_else(now_ms);
            self.post_feed_card(grokhub_core::router_update_card(&format!("router-why:{}", c.id), &title, &body, at));
        }
    }

    /// The Auto chip: the current rung's label and the router's last plain reason.
    pub(super) fn auto_chip(&self) -> (String, String) {
        match grokhub_agent::route::live::last_user_pick() {
            Some((effort, reason)) => (grokhub_agent::route::effort_word(effort.as_deref()).to_string(), reason),
            None => {
                let start = grokhub_agent::route::live::start_effort(grokhub_agent::route::live::DEFAULT_CLASS);
                let word = grokhub_agent::route::effort_word(start.as_deref());
                (word.to_string(), format!("{word}: everyday chat starts here. GrokHub picks how hard to think on each step. Click for /why."))
            }
        }
    }

    /// `/why` (last 10 route reasons) or `/why models` (one line per model; also
    /// asks for a refresh).
    pub(super) fn run_why(&mut self, models: bool) {
        if self.harness.router.why_rx.is_some() {
            return;
        }
        if models {
            if let Some(clock) = self.harness.router.clock.as_mut() {
                clock.demand = true;
            }
        }
        let dir = crate::config::config_dir();
        let (tx, rx) = mpsc::channel();
        self.harness.router.why_rx = Some(rx);
        std::thread::spawn(move || {
            let body = if models {
                grokhub_agent::route::why_models_text(&load_registry(&dir), &read_profiles(&dir))
            } else {
                grokhub_agent::route::why_text(&dir)
            };
            let _ = tx.send(body);
        });
    }

    pub(super) fn poll_why(&mut self) {
        let Some(rx) = self.harness.router.why_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(body) => {
                self.live_mut().push(("assistant".into(), mark_slash_result(&body)));
                self.stamp_current_access();
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.router.why_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    /// The live router keeps a pin while it answers; Auto is an empty pin.
    fn sync_router_pin(&mut self) {
        grokhub_agent::route::local::set_enabled(self.cfg.local_model);
        self.sync_provider_pick();
        let pin = self.cfg.model.trim();
        if self.harness.router.pin.as_deref() != Some(pin) {
            grokhub_agent::route::live::set_pin(pin);
            self.harness.router.pin = Some(pin.to_string());
        }
    }

    /// Work-tree lines, Home updates and pauses, by tier. Home updates go
    /// through the feed, which holds them in quiet hours.
    fn deliver_heal(&mut self, heal: HealOut) {
        let router = &mut self.harness.router;
        router.rows.retain(|(k, _)| !heal.cleared.contains(k));
        router.paused.retain(|k| !heal.cleared.contains(k));
        let now = now_ms();
        for m in heal.msgs {
            match m.tier {
                Tier::WorkRow => {
                    let router = &mut self.harness.router;
                    router.rows.retain(|(k, _)| *k != m.key);
                    router.rows.insert(0, (m.key, m.text));
                    router.rows.truncate(ROUTER_ROWS_MAX);
                }
                Tier::HomeUpdate => {
                    let card = grokhub_core::router_update_card(&m.key, &m.title, &m.text, now);
                    self.post_feed_card(card);
                }
                Tier::Pause => {
                    if self.harness.router.paused.contains(&m.key) {
                        continue;
                    }
                    self.harness.router.paused.push(m.key.clone());
                    self.status = m.title.clone();
                    let card = grokhub_core::suggestion_card(&format!("router-{}", m.key), &m.title, &m.text, now);
                    self.post_feed_card(card);
                }
            }
        }
    }

    /// Settings → Refresh models: list, probes and the table now.
    pub(super) fn refresh_models_now(&mut self) {
        self.harness.router.force_table = true;
        if let Some(clock) = self.harness.router.clock.as_mut() {
            clock.demand = true;
        }
        self.status = "Refreshing models...".into();
    }

    /// The Settings model list: Auto first, retired models gone (unless pinned),
    /// and a tag on a model that isn't in your plan or isn't answering.
    pub(super) fn router_model_choices(&self) -> Vec<(String, String)> {
        let (reg, _) = grokhub_agent::route::live::snapshot(&crate::config::config_dir());
        let pin = self.cfg.model.trim();
        super::settings::cabin_default_models()
            .into_iter()
            .filter(|(id, _)| id.is_empty() || !grokhub_agent::route::heal::settings_hidden(&reg, id, pin))
            .map(|(id, label)| {
                let retired = !id.is_empty() && reg.get(id).is_some_and(|r| r.state.tombstone());
                let label = match grokhub_agent::route::heal::settings_tag(&reg, id) {
                    _ if retired => format!("{label} (retired)"),
                    Some(tag) => format!("{label} · {tag}"),
                    None => label.to_string(),
                };
                (id.to_string(), label)
            })
            .collect()
    }

    /// "How Auto picks": read-only, one line per class from the routing table.
    pub(super) fn how_auto_picks_lines(&self) -> String {
        let table = grokhub_agent::route::live::table_snapshot(&crate::config::config_dir());
        if table.classes.is_empty() {
            return "How Auto picks: the table builds after the model list refreshes.".into();
        }
        let mut lines = vec![format!("How Auto picks (table v{}):", table.version)];
        lines.extend(grokhub_agent::route::table::how_auto_picks(&table));
        lines.join("\n")
    }

    /// `/why table`.
    pub(super) fn run_why_table(&mut self) {
        let table = grokhub_agent::route::live::table_snapshot(&crate::config::config_dir());
        let dir = crate::config::config_dir();
        let mut body = grokhub_agent::route::table::why_table_text(&table, now_ms());
        for line in learn::why_table_lines(&dir, now_ms()) {
            body.push('\n');
            body.push_str(&line);
        }
        self.live_mut().push(("assistant".into(), mark_slash_result(&body)));
        self.stamp_current_access();
        self.persist();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::route::heal::pause_msg;

    fn msg(tier: Tier, key: &str, text: &str) -> HealMsg {
        HealMsg { tier, key: key.into(), title: format!("title {key}"), text: text.into() }
    }

    #[test]
    fn heal_tiers_land_in_the_work_tree_on_home_or_as_a_pause() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("router-heal");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        let row = "grok-4.6 isn't answering, so I'm using grok-4.7 for now. Your pick is saved.";
        app.deliver_heal(HealOut {
            msgs: vec![
                msg(Tier::WorkRow, "unhealthy:grok-4.6", row),
                msg(Tier::HomeUpdate, "retired:grok-4.3", "grok-4.3 was retired by xAI"),
                pause_msg("grok-4.7", "It failed 3 times in a row.", &[]),
            ],
            cleared: Vec::new(),
        });
        assert_eq!(app.harness.router.rows, vec![("unhealthy:grok-4.6".to_string(), row.to_string())]);
        assert_eq!(app.status, "Paused: no model in your plan is answering");
        let titles: Vec<&str> = app.updates.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles.len(), 2, "{titles:?}");
        assert!(titles.contains(&"title retired:grok-4.3") && titles.contains(&"Paused: no model in your plan is answering"));
        // The same pause again says nothing; the model answering again clears the row and the pause.
        app.deliver_heal(HealOut { msgs: vec![pause_msg("grok-4.7", "It failed.", &[])], cleared: Vec::new() });
        assert_eq!(app.updates.len(), 2);
        app.deliver_heal(HealOut { msgs: Vec::new(), cleared: vec!["unhealthy:grok-4.6".into(), "noroute:grok-4.7".into()] });
        assert!(app.harness.router.rows.is_empty());
        assert!(app.harness.router.paused.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_pin_reaches_the_router_and_refresh_forces_a_table_rebuild() {
        let mut app = Cabin::quiet_for_test();
        app.cfg.model = "grok-4.6".into();
        app.sync_router_pin();
        assert_eq!(app.harness.router.pin.as_deref(), Some("grok-4.6"));
        app.cfg.model = String::new();
        app.sync_router_pin();
        assert_eq!(app.harness.router.pin.as_deref(), Some(""));
        app.harness.router.clock = Some(RefreshClock::new(1));
        app.refresh_models_now();
        assert!(app.harness.router.force_table);
        assert!(app.harness.router.clock.as_ref().unwrap().demand);
    }
}
