use std::time::{Duration, Instant};

use super::*;
use crate::brain::{classify, BriefInput};

const DISK_LINUX: Option<(&str, &str)> = Some(("diagnose", r#"{"probes":["disk_usage"]}"#));
const RAM_LINUX: Option<(&str, &str)> = Some(("diagnose", r#"{"probes":["memory"]}"#));
const LOCAL_AI: Option<(&str, &str)> = Some(("local_ai_status", "{}"));
const AUTOMATIONS: Option<(&str, &str)> = Some(("automation_list", "{}"));
const SKILLS: Option<(&str, &str)> = Some(("skill_list", "{}"));
const CONNECTIONS: Option<(&str, &str)> = Some(("connection_list", "{}"));

/// The Brief the engine writes for a plain chat turn.
fn brief(text: &str) -> Brief {
    classify(
        &BriefInput {
            text,
            ..Default::default()
        },
        None,
    )
}

fn pick(text: &str, os: Os) -> Option<(&'static str, &'static str)> {
    route(text, &brief(text), Turn::default(), os).map(|p| (p.tool, p.args))
}

/// Asks with the instant answer each must get on Linux. `None` means the model
/// answers. Near misses (a write, a second clause, another subject) dominate.
const LABELLED: &[(&str, Option<(&str, &str)>)] = &[
    ("how much disk space do I have", DISK_LINUX),
    ("How much disk space do I have?", DISK_LINUX),
    ("how much free space is on my drive?", DISK_LINUX),
    ("is my drive full?", DISK_LINUX),
    ("how much RAM am I using?", RAM_LINUX),
    ("how much RAM is left?", RAM_LINUX),
    ("what is my memory usage", RAM_LINUX),
    ("is local ai running?", LOCAL_AI),
    ("is my local model running?", LOCAL_AI),
    ("list my automations", AUTOMATIONS),
    ("list all my automations", AUTOMATIONS),
    ("what automations do I have?", AUTOMATIONS),
    ("list my skills", SKILLS),
    ("please list my skills", SKILLS),
    ("can you list all my skills", SKILLS),
    ("which skills are installed?", SKILLS),
    ("list my connections", CONNECTIONS),
    ("what connections do I have?", CONNECTIONS),
    ("what MCP servers do I have?", CONNECTIONS),
    // The Brief sorts these as chat, so the model answers them.
    ("check my disk space", None),
    ("please show me my disk usage", None),
    ("what's my memory usage", None),
    ("local ai status", None),
    ("Show my MCP servers.", None),
    // Near misses: a write, a second clause, another subject.
    ("how much disk space do I have and clean downloads", None),
    ("delete my automations", None),
    ("disable my automations", None),
    ("create an automation that checks disk space", None),
    ("delete my skills", None),
    ("make a skill that lists my skills", None),
    ("list my skills, then delete the old ones", None),
    ("list my skills and connections", None),
    ("what are my skills", None),
    ("what skills do I need for a dev job", None),
    ("how much RAM do I have", None),
    ("show my network connections", None),
    ("add a connection to github", None),
    ("remove all my connections", None),
    ("free up disk space", None),
    ("why is my disk full", None),
    ("clean my disk", None),
    ("how much disk space does Cyberpunk need", None),
    ("how much disk space do I have on my phone", None),
    ("open task manager", None),
    ("show me my GPU temperature", None),
    ("what's my CPU temperature", None),
    ("turn on local ai", None),
    ("is local ai worth it", None),
    ("run a diagnostics check on my pc", None),
    ("list the files in my downloads folder", None),
    ("/compact", None),
];

#[test]
fn labelled_asks_get_the_right_tool_or_none() {
    assert!(LABELLED.len() >= 40, "{}", LABELLED.len());
    let mut wrong = Vec::new();
    for (ask, want) in LABELLED {
        let got = pick(ask, Os::Linux);
        if got != *want {
            wrong.push(format!("{ask:?}: want {want:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn disk_space_picks_the_probe_for_each_os() {
    assert_eq!(pick("how much disk space do I have", Os::Linux), DISK_LINUX);
    assert_eq!(
        pick("how much disk space do I have", Os::Windows),
        Some(("diagnose", r#"{"probes":["volumes"]}"#))
    );
    // Windows has no memory probe: the model answers.
    assert_eq!(pick("how much RAM am I using?", Os::Windows), None);
    assert_eq!(pick("list my skills", Os::Windows), SKILLS);
}

#[test]
fn a_second_clause_or_a_write_falls_through() {
    assert_eq!(
        pick(
            "how much disk space do I have and clean downloads",
            Os::Linux
        ),
        None
    );
    assert_eq!(pick("delete my automations", Os::Linux), None);
}

/// Every listed phrase reaches its own row through the real Brief, so no row
/// is dead weight and no phrase sits in two rows.
#[test]
fn every_phrase_routes_to_its_row() {
    let mut seen = std::collections::HashSet::new();
    for row in ROWS {
        for phrase in row.phrases {
            assert!(seen.insert(*phrase), "{phrase:?} is listed twice");
            assert_eq!(normalize(phrase).as_deref(), Some(*phrase), "{phrase:?}");
            for os in [Os::Linux, Os::Windows] {
                let want = match os {
                    Os::Linux => row.linux,
                    Os::Windows => row.windows,
                };
                assert_eq!(
                    pick(phrase, os),
                    want.map(|args| (row.tool, args)),
                    "{phrase:?} on {os:?} (brief {:?})",
                    brief(phrase)
                );
            }
        }
    }
}

#[test]
fn only_a_sure_foreground_text_turn_is_answered() {
    let ask = "list my skills";
    let sure = brief(ask);
    assert_eq!(sure.intent, Intent::Task);
    assert!(sure.confidence >= MIN_CONFIDENCE, "{sure:?}");
    assert!(route(ask, &sure, Turn::default(), Os::Linux).is_some());
    for turn in [
        Turn {
            image: true,
            ..Turn::default()
        },
        Turn {
            background: true,
            ..Turn::default()
        },
        Turn {
            episode: true,
            ..Turn::default()
        },
    ] {
        assert_eq!(route(ask, &sure, turn, Os::Linux), None, "{turn:?}");
    }
    let unsure = Brief {
        confidence: MIN_CONFIDENCE - 1,
        ..sure.clone()
    };
    assert_eq!(route(ask, &unsure, Turn::default(), Os::Linux), None);
    for intent in [Intent::Chat, Intent::Repair, Intent::Background] {
        let other = Brief {
            intent,
            ..sure.clone()
        };
        assert_eq!(
            route(ask, &other, Turn::default(), Os::Linux),
            None,
            "{intent:?}"
        );
    }
}

#[test]
fn long_asks_fall_through() {
    let sure = brief("list my skills");
    let long = "please please please could you list my skills for me thanks";
    assert!(long.split_whitespace().count() > MAX_WORDS);
    assert_eq!(route(long, &sure, Turn::default(), Os::Linux), None);
}

#[test]
fn normalize_drops_polite_words_and_keeps_the_ask() {
    assert_eq!(
        normalize("Hey grok, list my skills").as_deref(),
        None,
        "a comma is not cleaned up"
    );
    assert_eq!(
        normalize("Could you list all my skills please?").as_deref(),
        Some("list my skills")
    );
    assert_eq!(normalize("what's my memory usage").as_deref(), None);
    assert_eq!(normalize("please").as_deref(), None);
    assert_eq!(normalize("list `skills`").as_deref(), None);
}

#[test]
fn reply_fills_the_template_or_says_there_is_nothing() {
    let skills = route(
        "list my skills",
        &brief("list my skills"),
        Turn::default(),
        Os::Linux,
    )
    .expect("instant");
    assert_eq!(
        skills.reply("qa: Ship check [builtin]\n"),
        "Your skills:\n\nqa: Ship check [builtin]"
    );
    assert_eq!(skills.reply("no skills"), "You don't have any skills yet.");
    let autos = route(
        "list my automations",
        &brief("list my automations"),
        Turn::default(),
        Os::Linux,
    )
    .expect("instant");
    assert_eq!(
        autos.reply("no automations"),
        "You don't have any automations yet."
    );
    let disk = route(
        "is my disk full",
        &brief("is my disk full"),
        Turn::default(),
        Os::Linux,
    )
    .expect("instant");
    assert_eq!(
        disk.reply("Your disks have enough free space."),
        "Your disks have enough free space."
    );
}

/// Routing is string work on a small table: well under 5 ms an ask, even in
/// a debug build on a busy CI runner.
#[test]
fn routing_takes_under_five_ms() {
    let briefs: Vec<(&str, Brief)> = LABELLED.iter().map(|(a, _)| (*a, brief(a))).collect();
    let mut worst = Duration::ZERO;
    for (ask, b) in &briefs {
        let start = Instant::now();
        let _ = route(ask, b, Turn::default(), Os::Linux);
        worst = worst.max(start.elapsed());
    }
    assert!(worst < Duration::from_millis(5), "{worst:?}");
}

/// The real engine: an instant answer makes no model request, shows the
/// tool card, and leaves a well-formed turn for the next model request.
mod engine {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use grokhub_core::wire::AcpEvent;

    use crate::events::Engine;
    use crate::gate::{ClosedPermits, Gate, PermMode};
    use crate::harness::{grant_scope, use_key_store_for, MemoryKeyStore, Scope, UserClick};
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

    impl Count {
        fn requests(&self) -> usize {
            self.inputs.lock().unwrap().len()
        }
        fn last(&self) -> Vec<InputItem> {
            self.inputs
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default()
        }
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

    struct Rig {
        dir: PathBuf,
        id: String,
        client: Arc<Count>,
        engine: NativeEngine,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A scratch config and workspace; the caller pins the config with a guard.
    fn rig(tag: &str) -> Rig {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gh-instant-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::create_dir_all(dir.join("ws")).unwrap();
        use_key_store_for(&dir.join("cfg"), Arc::new(MemoryKeyStore::new()));
        let id = format!("instant-{tag}-{n}");
        let client = Arc::new(Count::default());
        let engine = NativeEngine::new(EngineParts {
            client: client.clone(),
            workspace: dir.join("ws"),
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
        Rig {
            dir,
            id,
            client,
            engine,
        }
    }

    fn send(engine: &mut NativeEngine, text: &str) -> Vec<AcpEvent> {
        let mut events = Vec::new();
        engine
            .prompt(text, None, &mut |ev| events.push(ev))
            .unwrap();
        events
    }

    fn stored(id: &str) -> Vec<InputItem> {
        crate::session::load_session(id).expect("session").input()
    }

    fn statuses(events: &[AcpEvent], tool: &str) -> Vec<String> {
        events
            .iter()
            .filter_map(|ev| match ev {
                AcpEvent::Tool(card) if card.kind == tool => Some(card.status.clone()),
                _ => None,
            })
            .collect()
    }

    fn reply(events: &[AcpEvent]) -> String {
        events
            .iter()
            .filter_map(|ev| match ev {
                AcpEvent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    fn ended(events: &[AcpEvent]) -> bool {
        matches!(events.last(), Some(AcpEvent::Done { stop_reason }) if stop_reason == "end_turn")
    }

    fn text_of(item: &InputItem) -> Option<(&str, &str)> {
        match item {
            InputItem::Message { role, content } => match content.first() {
                Some(ContentPart::InputText(t)) => Some((role.as_str(), t.as_str())),
                _ => None,
            },
            _ => None,
        }
    }

    fn cfg(dir: &Path) -> PathBuf {
        dir.join("cfg")
    }

    #[test]
    fn list_my_skills_answers_with_no_model_request() {
        let mut rig = rig("skills");
        let _cfg = crate::perm::ConfigGuard::set(cfg(&rig.dir));
        // A question first: after small talk the Brief calls a short ask
        // chat, and chat goes to the model.
        send(&mut rig.engine, "what does a frame cap do?");
        assert_eq!(rig.client.requests(), 1);
        let before = stored(&rig.id).len();

        let events = send(&mut rig.engine, "list my skills");
        assert_eq!(rig.client.requests(), 1, "no model request");
        assert_eq!(
            statuses(&events, "skill_list"),
            ["in_progress", "completed"]
        );
        let text = reply(&events);
        let (builtin, _) = crate::skills::BUILTIN_SKILL_BODIES[0];
        assert!(text.starts_with("Your skills:\n\n"), "{text}");
        assert!(text.contains(builtin), "{text}");
        assert!(events.iter().any(|ev| matches!(ev, AcpEvent::Usage(_))));
        assert!(ended(&events), "{events:?}");

        let after = stored(&rig.id);
        assert_eq!(after.len(), before + 4, "{after:?}");
        let turn = &after[before..];
        assert_eq!(text_of(&turn[0]), Some(("user", "list my skills")));
        let InputItem::FunctionCall {
            call_id,
            name,
            arguments,
        } = &turn[1]
        else {
            panic!("{turn:?}");
        };
        assert_eq!((name.as_str(), arguments.as_str()), ("skill_list", "{}"));
        assert!(
            call_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{call_id}"
        );
        assert!(
            matches!(&turn[2], InputItem::FunctionCallOutput { call_id: out, output } if out == call_id && output.contains(builtin)),
            "{turn:?}"
        );
        assert_eq!(text_of(&turn[3]), Some(("assistant", text.as_str())));

        // The next model request carries the call and its output, paired.
        send(&mut rig.engine, "which one checks my work before I ship?");
        assert_eq!(rig.client.requests(), 2);
        let input = rig.client.last();
        let at = input
            .iter()
            .position(|i| matches!(i, InputItem::FunctionCall { name, .. } if name == "skill_list"))
            .expect("call in the next request");
        assert!(
            matches!(&input[at + 1], InputItem::FunctionCallOutput { call_id: out, .. } if out == call_id),
            "{input:?}"
        );
        assert!(matches!(text_of(&input[at + 2]), Some(("assistant", t)) if t == text));
    }

    #[test]
    fn a_first_turn_keeps_the_system_prompt_first() {
        let mut rig = rig("first");
        let _cfg = crate::perm::ConfigGuard::set(cfg(&rig.dir));
        let events = send(&mut rig.engine, "list my automations");
        assert_eq!(rig.client.requests(), 0);
        assert_eq!(
            statuses(&events, "automation_list"),
            ["in_progress", "completed"]
        );
        assert_eq!(reply(&events), "You don't have any automations yet.");
        let turn = stored(&rig.id);
        assert_eq!(turn.len(), 5, "{turn:?}");
        assert_eq!(text_of(&turn[0]), Some(("system", "sys")));
        assert_eq!(text_of(&turn[1]), Some(("user", "list my automations")));
    }

    #[test]
    fn diagnose_without_system_state_goes_to_the_model() {
        let mut rig = rig("noscope");
        let _cfg = crate::perm::ConfigGuard::set(cfg(&rig.dir));
        let events = send(&mut rig.engine, "how much disk space do I have");
        assert_eq!(rig.client.requests(), 1);
        assert!(statuses(&events, "diagnose").is_empty(), "{events:?}");
        assert_eq!(reply(&events), "ok");
    }

    #[test]
    fn diagnose_with_system_state_answers_with_no_model_request() {
        let mut rig = rig("scope");
        let _cfg = crate::perm::ConfigGuard::set(cfg(&rig.dir));
        grant_scope(
            &cfg(&rig.dir),
            &Scope::SystemState,
            None,
            UserClick::from_click(),
        )
        .expect("grant");
        let events = send(&mut rig.engine, "how much disk space do I have");
        assert_eq!(rig.client.requests(), 0);
        assert_eq!(statuses(&events, "diagnose"), ["in_progress", "completed"]);
        let text = reply(&events);
        assert!(
            text.contains("Nothing on your computer was changed."),
            "{text}"
        );
        assert!(ended(&events));
    }

    #[test]
    fn only_turn_hooks_keep_a_session_off_instant_answers() {
        let hook = |event: &str| crate::hooks::TestHook {
            event: event.into(),
            matcher: String::new(),
            command: "exit 0".into(),
            timeout: std::time::Duration::from_secs(1),
            source_dir: std::env::temp_dir(),
        };
        let pre = "instant-hooks-pre";
        let _a = crate::hooks::install_test_hooks(pre, vec![hook("PreToolUse")]);
        assert!(!crate::hooks::has_turn_hooks(pre));
        let stop = "instant-hooks-stop";
        let _b = crate::hooks::install_test_hooks(stop, vec![hook("Stop")]);
        assert!(crate::hooks::has_turn_hooks(stop));
        assert!(!crate::hooks::has_turn_hooks("instant-hooks-none"));
    }
}
