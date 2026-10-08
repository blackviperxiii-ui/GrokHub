//! Router R0 in the cabin: the model registry refresh on the heartbeat, the
//! passive-health fold, the onboarding probe, and `/why`. Network and file work
//! run off the UI thread; nothing here changes what a chat sends.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use grokhub_agent::route::refresh::{fold_health, probe_next, run_refresh, RefreshDone, RefreshJob};
use grokhub_agent::route::sources::{probe_model, GrokBuildSource, XaiApiSource};
use grokhub_agent::AuthKind;
use grokhub_core::model_registry::profile::read_profiles;
use grokhub_core::model_registry::store::load_registry;
use grokhub_core::model_registry::{CatalogSource, Credential, RefreshClock};

use super::*;

/// Passive health is folded into the registry this often.
pub(super) const HEALTH_FOLD_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub(super) struct RouterUi {
    pub clock: Option<RefreshClock>,
    pub rx: Option<mpsc::Receiver<RefreshDone>>,
    pub fold_rx: Option<mpsc::Receiver<bool>>,
    pub last_fold: Option<Instant>,
    pub why_rx: Option<mpsc::Receiver<String>>,
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
                Ok(done) => {
                    if let Some(clock) = self.harness.router.clock.as_mut() {
                        clock.signal |= done.signal;
                        if done.gb_version.is_some() {
                            clock.gb_version = done.gb_version;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => self.harness.router.rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(rx) = self.harness.router.fold_rx.take() {
            match rx.try_recv() {
                Ok(signal) => {
                    if let Some(clock) = self.harness.router.clock.as_mut() {
                        clock.signal |= signal;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => self.harness.router.fold_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if halted || self.scratch() {
            return;
        }
        let now = now_ms();
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
            let (tx, rx) = mpsc::channel();
            self.harness.router.fold_rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(fold_health(&dir).1);
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
            let job = RefreshJob { config_dir: dir.clone(), sources, credential, named, default_model, now_ms: now };
            let done = run_refresh(&job);
            if let (Some(b), Credential::Plan) = (bearer.as_deref(), credential) {
                while probe_next(&dir, credential, grokhub_core::now_ms(), &mut |m, c| probe_model(&dir, b, m, c)).is_some() {}
            }
            let _ = tx.send(done);
        });
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
}
