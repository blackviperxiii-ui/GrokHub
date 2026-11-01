use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::brain::{classify, BriefInput};
use crate::route::local::{ChatMessage, ForceOn, LocalRuntime};

/// Answers every chat with `reply` after `sleep`, and counts the calls.
struct Fake {
    reply: String,
    sleep: Duration,
    calls: AtomicUsize,
    seen: Mutex<Vec<Vec<ChatMessage>>>,
}

impl Fake {
    fn new(reply: &str, sleep_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            reply: reply.into(),
            sleep: Duration::from_millis(sleep_ms),
            calls: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl LocalRuntime for Fake {
    fn classify(&self, _text: &str, _labels: &[&str]) -> Result<Value, String> {
        Ok(json!({"label": "routine", "score": 0.9}))
    }
    fn summarize(&self, _text: &str, _max_words: u32) -> Result<Value, String> {
        Ok(json!({"summary": ""}))
    }
    fn difficulty(&self, _text: &str) -> Result<Value, String> {
        Ok(json!({"d": 0.5}))
    }
    fn chat_stream_at(
        &self,
        messages: &[ChatMessage],
        _effort: Option<&str>,
        _on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(messages.to_vec());
        std::thread::sleep(self.sleep);
        Ok(self.reply.clone())
    }
}

const TYPO: &str = "plz fix teh wifi its brokn agian ugh";
const FIXED: &str = "please fix the wifi, it's broken again";

fn brief(text: &str) -> Brief {
    classify(
        &BriefInput {
            text,
            ..Default::default()
        },
        None,
    )
}

fn lane() -> &'static AtomicBool {
    Box::leak(Box::new(AtomicBool::new(false)))
}

/// [`tidy_on`] with the switch on, a fresh lane and `fake` as the runtime.
fn run(fake: &Arc<Fake>, text: &str, limit: Duration) -> Option<String> {
    run_on(lane(), fake, text, limit)
}

fn run_on(
    lane: &'static AtomicBool,
    fake: &Arc<Fake>,
    text: &str,
    limit: Duration,
) -> Option<String> {
    let b = brief(text);
    let rt: Arc<dyn LocalRuntime> = fake.clone();
    tidy_on(lane, limit, true, || Some(rt), &turn(text, &b))
}

fn turn<'a>(text: &'a str, brief: &'a Brief) -> TidyIn<'a> {
    TidyIn {
        text,
        brief,
        recent: &[],
        image: false,
        background: false,
        instant: false,
    }
}

#[test]
fn a_typod_ask_comes_back_tidied() {
    let fake = Fake::new(FIXED, 0);
    assert_eq!(run(&fake, TYPO, LIMIT).as_deref(), Some(FIXED));
    assert_eq!(fake.calls(), 1);
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen[0][0].role, "system");
    assert_eq!(seen[0][0].content, CLEANUP_PROMPT);
    assert_eq!(
        (seen[0][1].role.as_str(), seen[0][1].content.as_str()),
        ("user", TYPO)
    );
}

#[test]
fn the_switch_off_sends_the_typed_text() {
    let fake = Fake::new(FIXED, 0);
    let b = brief(TYPO);
    let rt: Arc<dyn LocalRuntime> = fake.clone();
    assert_eq!(
        tidy_on(lane(), LIMIT, false, || Some(rt), &turn(TYPO, &b)),
        None
    );
    // The public path, with a runtime loaded but the switch at its default.
    let _on = ForceOn::with(Some(fake.clone() as Arc<dyn LocalRuntime>));
    assert!(!crate::route::local::prompt_cleanup());
    assert_eq!(tidy(&turn(TYPO, &b)), None);
    assert_eq!(fake.calls(), 0);
    // Switched on (this thread only), the same call tidies.
    let _switch = SwitchOn::new();
    assert_eq!(tidy(&turn(TYPO, &b)).as_deref(), Some(FIXED));
    assert_eq!(fake.calls(), 1);
}

#[test]
fn no_runtime_loaded_sends_the_typed_text() {
    let b = brief(TYPO);
    assert_eq!(tidy_on(lane(), LIMIT, true, || None, &turn(TYPO, &b)), None);
    let _switch = SwitchOn::new();
    // Local AI is off in tests: nothing is loaded, nothing is asked.
    assert_eq!(tidy(&turn(TYPO, &b)), None);
}

#[test]
fn short_long_slash_code_path_and_url_asks_are_never_sent() {
    let fake = Fake::new(FIXED, 0);
    let long = "plz ".repeat(160);
    for text in [
        "hi",
        "pls fix teh wifi its brokn",
        long.as_str(),
        "/compact pls fix teh wi fi its brokn agian",
        "pls fix teh code ```fn main() {}``` its brokn agian",
        "pls bakup D:/Loadout/saves befor teh game updaets",
        "pls chek https://example.com its brokn agian todya",
        r"pls clear C:\Games\cache its brokn agian todya ok",
    ] {
        assert_eq!(run(&fake, text, LIMIT), None, "{text}");
    }
    assert_eq!(fake.calls(), 0);
}

#[test]
fn background_and_instant_turns_are_never_sent() {
    let fake = Fake::new(FIXED, 0);
    let b = brief(TYPO);
    for (background, instant) in [(true, false), (false, true)] {
        let rt: Arc<dyn LocalRuntime> = fake.clone();
        let input = TidyIn {
            background,
            instant,
            ..turn(TYPO, &b)
        };
        assert_eq!(tidy_on(lane(), LIMIT, true, || Some(rt), &input), None);
    }
    assert_eq!(fake.calls(), 0);
}

/// One rejected answer per guard, each passing every other guard.
fn rejected(typed: &str, wrong: &str) {
    let fake = Fake::new(wrong, 0);
    assert_eq!(run(&fake, typed, LIMIT), None, "{wrong:?}");
    assert_eq!(fake.calls(), 1);
}

#[test]
fn a_dropped_path_number_quote_or_mention_keeps_the_original() {
    let typed = "pls bakup D:/Loadout/saves befor teh game updaets tonite";
    let b = brief(typed);
    assert!(!keeps_meaning(
        &turn(typed, &b),
        "please backup saves before the game updates tonite"
    ));
    let fps = "pls set teh fps cap to 144 in teh game settings";
    rejected(fps, "please set the fps cap to 141 in the game settings");
    rejected(
        r#"pls rename teh folder to "old saves" in teh game dir"#,
        "please rename the folder to old saves in the game dir",
    );
    rejected(
        "pls ask @sam abuot teh raid tonite after dinner time",
        "please ask sam about the raid tonite after dinner time",
    );
    // Every span kept goes through.
    let fake = Fake::new("please set the fps cap to 144 in the game settings", 0);
    assert_eq!(
        run(&fake, fps, LIMIT).as_deref(),
        Some("please set the fps cap to 144 in the game settings")
    );
}

#[test]
fn a_changed_word_answer_or_new_intent_keeps_the_original() {
    // A word swapped for another, or a prefix that flips it.
    let drivers = "pls instal teh new drivers for my gpu tonight";
    rejected(drivers, "please remove the new drivers for my gpu tonight");
    rejected(
        drivers,
        "please uninstall the new drivers for my gpu tonight",
    );
    // A "not" lost to a near miss.
    rejected(
        "pls i cant updaet teh game drivers until teh weekend",
        "please I can update the game drivers until the weekend",
    );
    // An answer, and a preamble around the fix.
    rejected(TYPO, "Restart your router, then try again.");
    rejected(TYPO, "Corrected: please fix the wifi it's broken");
    // Too long, too short, two lines from one, the same text, nothing.
    rejected(
        TYPO,
        "please fix the wifi, it's broken again ugh ugh ugh ugh",
    );
    rejected(TYPO, "fix wifi it's broken");
    rejected(TYPO, "please fix the wifi,\nit's broken again");
    rejected(TYPO, TYPO);
    rejected(TYPO, "");
    // The same words, but the fix sorts as a repair, not the task it was
    // briefed as: the turn keeps the text its Brief was written for.
    let crash = "pls chek why teh game keeps crashng when i alt tab out of it";
    let fixed = "please check why the game keeps crashing when I alt tab out of it";
    assert_ne!(brief(crash).intent, brief(fixed).intent);
    rejected(crash, fixed);
    // Right spelling everywhere goes through, small words and all.
    let fake = Fake::new("please install the new drivers for my gpu tonight", 0);
    assert_eq!(
        run(&fake, drivers, LIMIT).as_deref(),
        Some("please install the new drivers for my gpu tonight")
    );
}

#[test]
fn quotes_a_small_model_adds_are_dropped() {
    let fake = Fake::new(&format!("\"{FIXED}\""), 0);
    assert_eq!(run(&fake, TYPO, LIMIT).as_deref(), Some(FIXED));
}

#[test]
fn a_slow_model_is_cut_off_and_blocks_the_next_call() {
    let slow = Fake::new(FIXED, 500);
    let lane = lane();
    let started = Instant::now();
    assert_eq!(run_on(lane, &slow, TYPO, LIMIT), None);
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "{:?}",
        started.elapsed()
    );
    // The timed-out call still runs: a new one is skipped, not queued.
    let fast = Fake::new(FIXED, 0);
    assert_eq!(run_on(lane, &fast, TYPO, LIMIT), None);
    assert_eq!(fast.calls(), 0);
    // Once it ends, the lane is free again.
    let deadline = Instant::now() + Duration::from_secs(5);
    while lane.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(run_on(lane, &fast, TYPO, LIMIT).as_deref(), Some(FIXED));
    assert_eq!((slow.calls(), fast.calls()), (1, 1));
}

#[test]
fn the_saved_turn_gets_the_typed_text_back() {
    let volatile = crate::prefix_cache::user_message_text("x", FIXED);
    let mut items = vec![
        InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText("sys".into())],
        },
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(format!("{volatile}\nhook note"))],
        },
        InputItem::Message {
            role: "assistant".into(),
            content: vec![ContentPart::InputText(FIXED.into())],
        },
    ];
    restore_typed(&mut items, FIXED, TYPO);
    let text = |i: usize| match &items[i] {
        InputItem::Message { content, .. } => match &content[0] {
            ContentPart::InputText(t) => t.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(
        crate::prefix_cache::visible_user_text(&text(1)),
        format!("{TYPO}\nhook note")
    );
    assert!(text(1).starts_with(&volatile[..volatile.len() - FIXED.len()]));
    // Only the user message changes.
    assert_eq!(text(2), FIXED);
}

/// The real engine: the model reads the tidied text, and the reopened chat
/// shows exactly what was typed.
mod engine {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{Fake, FIXED, TYPO};
    use crate::events::Engine;
    use crate::gate::{ClosedPermits, Gate, PermMode};
    use crate::harness::{use_key_store_for, MemoryKeyStore};
    use crate::route::cleanup::SwitchOn;
    use crate::route::local::ForceOn;
    use crate::{
        AuthKind, CancelToken, ContentPart, EngineParts, HaltCheck, InputItem, ModelClient,
        NativeEngine, SteerQueue,
    };

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Answers "ok" and keeps every request's input.
    #[derive(Default)]
    struct Count {
        inputs: Mutex<Vec<Vec<InputItem>>>,
    }

    impl ModelClient for Count {
        fn stream(
            &self,
            req: &crate::ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            self.inputs.lock().unwrap().push(req.input.clone());
            sink(crate::StreamEvent::TextDelta("ok".into()));
            Ok(crate::TurnOutput {
                text: "ok".into(),
                ..Default::default()
            })
        }
    }

    struct NoHalt;
    impl HaltCheck for NoHalt {
        fn halted(&self) -> bool {
            false
        }
    }

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn user_texts(items: &[InputItem]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                InputItem::Message { role, content } if role == "user" => match content.first() {
                    Some(ContentPart::InputText(t)) => {
                        Some(crate::prefix_cache::visible_user_text(t).to_string())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_model_reads_the_tidied_text_and_the_chat_keeps_the_typed_one() {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = Dir(std::env::temp_dir().join(format!("gh-tidy-{}-{n}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir.0);
        std::fs::create_dir_all(dir.0.join("cfg")).unwrap();
        std::fs::create_dir_all(dir.0.join("ws")).unwrap();
        let _cfg = crate::perm::ConfigGuard::set(dir.0.join("cfg"));
        use_key_store_for(&dir.0.join("cfg"), Arc::new(MemoryKeyStore::new()));
        let fake = Fake::new(FIXED, 0);
        let _local = ForceOn::with(Some(
            fake.clone() as Arc<dyn crate::route::local::LocalRuntime>
        ));
        let _switch = SwitchOn::new();
        let id = format!("tidy-{n}");
        let client = Arc::new(Count::default());
        let mut engine = NativeEngine::new(EngineParts {
            client: client.clone(),
            workspace: dir.0.join("ws"),
            model: "grok-4.7".into(),
            effort: None,
            system: "sys".into(),
            conversation_id: id.clone(),
            auth_kind: AuthKind::ApiKey,
            max_turns: 2,
            cancel: CancelToken::new(),
            steer: SteerQueue::new(),
            halt: Box::new(NoHalt),
            gate: Gate {
                mode: PermMode::Ask,
                readonly_session: false,
                attended: true,
                desktop: false,
            },
            desktop: None,
            permits: Arc::new(ClosedPermits),
        });
        engine.prompt(TYPO, None, &mut |_| {}).unwrap();
        assert_eq!(fake.calls(), 1);
        let sent = client.inputs.lock().unwrap().last().cloned().unwrap();
        assert_eq!(user_texts(&sent), [FIXED]);
        let saved = crate::session::load_session(&id).expect("session");
        assert_eq!(user_texts(&saved.input()), [TYPO]);
        assert_eq!(saved.title, TYPO);
    }
}
