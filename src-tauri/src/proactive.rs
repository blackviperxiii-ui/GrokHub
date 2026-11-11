//! The shell's proactive engine (cross-platform parity with the cabin's
//! `proactive_ui.rs`). While the cabin is closed, a tick every 15 minutes
//! looks at the board, the automations and the cards already on Pulse, runs
//! the same rules (`grokhub_core::proactive`), and posts the cards the shared
//! budget allows. Nothing runs without your click: a card's action still goes
//! through the normal approvals.
//!
//! One engine at a time: the cabin wins while its scheduler beat is fresh
//! (`shell_may_fire`), and the card budget lives in `proactive.json`, the same
//! file the cabin keeps, so the two apps never double a card.
//!
//! Not read here: connector grants (calendar, mail, system state) and the
//! goal follow-ups stay with the cabin, so nothing behind a consent grant is
//! touched from this app.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::harness as hx;
use grokhub_core::proactive::{self as pro, Candidate, ProactiveBudget};
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::UpdateCard;

/// The budget file, shared with the cabin.
pub const BUDGET_FILE: &str = "proactive.json";
/// The proactive span log MindCheck reads, shared with the cabin.
const TRACE: &str = "proactive";
const MIND_LINES: usize = 2_000;
/// The engine looks this often; the budget decides what goes up.
const EVERY: Duration = Duration::from_millis(15 * 60 * 1000);
/// A short wait after launch so the window and its clock are up first.
const FIRST_LOOK: Duration = Duration::from_secs(90);
/// The budget file is a few small lists; anything bigger is not one.
const BUDGET_CAP: u64 = 256 * 1024;

/// One read-change-write of the budget at a time.
static BUDGET: Mutex<()> = Mutex::new(());

pub fn budget_path(config_dir: &Path) -> PathBuf {
    config_dir.join(BUDGET_FILE)
}

/// The saved card budget, or a fresh one.
pub fn load_budget(config_dir: &Path) -> ProactiveBudget {
    let path = budget_path(config_dir);
    if std::fs::metadata(&path).map_or(true, |m| m.len() > BUDGET_CAP) {
        return ProactiveBudget::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_budget(config_dir: &Path, budget: &ProactiveBudget) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(budget).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{BUDGET_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, budget_path(config_dir)).map_err(|e| e.to_string())
}

/// Open board cards as the engine sees them: blocked, or a report not opened.
pub fn work_items(config_dir: &Path) -> Vec<pro::WorkItem> {
    let cards = grokhub_core::work_file::read_board(&crate::workboard::path(config_dir))
        .unwrap_or_default();
    cards
        .iter()
        .filter_map(|c| {
            let blocked = c.status == grokhub_core::BoardStatus::Blocked;
            let fresh_report = c.status == grokhub_core::BoardStatus::FollowUp && c.fresh;
            (blocked || fresh_report).then(|| pro::WorkItem {
                title: c.title.clone(),
                blocked,
                fresh_report,
                due_at: None,
            })
        })
        .collect()
}

/// A topic you told us never to suggest again, or one MindCheck says you
/// might mind (an ask-first window).
struct Mind(hx::MindCheck);

impl Mind {
    fn read(config_dir: &Path) -> Self {
        let (spans, _) = hx::read_spans_tail(config_dir, TRACE, MIND_LINES);
        let mut mind = hx::MindCheck::new(Arc::new(hx::SystemClock));
        mind.extend(hx::signals_from_spans(&spans));
        Self(mind)
    }

    fn never(&self, key: &str) -> bool {
        self.0.prior(key).is_some_and(|p| p.never)
    }

    fn unsure(&self, key: &str) -> bool {
        self.0.prior(key).is_some()
            && self.0.mind_route(&hx::Candidate { key, hard: None }) == hx::MindRoute::Ask
    }
}

/// `proactive:<topic>`: the MindCheck key a proactive span on this topic folds into.
fn mind_key(topic: &str) -> String {
    format!(
        "{}:{}",
        hx::Origin::Proactive.as_str(),
        pro::topic_key(topic)
    )
}

/// The ranked candidates now, from what needs no grant.
pub fn candidates(config_dir: &Path, cards: &[UpdateCard], now_ms: u64) -> Vec<Candidate> {
    let work = work_items(config_dir);
    let health: Vec<grokhub_core::AutomationAlert> = crate::automations::load(config_dir)
        .unwrap_or_default()
        .iter()
        .filter_map(grokhub_core::automation_alert)
        .collect();
    let watch = pro::Watch {
        cards,
        work: &work,
        automation_health: &health,
        failed: &[],
        needs: &[],
    };
    pro::ProactiveEngine::new(Arc::new(move || now_ms)).gather(&watch, &[], &[])
}

/// One look. Returns the cards it posted. The cabin holding the lease, or a
/// quiet hour, posts nothing (quiet hours queue the best for later).
pub fn tick(
    config_dir: &Path,
    pid: u32,
    now_ms: u64,
    quiet: bool,
) -> Result<Vec<UpdateCard>, String> {
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now_ms) {
        return Ok(Vec::new());
    }
    let on_page = crate::pulse::cards(config_dir)?;
    let ranked = candidates(config_dir, &on_page, now_ms);
    // The budget lock is never held across a Pulse store call: a click's
    // feedback takes the store first and the budget second.
    let surfaced = {
        let _budget_lock = BUDGET.lock().unwrap_or_else(|e| e.into_inner());
        let mut budget = load_budget(config_dir);
        let surfaced = budget.surface(ranked, now_ms, quiet, false);
        save_budget(config_dir, &budget)?;
        surfaced
    };
    let mind = Mind::read(config_dir);
    let mut posted = Vec::new();
    for c in &surfaced {
        let key = mind_key(&c.topic);
        if mind.never(&key) {
            continue;
        }
        let Some(route) = pro::route(c, mind.unsure(&key)) else {
            continue;
        };
        // The cabin may have opened while this tick read its inputs.
        if !shell_may_fire(read_lease(config_dir).as_ref(), pid, grokhub_core::now_ms()) {
            break;
        }
        let card = pro::proactive_card(c, route, now_ms);
        if crate::pulse::add_post(config_dir, card.clone())? {
            posted.push(card);
        }
    }
    refund(config_dir, now_ms, surfaced.len() - posted.len());
    Ok(posted)
}

/// Only a card that reached the page costs a slot. `surface` charged one for
/// each candidate at `now_ms`; this gives back the `unposted` ones (a topic
/// you said never to suggest, a card you removed that day, the cabin opening).
fn refund(config_dir: &Path, now_ms: u64, unposted: usize) {
    if unposted == 0 {
        return;
    }
    let _lock = BUDGET.lock().unwrap_or_else(|e| e.into_inner());
    let mut budget = load_budget(config_dir);
    for _ in 0..unposted {
        if let Some(at) = budget.shown.iter().rposition(|t| *t == now_ms) {
            budget.shown.remove(at);
        }
    }
    let _ = save_budget(config_dir, &budget);
}

/// What a click on a card teaches the shared budget: Dismiss mutes the topic
/// for a day, Not this for a week, and both count toward the dismissal
/// streak; Later mutes it until the snooze ends and counts as nothing; using
/// a card resets the streak. Cards that are not proactive change nothing.
pub fn card_acted(config_dir: &Path, card: &UpdateCard, act: Feedback, now_ms: u64) {
    let Some(meta) = &card.pulse.proactive else {
        return;
    };
    let _lock = BUDGET.lock().unwrap_or_else(|e| e.into_inner());
    let mut budget = load_budget(config_dir);
    match act {
        Feedback::Dismiss => budget.dismiss(&meta.topic, now_ms),
        Feedback::NotThis => budget.not_this(&meta.topic, now_ms),
        Feedback::Later(until_ms) => {
            budget.mutes.retain(|(_, until)| now_ms < *until);
            budget.mutes.push((meta.topic.clone(), until_ms));
        }
        Feedback::Used => budget.engaged(),
    }
    let _ = save_budget(config_dir, &budget);
}

/// A click on a proactive card, as the budget hears it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    /// Dismiss, Delete or Ignore: off the page for a day.
    Dismiss,
    /// Not this or Not interested: off the page for a week.
    NotThis,
    /// Snooze: the card is hidden until this time, so its topic waits too.
    Later(u64),
    Used,
}

/// Quiet hours from Behavior, at the shell's wall clock. Equal or empty
/// hours are none.
pub fn is_quiet(behavior: &crate::settings::Behavior, hour: u32, minute: u32) -> bool {
    let parse = |s: &str| -> Option<u32> {
        let (h, m) = s.split_once(':')?;
        Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
    };
    let (Some(from), Some(to)) = (parse(&behavior.quiet_start), parse(&behavior.quiet_end)) else {
        return false;
    };
    let at = hour * 60 + minute;
    match from.cmp(&to) {
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Less => (from..to).contains(&at),
        std::cmp::Ordering::Greater => at >= from || at < to,
    }
}

/// The background thread: one look shortly after launch, then every 15
/// minutes. It waits for the window's clock like the scheduler does.
/// Desktop only: the scheduler clock it reads is not built on phones.
#[cfg(desktop)]
pub fn start(config_dir: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_LOOK);
        loop {
            if let Some(offset) = crate::scheduler::utc_offset() {
                let now = grokhub_core::now_ms();
                let clock = crate::scheduler::local_clock(now, offset);
                let behavior = crate::settings::load(&config_dir).behavior;
                let quiet = is_quiet(&behavior, clock.hour, clock.minute);
                let _ = tick(&config_dir, std::process::id(), now, quiet);
                let _ = crate::nudges::tick(&config_dir, std::process::id(), now, offset);
            }
            std::thread::sleep(EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};
    use grokhub_core::{BoardCard, BoardStatus};

    const NOW: u64 = 1_800_000_000_000;
    const MIN: u64 = 60_000;
    const PID: u32 = 4242;

    /// A test's scratch folder, removed when the test ends, pass or fail.
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-proactive-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Scratch(dir)
    }

    /// 08:01 on the test day (the wall clock at `NOW + MIN`), so Snooze
    /// hides a card until 09:00, which is `NOW + 60 * MIN`.
    fn day() -> crate::pulse::Day {
        crate::pulse::Day {
            date: "2027-01-15".into(),
            hour: 8,
            minute: 1,
        }
    }

    /// One look on the blocked board, then a click on the card it posted.
    fn posted_then(dir: &Path, act: crate::pulse::Act) -> UpdateCard {
        blocked_board(dir);
        let card = tick(dir, PID, NOW, false).expect("tick").remove(0);
        crate::pulse::act(dir, &card.id, act, &day(), NOW + MIN).expect("act");
        card
    }

    /// One blocked card on the board, as the cabin's test fixture has it.
    fn blocked_board(dir: &Path) {
        let mut card = BoardCard::new("b1", "", "");
        card.id = "b1".into();
        card.title = "Renew the domain".into();
        card.status = BoardStatus::Blocked;
        grokhub_core::work_file::write_board(&crate::workboard::path(dir), &[card])
            .expect("write board");
    }

    fn lease(dir: &Path, owner: LeaseOwner, pid: u32, beat_ms: u64) {
        write_lease(
            dir,
            &SchedulerLease {
                owner,
                pid,
                beat_ms,
            },
        )
        .expect("lease");
    }

    #[test]
    fn a_blocked_card_becomes_one_proactive_card() {
        let dir = temp_dir("blocked");
        blocked_board(&dir);
        let posted = tick(&dir, PID, NOW, false).expect("tick");
        assert_eq!(posted.len(), 1);
        let meta = posted[0].pulse.proactive.clone().expect("proactive meta");
        assert_eq!(meta.topic, "Renew the domain");
        assert_eq!(posted[0].title, "I can help unblock \"Renew the domain\"");
        assert!(posted[0].id.starts_with("proactive-renew-the-domain-"));
        let on_page = crate::pulse::cards(&dir).expect("cards");
        assert_eq!(on_page.len(), 1);
        assert_eq!(load_budget(&dir).shown, vec![NOW]);
    }

    #[test]
    fn a_fresh_cabin_lease_means_the_shell_posts_nothing() {
        let dir = temp_dir("cabin");
        blocked_board(&dir);
        lease(&dir, LeaseOwner::Cabin, 77, NOW - 30_000);
        assert!(tick(&dir, PID, NOW, false).expect("tick").is_empty());
        assert!(crate::pulse::cards(&dir).expect("cards").is_empty());
        assert!(!budget_path(&dir).exists(), "the budget stays untouched");
        // Once the beat is stale (three missed beats) the shell takes over.
        let later = NOW + 2 * MIN;
        assert_eq!(tick(&dir, PID, later, false).expect("tick").len(), 1);
    }

    #[test]
    fn a_second_tick_does_not_double_the_card() {
        let dir = temp_dir("twice");
        blocked_board(&dir);
        assert_eq!(tick(&dir, PID, NOW, false).expect("one").len(), 1);
        assert!(tick(&dir, PID, NOW + 15 * MIN, false)
            .expect("two")
            .is_empty());
        assert_eq!(crate::pulse::cards(&dir).expect("cards").len(), 1);
    }

    #[test]
    fn a_card_the_cabin_already_posted_is_not_doubled() {
        let dir = temp_dir("cabin-card");
        blocked_board(&dir);
        // The cabin ran first: same rules, same topic, its own id and time.
        let ranked = candidates(&dir, &[], NOW - 20 * MIN);
        assert_eq!(ranked.len(), 1);
        let route = pro::route(&ranked[0], false).expect("route");
        let theirs = pro::proactive_card(&ranked[0], route, NOW - 20 * MIN);
        assert!(crate::pulse::add_post(&dir, theirs).expect("post"));
        assert!(tick(&dir, PID, NOW, false).expect("tick").is_empty());
        assert_eq!(crate::pulse::cards(&dir).expect("cards").len(), 1);
    }

    #[test]
    fn quiet_hours_queue_the_card_and_post_none() {
        let dir = temp_dir("quiet");
        blocked_board(&dir);
        assert!(tick(&dir, PID, NOW, true).expect("tick").is_empty());
        assert!(crate::pulse::cards(&dir).expect("cards").is_empty());
        let budget = load_budget(&dir);
        assert_eq!(budget.queue.len(), 1);
        assert_eq!(budget.queue[0].topic, "Renew the domain");
        // The window ends: the queued card goes up.
        assert_eq!(
            tick(&dir, PID, NOW + 8 * 60 * MIN, false)
                .expect("tick")
                .len(),
            1
        );
    }

    #[test]
    fn dismissing_the_card_keeps_its_topic_off_for_a_day() {
        let dir = temp_dir("dismiss");
        blocked_board(&dir);
        let posted = tick(&dir, PID, NOW, false).expect("tick");
        card_acted(&dir, &posted[0], Feedback::Dismiss, NOW + MIN);
        let budget = load_budget(&dir);
        assert_eq!(budget.dismiss_streak, 1);
        assert!(budget.muted("Renew the domain", NOW + 2 * MIN));
        assert!(!budget.muted("Renew the domain", NOW + 25 * 60 * MIN));
        // A tick with the card gone from the page still posts nothing.
        std::fs::remove_file(dir.join("updates.json")).ok();
        assert!(tick(&dir, PID, NOW + 20 * MIN, false)
            .expect("tick")
            .is_empty());
    }

    #[test]
    fn a_deleted_or_ignored_card_does_not_come_back_on_the_next_look() {
        for act in [crate::pulse::Act::Delete, crate::pulse::Act::Ignore] {
            let dir = temp_dir("removed");
            let _config = grokhub_agent::perm::ConfigGuard::set(dir.to_path_buf());
            posted_then(&dir, act);
            let budget = load_budget(&dir);
            assert_eq!(budget.dismiss_streak, 1, "{act:?}");
            assert!(budget.muted("Renew the domain", NOW + 2 * MIN), "{act:?}");
            assert!(
                !budget.muted("Renew the domain", NOW + 25 * 60 * MIN),
                "{act:?}"
            );
            let again = tick(&dir, PID, NOW + 20 * MIN, false).expect("tick");
            assert_eq!(again, Vec::new(), "{act:?}");
            let live: Vec<UpdateCard> = crate::pulse::cards(&dir)
                .expect("cards")
                .into_iter()
                .filter(|c| c.status != grokhub_core::UpdateStatus::Dismissed)
                .collect();
            assert_eq!(live, Vec::new(), "{act:?}");
            assert_eq!(load_budget(&dir).shown, vec![NOW], "{act:?}");
        }
    }

    #[test]
    fn not_interested_mutes_the_topic_for_a_week_and_costs_no_more_budget() {
        let dir = temp_dir("dislike");
        let _config = grokhub_agent::perm::ConfigGuard::set(dir.to_path_buf());
        posted_then(&dir, crate::pulse::Act::Dislike);
        for later in [20 * MIN, 40 * MIN] {
            assert_eq!(
                tick(&dir, PID, NOW + later, false).expect("tick"),
                Vec::new()
            );
        }
        let budget = load_budget(&dir);
        assert_eq!(budget.shown, vec![NOW]);
        assert_eq!(budget.dismiss_streak, 1);
        assert!(budget.muted("Renew the domain", NOW + 6 * 24 * 60 * MIN));
        assert!(!budget.muted("Renew the domain", NOW + 8 * 24 * 60 * MIN));
    }

    #[test]
    fn a_snoozed_card_is_left_alone_until_it_is_back_and_counts_as_no_dismissal() {
        let dir = temp_dir("snooze");
        let _config = grokhub_agent::perm::ConfigGuard::set(dir.to_path_buf());
        let card = posted_then(&dir, crate::pulse::Act::Snooze);
        assert_eq!(
            tick(&dir, PID, NOW + 20 * MIN, false).expect("tick"),
            Vec::new()
        );
        let budget = load_budget(&dir);
        assert_eq!(budget.shown, vec![NOW]);
        assert_eq!(budget.dismiss_streak, 0);
        // Muted until 09:00, and not after.
        assert!(budget.muted("Renew the domain", NOW + 59 * MIN));
        assert!(!budget.muted("Renew the domain", NOW + 60 * MIN));
        let stored = crate::pulse::cards(&dir).expect("cards");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, card.id);
        assert_eq!(stored[0].runs, card.runs, "no repeat was merged into it");
    }

    #[test]
    fn a_card_set_to_remind_later_is_left_alone_until_then() {
        let dir = temp_dir("remind");
        let _config = grokhub_agent::perm::ConfigGuard::set(dir.to_path_buf());
        blocked_board(&dir);
        let card = tick(&dir, PID, NOW, false).expect("tick").remove(0);
        crate::pulse::remind(&dir, &card.id, &day(), NOW + MIN).expect("remind");
        assert_eq!(
            tick(&dir, PID, NOW + 20 * MIN, false).expect("tick"),
            Vec::new()
        );
        let budget = load_budget(&dir);
        assert_eq!(budget.shown, vec![NOW]);
        assert_eq!(budget.dismiss_streak, 0);
        assert!(budget.muted("Renew the domain", NOW + 20 * MIN));
    }

    #[test]
    fn a_card_that_does_not_land_costs_no_budget() {
        let dir = temp_dir("tombstone");
        blocked_board(&dir);
        let card = tick(&dir, PID, NOW, false).expect("tick").remove(0);
        // The cabin's Delete: a tombstone on the page, and the budget not told.
        let mut cards = crate::pulse::cards(&dir).expect("cards");
        assert!(grokhub_core::dismiss_update_at(
            &mut cards,
            &card.id,
            NOW + MIN
        ));
        std::fs::write(
            dir.join("updates.json"),
            serde_json::to_string(&cards).expect("json"),
        )
        .expect("write");
        for later in [20 * MIN, 40 * MIN] {
            assert_eq!(
                tick(&dir, PID, NOW + later, false).expect("tick"),
                Vec::new()
            );
        }
        assert_eq!(load_budget(&dir).shown, vec![NOW]);
    }

    #[test]
    fn two_dismissals_in_a_row_halve_the_budget_and_use_resets_the_streak() {
        let dir = temp_dir("streak");
        let card = {
            blocked_board(&dir);
            tick(&dir, PID, NOW, false).expect("tick").remove(0)
        };
        card_acted(&dir, &card, Feedback::Dismiss, NOW);
        card_acted(&dir, &card, Feedback::Used, NOW);
        assert_eq!(load_budget(&dir).dismiss_streak, 0);
        card_acted(&dir, &card, Feedback::NotThis, NOW);
        card_acted(&dir, &card, Feedback::Dismiss, NOW);
        let budget = load_budget(&dir);
        assert_eq!(budget.caps(NOW + MIN), (1, 4));
        assert!(budget.muted("Renew the domain", NOW + 6 * 24 * 60 * MIN));
    }

    #[test]
    fn a_card_that_is_not_proactive_teaches_nothing() {
        let dir = temp_dir("plain");
        let plain = grokhub_core::idea_card("idea-src", "A post", "Some detail", NOW);
        card_acted(&dir, &plain, Feedback::Dismiss, NOW);
        assert!(!budget_path(&dir).exists());
    }

    #[test]
    fn quiet_hours_wrap_midnight_and_empty_means_none() {
        let b = |start: &str, end: &str| crate::settings::Behavior {
            quiet_start: start.into(),
            quiet_end: end.into(),
            ..Default::default()
        };
        let night = b("22:00", "07:00");
        assert!(is_quiet(&night, 23, 30));
        assert!(is_quiet(&night, 6, 59));
        assert!(!is_quiet(&night, 7, 0));
        assert!(!is_quiet(&night, 12, 0));
        let day = b("09:00", "17:00");
        assert!(is_quiet(&day, 9, 0));
        assert!(!is_quiet(&day, 17, 0));
        assert!(!is_quiet(&b("", ""), 3, 0));
        assert!(!is_quiet(&b("08:00", "08:00"), 8, 0));
    }

    #[test]
    fn a_broken_or_oversized_budget_file_is_a_fresh_one() {
        let dir = temp_dir("broken");
        std::fs::write(budget_path(&dir), "not json").expect("write");
        assert_eq!(load_budget(&dir), ProactiveBudget::default());
        std::fs::write(budget_path(&dir), vec![b' '; (BUDGET_CAP + 1) as usize]).expect("write");
        assert_eq!(load_budget(&dir), ProactiveBudget::default());
    }
}
