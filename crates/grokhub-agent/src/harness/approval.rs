//! ApprovalGate: soft Ask/Auto/Always + hard class (Always cannot skip) + hard floor.
//!
//! Access is applied by the cabin when building Gate (Readonly ⇒ desktop=false).
//! This module owns hard floor / hard class ordering before soft Always.

use std::time::{Duration, Instant};

use crate::gate::{Decision, DeskFlags, Gate, PermMode};
use crate::harness::access::AccessMode;
use crate::harness::hard::{classify, HardClass, HardFloor, HardHit};

/// How long a parked hard-class card may wait before fail-closed Deny.
pub const APPROVAL_TTL: Duration = Duration::from_secs(300);

/// Outcome of the harness gate before any driver execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutcome {
    Refuse { reason: String },
    Park {
        reason: String,
        hard: Option<HardClass>,
        needs_jeremy: bool,
    },
    Allow,
}

impl GateOutcome {
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }

    pub fn needs_jeremy(&self) -> bool {
        matches!(self, Self::Park { needs_jeremy: true, .. })
    }

    pub fn as_decision(&self) -> Decision {
        match self {
            Self::Allow => Decision::Run,
            Self::Park { .. } => Decision::Ask,
            Self::Refuse { reason } => Decision::Refuse(reason.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardAnswer {
    ApproveOnce,
    ApproveSession,
    ApproveAlways,
    Deny,
}

#[derive(Debug, Clone)]
pub struct HardPark {
    pub id: String,
    pub tool: String,
    pub class: HardClass,
    pub reason: String,
    pub parked_at: Instant,
}

impl HardPark {
    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.parked_at) >= APPROVAL_TTL
    }
}

pub fn resolve_park(park: &HardPark, now: Instant, answer: Option<HardAnswer>) -> GateOutcome {
    if park.expired(now) {
        return GateOutcome::Refuse {
            reason: format!(
                "hard-class {} timed out ({}s) — fail-closed Deny",
                park.class.as_str(),
                APPROVAL_TTL.as_secs()
            ),
        };
    }
    match answer {
        Some(HardAnswer::Deny) | None => GateOutcome::Refuse {
            reason: format!("hard-class {} denied", park.class.as_str()),
        },
        Some(HardAnswer::ApproveOnce | HardAnswer::ApproveSession | HardAnswer::ApproveAlways) => {
            GateOutcome::Allow
        }
    }
}

/// Keys on a hard card. Esc denies. Enter never approves: a hard action takes a
/// click on Approve. An open overlay owns both keys.
pub fn hard_card_key(enter: bool, esc: bool, overlay_open: bool) -> Option<HardAnswer> {
    let _ = enter;
    if overlay_open || !esc {
        return None;
    }
    Some(HardAnswer::Deny)
}

/// Map Access → Gate flags. Readonly boot: computer tools absent.
pub fn apply_access(mut gate: Gate, access: AccessMode) -> Gate {
    match access {
        AccessMode::Readonly => {
            gate.desktop = false;
        }
        AccessMode::Supervised | AccessMode::Full => {
            // Desktop comes from the Settings switch ("Let Grok control the
            // desktop"). Access never turns it on by itself.
        }
    }
    gate
}

/// Host-owned gate with hard floor / hard class before soft Always.
pub fn decide_harness(
    gate: &Gate,
    name: &str,
    arguments: &str,
    latched_always: bool,
    desk: Option<DeskFlags>,
    access: AccessMode,
) -> GateOutcome {
    match classify(name, arguments) {
        HardHit::Floor(HardFloor { reason }) => {
            return GateOutcome::Refuse { reason };
        }
        HardHit::Class(class) => {
            return GateOutcome::Park {
                reason: format!(
                    "hard-class {}: {} — Always cannot skip",
                    class.as_str(),
                    class.label()
                ),
                hard: Some(class),
                needs_jeremy: true,
            };
        }
        HardHit::None => {}
    }

    if !access.allows_computer() && crate::gate::is_desktop(name) {
        return GateOutcome::Refuse {
            reason: format!(
                "Access is {} — computer tools are not available until you grant Supervised or Full",
                access.label()
            ),
        };
    }

    let soft = apply_access(*gate, access);
    match crate::gate::decide(&soft, name, latched_always, desk) {
        Decision::Run => GateOutcome::Allow,
        Decision::Ask => GateOutcome::Park {
            reason: format!("permission ask for `{name}`"),
            hard: None,
            needs_jeremy: false,
        },
        Decision::Refuse(reason) => GateOutcome::Refuse { reason },
    }
}

pub fn always_keeps_access(access_before: AccessMode, _perm: PermMode) -> AccessMode {
    access_before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_card_enter_never_approves() {
        assert_eq!(hard_card_key(true, false, false), None);
        assert_eq!(hard_card_key(false, true, false), Some(HardAnswer::Deny));
        assert_eq!(hard_card_key(true, true, false), Some(HardAnswer::Deny));
        assert_eq!(hard_card_key(false, true, true), None);
    }
    use crate::gate::PermMode;
    use crate::harness::access::AccessMode;

    fn gate(mode: PermMode, desktop: bool) -> Gate {
        Gate {
            mode,
            readonly_session: false,
            attended: true,
            desktop,
        }
    }

    #[test]
    fn readonly_hides_click() {
        let g = gate(PermMode::Always, true);
        let out = decide_harness(
            &g,
            "click",
            r#"{"x":1,"y":2}"#,
            true,
            None,
            AccessMode::Readonly,
        );
        assert_eq!(
            out,
            GateOutcome::Refuse {
                reason: "Access is Readonly — computer tools are not available until you grant Supervised or Full".into()
            }
        );
    }

    #[test]
    fn always_cannot_skip_hard_send() {
        let g = gate(PermMode::Always, true);
        let out = decide_harness(&g, "hard_send_stub", "{}", true, None, AccessMode::Full);
        assert_eq!(
            out,
            GateOutcome::Park {
                reason: "hard-class send: Send — Always cannot skip".into(),
                hard: Some(HardClass::Send),
                needs_jeremy: true,
            }
        );
    }

    #[test]
    fn hard_floor_no_ui() {
        let g = gate(PermMode::Always, true);
        let out = decide_harness(
            &g,
            "run_terminal_command",
            r#"{"command":"cat ~/.ssh/id_ed25519"}"#,
            true,
            None,
            AccessMode::Full,
        );
        assert_eq!(
            out,
            GateOutcome::Refuse {
                reason: "forbidden path: ssh keys".into()
            }
        );
    }

    #[test]
    fn soft_click_allow_under_supervised_always() {
        let g = gate(PermMode::Always, true);
        let desk = Some(DeskFlags {
            halted: false,
            locked: false,
        });
        let out = decide_harness(
            &g,
            "click",
            r#"{"x":10,"y":20}"#,
            true,
            desk,
            AccessMode::Supervised,
        );
        assert_eq!(out, GateOutcome::Allow);
    }

    #[test]
    fn ttl_fail_closed() {
        let park = HardPark {
            id: "1".into(),
            tool: "hard_send_stub".into(),
            class: HardClass::Send,
            reason: "test".into(),
            parked_at: Instant::now() - APPROVAL_TTL - Duration::from_secs(1),
        };
        let out = resolve_park(&park, Instant::now(), Some(HardAnswer::ApproveOnce));
        assert_eq!(
            out,
            GateOutcome::Refuse {
                reason: "hard-class send timed out (300s) — fail-closed Deny".into()
            }
        );
    }

    #[test]
    fn always_keeps_readonly_access() {
        let a = always_keeps_access(AccessMode::Readonly, PermMode::Always);
        assert_eq!(a, AccessMode::Readonly);
        assert!(!a.allows_computer());
    }

    #[test]
    fn apply_access_readonly_clears_desktop() {
        let g = apply_access(gate(PermMode::Ask, true), AccessMode::Readonly);
        assert!(!g.desktop);
    }
}
