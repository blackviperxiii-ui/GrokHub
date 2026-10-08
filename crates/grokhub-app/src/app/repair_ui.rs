//! Spike-8b: `/diagnose` and the "something's wrong with my computer" intent.
//! The cabin runs the read-only probes (`grokhub_agent::repair`) off the UI
//! thread and posts the plain findings as a chat line. No new chrome (D2).
//! Without the `system_state` grant no probe runs: the answer is the ask, and
//! Spike-8a's inline ask card (`ScopeAsks`) is queued in the Work tree.

use super::*;
use grokhub_agent::harness as hx;
use grokhub_agent::repair;

impl Cabin {
    /// A typed message that means "check my computer": the cabin answers it
    /// with diagnose instead of the model. True when it took the message.
    pub(super) fn try_diagnose_intent(&mut self, text: &str) -> bool {
        if self.running || self.scheduled_perm || self.harness.diagnose_rx.is_some() {
            return false;
        }
        let Some(probes) = repair::probes_for_intent(text, repair::Os::current()) else {
            return false;
        };
        self.touch();
        self.live_mut().push(("user".into(), text.to_string()));
        self.stamp_current_access();
        self.persist();
        self.run_diagnose(probes, false);
        true
    }

    /// Run `probes` (every one for this OS when empty). `slash` marks the
    /// answer as a slash result; an intent answer is a plain reply.
    pub(super) fn run_diagnose(&mut self, probes: Vec<repair::ProbeId>, slash: bool) {
        if self.harness.diagnose_rx.is_some() {
            self.status = "Already checking your computer".into();
            return;
        }
        let dir = crate::config::config_dir();
        let session = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "session".into());
        let access = self.access_mode();
        let (tx, rx) = mpsc::channel();
        self.harness.diagnose_rx = Some(rx);
        self.harness.diagnose_slash = slash;
        self.status = "Checking your computer (read only)…".into();
        std::thread::spawn(move || {
            let ledger = hx::ConsentLedger::load(&dir);
            let ctx = repair::DiagnoseCtx {
                config_dir: &dir,
                session_id: &session,
                ledger: &ledger,
                access,
                os: repair::Os::current(),
                has_bin: &repair::on_path,
                runner: &repair::run_spec,
            };
            let report = repair::diagnose(&ctx, &probes);
            let _ = tx.send((repair::report_text(&report), report.ask.is_some()));
        });
    }

    pub(super) fn poll_diagnose(&mut self) {
        let Some(rx) = self.harness.diagnose_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((body, ask)) => {
                if ask {
                    let ledger = hx::ConsentLedger::load_now(&crate::config::config_dir());
                    self.harness.indexer.asks.ask(hx::Scope::SystemState, repair::SCOPE_ASK_WHY, &ledger, now_ms());
                }
                let body = if self.harness.diagnose_slash { mark_slash_result(&body) } else { body };
                self.live_mut().push(("assistant".into(), body));
                self.status.clear();
                self.stamp_current_access();
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.diagnose_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => self.status = "Couldn't finish checking your computer".into(),
        }
    }
}
