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
fn the_eighty_card_shows_once_a_week_and_ok_or_change_cap_answers_it() {
    let (_g, dir) = fresh("eighty");
    landed(&dir.0, &at(79), NudgeFacts::default(), OPEN, None, NOW);
    assert_eq!(cards().card, None);

    landed(&dir.0, &at(82), NudgeFacts::default(), OPEN, None, NOW);
    let shown = cards().card.as_ref().map(view).unwrap();
    assert_eq!(
        (shown.key, shown.eyebrow, shown.primary, shown.secondary),
        ("eighty", "This week's usage", "OK", "Change cap")
    );
    assert_eq!(shown.title, budget::EIGHTY_CARD);
    let ok = answer(&dir.0, "eighty", true, NOW).unwrap();
    assert_eq!(ok.status, "OK. Chats stay at normal quality.");
    assert!(!ok.open_ai_models);

    // Same week: it doesn't come back.
    landed(
        &dir.0,
        &at(90),
        NudgeFacts::default(),
        OPEN,
        None,
        NOW + 1_000,
    );
    assert_eq!(cards().card, None);

    // Next week it does; Change cap opens Settings → AI models.
    let next = Budget {
        week: budget::week_of(NOW) + 1,
        ..at(85)
    };
    landed(
        &dir.0,
        &next,
        NudgeFacts::default(),
        OPEN,
        None,
        NOW + budget::WEEK_MS,
    );
    let change = answer(&dir.0, "eighty", false, NOW + budget::WEEK_MS).unwrap();
    assert!(change.open_ai_models);
    assert_eq!(
        answer(&dir.0, "eighty", true, NOW).unwrap_err(),
        "That card is gone."
    );
}

#[test]
fn keep_going_at_one_hundred_percent_lets_chats_go_on_this_week() {
    let (_g, dir) = fresh("ask");
    cards().card = Some(Card::Ask);
    let shown = current().unwrap();
    assert_eq!(
        (shown.key, shown.primary, shown.secondary),
        ("ask", "Keep going", "Wait")
    );
    assert_eq!(shown.title, budget::BUDGET_ASK);
    assert!(BudgetNotes::load(&dir.0).must_ask(&at(100)));
    let went = answer(&dir.0, "ask", true, NOW).unwrap();
    assert_eq!(went.status, "Chats go on this week. Background work waits.");
    assert!(!BudgetNotes::load(&dir.0).must_ask(&at(100)));
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
fn account_says_the_plan_refused_within_the_hour_else_the_pools_room() {
    let room = plan_room::PlanRoom {
        requests_left: Some(1_200),
        requests_limit: Some(2_000),
        reset_secs: Some(600),
        seen_ms: NOW,
        ..Default::default()
    };
    assert_eq!(
        plan_note(None, Some(&room), NOW + 60_000).as_deref(),
        Some("SuperGrok room: 1,200 of 2,000 requests left, resets in 9 min")
    );
    assert_eq!(
        plan_note(Some(NOW), Some(&room), NOW + 60_000).as_deref(),
        Some(ACCOUNT_PLAN_HINT)
    );
    // An old refusal and an old reading say nothing.
    let later = NOW + plan_room::FRESH_MS + 1;
    assert_eq!(plan_note(Some(NOW), Some(&room), later), None);
    assert_eq!(plan_note(None, None, NOW), None);
}
