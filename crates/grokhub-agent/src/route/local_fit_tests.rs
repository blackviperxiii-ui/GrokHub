use super::*;
use crate::gate::{Gate, PermMode};

fn text(role: &str, body: &str) -> InputItem {
    InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(body.into())],
    }
}

fn first_text(item: &InputItem) -> &str {
    match item {
        InputItem::Message { content, .. } => match content.first() {
            Some(ContentPart::InputText(t)) => t,
            _ => "",
        },
        _ => "",
    }
}

/// A chat of its own per request, so one test's sticky cuts never reach another.
fn chat_id() -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!(
        "local-fit-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    )
}

/// The real chat prompt: the composed system and every tool a chat offers.
fn real_request(history: Vec<InputItem>) -> ResponsesRequest {
    let ws = crate::harness::test_dir("local-fit");
    let voice =
        grokhub_core::personality::builtin(grokhub_core::personality::DEFAULT_ID).prompt_block();
    let system = crate::system_prompt_parts(
        &grokhub_core::cabin_rules::cabin_static_rules(false),
        &voice,
        &ws,
    );
    let tools = crate::tools::schemas_for(&Gate {
        mode: PermMode::Always,
        readonly_session: false,
        attended: true,
        desktop: false,
    });
    let mut input = vec![text("system", &system)];
    input.extend(history);
    ResponsesRequest {
        text_format: None,
        model: "local".into(),
        effort: None,
        input,
        conversation_id: chat_id(),
        tools,
        hosted_search: false,
        call_timeout: None,
    }
}

fn fits(req: &ResponsesRequest, n_ctx: u64) -> bool {
    request_tokens(req) + reply_reserve(n_ctx) <= n_ctx
}

#[test]
fn a_big_window_sends_the_request_unchanged() {
    let req = real_request(vec![text("user", "hi")]);
    let (out, done) = fit(&req, 131_072);
    assert_eq!(done, Fitted::default());
    assert_eq!(out.input, req.input);
    assert_eq!(out.tools, req.tools);
}

#[test]
fn the_full_chat_prompt_does_not_fit_small_windows_before_fitting() {
    let req = real_request(vec![text("user", "hi")]);
    assert!(!fits(&req, 4_096));
}

#[test]
fn an_8k_window_fits_a_chat_turn_with_room_to_answer() {
    let req = real_request(vec![text("user", "what's in my downloads folder?")]);
    let (out, done) = fit(&req, 8_192);
    assert!(fits(&out, 8_192), "{} tokens", request_tokens(&out));
    assert_eq!(done.turns_dropped, 0);
    let names: Vec<&str> = out
        .tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for core in [
        "read_file",
        "list_dir",
        "run_terminal_command",
        "web_search",
    ] {
        assert!(names.contains(&core), "{core} kept");
    }
    assert_eq!(
        first_text(out.input.last().unwrap()),
        "what's in my downloads folder?"
    );
}

#[test]
fn a_4k_window_fits_and_keeps_the_safety_and_cabin_rules() {
    let req = real_request(vec![text("user", "hey")]);
    let (out, done) = fit(&req, 4_096);
    assert!(fits(&out, 4_096), "{} tokens", request_tokens(&out));
    assert!(done.tools_cut && done.system_cut);
    let names: Vec<&str> = out
        .tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.iter().all(|n| CORE_TOOLS.contains(n)));
    assert!(names.contains(&"read_file"));
    let system = first_text(&out.input[0]);
    assert!(system.contains("<dangerous_actions>"));
    assert!(system.contains("Ask only before sending a message, paying"));
    assert!(system.contains("<memory>"));
    assert!(!system.contains("<formatting>"));
}

/// The prompt tells a model it may search "when the tool list includes"
/// web_search. A small on-device window and a plain-talk chat both keep it
/// in the list, so no turn is told it can search without the tool.
#[test]
fn a_small_window_and_a_talk_chat_still_offer_the_lookups() {
    let req = real_request(vec![text("user", "what's the best budget ssd right now?")]);
    let (out, done) = fit(&req, 4_096);
    assert!(done.tools_cut);
    let names: Vec<&str> = out
        .tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"web_search"), "{names:?}");
    for lookup in ["web_search", "web_fetch"] {
        assert!(!crate::tools::talk::is_demoted(lookup), "{lookup}");
    }
}

#[test]
fn a_long_chat_keeps_the_newest_whole_turns_and_says_so() {
    let mut history = Vec::new();
    for i in 0..30 {
        history.push(text(
            "user",
            &format!("question {i} {}", "filler ".repeat(60)),
        ));
        history.push(InputItem::FunctionCall {
            call_id: format!("c{i}"),
            name: "read_file".into(),
            arguments: r#"{"path":"notes.txt"}"#.into(),
        });
        history.push(InputItem::FunctionCallOutput {
            call_id: format!("c{i}"),
            output: "line ".repeat(80),
        });
        history.push(text("assistant", &format!("answer {i}")));
    }
    history.push(text("user", "my dog is called Biscuit, remember that"));
    history.push(text("assistant", "Got it."));
    history.push(text("user", "what's my dog called?"));
    let req = real_request(history);
    let (out, done) = fit(&req, 8_192);
    assert!(fits(&out, 8_192), "{} tokens", request_tokens(&out));
    assert!(done.turns_dropped > 0);
    assert!(first_text(&out.input[0]).ends_with(TRIMMED_NOTE));
    // After the system message comes a whole turn, never a stray tool result.
    assert!(matches!(&out.input[1], InputItem::Message { role, .. } if role == "user"));
    for (i, item) in out.input.iter().enumerate() {
        if let InputItem::FunctionCallOutput { call_id, .. } = item {
            assert!(out.input[..i].iter().any(
                |prev| matches!(prev, InputItem::FunctionCall { call_id: c, .. } if c == call_id)
            ));
        }
    }
    // The recent fact the question needs is still there.
    assert!(out
        .input
        .iter()
        .any(|item| first_text(item).contains("Biscuit")));
    assert_eq!(
        first_text(out.input.last().unwrap()),
        "what's my dog called?"
    );
}

#[test]
fn older_turns_lose_their_cabin_notes_and_the_newest_keeps_them() {
    let note = "Playbook for this turn: Code changes. ".repeat(40);
    let older = crate::prefix_cache::user_message_text(&note, "first");
    let newest =
        crate::prefix_cache::user_message_text("Talk style for this turn: short.", "second");
    let mut history = Vec::new();
    for _ in 0..8 {
        history.push(text("user", &older));
        history.push(text("assistant", "ok"));
    }
    history.push(text("user", &newest));
    let req = real_request(history);
    let (out, done) = fit(&req, 8_192);
    assert!(done.notes_cut);
    assert_eq!(done.turns_dropped, 0);
    let users: Vec<&str> = out
        .input
        .iter()
        .filter(|i| matches!(i, InputItem::Message { role, .. } if role == "user"))
        .map(first_text)
        .collect();
    assert_eq!(users.len(), 9);
    assert!(users[..8].iter().all(|u| *u == "first"));
    assert_eq!(users[8], newest);
}

#[test]
fn the_reply_reserve_is_a_quarter_within_bounds() {
    assert_eq!(reply_reserve(1_024), 512);
    assert_eq!(reply_reserve(8_192), 2_048);
    assert_eq!(reply_reserve(131_072), 4_096);
}

#[test]
fn a_huge_agents_md_is_cut_at_a_line_and_says_so() {
    let lines: Vec<String> = (0..3_000)
        .map(|i| format!("- rule {i}: keep it tidy"))
        .collect();
    let system = format!(
        "You are the cabin assistant.\n\n<dangerous_actions>\n- Ask before paying.\n</dangerous_actions>\n\n<agents>\n{}\n</agents>",
        lines.join("\n")
    );
    let req = ResponsesRequest {
        text_format: None,
        model: "local".into(),
        effort: None,
        input: vec![text("system", &system), text("user", "hi")],
        conversation_id: chat_id(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    };
    let (out, done) = fit(&req, 4_096);
    assert!(fits(&out, 4_096), "{} tokens", request_tokens(&out));
    assert!(done.agents_cut);
    assert_eq!(done.turns_dropped, 0);
    let sys = first_text(&out.input[0]);
    assert!(sys.contains("- Ask before paying."));
    assert!(sys.contains("<agents>\n- rule 0: keep it tidy\n"));
    assert!(sys.contains("left out to fit the on-device model.]\n</agents>"));
    // Cut on a whole line: every kept rule line is complete.
    let body = &sys[sys.find("<agents>\n").unwrap() + 9..sys.find("\n[The rest").unwrap()];
    assert!(body.lines().all(|l| l.ends_with("keep it tidy")));
    assert!(agents_tokens(&out.input) <= 4_096 / 4);
}

/// A device runtime that reports a small window and keeps what it was sent.
struct SmallWindow {
    n_ctx: Option<u64>,
    sent: std::sync::Mutex<Vec<ResponsesRequest>>,
}

impl crate::route::local::LocalRuntime for SmallWindow {
    fn classify(&self, _: &str, _: &[&str]) -> Result<serde_json::Value, String> {
        Err("unused".into())
    }
    fn summarize(&self, _: &str, _: u32) -> Result<serde_json::Value, String> {
        Err("unused".into())
    }
    fn difficulty(&self, _: &str) -> Result<serde_json::Value, String> {
        Err("unused".into())
    }
    fn context_tokens(&self) -> Option<u64> {
        self.n_ctx
    }
    fn chat_tools(
        &self,
        req: &ResponsesRequest,
        _: &crate::client::CancelToken,
    ) -> Result<crate::client::TurnOutput, String> {
        self.sent.lock().unwrap().push(req.clone());
        Ok(crate::client::TurnOutput {
            text: "ok".into(),
            ..Default::default()
        })
    }
}

#[test]
fn on_device_chat_sends_only_what_fits_the_device_window() {
    let rt = std::sync::Arc::new(SmallWindow {
        n_ctx: Some(4_096),
        sent: Default::default(),
    });
    let _on = crate::route::local::ForceOn::with(Some(rt.clone()));
    let req = real_request(vec![text("user", "hey")]);
    let out = crate::route::local::serve_request(
        "chat:default",
        &req,
        &crate::client::CancelToken::new(),
    )
    .unwrap();
    assert_eq!(out.text, "ok");
    let sent = rt.sent.lock().unwrap();
    assert!(fits(&sent[0], 4_096), "{} tokens", request_tokens(&sent[0]));
    assert_eq!(first_text(sent[0].input.last().unwrap()), "hey");
}

#[test]
fn an_unknown_window_falls_back_to_the_launch_default() {
    let rt = std::sync::Arc::new(SmallWindow {
        n_ctx: None,
        sent: Default::default(),
    });
    let _on = crate::route::local::ForceOn::with(Some(rt.clone()));
    let req = real_request(vec![text("user", "hey")]);
    crate::route::local::serve_request("chat:default", &req, &crate::client::CancelToken::new())
        .unwrap();
    let n_ctx = crate::route::local_perf::current_defaults().ctx;
    assert!(fits(&rt.sent.lock().unwrap()[0], n_ctx));
}

#[test]
fn one_message_longer_than_the_window_keeps_its_start_and_end() {
    let start = "START of the pasted log. ";
    let end = " END of the pasted log.";
    let body = format!("{start}{}{end}", "line of a very long log\n".repeat(4_000));
    let req = real_request(vec![text("user", &body)]);
    let n_ctx = 4_096;
    let (out, done) = fit(&req, n_ctx);
    assert!(done.newest_clipped, "{done:?}");
    assert!(
        request_tokens(&out) <= n_ctx - reply_reserve(n_ctx),
        "{} tokens",
        request_tokens(&out)
    );
    let sent = first_text(out.input.last().unwrap());
    assert!(sent.starts_with(start), "{}", &sent[..80]);
    assert!(sent.ends_with(end));
    assert!(sent.contains(CLIPPED_NOTE));
    // A message that fits is never clipped.
    let (_, small) = fit(&real_request(vec![text("user", "hi")]), n_ctx);
    assert!(!small.newest_clipped);
}

#[test]
fn a_clip_drops_the_newest_notes_first_and_never_cuts_a_short_message() {
    use crate::prefix_cache::{WRAPPER_END, WRAPPER_START};
    let typed = format!("TYPED START {} TYPED END", "word ".repeat(6_000));
    let notes = "cabin note line\n".repeat(200);
    let body = format!("{WRAPPER_START}{notes}{WRAPPER_END}\n\n{typed}");
    let req = real_request(vec![text("user", &body)]);
    let (out, done) = fit(&req, 4_096);
    let sent = first_text(out.input.last().unwrap());
    assert!(done.notes_cut && done.newest_clipped, "{done:?}");
    assert!(!sent.contains(WRAPPER_START) && !sent.contains("cabin note line"));
    assert!(sent.starts_with("TYPED START") && sent.ends_with("TYPED END"));
    // A short (multibyte) newest message under an oversized system prompt stays whole.
    let short = format!("a{}", "д".repeat(100));
    let mut input = vec![text("user", &short)];
    assert!(!clip_newest_user(&mut input, 50_000));
    assert_eq!(first_text(&input[0]), short);
}

fn long_chat(turns: usize) -> Vec<InputItem> {
    let mut history = Vec::new();
    for i in 0..turns {
        history.push(text(
            "user",
            &format!("question {i} {}", "filler ".repeat(60)),
        ));
        history.push(InputItem::FunctionCall {
            call_id: format!("c{i}"),
            name: "read_file".into(),
            arguments: r#"{"path":"notes.txt"}"#.into(),
        });
        history.push(InputItem::FunctionCallOutput {
            call_id: format!("c{i}"),
            output: "line ".repeat(80),
        });
        history.push(text("assistant", &format!("answer {i}")));
    }
    history.push(text("user", "what next?"));
    history
}

#[test]
fn a_chats_cuts_stay_put_so_the_next_turn_only_appends() {
    let mut req = real_request(long_chat(30));
    let (first, done) = fit(&req, 8_192);
    assert!(fits(&first, 8_192));
    assert!(done.tools_cut && done.turns_dropped > 0);
    // Two short turns later the same messages lead the request, unchanged.
    for i in 0..2 {
        req.input.push(text("assistant", "ok"));
        req.input.push(text("user", &format!("and then {i}?")));
        let (next, again) = fit(&req, 8_192);
        assert!(fits(&next, 8_192));
        assert_eq!(again.turns_dropped, done.turns_dropped);
        assert_eq!(
            format!("{:?}", &next.input[..first.input.len() - 1]),
            format!("{:?}", &first.input[..first.input.len() - 1])
        );
    }
    // A fresh chat with the same history cuts the same way from scratch.
    let mut fresh = req.clone();
    fresh.conversation_id = chat_id();
    assert_eq!(fit(&fresh, 8_192).1.turns_dropped, done.turns_dropped);
}

#[test]
fn when_the_history_must_move_it_moves_by_a_quarter_of_the_room() {
    let mut req = real_request(long_chat(30));
    let (_, done) = fit(&req, 8_192);
    let mut moved = None;
    for i in 0..40 {
        req.input.push(text("assistant", "ok"));
        req.input
            .push(text("user", &format!("more {i} {}", "filler ".repeat(60))));
        let (out, now) = fit(&req, 8_192);
        assert!(fits(&out, 8_192));
        assert!(now.turns_dropped >= done.turns_dropped, "never moves back");
        if now.turns_dropped != done.turns_dropped {
            moved = Some((i, now.turns_dropped - done.turns_dropped));
            break;
        }
    }
    let (after, step) = moved.expect("the boundary moved");
    // Several turns went by before it moved, and it moved several turns at once.
    assert!(after >= 3, "moved after {after} turns");
    assert!(step >= 8, "moved {step} messages");
}

#[test]
fn a_bigger_window_or_another_chat_start_begins_clean() {
    let req = real_request(long_chat(30));
    let (_, small) = fit(&req, 8_192);
    let (_, big) = fit(&req, 131_072);
    assert_eq!((small.tools_cut, big.tools_cut), (true, false));
    assert_eq!(big.turns_dropped, 0);
    let mut other = real_request(long_chat(3));
    other.conversation_id = req.conversation_id.clone();
    let (_, clean) = fit(&other, 131_072);
    assert_eq!(clean, Fitted::default());
}

fn tool_names(req: &ResponsesRequest) -> Vec<&str> {
    req.tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect()
}

#[test]
fn a_4k_window_keeps_load_tool_schema_so_deferred_tools_stay_reachable() {
    let req = real_request(vec![text("user", "hey")]);
    assert!(tool_names(&req).contains(&"load_tool_schema"));
    let (out, done) = fit(&req, 4_096);
    assert!(done.tools_cut);
    assert!(fits(&out, 4_096), "{} tokens", request_tokens(&out));
    let names = tool_names(&out);
    assert!(names.contains(&"load_tool_schema"), "{names:?}");
    assert!(names.contains(&"web_search"), "{names:?}");
    // Its index is cut to the names, and still names web_fetch.
    let load = out
        .tools
        .iter()
        .find(|t| t["name"] == "load_tool_schema")
        .unwrap();
    let desc = load["description"].as_str().unwrap();
    let listed = desc.split_once("Deferred tools: ").unwrap().1;
    assert!(listed.split(", ").any(|n| n == "web_fetch"), "{desc}");
    assert!(!desc.contains(" — "), "{desc}");
}

/// One on-device turn on a 4k window; what the device was sent.
fn sent_on_4k(ask: &str, can_draw: bool) -> ResponsesRequest {
    let rt = std::sync::Arc::new(SmallWindow {
        n_ctx: Some(4_096),
        sent: Default::default(),
    });
    let _on = crate::route::local::ForceOn::with(Some(rt.clone()));
    let _kit = crate::route::local_image::KitForTests::set(can_draw);
    let req = real_request(vec![text("user", ask)]);
    crate::route::local::serve_request("chat:default", &req, &crate::client::CancelToken::new())
        .unwrap();
    let sent = rt.sent.lock().unwrap()[0].clone();
    assert!(fits(&sent, 4_096), "{} tokens", request_tokens(&sent));
    sent
}

#[test]
fn a_small_window_keeps_the_picture_tool_when_this_computer_can_draw() {
    use crate::route::local_image as li;
    for ask in ["hey", "draw me a fox in the snow"] {
        let sent = sent_on_4k(ask, true);
        let names = tool_names(&sent);
        assert!(names.contains(&"image_generate"), "{ask}: {names:?}");
        assert!(names.contains(&"load_tool_schema"), "{ask}: {names:?}");
        let image = sent
            .tools
            .iter()
            .find(|t| t["name"] == "image_generate")
            .unwrap();
        assert_eq!(image["description"], "Generate images.");
        assert_eq!(
            image["parameters"]["required"],
            serde_json::json!(["prompt"])
        );
        let system = first_text(&sent.input[0]);
        assert!(system.contains(li::CAN_DRAW), "{ask}");
        assert!(!system.contains(li::NO_KIT), "{ask}");
        assert_eq!(system.contains(li::ASKED), ask != "hey", "{ask}");
    }
}

#[test]
fn with_nothing_to_draw_with_only_a_picture_ask_says_what_to_download() {
    use crate::route::local_image as li;
    let plain = sent_on_4k("hey", false);
    assert!(!tool_names(&plain).contains(&"image_generate"));
    assert!(!first_text(&plain.input[0]).contains("[Pictures]"));
    let ask = sent_on_4k("make a picture of a castle at night", false);
    // Kept, so an 18+ prompt gets the tool's own notice and a clean one can
    // still reach Grok Imagine when signed in.
    assert!(tool_names(&ask).contains(&"image_generate"));
    let system = first_text(&ask.input[0]);
    assert!(system.contains(li::NO_KIT));
    assert!(!system.contains(li::CAN_DRAW));
    assert!(li::NO_KIT.contains("Settings → Download models → Image"));
    assert!(li::NO_KIT.contains("picture guard"));
}
