//! Draft prep (LO-46): once typing pauses, read memory recall for the draft on
//! one worker thread, so Enter finds it ready. Local reads only: nothing goes
//! to a model before Enter, and library asks (sealed memory) are never
//! prepared. `"draft_prep": false` in `app.json` turns it off.

use super::*;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long typing must pause before the draft is prepared.
pub(super) const DRAFT_PREP_PAUSE: Duration = Duration::from_millis(150);

struct Job {
    workspace: PathBuf,
    text: String,
}

/// One waiting job at most: a newer draft replaces it.
#[derive(Default)]
struct Mailbox {
    job: Mutex<Option<Job>>,
    ready: Condvar,
}

#[derive(Default)]
pub(super) struct DraftPrep {
    /// When the latest edit's pause ends.
    deadline: Option<Instant>,
    mailbox: Option<Arc<Mailbox>>,
    /// Jobs handed to the worker (tests).
    handed: u64,
}

impl DraftPrep {
    /// The draft changed at `now`: wait for a pause before preparing it.
    pub(super) fn note_edit(&mut self, now: Instant) {
        self.deadline = Some(now + DRAFT_PREP_PAUSE);
    }

    /// The trimmed draft to prepare once the pause has passed, at most once
    /// per pause. `None` while waiting, when off, or when the draft is short,
    /// a slash command, or a library ask.
    pub(super) fn due(&mut self, now: Instant, enabled: bool, draft: &str) -> Option<String> {
        if !enabled {
            self.deadline = None;
            return None;
        }
        if now < self.deadline? {
            return None;
        }
        self.deadline = None;
        let text = draft.trim();
        grokhub_agent::draft_prep::eligible(text).then(|| text.to_string())
    }

    /// Time left before [`DraftPrep::due`] can answer, for a repaint.
    pub(super) fn wait_left(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|at| at.saturating_duration_since(now))
    }

    /// Hand a job to the one worker thread, replacing any job still waiting.
    fn hand(&mut self, workspace: PathBuf, text: String) {
        self.handed += 1;
        let mailbox = self.mailbox.get_or_insert_with(spawn_worker);
        *mailbox.job.lock().unwrap_or_else(|e| e.into_inner()) = Some(Job { workspace, text });
        mailbox.ready.notify_one();
    }

    #[cfg(test)]
    pub(super) fn handed(&self) -> u64 {
        self.handed
    }
}

/// The worker: takes the newest job, prepares it, waits for the next.
fn spawn_worker() -> Arc<Mailbox> {
    let mailbox = Arc::new(Mailbox::default());
    let inbox = mailbox.clone();
    // A worker that fails to start leaves jobs waiting: Enter reads afresh.
    let _ = std::thread::Builder::new()
        .name("draft-prep".into())
        .spawn(move || loop {
            let job = {
                let mut slot = inbox.job.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(job) = slot.take() {
                        break job;
                    }
                    slot = inbox.ready.wait(slot).unwrap_or_else(|e| e.into_inner());
                }
            };
            grokhub_agent::draft_prep::prepare(&job.workspace, &job.text);
        });
    mailbox
}

impl Cabin {
    /// Once per composer frame: note an edit, and hand the draft to the
    /// worker after the pause.
    pub(super) fn draft_prep_touch(&mut self, ctx: &egui::Context, edit: &egui::Response) {
        let _lap = grokhub_agent::timing::lap("ui:draft_prep");
        let now = Instant::now();
        if edit.changed() {
            self.draft_prep.note_edit(now);
        }
        if let Some(text) = self
            .draft_prep
            .due(now, self.cfg.draft_prep, &self.composer)
        {
            let workspace = self.native_workspace();
            self.draft_prep.hand(workspace, text);
        }
        if let Some(left) = self.draft_prep.wait_left(now) {
            ctx.request_repaint_after(left);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_inside_the_pause_give_one_job_after_it() {
        let mut prep = DraftPrep::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let text = "where is the save folder";
        let mut jobs = Vec::new();
        for (edit, frame) in [(0, 50), (100, 150), (200, 250)] {
            prep.note_edit(ms(edit));
            jobs.extend(prep.due(ms(frame), true, text));
        }
        assert_eq!(jobs, Vec::<String>::new(), "still typing");
        assert_eq!(prep.wait_left(ms(300)), Some(Duration::from_millis(50)));
        jobs.extend(prep.due(ms(349), true, text));
        assert_eq!(jobs, Vec::<String>::new());
        jobs.extend(prep.due(ms(350), true, text));
        jobs.extend(prep.due(ms(400), true, text));
        jobs.extend(prep.due(ms(900), true, text));
        assert_eq!(jobs, vec![text.to_string()], "one job per pause");
        assert_eq!(prep.wait_left(ms(900)), None);
    }

    #[test]
    fn off_short_slash_and_library_drafts_give_no_job() {
        let t0 = Instant::now();
        let later = t0 + Duration::from_millis(200);
        for (enabled, text) in [
            (false, "where is the save folder"),
            (true, "ab"),
            (true, "/compact"),
            (true, "what games do I have installed on steam"),
        ] {
            let mut prep = DraftPrep::default();
            prep.note_edit(t0);
            assert_eq!(prep.due(later, enabled, text), None, "{text}");
            assert_eq!(prep.wait_left(later), None, "{text}");
        }
        let mut prep = DraftPrep::default();
        prep.note_edit(t0);
        assert_eq!(
            prep.due(later, true, "  fix the parser test  "),
            Some("fix the parser test".to_string())
        );
    }

    #[test]
    fn a_newer_job_replaces_one_still_waiting() {
        let mut prep = DraftPrep::default();
        // A mailbox with no worker behind it, so nothing takes the jobs.
        prep.mailbox = Some(Arc::new(Mailbox::default()));
        prep.hand(PathBuf::from("/ws"), "first draft".into());
        prep.hand(PathBuf::from("/ws"), "second draft".into());
        assert_eq!(prep.handed(), 2);
        let waiting = prep.mailbox.as_ref().unwrap().job.lock().unwrap().take();
        assert_eq!(waiting.map(|j| j.text), Some("second draft".to_string()));
    }
}
