//! The handle the cabin holds for one native engine session. The engine owns
//! the event sender; the cabin sends prompts, steers and answers through here.

use grokhub_core::wire::{AcpEvent, PermissionAsk};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;

/// What a send to a chat engine that is gone says, instead of the channel's
/// own "sending on a closed channel".
pub const ENGINE_GONE: &str =
    "The chat engine stopped, so that message didn't send. Your chat is kept; send it again.";

/// Held by the engine thread for as long as it runs. Dropping it (the thread
/// returned or panicked) marks the handle dead, so the cabin starts a new one.
pub struct EngineLife(Arc<AtomicBool>);

impl Drop for EngineLife {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn next_handle_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// How one permission card is answered.
enum PermAnswer {
    Allow,
    AllowAlways,
    /// You chose Deny.
    Reject,
    /// The turn is stopping or replaying, so the ask is withdrawn.
    Cancel,
}

enum Cmd {
    Prompt {
        text: String,
        image: Option<String>,
    },
    Cancel,
    Permission {
        id: Value,
        answer: PermAnswer,
    },
    Elicit {
        id: Value,
        outcome: &'static str,
        content: Option<Value>,
    },
    /// Mid-turn note.
    Steer(String),
    Shutdown,
}

/// One answer to a native permission card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePerm {
    Allow,
    Always,
    Deny,
    Cancel,
}

/// Commands the native engine receives from the cabin.
#[derive(Debug)]
pub enum ExternalCmd {
    Prompt {
        text: String,
        image: Option<String>,
    },
    Cancel,
    Steer(String),
    Permission {
        id: String,
        answer: NativePerm,
    },
    Elicit {
        id: String,
        action: String,
        content: Option<Value>,
    },
    Shutdown,
}

/// One live engine session as the cabin sees it.
pub struct AcpHandle {
    cmd: Sender<Cmd>,
    pub events: Receiver<AcpEvent>,
    pub session_id: String,
    pub cwd: PathBuf,
    /// Unique per handle: an engine for the same session started later has a
    /// new one, so an old engine's cleanup can tell it is no longer current.
    pub id: u64,
    alive: Arc<AtomicBool>,
}

fn id_text(id: Value) -> String {
    match id {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

impl AcpHandle {
    /// The engine thread is still running. False once it returned or panicked.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn send(&self, cmd: Cmd) -> Result<(), String> {
        if !self.is_alive() {
            return Err(ENGINE_GONE.into());
        }
        self.cmd.send(cmd).map_err(|_| ENGINE_GONE.to_string())
    }

    pub fn prompt(&self, text: &str) -> Result<(), String> {
        self.prompt_with_image(text, None)
    }

    pub fn prompt_with_image(&self, text: &str, image: Option<&str>) -> Result<(), String> {
        self.send(Cmd::Prompt {
            text: text.to_string(),
            image: image
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string()),
        })
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.send(Cmd::Cancel)
    }

    pub fn steer(&self, text: &str) -> Result<(), String> {
        self.send(Cmd::Steer(text.to_string()))
    }

    /// A new session. `Ready` is already queued. The caller owns the event
    /// sender and the commands that leave this process, and holds the
    /// [`EngineLife`] for as long as its engine runs.
    pub fn external(
        cwd: PathBuf,
        session_id: String,
    ) -> (
        AcpHandle,
        Receiver<ExternalCmd>,
        Sender<AcpEvent>,
        EngineLife,
    ) {
        let alive = Arc::new(AtomicBool::new(true));
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (evt_tx, evt_rx) = mpsc::channel();
        let (ext_tx, ext_rx) = mpsc::channel();
        let _ = evt_tx.send(AcpEvent::Ready {
            session_id: session_id.clone(),
        });
        thread::spawn(move || {
            for cmd in cmd_rx {
                let mapped = match cmd {
                    Cmd::Prompt { text, image } => ExternalCmd::Prompt { text, image },
                    Cmd::Cancel => ExternalCmd::Cancel,
                    Cmd::Steer(text) => ExternalCmd::Steer(text),
                    Cmd::Shutdown => {
                        let _ = ext_tx.send(ExternalCmd::Shutdown);
                        return;
                    }
                    Cmd::Permission { id, answer } => {
                        let answer = match answer {
                            PermAnswer::Allow => NativePerm::Allow,
                            PermAnswer::AllowAlways => NativePerm::Always,
                            PermAnswer::Reject => NativePerm::Deny,
                            PermAnswer::Cancel => NativePerm::Cancel,
                        };
                        ExternalCmd::Permission {
                            id: id_text(id),
                            answer,
                        }
                    }
                    Cmd::Elicit {
                        id,
                        outcome,
                        content,
                    } => ExternalCmd::Elicit {
                        id: id_text(id),
                        action: outcome.to_string(),
                        content,
                    },
                };
                if ext_tx.send(mapped).is_err() {
                    return;
                }
            }
        });
        (
            AcpHandle {
                cmd: cmd_tx,
                events: evt_rx,
                session_id,
                cwd,
                id: next_handle_id(),
                alive: Arc::clone(&alive),
            },
            ext_rx,
            evt_tx,
            EngineLife(alive),
        )
    }

    /// Allow (`true`), or withdraw the ask (`false`) because the turn is stopping.
    /// A Deny you chose goes through [`Self::reject_permission`] instead.
    pub fn answer_permission(&self, id: Value, allow: bool) -> Result<(), String> {
        let answer = if allow {
            PermAnswer::Allow
        } else {
            PermAnswer::Cancel
        };
        self.send(Cmd::Permission { id, answer })
    }

    pub fn answer_permission_always(&self, id: Value) -> Result<(), String> {
        self.send(Cmd::Permission {
            id,
            answer: PermAnswer::AllowAlways,
        })
    }

    /// Deny the ask.
    pub fn reject_permission(&self, ask: &PermissionAsk) -> Result<(), String> {
        self.send(Cmd::Permission {
            id: ask.rpc_id.clone(),
            answer: PermAnswer::Reject,
        })
    }

    pub fn answer_elicit(
        &self,
        id: Value,
        outcome: &'static str,
        content: Option<Value>,
    ) -> Result<(), String> {
        self.send(Cmd::Elicit {
            id,
            outcome,
            content,
        })
    }

    /// A handle whose engine is gone: every send fails.
    #[cfg(test)]
    pub fn dead(cwd: PathBuf, session_id: String) -> AcpHandle {
        let (cmd, _) = mpsc::channel();
        let (_, events) = mpsc::channel();
        AcpHandle {
            cmd,
            events,
            session_id,
            cwd,
            id: next_handle_id(),
            alive: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn try_recv(&self) -> Result<AcpEvent, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for AcpHandle {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_send_to_a_gone_engine_says_so_plainly() {
        let handle = AcpHandle::dead(PathBuf::from("/tmp"), "native-gone".into());
        assert!(!handle.is_alive());
        assert_eq!(handle.prompt("hi").unwrap_err(), ENGINE_GONE);
        assert_eq!(handle.steer("hi").unwrap_err(), ENGINE_GONE);
        assert_eq!(handle.cancel().unwrap_err(), ENGINE_GONE);
    }

    #[test]
    fn the_handle_dies_with_its_engine_thread() {
        let (handle, _ext, _events, life) =
            AcpHandle::external(PathBuf::from("/tmp"), "native-life".into());
        assert!(handle.is_alive());
        assert_eq!(handle.prompt("hi"), Ok(()));
        let engine = std::thread::spawn(move || {
            let _life = life;
            panic!("test: engine thread panics");
        });
        assert!(engine.join().is_err());
        assert!(!handle.is_alive());
        assert_eq!(handle.prompt("again").unwrap_err(), ENGINE_GONE);
    }
}
