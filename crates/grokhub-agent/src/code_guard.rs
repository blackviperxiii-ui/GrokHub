//! Per-session tool guards an app adds on top of the permission gate.
//!
//! The shell registers one for a Code tab session, so the agent's own
//! `git push` goes through the Code tab's push guard. A guard sees each tool
//! call after the gate and can refuse it, ask, or let it run without a card.
//! It never loosens a refusal, a hard-class step or an explicit ask rule.
//! Helpers (`parent/child` session ids) use their parent chat's guard.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub use crate::gate::Decision;

/// `(tool name, arguments, workspace)` to what the guard says, or `None`
/// when the call isn't its business.
pub type Guard = Arc<dyn Fn(&str, &str, &Path) -> Option<Decision> + Send + Sync>;

fn guards() -> &'static Mutex<HashMap<String, Guard>> {
    static MAP: OnceLock<Mutex<HashMap<String, Guard>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Gives `session` a guard, or takes it away with `None`.
pub fn set(session: &str, guard: Option<Guard>) {
    if session.is_empty() {
        return;
    }
    let mut map = guards().lock().unwrap_or_else(|e| e.into_inner());
    match guard {
        Some(guard) => {
            map.insert(session.to_string(), guard);
        }
        None => {
            map.remove(session);
        }
    }
}

/// The session's guard, or the nearest parent chat's.
fn find(session: &str) -> Option<Guard> {
    let map = guards().lock().unwrap_or_else(|e| e.into_inner());
    let mut id = session;
    loop {
        if let Some(guard) = map.get(id) {
            return Some(Arc::clone(guard));
        }
        id = id.rsplit_once('/')?.0;
    }
}

/// True when `session` (or its parent chat) has a guard.
pub fn is_set(session: &str) -> bool {
    !session.is_empty() && find(session).is_some()
}

/// The gate's `base`, after the session's guard.
pub fn apply(
    session: &str,
    name: &str,
    arguments: &str,
    workspace: &Path,
    policy: Option<&crate::perm::Policy>,
    base: Decision,
) -> Decision {
    let Some(guard) = find(session) else {
        return base;
    };
    let Some(said) = guard(name, arguments, workspace) else {
        return base;
    };
    match (said, base) {
        (Decision::Refuse(why), _) | (_, Decision::Refuse(why)) => Decision::Refuse(why),
        (Decision::Ask, _) => Decision::Ask,
        (Decision::Run, Decision::Run) => Decision::Run,
        (Decision::Run, Decision::Ask) => {
            let hard =
                !crate::harness::decide(crate::harness::Step::Tool { name, arguments }).is_allow();
            let asked_by_rule =
                policy.is_some_and(|p| crate::perm::explicit_ask(p, name, arguments, workspace));
            if hard || asked_by_rule {
                Decision::Ask
            } else {
                Decision::Run
            }
        }
    }
}

#[cfg(test)]
#[path = "code_guard_tests.rs"]
mod tests;
