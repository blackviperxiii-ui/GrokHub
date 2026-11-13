use super::*;

const NOW: u64 = 1_800_000_000_000;

/// The waiter is one per app: tests that use it take turns.
static SERIAL: Mutex<()> = Mutex::new(());

struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fresh(name: &str) -> (std::sync::MutexGuard<'static, ()>, Scratch) {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    *waiter() = Waiter::default();
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-model-wait-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (guard, Scratch(dir))
}

fn wait_cards(dir: &Path) -> Vec<grokhub_core::UpdateCard> {
    crate::pulse::cards(dir)
        .unwrap()
        .into_iter()
        .filter(grokhub_core::is_model_wait_card)
        .collect()
}

#[test]
fn a_no_route_turn_waits_on_a_named_card_and_resumes_once_when_a_model_answers() {
    let (_g, dir) = fresh("resume");
    let err = format!("protocol error: {}", *NO_ROUTE_MSG);
    assert!(turn_failed(&dir.0, "chat-1", "Summarize inbox", &err, NOW));
    start(&dir.0, "chat:default", "grok-4.7", NOW, "3:05 PM");

    let cards = wait_cards(&dir.0);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Waiting for xAI");
    assert_eq!(
        cards[0].body.as_deref(),
        Some("xAI isn't answering since 3:05 PM; retrying. “Summarize inbox” picks up on its own once it answers.")
    );

    // Not due before the first 15 s; Retry now makes it due.
    assert_eq!(due_check(NOW + 1_000), None);
    assert!(retry_now(NOW + 1_000));
    assert_eq!(due_check(NOW + 1_000), Some("grok-4.7".to_string()));
    // One check at a time.
    assert_eq!(due_check(NOW + 1_000), None);

    // Nothing answered: the next check backs off 30 s, and the card stays.
    assert_eq!(checked(&dir.0, None, NOW + 2_000), None);
    assert_eq!(due_check(NOW + 2_000 + 29_999), None);
    assert_eq!(wait_cards(&dir.0).len(), 1);

    assert_eq!(due_check(NOW + 32_000), Some("grok-4.7".to_string()));
    let back = checked(&dir.0, Some("grok-4.7"), NOW + 33_000);
    let want = Resume {
        conversation_id: "chat-1".into(),
        ask: "Summarize inbox".into(),
        note: "xAI is answering again; resumed: Summarize inbox".into(),
    };
    assert_eq!(back, Some(want.clone()));
    assert!(wait_cards(&dir.0).is_empty());

    // Only that chat takes the resume, and only once.
    assert_eq!(take_resume("chat-2"), None);
    assert_eq!(take_resume("chat-1"), Some(want));
    assert_eq!(take_resume("chat-1"), None);
    assert!(!retry_now(NOW + 40_000));
}

#[test]
fn an_offline_turn_with_no_local_model_resumes_with_the_reconnect_note() {
    let (_g, dir) = fresh("offline");
    let why = grokhub_agent::route::local::offline_pause_text("runtime missing");
    assert!(turn_failed(
        &dir.0,
        "chat-9",
        "test",
        &format!("protocol error: {why}"),
        NOW
    ));
    start(&dir.0, "chat:default", "grok-4.7", NOW, "9:00 AM");
    assert_eq!(due_check(NOW + 15_000), Some("grok-4.7".to_string()));
    let back = checked(&dir.0, Some("grok-4.7"), NOW + 15_500).unwrap();
    assert_eq!(back.note, "Back online: answered after reconnect (test)");
    assert_eq!(back.conversation_id, "chat-9");
}

#[test]
fn a_budget_pause_or_other_failure_is_not_a_wait() {
    let (_g, dir) = fresh("not-a-wait");
    let budget = *grokhub_agent::route::budget::BUDGET_PAUSE_MSG;
    assert!(!turn_failed(&dir.0, "chat-1", "hi", budget, NOW));
    assert!(!turn_failed(
        &dir.0,
        "chat-1",
        "hi",
        "network error: timed out",
        NOW
    ));
    assert!(waiter().paused.is_none());
    assert!(wait_cards(&dir.0).is_empty());
}

#[test]
fn a_background_wait_ends_with_no_resume_and_the_same_model_keeps_its_backoff() {
    let (_g, dir) = fresh("carry");
    start(&dir.0, "automation:run", "grok-4.7", NOW, "1:00 AM");
    assert_eq!(due_check(NOW + 15_000), Some("grok-4.7".to_string()));
    assert_eq!(checked(&dir.0, Some("grok-4.7"), NOW + 15_000), None);
    assert!(wait_cards(&dir.0).is_empty());
    // It pauses again a minute later: the next check waits 30 s, not 15 s.
    start(
        &dir.0,
        "automation:run",
        "grok-4.7",
        NOW + 60_000,
        "1:01 AM",
    );
    assert_eq!(due_check(NOW + 60_000 + 15_000), None);
    assert_eq!(
        due_check(NOW + 60_000 + 30_000),
        Some("grok-4.7".to_string())
    );
}

#[test]
fn the_waiting_card_shows_in_pulse_with_retry_now() {
    let (_g, dir) = fresh("pulse");
    start(&dir.0, "chat:default", "grok-4.7", NOW, "3:05 PM");
    let page = crate::pulse::load(&dir.0, None, NOW + 1_000).unwrap();
    let post = page
        .feed
        .iter()
        .find(|p| p.title == "Waiting for xAI")
        .expect("the waiting card is on the feed");
    assert_eq!(post.review, RETRY_LABEL);
}
