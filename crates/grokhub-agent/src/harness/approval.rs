//! ApprovalGate: soft Ask/Auto/Always + hard class (Always cannot skip) + hard floor.
//!
//! Access is applied by the cabin when building Gate (Readonly ⇒ desktop=false).
//! This module owns hard floor / hard class ordering before soft Always.

use std::time::{Duration, Instant};

use crate::gate::{Decision, DeskFlags, Gate, PermMode};
use crate::harness::access::AccessMode;
use crate::harness::consent::{ConsentLedger, Scope};
use crate::harness::egress::DataClass;
use crate::harness::hard::{classify, classify_ask, desk_classify, HardClass, HardFloor, HardHit};
use crate::harness::mindcheck::{proactive_outcome, MindCheck};

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
/// One step a caller wants to run, in the shape that caller has.
#[derive(Debug, Clone, Copy)]
pub enum Step<'a> {
    /// A tool call: name plus JSON arguments (native gate, headless `grok -p` cards).
    Tool { name: &'a str, arguments: &'a str },
    /// A `grokhub-desktop` `tools/call` (path A).
    Desk { tool: &'a str, args: &'a serde_json::Value },
    /// A Grok Build permission ask: card title plus action text (paths B / E).
    Ask { title: &'a str, action: &'a str },
    /// A cabin-owned outbound call (EgressGuard, Spike-4a): destination key
    /// from `egress_dest`, the user data it carries, and the consent ledger.
    Egress { dest: &'a str, data: &'a [DataClass], ledger: &'a ConsentLedger },
    /// A read of a learning scope (P5). Off unless the user granted it.
    Scope { scope: &'a Scope, ledger: &'a ConsentLedger },
    /// A Spike-9 repair step (its command plus the files it touches): the
    /// shell floor and hard class, then the repair floor and repair table.
    Repair { command: &'a str },
    /// A proactive candidate (Spike-5a): a tool call plus its MindCheck key.
    /// Floor and hard class come first and ignore the prior; only a soft
    /// call reads MindCheck, which parks a soft card unless it may auto.
    Proactive { name: &'a str, arguments: &'a str, key: &'a str, mind: &'a MindCheck },
    /// A premium route (Router R2b): `service_tier: "priority"`, the US
    /// endpoint, 16-agent `xhigh`, or a model over your $/M ceiling. Money is
    /// hard class: allowed only under your grant for exactly this route key,
    /// else a hard money card, even under Always, YOLO or Full.
    Premium { route_key: &'a str, ledger: &'a ConsentLedger },
    /// A call to a provider you added with your own key (Router R3b): its
    /// destination key and the data the call carries. Unlike [`Step::Egress`]
    /// chat alone is not enough: it goes only under your destination grant
    /// that covers every class, else it is hard class send, even under Always.
    Provider { dest: &'a str, data: &'a [DataClass], ledger: &'a ConsentLedger },
}

/// The single entry for the hard floor and the hard class. Every caller asks
/// here: native gate, desktop MCP, ACP asks, headless cards, and later ones.
/// Refuse = floor, Park = hard class (Always cannot skip), Allow = soft, left
/// to Grok Build's own Ask / Auto / Always.
pub fn decide(step: Step<'_>) -> GateOutcome {
    let hit = match step {
        Step::Egress { dest, data, ledger } => {
            return crate::harness::egress::check(dest, data, ledger).0;
        }
        Step::Premium { route_key, ledger } => {
            if ledger.locked().is_none() && ledger.premium_grant(route_key).is_some() {
                return GateOutcome::Allow;
            }
            HardHit::Class(HardClass::Money)
        }
        Step::Provider { dest, data, ledger } => {
            if ledger.locked().is_none() && ledger.destination_grant(dest, data).is_some() {
                return GateOutcome::Allow;
            }
            let classes: Vec<&str> = data.iter().map(|c| c.as_str()).collect();
            let class = HardClass::Send;
            return GateOutcome::Park {
                reason: format!(
                    "hard-class {}: {} to {dest} ({}) with no grant — Always cannot skip",
                    class.as_str(),
                    class.label(),
                    if classes.is_empty() { "model list".to_string() } else { classes.join(", ") }
                ),
                hard: Some(class),
                needs_jeremy: true,
            };
        }
        Step::Scope { scope, ledger } => {
            return match ledger.scope_grant(scope) {
                Some(_) => GateOutcome::Allow,
                None => GateOutcome::Refuse {
                    reason: format!("scope {} is off — only your click in Settings turns it on", scope.key()),
                },
            };
        }
        Step::Tool { name, arguments } => classify(name, arguments),
        Step::Desk { tool, args } => desk_classify(tool, args),
        Step::Ask { title, action } => classify_ask(title, action),
        Step::Repair { command } => crate::repair::repair_hit(command),
        Step::Proactive { name, arguments, key, mind } => match classify(name, arguments) {
            HardHit::None => return proactive_outcome(name, key, mind),
            hit => hit,
        },
    };
    match hit {
        HardHit::Floor(HardFloor { reason }) => GateOutcome::Refuse { reason },
        HardHit::Class(class) => GateOutcome::Park {
            reason: format!("hard-class {}: {} — Always cannot skip", class.as_str(), class.label()),
            hard: Some(class),
            needs_jeremy: true,
        },
        HardHit::None => GateOutcome::Allow,
    }
}

pub fn decide_harness(
    gate: &Gate,
    name: &str,
    arguments: &str,
    latched_always: bool,
    desk: Option<DeskFlags>,
    access: AccessMode,
) -> GateOutcome {
    let hard = decide(Step::Tool { name, arguments });
    if !hard.is_allow() {
        return hard;
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
    fn decide_is_one_verdict_for_every_step_shape() {
        let rm = serde_json::json!({ "text": "rm -rf ~/old" });
        let shapes = [
            decide(Step::Tool { name: "run_terminal_command", arguments: r#"{"command":"rm -rf ~/old"}"# }),
            decide(Step::Desk { tool: "type", args: &rm }),
            decide(Step::Ask { title: "bash", action: "rm -rf ~/old" }),
        ];
        for got in shapes {
            assert_eq!(
                got,
                GateOutcome::Park {
                    reason: "hard-class delete: Delete — Always cannot skip".into(),
                    hard: Some(HardClass::Delete),
                    needs_jeremy: true,
                }
            );
        }
        assert_eq!(
            decide(Step::Ask { title: "bash", action: "rm -rf /" }),
            GateOutcome::Refuse { reason: "hard floor: rm -rf /".into() }
        );
        assert_eq!(decide(Step::Ask { title: "bash", action: "ls" }), GateOutcome::Allow);
    }

    #[test]
    fn scopes_are_off_until_granted() {
        let none = ConsentLedger::empty();
        assert_eq!(
            decide(Step::Scope { scope: &Scope::Calendar, ledger: &none }),
            GateOutcome::Refuse {
                reason: "scope calendar is off — only your click in Settings turns it on".into()
            }
        );
        assert_eq!(
            decide(Step::Scope { scope: &Scope::Files("/home/me/Documents".into()), ledger: &none }),
            GateOutcome::Refuse {
                reason: "scope files:/home/me/Documents is off — only your click in Settings turns it on".into()
            }
        );
    }

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

    #[test]
    fn a_premium_route_is_a_hard_money_card_until_your_click_grants_exactly_that_route() {
        let dir = crate::harness::test_dir("approval-premium");
        let money = GateOutcome::Park {
            reason: "hard-class money: Purchase / money — Always cannot skip".into(),
            hard: Some(HardClass::Money),
            needs_jeremy: true,
        };
        let ledger = ConsentLedger::load(&dir);
        assert_eq!(decide(Step::Premium { route_key: "premium:grok-heavy", ledger: &ledger }), money);
        let g = crate::harness::grant_premium(&dir, "premium:grok-heavy", crate::harness::UserClick::from_click()).unwrap();
        assert_eq!((g.source.as_str(), g.scope.as_str(), g.by.as_str()), ("premium:grok-heavy", "spend", "user"));
        let ledger = ConsentLedger::load(&dir);
        assert_eq!(decide(Step::Premium { route_key: "premium:grok-heavy", ledger: &ledger }), GateOutcome::Allow);
        assert_eq!(decide(Step::Premium { route_key: "premium:grok-heavy+priority", ledger: &ledger }), money);
        // Revoking it in Settings → Permissions brings the card back.
        assert_eq!(crate::harness::revoke_grant(&dir, &g.id), Ok(true));
        let ledger = ConsentLedger::load(&dir);
        assert_eq!(decide(Step::Premium { route_key: "premium:grok-heavy", ledger: &ledger }), money);
        assert_eq!(
            crate::harness::grant_premium(&dir, "hub", crate::harness::UserClick::from_click()).unwrap_err(),
            "a premium grant needs a premium route"
        );
        // The card takes a click: Enter never approves it, Esc denies, and the TTL denies.
        assert_eq!(hard_card_key(true, false, false), None);
        assert_eq!(hard_card_key(false, true, false), Some(HardAnswer::Deny));
        let park = HardPark { id: "p".into(), tool: "route".into(), class: HardClass::Money, reason: String::new(), parked_at: Instant::now() };
        let later = park.parked_at + APPROVAL_TTL;
        assert!(matches!(resolve_park(&park, later, Some(HardAnswer::ApproveOnce)), GateOutcome::Refuse { .. }));
        let _ = std::fs::remove_dir_all(dir);
    }
}
