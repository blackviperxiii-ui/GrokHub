use super::*;

const NOW: u64 = 1_800_000_000_000;

/// The card slot is one per app: tests that use it take turns.
static SERIAL: Mutex<()> = Mutex::new(());

struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fresh(name: &str) -> (std::sync::MutexGuard<'static, ()>, Scratch) {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    *cards() = Cards::default();
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-spend-cards-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (guard, Scratch(dir))
}

fn at(pct: u8) -> Budget {
    Budget {
        week: budget::week_of(NOW),
        pct: Some(pct),
        ..Budget::default()
    }
}

const OPEN: NudgeGate = NudgeGate {
    in_task: false,
    quiet_hours: false,
    throttled: false,
};

fn nudge_facts() -> NudgeFacts {
    NudgeFacts {
        would_upgrade: 6,
        fails: 3,
        last_failed: 2,
        last_of: 5,
    }
}

#[test]
fn app_side_eighty_percent_card_is_disabled() {
    let (_g, dir) = fresh("eighty");
    landed(&dir.0, &at(82), NudgeFacts::default(), OPEN, None, NOW);
    assert_eq!(cards().card, None);
    landed(
        &dir.0,
        &at(100),
        NudgeFacts::default(),
        OPEN,
        None,
        NOW + 1_000,
    );
    assert_eq!(cards().card, None);
}

#[test]
fn current_never_promotes_take_budget_ask() {
    let (_g, _dir) = fresh("ask");
    assert!(!grokhub_agent::route::live::take_budget_ask());
    assert_eq!(current(), None);
}

#[test]
fn the_nudge_waits_for_a_quiet_moment_and_not_now_mutes_it_for_thirty_days() {
    let (_g, dir) = fresh("nudge");
    let busy = NudgeGate {
        in_task: true,
        ..OPEN
    };
    landed(&dir.0, &at(10), nudge_facts(), busy, Some("SuperGrok"), NOW);
    assert_eq!(cards().card, None);

    landed(&dir.0, &at(10), nudge_facts(), OPEN, Some("SuperGrok"), NOW);
    let shown = current().unwrap();
    assert_eq!(shown.key, "nudge");
    assert_eq!(
        shown.title,
        "2 of your last 5 big tasks failed the check on your plan's models. SuperGrok Heavy includes more capacity for this kind of work. Want details?"
    );
    let details = answer(&dir.0, "nudge", true, NOW).unwrap().details.unwrap();
    assert!(
        details.starts_with("In the last 7 days, 6 tasks would have used a model outside your plan, and 3 checks failed"),
        "{details}"
    );

    // Shown within 30 days: not again.
    landed(
        &dir.0,
        &at(10),
        nudge_facts(),
        OPEN,
        None,
        NOW + budget::DAY_MS,
    );
    assert_eq!(cards().card, None);
    let later = NOW + budget::NUDGE_EVERY_MS + 1;
    landed(&dir.0, &at(10), nudge_facts(), OPEN, None, later);
    let muted = answer(&dir.0, "nudge", false, later).unwrap();
    assert!(
        muted.status.ends_with("won't bring this up for 30 days."),
        "{}",
        muted.status
    );
    assert_eq!(
        BudgetNotes::load(&dir.0).nudge_muted_until_ms,
        later + budget::NUDGE_EVERY_MS
    );
}

#[test]
fn account_says_only_recent_plan_refusal_never_supergrok_room_meter() {
    let room = plan_room::PlanRoom {
        requests_left: Some(1_200),
        requests_limit: Some(2_000),
        reset_secs: Some(600),
        seen_ms: NOW,
        ..Default::default()
    };
    assert_eq!(plan_note(None, Some(&room), NOW + 60_000), None);
    assert_eq!(
        plan_note(Some(NOW), Some(&room), NOW + 60_000).as_deref(),
        Some(ACCOUNT_PLAN_HINT)
    );
    // An old refusal says nothing.
    let later = NOW + plan_room::FRESH_MS + 1;
    assert_eq!(plan_note(Some(NOW), Some(&room), later), None);
    assert_eq!(plan_note(None, None, NOW), None);
}
