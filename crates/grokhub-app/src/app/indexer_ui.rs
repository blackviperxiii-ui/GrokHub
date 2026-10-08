//! Spike-8a: the local indexers in the cabin.
//!
//! The heartbeat's Housekeep slot calls [`Cabin::tick_indexers`]: one
//! `Scheduler::tick` at a time on a low-priority worker, never while halted or
//! in scratch. The worker hands back the scheduler and, after a write or an
//! unlock, a fresh in-memory [`ScopeIndex`]. Settings → Permissions lists what
//! each granted scope taught GrokHub with a "Forget these" ghost pill, and a
//! queued `ScopeAsks` entry paints as an inline Allow card in the Work tree.
//! Only the click on that card grants (through `grant_scope_click`, the one caller of
//! `grant_scope`); asking never does.

use super::*;
use grokhub_agent::harness as hx;
use grokhub_agent::indexers::{self as idx, IndexedFact, ScopeIndex, Scheduler, TickOutcome};

/// Lines of learned facts shown under a scope row before "and N more".
const FACTS_SHOWN: usize = 3;
pub(super) const FORGET_THESE: &str = "Forget these";
const ASK_EYEBROW: &str = "Learn from your computer";
const ASK_NOTE: &str = "Read on this computer only and sealed at rest. Revoke it any time in Settings → Permissions.";

/// What the worker hands back after one tick.
#[derive(Debug)]
struct TickDone {
    sched: Scheduler,
    outcome: TickOutcome,
    index: Option<ScopeIndex>,
}

/// A finished "Forget these": the scope's label, how many, the index after.
type ForgetDone = (String, Result<usize, String>, ScopeIndex);

#[derive(Debug, Default)]
pub(super) struct IndexerUi {
    sched: Scheduler,
    rx: Option<mpsc::Receiver<TickDone>>,
    /// What the indexers learned, by scope key. Shared with the Settings paint.
    pub index: Arc<ScopeIndex>,
    /// The index has not been read since launch or since an unlock.
    loaded: bool,
    pub last: Option<TickOutcome>,
    forget_rx: Option<mpsc::Receiver<ForgetDone>>,
    /// In-context scope asks (Spike-6a's engine pushes them).
    pub asks: idx::ScopeAsks,
}

/// The scope rows' learned-facts block. Returns true on a Forget these click.
fn paint_facts(ui: &mut egui::Ui, source: &str, facts: &[IndexedFact], locked: bool, busy: bool) -> bool {
    let reader = hx::Scope::parse(source).is_some_and(|s| idx::has_reader(&s));
    if !reader {
        let what = if source == "calendar" { "calendar" } else { "mail" };
        crate::cards::settings_note(ui, &format!("Nothing reads it yet: GrokHub has no {what} reader of its own."));
        return false;
    }
    if facts.is_empty() {
        crate::cards::settings_note(ui, "Nothing learned to show. GrokHub reads it on a heartbeat, never on battery or in quiet hours.");
        return false;
    }
    let n = facts.len();
    let head = if n == 1 { "Learned 1 thing, sealed on this computer:".to_string() } else { format!("Learned {n} things, sealed on this computer:") };
    ui.label(RichText::new(head).size(13.0).color(crate::theme::fg()));
    for f in facts.iter().take(FACTS_SHOWN) {
        ui.add(egui::Label::new(RichText::new(&f.line).size(12.0).color(crate::theme::muted())).truncate());
    }
    if n > FACTS_SHOWN {
        ui.label(RichText::new(format!("and {} more", n - FACTS_SHOWN)).size(12.0).color(crate::theme::muted()));
    }
    ui.add_space(4.0);
    let hit = ui.add_enabled_ui(!locked && !busy, |ui| crate::cards::ghost_pill(ui, FORGET_THESE)).inner;
    ui.add_space(8.0);
    hit
}

impl Cabin {
    /// Heartbeat: collect a finished tick, or start the next one.
    pub(super) fn tick_indexers(&mut self) {
        let ui = &mut self.harness.indexer;
        if let Some(rx) = ui.rx.take() {
            match rx.try_recv() {
                Ok(done) => {
                    ui.sched = done.sched;
                    if let Some(index) = done.index {
                        ui.index = Arc::new(index);
                        ui.loaded = true;
                    }
                    if matches!(done.outcome, TickOutcome::Paused(_)) {
                        // Locked: show nothing learned, read it again at unlock.
                        ui.index = Arc::default();
                        ui.loaded = false;
                    }
                    ui.last = Some(done.outcome);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.rx = Some(rx);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if self.scratch() {
            return;
        }
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let ui = &mut self.harness.indexer;
        let mut sched = std::mem::take(&mut ui.sched);
        let reload = !ui.loaded;
        let (tx, rx) = mpsc::channel();
        ui.rx = Some(rx);
        let dir = crate::config::config_dir();
        std::thread::spawn(move || {
            idx::power::lower_thread_priority();
            let dirs = idx::paths::PlatformDirs::from_env();
            let env = idx::TickEnv {
                config_dir: &dir,
                now_ms: now_ms(),
                quiet,
                power: &idx::power::OsPower,
                fs: &idx::fs::RealFs,
                probe: &idx::system_state::OsProbe,
                dirs: &dirs,
            };
            let outcome = sched.tick(&env);
            let wrote = matches!(outcome, TickOutcome::Ran { written, .. } if written > 0);
            let paused = matches!(outcome, TickOutcome::Paused(_));
            let index = ((reload || wrote) && !paused).then(|| ScopeIndex::load(&dir));
            let _ = tx.send(TickDone { sched, outcome, index });
        });
    }

    /// The oldest scope ask as a Work-tree card. Click only; no answer in
    /// `APPROVAL_TTL` means Not now.
    pub(super) fn paint_scope_asks(&mut self, ui: &mut egui::Ui) {
        self.harness.indexer.asks.expire(now_ms());
        let Some(ask) = self.harness.indexer.asks.first().cloned() else {
            return;
        };
        let title = format!("Allow {}?", super::scope_ui::scope_label(&ask.scope.key()));
        let text = super::harness_ui::CardText {
            eyebrow: ASK_EYEBROW,
            title: &title,
            action: &ask.why,
            note: ASK_NOTE,
            primary: "Allow",
            secondary: "Not now",
            hard: false,
        };
        let id = ("scope-ask", ask.scope.key());
        let top = ui.cursor().min.y;
        let answer = super::harness_ui::harness_card(ui, id, text, self.running);
        self.scroll_if_jumped(ui, "scope", top);
        // A grant takes a pointer click: Enter or Space on a focused Allow does nothing.
        let pointer = ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary));
        if let Some(allow) = answer.filter(|allow| !allow || pointer) {
            self.harness.indexer.asks.remove(&ask.scope);
            if allow {
                self.grant_scope_click(&ask.scope);
            } else {
                self.status = format!("{} stays off.", super::scope_ui::scope_label(&ask.scope.key()));
            }
        }
    }

    /// Under a granted scope row in Settings: what it taught GrokHub, and
    /// "Forget these". Returns the fact ids to forget on a click.
    pub(super) fn ui_scope_facts(&mut self, ui: &mut egui::Ui, source: &str, locked: bool) -> Option<Vec<String>> {
        self.poll_forget();
        let index = self.harness.indexer.index.clone();
        let facts = index.for_scope(source);
        let busy = self.harness.indexer.forget_rx.is_some();
        paint_facts(ui, source, facts, locked, busy).then(|| facts.iter().map(|f| f.id.clone()).collect())
    }

    /// "Forget these": tombstone the listed facts off the UI thread, then
    /// read the index again.
    pub(super) fn forget_scope_facts(&mut self, source: &str, ids: Vec<String>) {
        if self.harness.indexer.forget_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.harness.indexer.forget_rx = Some(rx);
        let dir = crate::config::config_dir();
        let label = super::scope_ui::scope_label(source);
        std::thread::spawn(move || {
            let got = idx::forget_facts(&dir, &ids);
            let _ = tx.send((label, got, ScopeIndex::load(&dir)));
        });
    }

    pub(super) fn poll_forget(&mut self) {
        let Some(rx) = self.harness.indexer.forget_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((label, got, index)) => {
                self.status = match got {
                    Ok(n) => format!("Forgot {n} things learned from {label}. It stays allowed; revoke it to stop reading."),
                    Err(e) => format!("Could not forget: {e}"),
                };
                self.harness.indexer.index = Arc::new(index);
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.indexer.forget_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }
}
