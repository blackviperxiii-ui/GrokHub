//! Why a turn stopped (LO-51): every halt names its cause, the stopped reply
//! ends with "*Stopped: <cause>*", and the app log records it with the chat
//! and model, so a stop is never a mystery to the user or the next turn.

use super::{Cabin, STOPPED_TURN_SEAM};

/// The closing line of a stopped reply: "*Stopped: <cause>*" when the halt
/// named one, so nobody (the model included) has to guess why it ended.
pub(super) fn stopped_seam(cause: Option<&str>) -> String {
    match cause.map(str::trim).filter(|c| !c.is_empty()) {
        Some(cause) => format!("\n\n*Stopped: {cause}*"),
        None => STOPPED_TURN_SEAM.to_string(),
    }
}

/// The cause a [`Cabin::halt_work`] status names.
pub(super) fn stop_cause_of(status: &str) -> &'static str {
    match status {
        "Redirected" => "you sent a new message",
        "Hands on — halted" => "you took the controls",
        "Voice off" => "voice was turned off",
        _ => "you stopped it",
    }
}

impl Cabin {
    /// [`Self::halt_in_flight`], with the cause its Stopped line names, so the
    /// next turn (and the user) sees why the reply ended.
    pub(super) fn halt_in_flight_for(&mut self, cause: &'static str) {
        self.halt_cause.get_or_insert(cause);
        self.halt_in_flight();
    }

    /// The cause the coming halt names, logged with the chat and model.
    pub(super) fn take_halt_cause(&mut self) -> Option<&'static str> {
        let cause = self.halt_cause.take();
        if let Some(thread) = self.chat_job_thread.as_deref() {
            eprintln!(
                "grokhub halt: cause={} thread={thread} model={}",
                cause.unwrap_or("unnamed"),
                self.run_model()
            );
        }
        cause
    }
}
