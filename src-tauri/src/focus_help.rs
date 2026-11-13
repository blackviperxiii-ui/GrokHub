//! Focus help (`grokhub_core::focus_app`), opt-in under Settings → Your PC.
//! When the window loses focus it reads, once and off the UI thread, the
//! process name of the app that took it. When the shell is back in front, a
//! known app puts one chip first in the composer row and one context line on
//! the next turn. Off reads nothing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_core::focus_app::{self, FocusApp};
use grokhub_core::QuickChip;

/// Let the other app finish taking focus before the one read.
pub const READ_DELAY: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct FocusHelp {
    was_focused: bool,
    /// The process name the last read found; taken when the shell is back.
    read: Arc<Mutex<Option<String>>>,
    /// The app the user was just in, while the shell is in front.
    app: Option<FocusApp>,
    /// The next turn carries the app's context line once.
    context_pending: bool,
}

impl FocusHelp {
    /// Each focus transition. Only edges do anything; off never reads.
    pub fn tick(&mut self, focused: bool, on: bool) {
        let was = std::mem::replace(&mut self.was_focused, focused);
        if !on || was == focused {
            return;
        }
        if focused {
            let name = self.read.lock().ok().and_then(|mut g| g.take());
            self.note_process(name.as_deref(), &[]);
        } else {
            self.app = None;
            let slot = Arc::clone(&self.read);
            std::thread::spawn(move || {
                std::thread::sleep(READ_DELAY);
                let name = grokhub_desktop::foreground::foreground_process();
                if let Ok(mut g) = slot.lock() {
                    *g = name;
                }
            });
        }
    }

    /// The shell is back in front after the user was in `process`.
    pub fn note_process(&mut self, process: Option<&str>, games: &[String]) {
        self.app = process.and_then(|p| focus_app::categorize(p, games));
        self.context_pending = self.app.is_some();
    }

    /// The focus chip while the helper is on, unless the caller filters it.
    pub fn chip(&self, on: bool) -> Option<QuickChip> {
        if !on {
            return None;
        }
        self.app.as_ref().map(FocusApp::chip)
    }

    /// The context line for the next turn, once per return to the shell.
    pub fn take_context(&mut self, on: bool) -> Option<String> {
        if !on || !std::mem::take(&mut self.context_pending) {
            return None;
        }
        self.app.as_ref().map(FocusApp::context_line)
    }

    /// Clear everything when the switch turns off.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
#[path = "focus_help_tests.rs"]
mod tests;
