//! Spike-6b autonomy ceiling: when GrokHub may do a small, safe, undoable
//! thing without asking, then say so on a Done-for-you card.
//!
//! Harness design §12.0 rule 2 (locked): the agent acts without asking only
//! when every term holds: soft class, reversible through a ledger undo,
//! inside a granted scope; Access allows it; the permission pill allows it;
//! MindCheck says "won't mind"; not quiet hours, not busy, not halted, and
//! under budget. On top, `confidence >= 0.8`, `p_mind < 0.2` with history
//! (no history asks), and `reversibility == 1.0`. No setting raises this
//! ceiling. Anything that misses stays on the ask-card path.
//!
//! Pure: the cabin fills [`CeilingCtx`] from its own state and the harness
//! (`harness::decide` with `Step::Proactive`) still runs after this, so both
//! must agree. [`AutoAct`] can only be built by [`AutoAct::admit`], which
//! refuses hard class first, so no auto-act exists for a hard candidate.

use serde::{Deserialize, Serialize};

/// Auto-acts allowed per local day (on top of the card budget).
pub const AUTO_PER_DAY: u32 = 5;
/// Lowest confidence that may auto-act.
pub const AUTO_CONFIDENCE_MIN: f64 = 0.8;
/// `p_mind` at or over this asks.
pub const AUTO_P_MIND_MAX: f64 = 0.2;
/// Only a fully reversible step (a ledger undo exists) may auto-act.
pub const AUTO_REVERSIBILITY: f64 = 1.0;

/// Cabin Access, as the ceiling reads it (`AccessMode` in the harness).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessTier {
    /// Watch only: nothing mutates.
    Readonly,
    Supervised,
    Full,
}

/// The Grok Build permission pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillMode {
    Ask,
    Auto,
    Always,
}

/// One thing GrokHub could do on its own. `hard` is the harness class
/// (`send`, `delete`, …, or `floor`) the cabin read for this step.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoCandidate {
    /// MindCheck key (`proactive:connection_disable`).
    pub key: String,
    pub tool: String,
    pub arguments: String,
    pub hard: Option<String>,
    /// Learning scope key this step stays inside (`system_state`).
    pub scope: Option<String>,
    /// Needs the desktop tools (click, type, …).
    pub desktop: bool,
    pub value: f64,
    pub confidence: f64,
    /// 1.0 only when the step writes through the change ledger.
    pub reversibility: f64,
    /// What the card says was done ("Turned off the notes connection").
    pub summary: String,
    /// The same step offered, read after "I can …" ("turn off the notes
    /// connection"), for the card it becomes when the ceiling misses.
    pub offer: String,
    /// Why GrokHub thought it would help, one line.
    pub why: String,
}

/// Everything the ceiling reads besides the candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CeilingCtx {
    /// The candidate's scope has an active grant.
    pub in_scope: bool,
    pub access: AccessTier,
    pub pill: PillMode,
    /// MindCheck prior for the key; `None` means no history.
    pub p_mind: Option<f64>,
    /// MindCheck routes this key to Ask (an open ask-first window).
    pub mind_asks: bool,
    pub quiet: bool,
    pub busy: bool,
    pub halted: bool,
    /// Auto-acts left today.
    pub budget_left: u32,
}

/// Which ceiling term held the candidate back, for `/why`-style text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeilingMiss {
    HardClass,
    NotInScope,
    Access,
    Pill,
    Halted,
    QuietHours,
    Busy,
    Budget,
    NoHistory,
    PMind,
    MindCheck,
    Confidence,
    Reversibility,
}

impl CeilingMiss {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HardClass => "hard_class",
            Self::NotInScope => "not_in_scope",
            Self::Access => "access",
            Self::Pill => "pill",
            Self::Halted => "halted",
            Self::QuietHours => "quiet_hours",
            Self::Busy => "busy",
            Self::Budget => "budget",
            Self::NoHistory => "no_history",
            Self::PMind => "p_mind",
            Self::MindCheck => "mindcheck",
            Self::Confidence => "confidence",
            Self::Reversibility => "reversibility",
        }
    }

    /// Why GrokHub asked instead of doing it.
    pub fn why(self) -> &'static str {
        match self {
            Self::HardClass => "It sends, pays, deletes, or touches a secret, so it always asks.",
            Self::NotInScope => "It reaches outside what you've allowed in Permissions.",
            Self::Access => "Access is Watch. Doing things on its own needs Supervised or Full.",
            Self::Pill => "The permission pill is on Ask.",
            Self::Halted => "Halt is on.",
            Self::QuietHours => "It's quiet hours.",
            Self::Busy => "You're busy right now.",
            Self::Budget => "It already did 5 things on its own today.",
            Self::NoHistory => "You haven't answered this kind of thing before.",
            Self::PMind => "You might mind: you've turned this kind of thing down.",
            Self::MindCheck => "You undid or denied this kind of thing lately, so it asks first.",
            Self::Confidence => "It isn't sure enough this would help.",
            Self::Reversibility => "It can't be undone in one click.",
        }
    }
}

/// The ceiling, term by term. `Ok` only when every term holds.
pub fn ceiling_allows(candidate: &AutoCandidate, ctx: &CeilingCtx) -> Result<(), CeilingMiss> {
    if candidate.hard.is_some() {
        return Err(CeilingMiss::HardClass);
    }
    if !ctx.in_scope || candidate.scope.as_deref().is_none_or(|s| s.trim().is_empty()) {
        return Err(CeilingMiss::NotInScope);
    }
    // Watch never mutates; desktop tools need Supervised or Full, and so
    // does any other write.
    if ctx.access == AccessTier::Readonly {
        return Err(CeilingMiss::Access);
    }
    if ctx.pill == PillMode::Ask {
        return Err(CeilingMiss::Pill);
    }
    if ctx.halted {
        return Err(CeilingMiss::Halted);
    }
    if ctx.quiet {
        return Err(CeilingMiss::QuietHours);
    }
    if ctx.busy {
        return Err(CeilingMiss::Busy);
    }
    if ctx.budget_left == 0 {
        return Err(CeilingMiss::Budget);
    }
    let Some(p_mind) = ctx.p_mind else {
        return Err(CeilingMiss::NoHistory);
    };
    if p_mind >= AUTO_P_MIND_MAX {
        return Err(CeilingMiss::PMind);
    }
    if ctx.mind_asks {
        return Err(CeilingMiss::MindCheck);
    }
    if candidate.confidence < AUTO_CONFIDENCE_MIN {
        return Err(CeilingMiss::Confidence);
    }
    if candidate.reversibility < AUTO_REVERSIBILITY {
        return Err(CeilingMiss::Reversibility);
    }
    Ok(())
}

/// A candidate that passed the ceiling. Only [`AutoAct::admit`] builds one,
/// so a hard-class candidate can never become an auto-act.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoAct {
    candidate: AutoCandidate,
}

impl AutoAct {
    pub fn admit(candidate: AutoCandidate, ctx: &CeilingCtx) -> Result<Self, CeilingMiss> {
        ceiling_allows(&candidate, ctx)?;
        Ok(Self { candidate })
    }

    pub fn candidate(&self) -> &AutoCandidate {
        &self.candidate
    }
}

/// Auto-acts left today: none in quiet hours or while busy.
/// Stand-in for the auto slice of Spike-6a's `ProactiveBudget`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoBudget {
    day: String,
    used: u32,
}

impl AutoBudget {
    pub fn left(&self, day: &str, quiet: bool, busy: bool) -> u32 {
        if quiet || busy {
            return 0;
        }
        let used = if self.day == day { self.used } else { 0 };
        AUTO_PER_DAY.saturating_sub(used)
    }

    pub fn spend(&mut self, day: &str) {
        if self.day != day {
            self.day = day.to_string();
            self.used = 0;
        }
        self.used += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soft() -> AutoCandidate {
        AutoCandidate {
            key: "proactive:connection_disable".into(),
            tool: "connection_disable".into(),
            arguments: r#"{"name":"notes"}"#.into(),
            hard: None,
            scope: Some("system_state".into()),
            desktop: false,
            value: 0.8,
            confidence: 0.9,
            reversibility: 1.0,
            summary: "Turned off the notes connection".into(),
            offer: "turn off the notes connection".into(),
            why: "It failed every start this week.".into(),
        }
    }

    fn open() -> CeilingCtx {
        CeilingCtx {
            in_scope: true,
            access: AccessTier::Full,
            pill: PillMode::Auto,
            p_mind: Some(0.0),
            mind_asks: false,
            quiet: false,
            busy: false,
            halted: false,
            budget_left: AUTO_PER_DAY,
        }
    }

    #[test]
    fn every_term_holding_admits_the_soft_candidate() {
        assert_eq!(ceiling_allows(&soft(), &open()), Ok(()));
        let act = AutoAct::admit(soft(), &open()).expect("admitted");
        assert_eq!(act.candidate().tool, "connection_disable");
    }

    #[test]
    fn each_term_off_names_its_miss() {
        let cases: Vec<(AutoCandidate, CeilingCtx, CeilingMiss)> = vec![
            (soft(), CeilingCtx { in_scope: false, ..open() }, CeilingMiss::NotInScope),
            (AutoCandidate { scope: None, ..soft() }, open(), CeilingMiss::NotInScope),
            (soft(), CeilingCtx { access: AccessTier::Readonly, ..open() }, CeilingMiss::Access),
            (soft(), CeilingCtx { pill: PillMode::Ask, ..open() }, CeilingMiss::Pill),
            (soft(), CeilingCtx { halted: true, ..open() }, CeilingMiss::Halted),
            (soft(), CeilingCtx { quiet: true, ..open() }, CeilingMiss::QuietHours),
            (soft(), CeilingCtx { busy: true, ..open() }, CeilingMiss::Busy),
            (soft(), CeilingCtx { budget_left: 0, ..open() }, CeilingMiss::Budget),
            (soft(), CeilingCtx { p_mind: Some(0.2), ..open() }, CeilingMiss::PMind),
            (soft(), CeilingCtx { p_mind: None, ..open() }, CeilingMiss::NoHistory),
            (soft(), CeilingCtx { mind_asks: true, ..open() }, CeilingMiss::MindCheck),
            (AutoCandidate { confidence: 0.79, ..soft() }, open(), CeilingMiss::Confidence),
            (AutoCandidate { reversibility: 0.5, ..soft() }, open(), CeilingMiss::Reversibility),
            (AutoCandidate { hard: Some("send".into()), ..soft() }, open(), CeilingMiss::HardClass),
        ];
        for (candidate, ctx, miss) in cases {
            assert_eq!(ceiling_allows(&candidate, &ctx), Err(miss), "{}", miss.as_str());
            assert_eq!(AutoAct::admit(candidate, &ctx), Err(miss));
        }
    }

    #[test]
    fn supervised_and_always_are_enough_but_watch_never_is() {
        let sup = CeilingCtx { access: AccessTier::Supervised, pill: PillMode::Always, ..open() };
        assert_eq!(ceiling_allows(&soft(), &sup), Ok(()));
        let desk = AutoCandidate { desktop: true, ..soft() };
        assert_eq!(ceiling_allows(&desk, &sup), Ok(()));
        assert_eq!(
            ceiling_allows(&desk, &CeilingCtx { access: AccessTier::Readonly, ..sup }),
            Err(CeilingMiss::Access)
        );
    }

    #[test]
    fn hard_class_misses_first_whatever_else_is_open() {
        let send = AutoCandidate { hard: Some("send".into()), confidence: 1.0, reversibility: 1.0, ..soft() };
        let widest = CeilingCtx { pill: PillMode::Always, ..open() };
        assert_eq!(ceiling_allows(&send, &widest), Err(CeilingMiss::HardClass));
        assert_eq!(CeilingMiss::HardClass.why(), "It sends, pays, deletes, or touches a secret, so it always asks.");
    }

    #[test]
    fn five_a_day_then_none_and_none_while_quiet_or_busy() {
        let mut budget = AutoBudget::default();
        assert_eq!(budget.left("2026-10-07", false, false), 5);
        for _ in 0..5 {
            budget.spend("2026-10-07");
        }
        assert_eq!(budget.left("2026-10-07", false, false), 0);
        assert_eq!(budget.left("2026-10-08", false, false), 5);
        assert_eq!(budget.left("2026-10-08", true, false), 0);
        assert_eq!(budget.left("2026-10-08", false, true), 0);
        budget.spend("2026-10-08");
        assert_eq!(budget.left("2026-10-08", false, false), 4);
    }
}
