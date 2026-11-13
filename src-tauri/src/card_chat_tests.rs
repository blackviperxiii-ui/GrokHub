use std::path::PathBuf;

use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};
use grokhub_core::{BoardCard, BoardStatus};

use super::*;
use crate::workboard::{load, Lane};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-card-chat-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn todo(id: &str, title: &str) -> BoardCard {
    let mut c = BoardCard::new(title, "", "");
    c.id = id.into();
    c.status = BoardStatus::Todo;
    c
}

fn write(dir: &Path, cards: &[BoardCard]) {
    std::fs::write(
        workboard::path(dir),
        serde_json::to_string_pretty(cards).expect("json"),
    )
    .expect("board");
}

fn on_disk(dir: &Path) -> Vec<BoardCard> {
    read_board(&workboard::path(dir)).expect("read")
}

fn card<'a>(page: &'a Page, lane: Lane, id: &str) -> &'a workboard::Card {
    page.columns
        .iter()
        .find(|c| c.lane == lane)
        .and_then(|c| c.cards.iter().find(|c| c.id == id))
        .unwrap_or_else(|| panic!("{id} not in {lane:?}"))
}

fn tick(queue: &mut Queue, dir: &Path, now: u64, gate: QueueGate) -> Option<CardSay> {
    let (ready, chat) = (|| true, move || format!("shell-q{now}"));
    queue.tick(
        dir,
        Look {
            now,
            gate,
            ready: &ready,
            new_chat: &chat,
            mode: PermissionMode::Ask,
        },
    )
}

#[test]
fn say_gives_a_card_its_chat_and_shows_stop_while_its_turn_runs() {
    clear_for_test();
    let dir = temp_dir("say");
    write(&dir, &[todo("a", "Fix the printer")]);
    assert_eq!(
        say(&dir, "a", "hi", "cabin-1").map(|s| s.conversation_id),
        Err("That chat isn't one of this app's chats.".to_string())
    );
    let said = say(&dir, "a", "start with the driver", "shell-1").expect("say");
    assert_eq!(said.conversation_id, "shell-1");
    assert_eq!(
        said.message,
        "Work on this workboard card: Fix the printer\n\nWhen it is done, say so in one \
         line.\n\nstart with the driver"
    );
    let a = card(&said.page, Lane::Doing, "a");
    assert_eq!(a.chat.as_deref(), Some("shell-1"));
    assert!(!a.running);

    turn_began("shell-1");
    turn_began("shell-other");
    assert!(running("shell-1"));
    assert!(!running("shell-other"), "only a card's chat is tracked");
    assert!(card(&load(&dir).unwrap(), Lane::Doing, "a").running);

    // Stop: the turn ends cancelled and the card waits in Blocked.
    turn_ended(&dir, "shell-1", &Ok("cancelled".into()));
    assert!(!running("shell-1"));
    let page = load(&dir).unwrap();
    let a = card(&page, Lane::Blocked, "a");
    assert!(!a.running);
    assert_eq!(
        a.checkpoint.as_ref().map(|c| c.blocker.as_str()),
        Some("Stopped before it finished.")
    );
    let log = workboard::log_tail(&dir, "a");
    assert_eq!(log.last().map(|l| l.text.as_str()), Some("stopped"));

    // Say again carries on in the same chat; a finished turn lands in Done.
    let again = say(&dir, "a", "try again", "shell-2").expect("again");
    assert_eq!(again.conversation_id, "shell-1");
    assert_eq!(again.message, "try again");
    turn_began("shell-1");
    turn_ended(&dir, "shell-1", &Ok("end_turn".into()));
    assert_eq!(on_disk(&dir)[0].status, BoardStatus::Done);
}

#[test]
fn a_queued_card_starts_on_its_own_and_the_next_waits_for_it() {
    clear_for_test();
    let dir = temp_dir("queue");
    let mut a = todo("a", "Clean the cache");
    a.queued_ms = 1;
    let mut b = todo("b", "Sort downloads");
    b.queued_ms = 2;
    write(&dir, &[a, b]);
    let mut queue = Queue::default();
    let busy = QueueGate {
        foreground: true,
        ..QueueGate::default()
    };
    assert_eq!(tick(&mut queue, &dir, 1_000, busy), None);
    assert_eq!(
        load(&dir).unwrap().queue_note.as_deref(),
        Some("Waits for the open chat to finish.")
    );
    assert_eq!(tick(&mut queue, &dir, 2_000, QueueGate::default()), None);
    assert_eq!(
        load(&dir).unwrap().queue_note.as_deref(),
        Some("Starts in a moment.")
    );
    let start = tick(&mut queue, &dir, 5_000, QueueGate::default()).expect("starts");
    assert_eq!(start.chat, "shell-q5000");
    assert_eq!(
        start.message,
        "Work on this workboard card: Clean the cache\n\nWhen it is done, say so in one line."
    );
    let page = load(&dir).unwrap();
    let a = card(&page, Lane::Doing, "a");
    assert!(a.running);
    assert_eq!(a.chat.as_deref(), Some("shell-q5000"));
    assert_eq!(a.queued, None);
    assert_eq!(
        card(&page, Lane::Todo, "b").queued.as_deref(),
        Some("Queued")
    );
    assert_eq!(
        workboard::log_tail(&dir, "a")
            .last()
            .map(|l| l.text.as_str()),
        Some("started from the queue")
    );

    assert_eq!(tick(&mut queue, &dir, 9_000, QueueGate::default()), None);
    assert_eq!(
        load(&dir).unwrap().queue_note.as_deref(),
        Some("A queued card is running. The next one waits for it.")
    );
    // Its turn ends: a goes to Done and b starts after the settle.
    turn_ended(&dir, "shell-q5000", &Ok("end_turn".into()));
    assert_eq!(tick(&mut queue, &dir, 10_000, QueueGate::default()), None);
    let next = tick(&mut queue, &dir, 13_000, QueueGate::default()).expect("b starts");
    assert_eq!(next.chat, "shell-q13000");
    let disk = on_disk(&dir);
    assert_eq!(disk[0].status, BoardStatus::Done);
    assert_eq!(disk[1].status, BoardStatus::InProgress);
}

#[test]
fn the_queue_leaves_queued_cards_to_an_open_cabin() {
    clear_for_test();
    let dir = temp_dir("cabin");
    let mut a = todo("a", "Clean the cache");
    a.queued_ms = 1;
    write(&dir, &[a]);
    let now = grokhub_core::now_ms();
    let lease = SchedulerLease {
        owner: LeaseOwner::Cabin,
        pid: 1,
        beat_ms: now,
    };
    write_lease(&dir, &lease).unwrap();
    let mut queue = Queue::default();
    assert_eq!(tick(&mut queue, &dir, now, QueueGate::default()), None);
    assert_eq!(
        tick(&mut queue, &dir, now + 4_000, QueueGate::default()),
        None
    );
    assert_eq!(on_disk(&dir)[0].status, BoardStatus::Todo);
    assert_eq!(load(&dir).unwrap().queue_note, None);
}

#[test]
fn a_card_run_from_an_earlier_launch_still_settles() {
    clear_for_test();
    let dir = temp_dir("seed");
    let mut a = todo("a", "Fix the printer");
    a.status = BoardStatus::InProgress;
    a.run = true;
    a.thread_id = Some("shell-old".into());
    write(&dir, &[a]);
    seed(&dir);
    turn_began("shell-old");
    assert!(running("shell-old"));
    // A card's chat: its failed ask is not sent again on its own.
    assert!(turn_ended(&dir, "shell-old", &Err("no model".into())));
    assert!(!turn_ended(&dir, "shell-plain", &Err("no model".into())));
    let disk = on_disk(&dir);
    assert_eq!(disk[0].status, BoardStatus::Blocked);
    assert_eq!(
        disk[0].checkpoint.as_ref().map(|c| c.blocker.as_str()),
        Some("Failed: no model")
    );
}
