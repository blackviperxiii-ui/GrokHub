use super::*;
use grokhub_core::UpdateKind;

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

fn scratch(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-done-for-you-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

const PID: u32 = 4242;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// The wall clock: the spans these tests write carry it too.
fn now() -> u64 {
    grokhub_core::now_ms()
}

fn job(id: &str, name: &str, enabled: bool, streak: u32, error: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "schedule": "daily",
        "time": "09:00",
        "instructions": format!("{name} for me"),
        "enabled": enabled,
        "health": {"outcome": "failed", "error": error, "failStreak": streak}
    })
}

fn write_jobs(dir: &Path, jobs: &[serde_json::Value]) {
    std::fs::write(
        dir.join("automations.json"),
        serde_json::to_string_pretty(jobs).unwrap(),
    )
    .unwrap();
}

fn rows(dir: &Path) -> Vec<Automation> {
    crate::automations::load(dir).unwrap()
}

fn enabled(dir: &Path, id: &str) -> bool {
    rows(dir).iter().find(|a| a.id == id).unwrap().enabled
}

fn approvals(dir: &Path, a: crate::settings::Approvals) {
    let mut s = crate::settings::load(dir);
    s.approvals = a;
    crate::settings::save(dir, &s).unwrap();
}

fn approve_once(dir: &Path) {
    hx::note_proactive(
        dir,
        &hx::proactive_span(
            TOOL,
            r#"{"id":"auto-earlier"}"#,
            hx::DECISION_APPROVE,
            "",
            AccessMode::Supervised,
        ),
    );
}

/// Switch on, Approvals "Don't ask", one approval: nothing holds a pass back.
fn open(dir: &Path) {
    set_on(dir, true).unwrap();
    approvals(dir, crate::settings::Approvals::Always);
    approve_once(dir);
}

fn spans(dir: &Path) -> Vec<hx::Span> {
    hx::read_spans(dir, hx::PROACTIVE_TRACE).unwrap()
}

fn decisions(dir: &Path) -> Vec<(String, String)> {
    spans(dir)
        .iter()
        .map(|s| (s.decision.clone(), s.claim.clone()))
        .collect()
}

fn tick(dir: &Path, quiet: bool, busy: bool) -> Option<UpdateCard> {
    tick_one(dir, PID, now(), 0, quiet, busy, true)
}

fn card(dir: &Path, id: &str) -> UpdateCard {
    crate::pulse::cards(dir)
        .unwrap()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
}

fn ledger(dir: &Path) -> Vec<hx::Change> {
    hx::ChangeLedger::load_kind(dir, ChangeKind::Automation)
        .all()
        .to_vec()
}

fn drain(dir: &Path) {
    let _ = grokhub_agent::take_automation_changes();
    let _ = hx::take_self_changes(dir);
}

fn auto(id: &str, enabled: bool, streak: u32, error: &str) -> Automation {
    serde_json::from_value(job(id, "Job", enabled, streak, error)).unwrap()
}

fn clock() -> grokhub_core::LocalClock {
    grokhub_core::LocalClock {
        now_ms: now(),
        weekday: 1,
        hour: 9,
        minute: 0,
    }
}

/// The Pause it chips on the page, by card id.
fn pause_chips(dir: &Path) -> Vec<String> {
    let page = crate::pulse::peek(dir, now()).unwrap();
    let feed = page.feed.iter().map(|p| (&p.id, &p.chips));
    let ideas = page
        .ideas
        .iter()
        .flat_map(|g| g.ideas.iter().map(|i| (&i.id, &i.chips)));
    feed.chain(ideas)
        .filter(|(_, chips)| chips.iter().any(|c| c.act == "pause"))
        .map(|(id, _)| id.clone())
        .collect()
}

/// One job's row in the file, changed in place, as a run or another part of
/// the app would leave it (no ledger line).
fn edit_row(dir: &Path, id: &str, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = crate::automations::path(dir);
    let mut jobs: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edit(jobs.iter_mut().find(|j| j["id"] == json!(id)).unwrap());
    write_jobs(dir, &jobs);
}

/// `n` more runs that failed.
fn fail_more(dir: &Path, id: &str, n: u32) {
    edit_row(dir, id, |row| {
        let streak = row["health"]["failStreak"].as_u64().unwrap_or(0) as u32;
        let runs = row["runCount"].as_u64().unwrap_or(0) as u32;
        row["health"]["failStreak"] = json!(streak + n);
        row["runCount"] = json!(runs + n);
    });
}

fn paused(id: &str, runs: u32) -> Paused {
    Paused {
        id: id.into(),
        seq: 1,
        runs,
        key: "proactive:automation_disable".into(),
        answered: true,
    }
}

fn ran(mut a: Automation, runs: u32) -> Automation {
    a.run_count = runs;
    a
}

#[test]
fn the_switch_starts_off_and_a_pass_then_reads_and_writes_nothing() {
    let d = scratch("off");
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    approvals(&d, crate::settings::Approvals::Always);
    approve_once(&d);
    assert!(!on(&d));
    let jobs = std::fs::read(d.join("automations.json")).unwrap();
    let span_log = std::fs::read(hx::span_path(&d, hx::PROACTIVE_TRACE)).unwrap();
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(std::fs::read(d.join("automations.json")).unwrap(), jobs);
    assert_eq!(
        std::fs::read(hx::span_path(&d, hx::PROACTIVE_TRACE)).unwrap(),
        span_log
    );
    assert!(!d.join("updates.json").exists());
    assert!(!d.join(STATE_FILE).exists());
    assert_eq!(held(&d, now(), 0), None);
}

#[test]
fn only_a_running_job_failing_5_times_for_its_own_reason_can_be_paused() {
    let a6 = auto("auto-a", true, 6, "x");
    let b5 = auto("auto-b", true, 5, "y");
    let list = [a6.clone(), b5.clone()];
    assert_eq!(
        pausable(&list, true, &[]).map(|a| a.id.as_str()),
        Some("auto-a")
    );
    assert_eq!(pausable(&[auto("auto-a", true, 4, "x")], true, &[]), None);
    // Ties go by id.
    let c6 = auto("auto-c", true, 6, "z");
    assert_eq!(
        pausable(&[c6, a6.clone()], true, &[]).map(|a| a.id.as_str()),
        Some("auto-a")
    );
    // An error two failing jobs share is not about either job.
    let missing = "notes.md was not found";
    let shared = [
        auto("auto-a", true, 6, missing),
        auto("auto-b", true, 1, missing),
    ];
    assert_eq!(pausable(&shared, true, &[]), None);
    // A key, credit, rate or network error fails every job, even when only
    // one job has run into it so far. These are the lines the run loop and
    // the model client write.
    for error in [
        grokhub_agent::BUSY_GAVE_UP,
        "transport: error sending request for url (https://openrouter.ai/api/v1/chat)",
        "model stopped responding",
        "stream disconnected before it completed",
        "API key rejected (HTTP 401): invalid key",
        "sign-in was rejected (HTTP 403): expired",
        "rate limited: slow down",
        "HTTP 503: upstream connect error",
        "HTTP 402: payment required",
        grokhub_agent::xai_keychain::EXPIRED,
        grokhub_core::XAI_NEED_SIGNIN,
        "The system keychain is not available: no secret service",
        "https://auth.example/token: status code 503",
        "transport: dns failed for api.example",
        "OpenRouter has no credits left.",
        "429 Too Many Requests",
        "request timed out",
        "503 Service Unavailable",
    ] {
        assert_eq!(
            pausable(&[auto("auto-a", true, 9, error)], true, &[]),
            None,
            "{error}"
        );
    }
    // Words and numbers that only look like one are the job's own errors.
    for error in [
        "HTTP 400: maximum context length is 131072 tokens, you requested 150043 tokens",
        "The credit card statement PDF has no pages",
        "Couldn't design in the sheet: tab Totals is missing",
        "localhost:5000 returned an empty page",
    ] {
        assert_eq!(
            pausable(&[auto("auto-a", true, 9, error)], true, &[]).map(|a| a.id.as_str()),
            Some("auto-a"),
            "{error}"
        );
    }
    assert_eq!(pausable(&[auto("auto-a", false, 9, "x")], true, &[]), None);
    assert_eq!(pausable(&list, false, &[]), None);
    // An id the ledger can't name as written is left alone.
    assert_eq!(pausable(&[auto("auto a", true, 9, "x")], true, &[]), None);
    // Paused at run 20 and on again: held until 5 more runs, whatever the
    // streak says, then free, also when it ran fine in between and the new
    // streak is back at 5.
    let once = [paused("auto-a", 20)];
    let a = |streak, runs| ran(auto("auto-a", true, streak, "x"), runs);
    assert_eq!(pausable(&[a(6, 20)], true, &once), None);
    assert_eq!(pausable(&[a(10, 24)], true, &once), None);
    assert_eq!(
        pausable(&[a(11, 25)], true, &once).map(|a| a.id.as_str()),
        Some("auto-a")
    );
    assert_eq!(
        pausable(&[a(5, 31)], true, &once).map(|a| a.id.as_str()),
        Some("auto-a")
    );
    // Another job's pause holds nothing back.
    assert_eq!(
        pausable(&[a(6, 20)], true, &[paused("auto-b", 20)]).map(|a| a.id.as_str()),
        Some("auto-a")
    );
}

#[test]
fn an_open_pass_pauses_the_job_and_posts_a_done_card_with_its_ledger_line() {
    let d = scratch("admit");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(
        &d,
        &[
            job("auto-sync", "Sync notes", true, 6, "disk full"),
            job("auto-mail", "Mail digest", true, 5, "bad folder"),
        ],
    );
    open(&d);

    let posted = tick(&d, false, false).expect("a Done card");
    assert!(!enabled(&d, "auto-sync"));
    // One per pass: the other job waits.
    assert!(enabled(&d, "auto-mail"));
    let line = ledger(&d);
    assert_eq!(line.len(), 1);
    assert_eq!(line[0].id, "auto-sync");
    let seq = line[0].seq;

    let all = spans(&d);
    let auto_span = all.last().unwrap();
    assert_eq!(auto_span.decision, "auto");
    assert_eq!(auto_span.result, "done");
    assert_eq!(auto_span.undo_ref, format!("automation:{seq}"));

    let stored = card(&d, &posted.id);
    assert_eq!(stored.kind, UpdateKind::DoneForYou);
    assert!(stored.id.starts_with("dfy"), "{}", stored.id);
    assert_eq!(
        stored.title,
        "Paused \"Sync notes\": it failed 6 times in a row"
    );
    assert_eq!(
        stored.body.as_deref(),
        Some("Last error: disk full. Undo turns it back on.")
    );
    assert_eq!(
        stored.done_for_you,
        Some(DoneForYou {
            kind: "automation".into(),
            target: "auto-sync".into(),
            undo_ref: seq,
            mind_key: "proactive:automation_disable".into(),
            answered: false,
        })
    );
    assert_eq!(
        serde_json::to_value(load_state(&d).budget).unwrap()["used"],
        json!(1)
    );
    // The page shows Undo and Stop doing this for it.
    let page = crate::pulse::peek(&d, now()).unwrap();
    let post = page.feed.iter().find(|p| p.id == posted.id).unwrap();
    assert_eq!(post.done, Some(crate::pulse::DoneView { answered: false }));
    drain(&d);
}

#[test]
fn each_miss_writes_one_ask_span_and_changes_nothing() {
    let d = scratch("misses");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    set_on(&d, true).unwrap();
    let jobs = std::fs::read(d.join("automations.json")).unwrap();
    let asks = |d: &Path| -> Vec<String> {
        decisions(d)
            .into_iter()
            .filter(|(decision, _)| decision == "ask")
            .map(|(_, claim)| claim)
            .collect()
    };

    // Approvals start on Ask me first.
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(asks(&d), vec!["pill"]);
    assert_eq!(
        held(&d, now(), 0).as_deref(),
        Some("Waiting: \"When the AI wants to change things\" is on \"Ask me first\".")
    );

    approvals(&d, crate::settings::Approvals::Always);
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(asks(&d), vec!["pill", "no_history"]);
    assert_eq!(
        held(&d, now(), 0).as_deref(),
        Some("Waiting: tap Pause it on a failing automation's Pulse card once first.")
    );

    approve_once(&d);
    assert_eq!(tick(&d, true, false), None);
    assert_eq!(tick(&d, false, true), None);
    assert_eq!(asks(&d), vec!["pill", "no_history", "quiet_hours", "busy"]);
    // Quiet hours and a busy moment pass on their own: nothing to show.
    assert_eq!(held(&d, now(), 0), None);

    std::fs::write(
        d.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE),
        (grokhub_desktop::process_started_ms() + 1_000).to_string(),
    )
    .unwrap();
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(asks(&d).last().map(String::as_str), Some("halted"));
    std::fs::remove_file(d.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE)).unwrap();

    change_state(&d, |s| {
        for _ in 0..5 {
            s.budget.spend(
                &crate::nudges::local_day(now(), 0),
                grokhub_core::learn::Undo::OneClick,
            );
        }
    });
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(asks(&d).last().map(String::as_str), Some("budget"));
    assert_eq!(
        held(&d, now(), 0).as_deref(),
        Some("Waiting: it already paused 5 automations today.")
    );
    std::fs::remove_file(d.join(STATE_FILE)).unwrap();

    hx::note_proactive(
        &d,
        &hx::answer_span(
            "proactive:automation_disable",
            hx::DECISION_UNDO,
            AccessMode::Supervised,
        ),
    );
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(asks(&d).last().map(String::as_str), Some("p_mind"));
    assert_eq!(
        asks(&d),
        vec![
            "pill",
            "no_history",
            "quiet_hours",
            "busy",
            "halted",
            "budget",
            "p_mind"
        ]
    );
    assert_eq!(std::fs::read(d.join("automations.json")).unwrap(), jobs);
    assert!(ledger(&d).is_empty());
}

#[test]
fn a_miss_leaves_the_normal_card_with_a_pause_it_chip_and_a_paused_job_gets_none() {
    let d = scratch("chip");
    write_jobs(
        &d,
        &[
            job("auto-sync", "Sync notes", true, 6, "disk full"),
            job("auto-old", "Old backup", false, 7, "no drive"),
        ],
    );
    set_on(&d, true).unwrap();
    assert_eq!(tick(&d, false, false), None);
    let posted = crate::proactive::tick(&d, PID, now(), false, 12).unwrap();
    let titles: Vec<&str> = posted.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["\"Sync notes\" failed 6 times in a row"]);
    let page = crate::pulse::peek(&d, now()).unwrap();
    let chips: Vec<(String, &str)> = page
        .feed
        .iter()
        .map(|p| &p.chips)
        .chain(
            page.ideas
                .iter()
                .flat_map(|g| g.ideas.iter().map(|i| &i.chips)),
        )
        .flatten()
        .map(|c| (c.label.clone(), c.act))
        .collect();
    assert!(
        chips.contains(&("Pause it".to_string(), "pause")),
        "{chips:?}"
    );
    let at_find = chips.iter().position(|c| c.0 == "Find out why").unwrap();
    assert_eq!(chips[at_find + 1], ("Pause it".to_string(), "pause"));
}

#[test]
fn a_pass_that_pauses_the_job_takes_pause_it_off_its_failing_card() {
    let d = scratch("chip-retired");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    set_on(&d, true).unwrap();
    // Held back by Ask me first: the normal card goes up with Pause it.
    assert_eq!(tick(&d, false, false), None);
    let health = health_card(&d);
    assert_eq!(pause_chips(&d), vec![health.id.clone()]);

    approvals(&d, crate::settings::Approvals::Always);
    approve_once(&d);
    let done = tick(&d, false, false).expect("a Done card");
    assert!(!enabled(&d, "auto-sync"));
    assert!(pause_chips(&d).is_empty(), "{:?}", pause_chips(&d));
    assert!(!offers_pause(&card(&d, &health.id)));
    assert_eq!(
        pause_from_card(&d, &health.id, now()),
        Err(NOT_PAUSABLE.to_string())
    );
    assert!(card(&d, &done.id).done_for_you.is_some());
    drain(&d);
}

#[test]
fn a_job_name_pulse_would_turn_down_still_gets_a_done_card() {
    let d = scratch("vague-name");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(
        &d,
        &[job(
            "auto-board",
            "Check this job board each morning",
            true,
            6,
            "page layout changed",
        )],
    );
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    let stored = card(&d, &done.id);
    assert_eq!(
        stored.title,
        "Paused an automation: it failed 6 times in a row"
    );
    assert_eq!(stored.body.as_deref(), Some("Undo turns it back on."));
    assert_eq!(stored.done_for_you.unwrap().target, "auto-board");
    assert!(!enabled(&d, "auto-board"));
    drain(&d);
}

/// A failing job's normal card, as the proactive pass posts it.
fn health_card(dir: &Path) -> UpdateCard {
    let posted = crate::proactive::tick(dir, PID, now(), false, 12).unwrap();
    posted
        .into_iter()
        .find(offers_pause)
        .expect("a card with Pause it")
}

#[test]
fn pause_it_pauses_once_teaches_the_approval_and_the_next_pass_can_act() {
    let d = scratch("tap");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    let first = health_card(&d);
    assert_eq!(
        pause_from_card(&d, &first.id, now()).unwrap(),
        "Paused \"Sync notes\". Undo is in Settings > Behavior."
    );
    assert!(!enabled(&d, "auto-sync"));
    assert_eq!(
        decisions(&d),
        vec![("approve".to_string(), "ran from its card".to_string())]
    );
    let stored = card(&d, &first.id);
    let receipts = &stored.pulse.report.as_ref().unwrap().receipts;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].what, "Paused \"Sync notes\"");
    assert_eq!(receipts[0].result, vec![PAUSED_RECEIPT.to_string()]);
    assert!(!offers_pause(&stored));
    // A second tap has nothing left to run.
    assert_eq!(
        pause_from_card(&d, &first.id, now()),
        Err(NOT_PAUSABLE.to_string())
    );

    // With that approval, the next failing job is paused on its own.
    let mut jobs: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(d.join("automations.json")).unwrap())
            .unwrap();
    jobs.push(job("auto-mail", "Mail digest", true, 5, "bad folder"));
    write_jobs(&d, &jobs);
    set_on(&d, true).unwrap();
    approvals(&d, crate::settings::Approvals::Auto);
    let done = tick(&d, false, false).expect("a Done card");
    assert_eq!(
        done.title,
        "Paused \"Mail digest\": it failed 5 times in a row"
    );
    assert!(!enabled(&d, "auto-mail"));
    drain(&d);
}

#[test]
fn a_card_that_stored_another_step_cannot_run_it() {
    let d = scratch("tampered");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    let mut forged = health_card(&d);
    let id = forged.id.clone();
    crate::pulse::edit_cards(&d, &mut |cards| {
        let c = cards.iter_mut().find(|c| c.id == id).unwrap();
        let meta = c.pulse.proactive.as_mut().unwrap();
        meta.tool = "connection_add".into();
        meta.arguments =
            Some(r#"{"name":"crm","url":"http://127.0.0.1:9/mcp","needs_token":true}"#.into());
        1
    })
    .unwrap();
    forged = card(&d, &id);
    assert!(!offers_pause(&forged));
    assert_eq!(
        pause_from_card(&d, &id, now()),
        Err(NOT_PAUSABLE.to_string())
    );
    assert!(enabled(&d, "auto-sync"));
    assert!(!d.join("mcp.json").exists());
    assert!(spans(&d).is_empty());
    assert!(ledger(&d).is_empty());
}

#[test]
fn undo_on_the_card_turns_it_back_on_and_holds_the_next_pause() {
    let d = scratch("undo");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    let seq = done.done_for_you.as_ref().unwrap().undo_ref;

    let line = undo(&d, &done.id).unwrap();
    assert_eq!(line, "Turned \"Sync notes\" back on.");
    assert!(enabled(&d, "auto-sync"));
    let undo_line = ledger(&d).last().cloned().unwrap();
    assert_eq!(undo_line.op, hx::ChangeOp::Undo);
    assert_eq!(undo_line.undoes, Some(seq));
    let undos: Vec<String> = spans(&d)
        .iter()
        .filter(|s| s.decision == "undo")
        .map(hx::mind_key)
        .collect();
    assert_eq!(undos, vec!["proactive:automation_disable".to_string()]);
    assert!(card(&d, &done.id).done_for_you.unwrap().answered);

    // On again with its 6 failures from before: the next pass leaves it
    // alone, with no ask, and no Done card goes up.
    let cards_before = crate::pulse::cards(&d).unwrap().len();
    let spans_before = spans(&d).len();
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(spans(&d).len(), spans_before);
    assert!(enabled(&d, "auto-sync"));
    // Five failed runs later it is a candidate again, and MindCheck asks first.
    fail_more(&d, "auto-sync", 4);
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(spans(&d).len(), spans_before);
    fail_more(&d, "auto-sync", 1);
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(
        decisions(&d).last().unwrap(),
        &("ask".to_string(), "p_mind".to_string())
    );
    assert!(enabled(&d, "auto-sync"));
    assert_eq!(crate::pulse::cards(&d).unwrap().len(), cards_before);

    // A second Undo changes nothing.
    let ledger_len = ledger(&d).len();
    assert_eq!(undo(&d, &done.id).unwrap(), ALREADY_ANSWERED);
    assert_eq!(ledger(&d).len(), ledger_len);
    assert_eq!(spans(&d).iter().filter(|s| s.decision == "undo").count(), 1);
    drain(&d);
}

#[test]
fn undo_refuses_a_newer_change_and_a_job_that_is_gone() {
    let d = scratch("refuse");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    // A later change on the same job, through the ledger.
    let path = crate::automations::path(&d);
    hx::record_change(
        &d,
        &hx::AutomationsFile {
            path: &path,
            id: "auto-sync",
        },
        hx::Origin::SelfManage,
        "renamed it",
        || {
            let mut jobs: Vec<serde_json::Value> =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            jobs[0]["name"] = json!("Sync notes daily");
            std::fs::write(&path, serde_json::to_string_pretty(&jobs).unwrap())
                .map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let jobs = std::fs::read(&path).unwrap();
    assert_eq!(
        undo(&d, &done.id),
        Err(crate::changes::CHANGED_SINCE.to_string())
    );
    assert_eq!(std::fs::read(&path).unwrap(), jobs);
    assert!(!card(&d, &done.id).done_for_you.unwrap().answered);

    // The job is gone: nothing to turn back on, and the card is answered.
    write_jobs(&d, &[]);
    assert_eq!(
        undo(&d, &done.id).unwrap(),
        "That automation is gone, so there is nothing to turn back on."
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "[]");
    assert!(card(&d, &done.id).done_for_you.unwrap().answered);
    drain(&d);
}

#[test]
fn undo_keeps_what_changed_while_the_job_was_paused() {
    let d = scratch("undo-keeps");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    // Fixed on the Automations page while paused: no ledger line.
    crate::automations::edit(
        &d,
        "auto-sync",
        "every day at 8am sync my notes folder",
        clock(),
    )
    .unwrap();
    // And a run that failed again (as Run now would leave it).
    fail_more(&d, "auto-sync", 1);
    let edited = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    assert!(!edited.enabled);
    assert_ne!(edited.instructions, "Sync notes for me");
    let ledger_len = ledger(&d).len();

    undo(&d, &done.id).unwrap();
    let back = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    assert!(back.enabled);
    assert_eq!(back.name, edited.name);
    assert_eq!(back.instructions, edited.instructions);
    assert_eq!(back.schedule, edited.schedule);
    assert_eq!(back.times, edited.times);
    assert_eq!(back.health.fail_streak, 7);
    assert_eq!(back.health.error, "disk full");
    assert!(back.next_run.is_some_and(|t| t > now() - 1_000));
    let lines = ledger(&d);
    assert_eq!(lines.len(), ledger_len + 1);
    assert_eq!(
        lines.last().unwrap().undoes,
        Some(done.done_for_you.unwrap().undo_ref)
    );
    drain(&d);
}

#[test]
fn turning_it_back_on_elsewhere_counts_as_the_same_undo() {
    let d = scratch("elsewhere");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(
        &d,
        &[
            job("auto-sync", "Sync notes", true, 6, "disk full"),
            job("auto-mail", "Mail digest", true, 5, "bad folder"),
        ],
    );
    open(&d);
    let first = tick(&d, false, false).expect("first Done card");
    let second = tick(&d, false, false).expect("second Done card");
    assert_eq!(second.done_for_you.as_ref().unwrap().target, "auto-mail");

    // Fixed on the Automations page while paused, then Settings >
    // Behavior's Undo on the pause line: it comes back on with the fix.
    crate::automations::edit(
        &d,
        "auto-sync",
        "every day at 8am sync my notes folder",
        clock(),
    )
    .unwrap();
    let edited = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    assert_eq!(
        crate::change_rows::undo(&d, "automation", "auto-sync").unwrap(),
        format!("Turned \"{}\" back on.", edited.name)
    );
    let back = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    assert!(back.enabled);
    assert_eq!(back.instructions, edited.instructions);
    assert_eq!(back.times, edited.times);
    assert!(card(&d, &first.id).done_for_you.unwrap().answered);
    assert!(!card(&d, &second.id).done_for_you.unwrap().answered);
    assert_eq!(spans(&d).iter().filter(|s| s.decision == "undo").count(), 1);

    // The Automations page's switch.
    let clock = grokhub_core::LocalClock {
        now_ms: now(),
        weekday: 1,
        hour: 9,
        minute: 0,
    };
    crate::automations::set_enabled(&d, "auto-mail", true, clock).unwrap();
    assert!(card(&d, &second.id).done_for_you.unwrap().answered);
    assert_eq!(spans(&d).iter().filter(|s| s.decision == "undo").count(), 2);
    drain(&d);
}

#[test]
fn turning_it_back_on_by_hand_counts_with_no_card_and_is_not_undone_next_pass() {
    let d = scratch("by-hand");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(
        &d,
        &[
            job("auto-sync", "Sync notes", true, 6, "disk full"),
            job("auto-mail", "Mail digest", true, 5, "bad folder"),
        ],
    );
    open(&d);
    // An auto pause whose Done card was dismissed.
    let done = tick(&d, false, false).expect("a Done card");
    crate::pulse::edit_cards(&d, &mut |cards| {
        let before = cards.len();
        cards.retain(|c| c.id != done.id);
        before - cards.len()
    })
    .unwrap();
    // A Pause it tap: an approve span and no Done card.
    let health = health_card(&d);
    pause_from_card(&d, &health.id, now()).unwrap();
    assert!(!enabled(&d, "auto-mail"));
    let undos = |d: &Path| spans(d).iter().filter(|s| s.decision == "undo").count();
    assert_eq!(undos(&d), 0);

    crate::automations::set_enabled(&d, "auto-sync", true, clock()).unwrap();
    assert_eq!(undos(&d), 1);
    crate::automations::set_enabled(&d, "auto-mail", true, clock()).unwrap();
    assert_eq!(undos(&d), 2);
    // Off and on again by hand: the same pause is not heard twice.
    crate::automations::set_enabled(&d, "auto-sync", false, clock()).unwrap();
    crate::automations::set_enabled(&d, "auto-sync", true, clock()).unwrap();
    assert_eq!(undos(&d), 2);

    // Both still show 5 or more failures, but the user just turned them on:
    // the pass holds them back itself, before MindCheck (no ask span).
    let before = spans(&d).len();
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(spans(&d).len(), before);
    assert!(enabled(&d, "auto-sync"));
    assert!(enabled(&d, "auto-mail"));
    drain(&d);
}

#[test]
fn a_job_turned_back_on_outside_the_switch_is_heard_on_the_next_pass() {
    let d = scratch("outside");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    // Chat turns it back on with a new interval: the row changes, and no
    // switch or Undo in this app ran.
    edit_row(&d, "auto-sync", |row| {
        row["enabled"] = json!(true);
        row["time"] = json!("07:30");
    });
    let before = spans(&d).len();
    assert_eq!(tick(&d, false, false), None);
    assert!(enabled(&d, "auto-sync"));
    let new: Vec<String> = spans(&d)[before..]
        .iter()
        .map(|s| s.decision.clone())
        .collect();
    assert_eq!(new, vec!["undo".to_string()]);
    assert!(card(&d, &done.id).done_for_you.unwrap().answered);
    assert_eq!(undo(&d, &done.id).unwrap(), ALREADY_ANSWERED);
    // Heard once: the next pass adds nothing.
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(spans(&d).len(), before + 1);
    drain(&d);
}

#[test]
fn undo_on_a_card_whose_job_is_already_on_again_counts_once() {
    let d = scratch("already-on");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    edit_row(&d, "auto-sync", |row| row["enabled"] = json!(true));
    let ledger_len = ledger(&d).len();
    assert_eq!(
        undo(&d, &done.id).unwrap(),
        "\"Sync notes\" is already back on."
    );
    assert_eq!(ledger(&d).len(), ledger_len);
    let undos = |d: &Path| spans(d).iter().filter(|s| s.decision == "undo").count();
    assert_eq!(undos(&d), 1);
    assert!(card(&d, &done.id).done_for_you.unwrap().answered);
    assert_eq!(tick(&d, false, false), None);
    assert_eq!(undos(&d), 1);
    drain(&d);
}

#[test]
fn an_undo_that_leaves_the_job_off_teaches_nothing() {
    let d = scratch("still-off");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    // A newer ledger change while it is paused, then Settings > Behavior's
    // Undo of that change: the job stays off.
    let path = crate::automations::path(&d);
    hx::record_change(
        &d,
        &hx::AutomationsFile {
            path: &path,
            id: "auto-sync",
        },
        hx::Origin::SelfManage,
        "renamed it",
        || {
            edit_row(&d, "auto-sync", |row| {
                row["name"] = json!("Sync notes daily")
            });
            Ok(())
        },
    )
    .unwrap();
    crate::change_rows::undo(&d, "automation", "auto-sync").unwrap();
    assert!(!enabled(&d, "auto-sync"));
    assert_eq!(rows(&d)[0].name, "Sync notes");
    assert_eq!(spans(&d).iter().filter(|s| s.decision == "undo").count(), 0);
    assert!(!card(&d, &done.id).done_for_you.unwrap().answered);
    drain(&d);
}

#[test]
fn a_job_paused_twice_keeps_its_edits_when_settings_undoes_the_first_pause() {
    let d = scratch("twice");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    tick(&d, false, false).expect("first pause");
    // On again with its switch, fixed, then 5 more failed runs.
    crate::automations::set_enabled(&d, "auto-sync", true, clock()).unwrap();
    crate::automations::edit(
        &d,
        "auto-sync",
        "every day at 8am sync my notes folder",
        clock(),
    )
    .unwrap();
    fail_more(&d, "auto-sync", 5);
    // Paused again, the way a Pause it tap runs it, then that pause undone
    // in Settings.
    let args = r#"{"id":"auto-sync","reason":"paused from its Pulse card"}"#;
    let again = hx::run_offered_step(&d, &d, TOOL, args, AccessMode::Supervised);
    let hx::AutoRun::Done { change, .. } = again else {
        panic!("second pause");
    };
    let job_now = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    note_paused(&d, &job_now, change.seq, &hx::proactive_key(TOOL, args));
    assert!(!enabled(&d, "auto-sync"));
    crate::change_rows::undo(&d, "automation", "auto-sync").unwrap();
    assert!(enabled(&d, "auto-sync"));
    let edited = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    // Settings > Behavior now lists the first pause: its Undo turns the job
    // on as it is, never back to the row from before the first pause.
    crate::automations::set_enabled(&d, "auto-sync", false, clock()).unwrap();
    crate::change_rows::undo(&d, "automation", "auto-sync").unwrap();
    let back = rows(&d).into_iter().find(|a| a.id == "auto-sync").unwrap();
    assert!(back.enabled);
    assert_eq!(back.instructions, edited.instructions);
    assert_eq!(back.run_count, edited.run_count);
    drain(&d);
}

#[test]
fn a_job_that_is_running_now_is_not_paused() {
    let d = scratch("running");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-run", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let held = crate::automations::Running::claim("auto-run").unwrap();
    assert_eq!(tick(&d, false, false), None);
    assert!(enabled(&d, "auto-run"));
    drop(held);
    assert!(tick(&d, false, false).is_some());
    assert!(!enabled(&d, "auto-run"));
    drain(&d);
}

#[test]
fn the_cap_drops_answered_pauses_and_keeps_a_job_that_is_still_paused() {
    let d = scratch("cap");
    let old = Automation {
        id: "auto-old".into(),
        ..auto("auto-old", false, 6, "x")
    };
    note_paused(&d, &old, 1, "k");
    for i in 0..(KEEP as u64 + 4) {
        let other = auto(&format!("auto-{i}"), true, 6, "x");
        note_paused(&d, &other, i + 2, "k");
        change_state(&d, |s| s.paused.last_mut().unwrap().answered = true);
    }
    let kept = load_state(&d).paused;
    assert_eq!(kept.len(), KEEP);
    assert_eq!(kept[0].id, "auto-old");
    assert!(!kept[0].answered);
    assert_eq!(kept.last().unwrap().seq, KEEP as u64 + 5);
}

#[test]
fn stop_doing_this_turns_the_switch_off_and_teaches_nothing() {
    let d = scratch("stop");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    write_jobs(&d, &[job("auto-sync", "Sync notes", true, 6, "disk full")]);
    open(&d);
    let done = tick(&d, false, false).expect("a Done card");
    let line = stop(&d).unwrap();
    assert!(line.starts_with("Turned off."), "{line}");
    assert_eq!(
        grokhub_core::app_json::read(&d).unwrap().get(SWITCH_KEY),
        Some(&json!(false))
    );
    assert!(spans(&d).iter().all(|s| s.decision != "never"));
    assert!(!card(&d, &done.id).done_for_you.unwrap().answered);
    drain(&d);
}

#[test]
fn the_day_budget_survives_a_relaunch() {
    let d = scratch("budget");
    let _pin = grokhub_agent::perm::ConfigGuard::set(&*d);
    let ids: Vec<String> = (0..6).map(|i| format!("auto-{i}")).collect();
    let jobs: Vec<serde_json::Value> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            job(
                id,
                &format!("Job {i}"),
                true,
                10 - i as u32,
                &format!("error {i}"),
            )
        })
        .collect();
    write_jobs(&d, &jobs);
    open(&d);
    let today = now();
    for _ in 0..5 {
        assert!(tick_one(&d, PID, today, 0, false, false, true).is_some());
    }
    // The state is read back from the file on every pass, as after a relaunch.
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join(STATE_FILE)).unwrap()).unwrap();
    assert_eq!(saved["budget"]["used"], json!(5));
    assert_eq!(tick_one(&d, PID, today, 0, false, false, true), None);
    assert_eq!(
        decisions(&d).last().unwrap(),
        &("ask".to_string(), "budget".to_string())
    );
    assert!(enabled(&d, "auto-5"));
    let next_day = tick_one(&d, PID, today + DAY_MS, 0, false, false, true);
    assert!(next_day.is_some());
    assert!(!enabled(&d, "auto-5"));
    drain(&d);
}
