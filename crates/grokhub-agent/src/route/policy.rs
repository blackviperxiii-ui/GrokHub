//! The §14.3 class table, as data. R0 only reads it to say what the router
//! *would* pick (shadow); R1 turns it on. Effort rungs are names on
//! [`grokhub_core::model_registry::EFFORT_LADDER`].

/// R0: the table shapes the shadow route only. No live call reads it.
pub const POLICY_LIVE: bool = false;

/// One class: where effort starts, and the band it may move in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassRow {
    pub class: &'static str,
    pub start: &'static str,
    pub floor: &'static str,
    pub ceiling: &'static str,
    /// Plain words for the reason sentence.
    pub plain: &'static str,
}

const fn row(class: &'static str, start: &'static str, floor: &'static str, ceiling: &'static str, plain: &'static str) -> ClassRow {
    ClassRow { class, start, floor, ceiling, plain }
}

/// start / floor / ceiling per class (§14.3).
pub const CLASS_TABLE: &[ClassRow] = &[
    row("background:classify", "low", "none", "medium", "background sorting"),
    row("background:triage", "low", "none", "medium", "background triage"),
    row("background:redact", "low", "none", "medium", "background redaction"),
    row("background:summarize", "low", "none", "medium", "a background summary"),
    row("background:judge", "low", "low", "high", "an independent check"),
    row("background:compact", "low", "low", "medium", "folding old context"),
    row("background:memory", "low", "low", "medium", "memory upkeep"),
    row("background:dream", "low", "low", "medium", "the nightly memory tidy"),
    row("chat:quick", "low", "none", "high", "a quick answer"),
    row("chat:default", "medium", "low", "xhigh", "everyday chat"),
    row("code:edit-small", "medium", "low", "xhigh", "a small code edit"),
    row("plan", "high", "medium", "xhigh", "planning"),
    row("design", "high", "medium", "xhigh", "design work"),
    row("code:multi-file", "high", "medium", "xhigh", "a multi-file change"),
    row("debug", "high", "medium", "xhigh", "debugging"),
    row("desktop:soft", "medium", "low", "high", "a desktop step"),
    row("prepare:hard", "high", "high", "xhigh", "preparing a hard action"),
    row("repair:diagnose", "medium", "low", "high", "a computer check"),
];

/// Call-site names that already exist, mapped onto table classes.
pub const CLASS_ALIASES: &[(&str, &str)] = &[
    ("episode:step", "desktop:soft"),
    ("background:review", "background:judge"),
    ("chat:turn", "chat:default"),
];

/// The table row for a call class (after aliases), or `None` for a class the
/// table doesn't list (the route then keeps today's effort).
pub fn class_row(class: &str) -> Option<&'static ClassRow> {
    let c = class.trim();
    let c = CLASS_ALIASES.iter().find(|(from, _)| *from == c).map(|(_, to)| *to).unwrap_or(c);
    CLASS_TABLE.iter().find(|r| r.class == c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::EFFORT_LADDER;

    fn rung(e: &str) -> usize {
        EFFORT_LADDER.iter().position(|l| *l == e).unwrap()
    }

    #[test]
    fn every_row_is_on_the_ladder_and_floor_le_start_le_ceiling() {
        assert_eq!(CLASS_TABLE.len(), 18);
        for r in CLASS_TABLE {
            assert!(rung(r.floor) <= rung(r.start) && rung(r.start) <= rung(r.ceiling), "{}", r.class);
        }
        let hard = class_row("prepare:hard").unwrap();
        assert_eq!((hard.start, hard.floor, hard.ceiling), ("high", "high", "xhigh"));
        assert_eq!(class_row("episode:step").unwrap().class, "desktop:soft");
        assert_eq!(class_row("eval:item"), None);
        const { assert!(!POLICY_LIVE) };
    }
}
