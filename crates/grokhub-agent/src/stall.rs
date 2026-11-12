//! The run's runaway guard, in place of a token cap.
//!
//! A run goes on until the task is done, however many tokens that takes. What
//! stops a loop that gets nowhere is this: model steps in a row where no tool
//! call did new work. A call that failed, was refused, or repeated one already
//! made is no work; a call that ran and came back fine is. After
//! [`REDIRECT_AFTER`] such steps the model gets one note to change approach;
//! after [`STOP_AFTER`] the run stops and says why.

/// Steps in a row with no new work before the model is told to change approach.
pub const REDIRECT_AFTER: u32 = 6;
/// Steps in a row with no new work before the run stops.
pub const STOP_AFTER: u32 = 12;

/// The note the model gets at [`REDIRECT_AFTER`].
pub const REDIRECT_NOTE: &str = "Redirect: the last several steps did no new work (every call \
failed, was refused or repeated one already made). Stop trying variations of the same thing. \
Say in one line what is blocking you, then take a different route: the app's own updater or \
installer, a built-in tool instead of the shell, or a smaller step. If only the user can unblock \
it, say exactly what you need and stop.";

/// What the chat shows when the run stops at [`STOP_AFTER`].
pub const STOP_NOTE: &str = "I stopped here: my last steps kept failing or repeating without \
getting anywhere. Say continue with a hint (or what changed) and I'll try a different way.";

/// What the run loop does after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Redirect(&'static str),
    Stop,
}

/// Steps since the run last did new work.
#[derive(Debug, Default)]
pub struct Stall {
    work_seen: usize,
    idle: u32,
    redirected: bool,
}

impl Stall {
    /// One model step that made tool calls ended; `work` is the run's count of
    /// calls so far that ran and came back fine.
    pub fn after_step(&mut self, work: usize) -> Verdict {
        if work > self.work_seen {
            self.work_seen = work;
            self.idle = 0;
            self.redirected = false;
            return Verdict::Go;
        }
        self.idle = self.idle.saturating_add(1);
        if self.idle >= STOP_AFTER {
            return Verdict::Stop;
        }
        if self.idle >= REDIRECT_AFTER && !self.redirected {
            self.redirected = true;
            return Verdict::Redirect(REDIRECT_NOTE);
        }
        Verdict::Go
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_resets_the_count_and_idle_steps_redirect_then_stop() {
        let mut stall = Stall::default();
        // Steady work never trips anything, however long the run.
        for work in 1..=500 {
            assert_eq!(stall.after_step(work), Verdict::Go);
        }
        // Five idle steps: still going.
        for _ in 0..5 {
            assert_eq!(stall.after_step(500), Verdict::Go);
        }
        // The sixth gets the one redirect, the next five go on quietly.
        assert_eq!(stall.after_step(500), Verdict::Redirect(REDIRECT_NOTE));
        for _ in 0..5 {
            assert_eq!(stall.after_step(500), Verdict::Go);
        }
        // The twelfth idle step stops the run.
        assert_eq!(stall.after_step(500), Verdict::Stop);
    }

    #[test]
    fn new_work_after_a_redirect_earns_a_fresh_redirect() {
        let mut stall = Stall::default();
        for _ in 0..5 {
            stall.after_step(0);
        }
        assert_eq!(stall.after_step(0), Verdict::Redirect(REDIRECT_NOTE));
        assert_eq!(stall.after_step(1), Verdict::Go);
        for _ in 0..5 {
            assert_eq!(stall.after_step(1), Verdict::Go);
        }
        assert_eq!(stall.after_step(1), Verdict::Redirect(REDIRECT_NOTE));
    }
}
