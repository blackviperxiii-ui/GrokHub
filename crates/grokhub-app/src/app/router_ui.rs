//! The router in the cabin: the model registry refresh on the heartbeat, the
//! passive-health fold, the onboarding and half-open probes, the routing table
//! rebuild (R2a) and what the user hears when a model breaks. Network and file
//! work run off the UI thread. Nothing here shows which model or effort the
//! router picked: only a chat question about it is told.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use grokhub_agent::route::guard::{run_guard, GuardRun};
use grokhub_agent::route::heal::{heal_messages, joined_messages, HealMsg, HealNotes, Heard};
use grokhub_agent::route::learn::{self, TuneRun};
use grokhub_agent::route::local::LocalSource;
use grokhub_agent::route::refresh::{
    fold_health, probe_next, run_refresh, RefreshDone, RefreshJob,
};
use grokhub_agent::route::signals::{LedgerOutcomeSource, SpanVerifySource};
use grokhub_agent::route::sources::{probe_model, rebuild_table, XaiApiSource};
use grokhub_agent::AuthKind;
use grokhub_core::model_registry::profile::read_profiles;
use grokhub_core::model_registry::store::load_registry;
use grokhub_core::model_registry::RegistryEvent;
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
fn heal_after(dir: &std::path::Path, events: &[RegistryEvent], joined: &[String]) -> HealOut {
    let now = grokhub_core::now_ms();
    let mut heard = Heard::default();
    if !events.is_empty() {
        heard = heal_messages(events, &load_registry(dir), &read_profiles(dir), now);
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
    pub guard_rx: Option<mpsc::Receiver<GuardRun>>,
    pub last_guard: Option<Instant>,
    /// R3a self-tuning, on the guard's hourly slot.
    pub tune_rx: Option<mpsc::Receiver<Result<TuneRun, String>>>,
    /// A provider or Local AI was just set up: the next refresh rebuilds the
    /// table even unchanged.
    pub force_table: bool,
    /// Quiet router notes (key, text), newest first: a wait that ended.
    pub rows: Vec<(String, String)>,
    /// Router R2b: spend settings, premium asks, budget cards.
    pub budget: super::budget_ui::BudgetUi,
    /// Router R3b: the "Add a provider" fields and the data policy the router last heard.
    pub providers: super::provider_ui::ProviderUi,
    /// A no-route pause waiting on its own for a model to answer.
    pub wait: super::model_wait_ui::ModelWaitUi,
}

impl Cabin {
    /// Sign-in kind only (no token): a change asks for a refresh.
    fn router_auth_stamp(&self) -> String {
        let plan = self.secrets.oauth.is_some() || self.imagine_native.tokens.is_some();
        let key = !self.console_key().trim().is_empty();
        format!("plan={plan};key={key}")
    }

    /// Ids a default names (a ghost if no source lists them).
    fn router_named_models(&self) -> Vec<String> {
        let mut named: Vec<String> = vec![
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
                    let added = grokhub_agent::route::providers::load_providers(
                        &crate::config::config_dir(),
                    );
                    if let Some(note) = super::provider_ui::provider_refresh_note(
                        &done.events,
                        &done.errors,
                        &added,
                    ) {
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
        super::connectivity_ui::tick_connectivity(now_ms());
        self.sync_router_pin();
        self.sync_spend();
        self.poll_route_asks();
        if let Some((class, model, _)) = grokhub_agent::route::live::take_no_route() {
            self.start_model_wait(&class, &model, now_ms());
        }
        self.poll_model_wait(halted);
        if halted || self.scratch() {
            return;
        }
        let now = now_ms();
        self.tick_budget(now);
        if self
            .harness
            .router
            .last_guard
            .is_none_or(|t| t.elapsed() >= GUARD_EVERY)
            && self.harness.router.guard_rx.is_none()
        {
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
                    let verify = SpanVerifySource {
                        config_dir: dir.clone(),
                        session: String::new(),
                    };
                    let _ = tx.send(learn::tick(
                        &dir,
                        &verify,
                        &LedgerOutcomeSource::new(&dir),
                        now,
                    ));
                });
            }
        }
        let stamp = self.router_auth_stamp();
        let idle = !self.running && !self.heartbeat_busy();
        let clock = self
            .harness
            .router
            .clock
            .get_or_insert_with(|| RefreshClock::new(now));
        if clock.due(now, idle, None, &stamp).is_some() && self.harness.router.rx.is_none() {
            clock.ran(now, None, &stamp);
            self.start_model_refresh(now);
            return;
        }
        let fold_due = self
            .harness
            .router
            .last_fold
            .is_none_or(|t| t.elapsed() >= HEALTH_FOLD_EVERY);
        if fold_due && self.harness.router.rx.is_none() && self.harness.router.fold_rx.is_none() {
            self.harness.router.last_fold = Some(Instant::now());
            let dir = crate::config::config_dir();
            let (tx, rx) = mpsc::channel();
            self.harness.router.fold_rx = Some(rx);
            std::thread::spawn(move || {
                let (events, signal) = fold_health(&dir);
                let heal = heal_after(&dir, &events, &[]);
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
        let named = self.router_named_models();
        let default_model = grokhub_agent::DEFAULT_MODEL.to_string();
        let force = std::mem::take(&mut self.harness.router.force_table);
        let dir = crate::config::config_dir();
        let (tx, rx) = mpsc::channel();
        self.harness.router.rx = Some(rx);
        std::thread::spawn(move || {
            let api = bearer.clone().map(|b| XaiApiSource {
                config_dir: dir.clone(),
                bearer: b,
            });
            let mut sources: Vec<&dyn CatalogSource> = Vec::new();
            if let Some(a) = api.as_ref() {
                sources.push(a);
            }
            // Lists nothing while `localModel` is off (the default).
            sources.push(&LocalSource);
            // R3b: each provider you added lists only with its key and your grant.
            let added = grokhub_agent::route::providers::sources(&dir);
            for p in &added {
                sources.push(p);
            }
            let job = RefreshJob {
                config_dir: dir.clone(),
                sources,
                credential,
                named,
                default_model,
                now_ms: now,
            };
            let done = run_refresh(&job);
            let plan = bearer.as_deref().filter(|_| credential == Credential::Plan);
            if let Some(b) = plan {
                while probe_next(&dir, credential, grokhub_core::now_ms(), &mut |m, c| {
                    probe_model(&dir, b, m, c)
                })
                .is_some()
                {}
            }
            // Evals run only on a plan sign-in (included routes); a key rebuilds from metadata.
            let table = rebuild_table(&dir, plan, force, grokhub_core::now_ms());
            let heal = heal_after(&dir, &done.events, &table.joined);
            let _ = tx.send((done, heal));
        });
    }

    /// The accuracy guard reverts on its own and tells nothing: which effort a
    /// class starts at shows nowhere.
    fn poll_guard(&mut self) {
        let Some(rx) = self.harness.router.guard_rx.take() else {
            return;
        };
        if let Err(mpsc::TryRecvError::Empty) = rx.try_recv() {
            self.harness.router.guard_rx = Some(rx);
        }
    }

    /// Self-tuning runs on its own and asks nothing; a promotion or rollback
    /// is a ledger line, never a card or a notice.
    fn poll_tune(&mut self) {
        let Some(rx) = self.harness.router.tune_rx.take() else {
            return;
        };
        if let Err(mpsc::TryRecvError::Empty) = rx.try_recv() {
            self.harness.router.tune_rx = Some(rx);
        }
    }

    /// What the live router reads from Settings: Local AI, the prompt tidy,
    /// OpenRouter's data policy and the local tuning. It takes no model pick.
    pub(super) fn sync_router_pin(&mut self) {
        grokhub_agent::route::local::set_enabled(self.cfg.local_model);
        grokhub_agent::route::local::set_prompt_cleanup(
            self.cfg.local_model && self.cfg.prompt_cleanup,
        );
        if self.cfg.local_model {
            super::local_signals::start(crate::config::config_dir());
        }
        self.sync_provider_routing();
        self.sync_local_tuning();
    }

    /// Tell-only Home updates, through the feed, which holds them in quiet hours.
    pub(super) fn deliver_heal(&mut self, heal: HealOut) {
        self.harness
            .router
            .rows
            .retain(|(k, _)| !heal.cleared.contains(k));
        let now = now_ms();
        for m in heal.msgs {
            let card = grokhub_core::router_update_card(&m.key, &m.title, &m.text, now);
            self.post_feed_card(card);
        }
    }

    /// A provider or Local AI was just set up: list, probes and the table now.
    /// There is no button for it; the heartbeat refreshes on its own too.
    pub(super) fn refresh_models_now(&mut self) {
        self.harness.router.force_table = true;
        if let Some(clock) = self.harness.router.clock.as_mut() {
            clock.demand = true;
        }
        self.status = "Refreshing models...".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::route::heal::plan_change_msg;

    #[test]
    fn heal_updates_land_on_home_and_name_no_model() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("router-heal");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        app.deliver_heal(HealOut {
            msgs: [
                vec![plan_change_msg("SuperGrok", true)],
                joined_messages(&["grok-4.8".to_string()]),
            ]
            .concat(),
            cleared: Vec::new(),
        });
        let shown: Vec<(&str, &str)> = app
            .updates
            .iter()
            .map(|c| (c.title.as_str(), c.body.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            shown,
            vec![
                (
                    "Your plan changed",
                    "Your plan no longer includes some of the AI models it had. Auto keeps going on the ones it still has."
                ),
                (
                    "New in your plan",
                    "Your plan has a new AI model. Auto uses it where it does best."
                ),
            ]
        );
        assert!(app.notices.items.is_empty(), "nothing goes to the bell");
        assert!(app.harness.router.rows.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn several_new_models_make_one_update() {
        let msgs = joined_messages(&["b".to_string(), "a".to_string(), "b".to_string()]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            (msgs[0].key.as_str(), msgs[0].text.as_str()),
            (
                "new:a,b",
                "Your plan has 2 new AI models. Auto uses them where they do best."
            )
        );
        assert!(joined_messages(&[]).is_empty());
    }

    #[test]
    fn refresh_forces_a_table_rebuild() {
        let mut app = Cabin::quiet_for_test();
        app.harness.router.clock = Some(RefreshClock::new(1));
        app.refresh_models_now();
        assert!(app.harness.router.force_table);
        assert!(app.harness.router.clock.as_ref().unwrap().demand);
    }
}
