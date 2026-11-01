//! Workboard and ideas board tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

/// Drives the real Workboards page one 60 fps frame at a time.
struct BoardPage {
    ctx: egui::Context,
    frame: u32,
    /// A text field painted above the page, standing in for any focused field.
    field: Option<egui::Id>,
}

impl BoardPage {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        Self {
            ctx,
            frame: 0,
            field: None,
        }
    }

    fn step(&mut self, cabin: &mut Cabin, events: Vec<egui::Event>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            time: Some(f64::from(self.frame) / 60.0),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        let field = self.field;
        let _ = crate::theme::test_pass(&self.ctx, raw, |ui| {
            if let Some(id) = field {
                let mut text = String::from("half a thought");
                ui.add(egui::TextEdit::singleline(&mut text).id(id));
            }
            cabin.ui_board(ui);
        });
        self.frame += 1;
    }

    fn rect(&self, key: (&str, &str)) -> egui::Rect {
        self.ctx
            .read_response(egui::Id::new(key))
            .unwrap_or_else(|| panic!("{key:?} is painted"))
            .rect
    }

    fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn click(&mut self, cabin: &mut Cabin, pos: egui::Pos2) {
        self.step(
            cabin,
            vec![egui::Event::PointerMoved(pos), Self::press(pos, true)],
        );
        self.step(cabin, vec![Self::press(pos, false)]);
        self.step(cabin, Vec::new());
    }

    fn esc(&mut self, cabin: &mut Cabin, modifiers: egui::Modifiers) {
        self.step(
            cabin,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
        );
        self.step(
            cabin,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            }],
        );
    }

    /// Press at `from`, slide to `to` over 20 frames, let go there.
    fn drag(&mut self, cabin: &mut Cabin, from: egui::Pos2, to: egui::Pos2) -> Vec<Option<String>> {
        let mut open = Vec::new();
        self.step(
            cabin,
            vec![egui::Event::PointerMoved(from), Self::press(from, true)],
        );
        for i in 1..=20 {
            let at = from + (to - from) * (i as f32 / 20.0);
            self.step(cabin, vec![egui::Event::PointerMoved(at)]);
            open.push(cabin.board_view.open.clone());
        }
        self.step(cabin, vec![Self::press(to, false)]);
        open.push(cabin.board_view.open.clone());
        self.step(cabin, Vec::new());
        open.push(cabin.board_view.open.clone());
        open
    }
}

#[test]
fn workboard_cards_open_on_click_not_hover_and_fold_on_a_second_click_or_esc() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("board-click-open");
    let mut a = grokhub_core::BoardCard::new("Harbor lamp", "Swap the bulb on the north pier.", "");
    a.status = grokhub_core::BoardStatus::Todo;
    a.id = "card-a".into();
    let mut b = grokhub_core::BoardCard::new("Tide chart", "Print the October tides.", "");
    // In its own column, so card A opening above it never pushes it off screen.
    b.status = grokhub_core::BoardStatus::Blocked;
    b.id = "card-b".into();
    let mut f =
        grokhub_core::BoardCard::new("Nightly ledger report", "Two invoices are overdue.", "");
    f.status = grokhub_core::BoardStatus::FollowUp;
    f.id = "card-f".into();
    cabin.board = vec![a, b, f];
    let mut page = BoardPage::new();
    page.step(&mut cabin, Vec::new());
    page.step(&mut cabin, Vec::new());
    let card_a = page.rect(("board-card", "card-a")).center();
    let card_f = page.rect(("board-card", "card-f")).center();

    // Rest on card A for 2 s, far past the old 0.3 s hover delay.
    page.step(&mut cabin, vec![egui::Event::PointerMoved(card_a)]);
    for _ in 0..120 {
        page.step(&mut cabin, Vec::new());
    }
    assert_eq!(
        cabin.board_view.open, None,
        "hover alone never opens a card"
    );
    // The same for a Follow up card.
    page.step(&mut cabin, vec![egui::Event::PointerMoved(card_f)]);
    for _ in 0..120 {
        page.step(&mut cabin, Vec::new());
    }
    assert_eq!(
        cabin.board_view.open, None,
        "hover never opens a Follow up card"
    );

    // A click opens card A; a click on its title row folds it.
    page.click(&mut cabin, card_a);
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-a"));
    page.click(&mut cabin, page.rect(("board-grip", "card-a")).center());
    assert_eq!(cabin.board_view.open, None, "a second click folds it");

    // Esc folds it. A chord such as the Super+Shift+Esc Halt hotkey does not.
    page.click(&mut cabin, card_a);
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-a"));
    page.esc(
        &mut cabin,
        egui::Modifiers::SHIFT | egui::Modifiers::COMMAND,
    );
    assert_eq!(
        cabin.board_view.open.as_deref(),
        Some("card-a"),
        "a chord is not a bare Esc"
    );
    page.esc(&mut cabin, egui::Modifiers::NONE);
    assert_eq!(cabin.board_view.open, None, "Esc folds the open card");

    // With a text field focused, Esc leaves the field first; the card stays open.
    page.click(&mut cabin, card_a);
    let field = egui::Id::new("board-click-open-field");
    page.field = Some(field);
    page.step(&mut cabin, Vec::new());
    page.ctx.memory_mut(|m| m.request_focus(field));
    page.step(&mut cabin, Vec::new());
    assert!(
        page.ctx.memory(|m| m.has_focus(field)),
        "the field holds the keyboard"
    );
    page.esc(&mut cabin, egui::Modifiers::NONE);
    assert_eq!(
        cabin.board_view.open.as_deref(),
        Some("card-a"),
        "the field takes this Esc"
    );
    page.esc(&mut cabin, egui::Modifiers::NONE);
    assert_eq!(cabin.board_view.open, None);
    page.field = None;
    page.step(&mut cabin, Vec::new());

    // A click on card B after card A moves the open spot to B.
    page.click(&mut cabin, card_a);
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-a"));
    let card_b_now = page.rect(("board-card", "card-b")).center();
    page.click(&mut cabin, card_b_now);
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-b"));
    page.click(&mut cabin, page.rect(("board-grip", "card-b")).center());
    assert_eq!(cabin.board_view.open, None);

    // Follow up cards open and fold the same way.
    page.click(&mut cabin, card_f);
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-f"));
    page.click(&mut cabin, page.rect(("board-grip", "card-f")).center());
    assert_eq!(cabin.board_view.open, None);
    page.click(&mut cabin, card_f);
    page.esc(&mut cabin, egui::Modifiers::NONE);
    assert_eq!(cabin.board_view.open, None);

    // A drag is not a click: card A moves to Doing and nothing opens.
    let doing = page
        .ctx
        .read_response(egui::Id::new(("board-drop", 1_usize)))
        .expect("Doing column")
        .rect;
    // Just under the column title: the column below it grows and shrinks with the drag.
    let open_during = page.drag(
        &mut cabin,
        card_a,
        doing.center_top() + egui::vec2(0.0, 40.0),
    );
    assert_eq!(open_during, vec![None; 22], "a drag never opens the card");
    let moved = cabin
        .board
        .iter()
        .find(|c| c.id == "card-a")
        .expect("card A");
    assert_eq!(
        moved.status,
        grokhub_core::BoardStatus::InProgress,
        "the drag still moves the card"
    );

    // Dragging an open card by its title row folds it and does not reopen it on release.
    page.click(&mut cabin, page.rect(("board-card", "card-b")).center());
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-b"));
    let grip = page.rect(("board-grip", "card-b")).center();
    let open_during = page.drag(&mut cabin, grip, grip + egui::vec2(0.0, 40.0));
    assert_eq!(open_during.last(), Some(&None), "{open_during:?}");
    assert_eq!(cabin.board_view.open, None);

    release_isolated(&root, cabin);
}

#[test]
fn ideas_come_from_the_model_with_their_work_and_post_with_a_prompt() {
    let root = config::test_config_root("ideas-gen");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.messages = std::sync::Arc::new(vec![
        (
            "user".into(),
            "run the grokhub tests on the chips branch".into(),
        ),
        ("assistant".into(), "All green.".into()),
        (
            "user".into(),
            "run the grokhub tests again and tell me what broke".into(),
        ),
        ("assistant".into(), "Two failures in chips.rs.".into()),
        ("user".into(), "actually still no, the other monitor".into()),
        ("user".into(), "/compact".into()),
        ("user".into(), "bump the AUR pkgver for the release".into()),
    ]);
    let asks = app.recent_asks(10);
    assert_eq!(
        asks,
        vec![
            "bump the AUR pkgver for the release".to_string(),
            "run the grokhub tests again and tell me what broke".to_string(),
            "run the grokhub tests on the chips branch".to_string(),
        ],
        "newest first, no slash commands, no feedback on the last try"
    );
    let (prompt, inputs) = app.idea_request();
    assert!(
        prompt.contains("- (1x) bump the AUR pkgver for the release"),
        "{prompt}"
    );
    assert!(
        prompt.contains("- (2x) run the grokhub tests again"),
        "{prompt}"
    );
    assert!(
        prompt.contains("IDEA: kind | title | short description | reason | details | what to send")
    );
    assert_eq!(inputs.asks.len(), 3);

    let (tx, rx) = std::sync::mpsc::channel();
    app.ideas_rx = Some((rx, inputs));
    tx.send(
        "IDEA: automation | Morning test run | Every weekday at 8 your GrokHub tests run and failures land on your feed before you sit down. | every weekday at 8, run cargo test in ~/GrokHub and summarize failures\n\
         IDEA: try | A chip for the next step | A chip takes the next step for you here. | take the next step\n"
            .into(),
    )
    .unwrap();
    app.poll_ideas();
    let ideas: Vec<&UpdateCard> = app
        .updates
        .iter()
        .filter(|c| c.kind == UpdateKind::Idea)
        .collect();
    assert_eq!(ideas.len(), 1, "the template-shaped line is dropped");
    assert_eq!(ideas[0].title, "Morning test run");
    assert_eq!(
        grokhub_core::idea_dialogue(ideas[0]),
        "every weekday at 8, run cargo test in ~/GrokHub and summarize failures",
        "Accept puts the ready-to-send message in the draft"
    );
    assert!(app.ideas_rx.is_none());
}

#[test]
fn launch_clears_template_ideas_and_chat_turns_no_longer_post_them() {
    let root = config::test_config_root("launch-clears-template-ideas");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.updates = vec![
        grokhub_core::idea_card(
            "profile",
            "A chip for the next slice",
            "A chip does the next slice.",
            1,
        ),
        grokhub_core::idea_card(
            "profile",
            "Set up: exit 1 · 9ms",
            "Turn this into a reminder.",
            1,
        ),
    ];
    app.ensure_useful_ideas();
    assert!(app.updates.is_empty(), "{:?}", app.updates);
    assert!(
        app.ideas_rx.is_none(),
        "launch must not start the model call"
    );
    let src = cabin_src();
    assert!(
        !src.contains("offer_learned_move"),
        "a chat turn must not post a template idea"
    );
    assert!(!src.contains("fill_useful_ideas"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn board_card_notes_edit_save_and_ride_the_next_turn() {
    let root = config::test_config_root("board-notes");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    let mut card = grokhub_core::BoardCard::new("Ship the AUR release", "", "");
    card.status = grokhub_core::BoardStatus::Todo;
    let id = card.id.clone();
    app.board.push(card);
    assert!(!app.apply_board_act(Some(BoardAct::EditNotes(id.clone()))));
    assert_eq!(
        app.board_notes_edit.as_ref().map(|(i, _)| i.as_str()),
        Some(id.as_str())
    );
    assert!(app.apply_board_act(Some(BoardAct::SaveNotes {
        id: id.clone(),
        notes: "  bump pkgver, then makepkg  ".into(),
    })));
    assert!(app.board_notes_edit.is_none());
    assert_eq!(app.board[0].notes, "bump pkgver, then makepkg");
    assert!(
        app.board[0].detail.is_empty(),
        "notes never land in the run status line"
    );

    let src = cabin_src();
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("self.card_notes_follow.as_deref()"),
        "notes reach the agent's prompt: {kick}"
    );
    let send = fn_src(&src, "send_chat");
    assert!(send.contains("take_card_notes_block"), "{send}");
    let open = fn_src(&src, "paint_board_card_open");
    assert!(
        !open.contains("dnd_drag_source"),
        "an open card's controls must not sit inside a drag source"
    );
    assert!(
        open.contains("egui::DragAndDrop::set_payload"),
        "the title row drags"
    );
}

#[test]
fn skill_suggestions_move_onto_the_ideas_board() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("skill-ideas-move");
    let _ = std::fs::remove_dir_all(&root);
    let _dir = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.updates.clear();
    cabin.suggestions.skills = vec![grokhub_core::LearnedSuggestion {
        kind: grokhub_core::SuggestionKind::Skill,
        title: "Desk tidy".into(),
        body: "Straighten windows".into(),
        name: Some("desk-tidy".into()),
        seed: None,
        trigger: Some("when the desk is messy".into()),
        instructions: Some("stack the windows".into()),
        provider: None,
        tool: None,
    }];
    cabin.skill_suggestions_to_ideas();
    assert!(
        cabin.suggestions.skills.is_empty(),
        "suggestions drain into Ideas"
    );
    let ideas = grokhub_core::ideas_board(&cabin.updates);
    assert_eq!(ideas.len(), 1);
    assert_eq!(ideas[0].idea_type_label(), "Skill");
    assert!(ideas[0].skill.is_some());
    // A second pass never duplicates the card.
    cabin.suggestions.skills = vec![ideas[0].skill.clone().unwrap()];
    cabin.skill_suggestions_to_ideas();
    assert_eq!(grokhub_core::ideas_board(&cabin.updates).len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

fn board_idea(
    id: &str,
    kind: grokhub_core::IdeaKind,
    prompt: &str,
    created_at: u64,
) -> grokhub_core::UpdateCard {
    let mut c = grokhub_core::idea_card(id, &format!("Idea {id}"), "Short line", created_at);
    c.id = id.into();
    c.idea_kind = Some(kind);
    c.prompt = Some(prompt.into());
    c.details = Some("Longer details".into());
    c
}

#[test]
fn idea_card_chat_takes_the_agents_action_and_stays_on_the_board() {
    let mut quiet = QuietCabin::boot("idea-card-chat");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.updates.push(board_idea(
        "idea-a",
        grokhub_core::IdeaKind::Automation,
        "every day at 8 sort my inbox",
        5,
    ));
    // Opening from the home feed lands on the Ideas board with the card open.
    cabin.discuss_card("idea-a");
    assert_eq!(cabin.nav, super::Nav::Pulse);
    assert_eq!(cabin.idea_board.open.as_deref(), Some("idea-a"));
    let tid = cabin.updates[0].discuss_thread.clone().expect("card chat");
    // The agent's reply rewrites the action once and the raw line is hidden.
    let thread = cabin.threads.iter_mut().find(|t| t.id == tid).unwrap();
    thread
        .messages_mut()
        .push(("user".into(), "weekdays only".into()));
    thread.messages_mut().push((
        "assistant".into(),
        "Done.\nCARD_ACTION: every weekday at 8 sort my inbox".into(),
    ));
    cabin.sync_idea_card_actions();
    let card = &cabin.updates[0];
    assert_eq!(card.idea_action(), "every weekday at 8 sort my inbox");
    assert!(
        card.modified,
        "a card you worked on stays until applied or deleted"
    );
    let last = cabin
        .threads
        .iter()
        .find(|t| t.id == tid)
        .unwrap()
        .messages
        .last()
        .cloned()
        .unwrap();
    assert!(last.1.contains(grokhub_core::CARD_ACTION_DONE) && !last.1.contains("CARD_ACTION:"));
    // The brief for the next turn carries the new action.
    cabin.chat_job_thread = Some(tid);
    let brief = cabin.idea_talk_brief().expect("idea brief");
    assert!(brief.contains("every weekday at 8 sort my inbox"));
}

#[test]
fn idea_card_apply_and_delete_follow_the_type() {
    let mut quiet = QuietCabin::boot("idea-card-apply");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.updates.push(board_idea(
        "idea-auto",
        grokhub_core::IdeaKind::Automation,
        "every weekday at 9 summarize the workboard",
        5,
    ));
    cabin.updates.push(board_idea(
        "idea-try",
        grokhub_core::IdeaKind::Try,
        "draft my standup",
        6,
    ));
    let autos = cabin.automations.len() + cabin.grok_loops.len();
    cabin.apply_idea("idea-auto");
    assert_eq!(cabin.automations.len() + cabin.grok_loops.len(), autos + 1);
    assert!(
        !cabin.updates.iter().any(|c| c.id == "idea-auto"),
        "applied cards leave the board"
    );
    // No Grok in this cabin: a Suggestion cannot run, so the card stays with a note.
    cabin.apply_idea("idea-try");
    assert!(cabin.updates.iter().any(|c| c.id == "idea-try"));
    assert!(cabin
        .idea_board
        .note
        .as_ref()
        .is_some_and(|(id, _)| id == "idea-try"));
    cabin.delete_idea("idea-try");
    assert!(!cabin.updates.iter().any(|c| c.id == "idea-try"));
    assert!(cabin.idea_board.note.is_none());
    let pulse = &cabin.cfg.feed_pulse;
    assert!(
        pulse.turned_down.iter().any(|t| t == "Idea idea-try")
            && pulse.dismissed_sources.iter().any(|s| s == "idea-try"),
        "Delete must be remembered, or the same idea comes back: {pulse:?}"
    );
    assert!(grokhub_core::turned_down_topic(pulse, "Idea idea-try"));
}

#[test]
fn feed_x_on_an_idea_keeps_it_on_the_board_and_turns_its_topic_down() {
    let mut quiet = QuietCabin::boot("idea-feed-x");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    let mut card = board_idea(
        "idea-pin",
        grokhub_core::IdeaKind::Try,
        "draft my standup",
        5,
    );
    card.feed_pin = true;
    cabin.updates.push(card);
    cabin.dismiss_feed_card("idea-pin");
    let card = cabin
        .updates
        .iter()
        .find(|c| c.id == "idea-pin")
        .expect("still on the board");
    assert!(!card.feed_pin);
    assert!(cabin
        .cfg
        .feed_pulse
        .turned_down
        .iter()
        .any(|t| t == "Idea idea-pin"));
}

#[test]
fn idea_card_edits_stop_at_ten_in_progress() {
    let mut quiet = QuietCabin::boot("idea-card-cap");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    for i in 0..11u64 {
        cabin.updates.push(board_idea(
            &format!("idea-{i}"),
            grokhub_core::IdeaKind::Try,
            "draft my standup",
            i,
        ));
    }
    for i in 0..10 {
        grokhub_core::set_idea_draft(&mut cabin.updates, &format!("idea-{i}"), "changed").unwrap();
    }
    assert!(grokhub_core::set_idea_draft(&mut cabin.updates, "idea-10", "changed").is_err());
    assert_eq!(grokhub_core::modified_ideas(&cabin.updates), 10);
}

#[test]
fn a_finished_turn_keeps_each_reply_and_tool_run_apart() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("turn-timeline");

    let mut t = crate::threads::ChatThread::new("Bug pass", false);
    t.messages = Arc::new(vec![("user".into(), "check yourself for bugs".into())]);
    let tid = t.id.clone();
    cabin.threads = vec![t];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(tid);
    let card = |id: &str, title: &str| grokhub_core::wire::ToolCard {
        id: id.into(),
        title: title.into(),
        kind: String::new(),
        status: "completed".into(),
        detail: String::new(),
        diff: String::new(),
        image_data_url: None,
        raw_input: String::new(),
    };

    // Thought summaries and post-tool messages arrive with no leading space.
    assert!(cabin.ingest_stream_chunk(LiveKind::Thought, "Rules say be brief."));
    assert!(cabin.ingest_stream_chunk(LiveKind::Thought, "I will locate the app."));
    cabin.ingest_stream_chunk(
        LiveKind::Say,
        "I'll find the app, then write up what I hit.",
    );
    cabin.ingest_tool_card(&card("t1", "grep"));
    cabin.ingest_tool_card(&card("t2", "read_file"));
    cabin.ingest_stream_chunk(LiveKind::Say, "GrokHub is running.");
    cabin.upsert_stream_assistant();
    cabin.ingest_tool_card(&card("t3", "screenshot"));
    cabin.ingest_stream_chunk(LiveKind::Say, "Walked GrokHub.");
    assert!(
        !cabin.stream_buf.contains("hit.GrokHub"),
        "messages split by a tool call must not glue: {}",
        cabin.stream_buf
    );
    let live: Vec<LiveKind> = cabin.live_blocks.iter().map(|b| b.kind).collect();
    assert_eq!(
        live,
        vec![
            LiveKind::Thought,
            LiveKind::Say,
            LiveKind::Tool,
            LiveKind::Tool,
            LiveKind::Say,
            LiveKind::Tool,
            LiveKind::Say,
        ]
    );

    let text = merge_thinking_capped(&cabin.thought_buf, &cabin.stream_buf, TEXT_FILE_CAP);
    cabin.finish_acp_turn(text);
    assert!(!cabin.running);
    assert!(cabin.turn_log.is_empty());

    let views = cabin.cached_chat_views().to_vec();
    let replies: Vec<&str> = views
        .iter()
        .filter(|v| v.kind == grokhub_core::ChatKind::Assistant)
        .map(|v| v.body.as_str())
        .collect();
    assert_eq!(
        replies,
        vec![
            "I'll find the app, then write up what I hit.",
            "GrokHub is running.",
            "Walked GrokHub.",
        ],
        "a finished turn must not fold every reply into one bubble"
    );
    let thought = views
        .iter()
        .find(|v| v.kind == grokhub_core::ChatKind::Thought)
        .expect("thought");
    assert!(
        thought
            .body
            .contains("Rules say be brief. I will locate the app."),
        "thought summaries must not glue: {}",
        thought.body
    );
    let tools: Vec<&str> = views
        .iter()
        .filter(|v| v.kind == grokhub_core::ChatKind::Tool)
        .map(|v| v.title.as_str())
        .collect();
    assert_eq!(tools, vec!["2 steps · Grep, Read file", "Screenshot"]);
    release_isolated(&root, cabin);
}

#[test]
fn idea_card_threads_leave_with_the_card() {
    let mut quiet = QuietCabin::boot("idea-card-thread-gone");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.updates.push(board_idea(
        "idea-auto",
        grokhub_core::IdeaKind::Automation,
        "every weekday at 9 summarize the workboard",
        5,
    ));
    cabin.updates.push(board_idea(
        "idea-try",
        grokhub_core::IdeaKind::Try,
        "draft my standup",
        6,
    ));
    cabin.discuss_card("idea-auto");
    cabin.discuss_card("idea-try");
    let auto_tid = cabin
        .updates
        .iter()
        .find(|c| c.id == "idea-auto")
        .unwrap()
        .discuss_thread
        .clone()
        .unwrap();
    let try_tid = cabin
        .updates
        .iter()
        .find(|c| c.id == "idea-try")
        .unwrap()
        .discuss_thread
        .clone()
        .unwrap();
    let visible = cabin.visible_thread_id();
    cabin.apply_idea("idea-auto");
    assert!(
        !cabin.threads.iter().any(|t| t.id == auto_tid),
        "an applied card takes its chat along"
    );
    assert!(
        cabin.status.contains("Automation added") || cabin.status.contains("loop"),
        "{}",
        cabin.status
    );
    cabin.delete_idea("idea-try");
    assert!(
        !cabin.threads.iter().any(|t| t.id == try_tid),
        "a deleted card takes its chat along"
    );
    assert_eq!(cabin.status, "Idea deleted");
    assert_eq!(
        cabin.visible_thread_id(),
        visible,
        "the open chat stays open"
    );
}

#[test]
fn idea_card_chat_reads_only_the_reply_of_a_stored_turn() {
    let mut quiet = QuietCabin::boot("idea-card-turn");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.updates.push(board_idea(
        "idea-a",
        grokhub_core::IdeaKind::Try,
        "draft my standup",
        5,
    ));
    cabin.discuss_card("idea-a");
    let tid = cabin.updates[0].discuss_thread.clone().unwrap();
    let thread = cabin.threads.iter_mut().find(|t| t.id == tid).unwrap();
    thread
        .messages_mut()
        .push(("user".into(), "make it shorter".into()));
    thread.messages_mut().push((
        "assistant".into(),
        "TURN_THOUGHT:\nCould use CARD_ACTION: write a haiku\nTURN_SAY:\nShorter now.\nCARD_ACTION: draft a three-line standup\n".into(),
    ));
    cabin.sync_idea_card_actions();
    assert_eq!(cabin.updates[0].idea_action(), "draft a three-line standup");
    let last = cabin
        .threads
        .iter()
        .find(|t| t.id == tid)
        .unwrap()
        .messages
        .last()
        .cloned()
        .unwrap();
    let shown = grokhub_core::assistant_prose(&last.1);
    assert!(
        shown.contains("Shorter now.") && shown.contains(grokhub_core::CARD_ACTION_DONE),
        "{shown}"
    );
    assert!(
        !shown.contains("haiku"),
        "the thought stays off the card chat: {shown}"
    );
}

#[test]
fn a_hung_ideas_ask_stops_thinking() {
    let mut quiet = QuietCabin::boot("ideas-ask-timeout");
    let cabin = &mut quiet.cabin;
    let (_tx, rx) = std::sync::mpsc::channel::<String>();
    cabin.ideas_rx = Some((rx, super::feed_ui::IdeaInputs::default()));
    cabin.cfg.feed_pulse.last_ideas_ms = super::now_ms();
    cabin.poll_ideas();
    assert!(cabin.ideas_rx.is_some(), "a fresh ask keeps waiting");
    cabin.cfg.feed_pulse.last_ideas_ms =
        super::now_ms().saturating_sub(super::feed_ui::IDEAS_WAIT_MS + 1_000);
    cabin.poll_ideas();
    assert!(
        cabin.ideas_rx.is_none(),
        "an ask past the wait lets the button go back to Suggest ideas"
    );
}

#[test]
fn a_report_run_lands_in_follow_up_with_a_chat_and_a_chore_does_not() {
    let (root, mut cabin) = isolated_cabin("follow-up-filing");
    cabin.board.clear();
    cabin.automations = vec![
        test_automation(
            "a-issues",
            "Morning issues",
            "every weekday at 9, summarize my open GitHub issues",
        ),
        test_automation(
            "a-tidy",
            "Tidy downloads",
            "every day at 18, clean the downloads folder",
        ),
    ];
    // You are on another chat; a scheduled run in its own process ends with a report.
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    let mine = cabin.threads[0].id.clone();
    // A clock automation's run posts no feed card itself; filing the Follow up adds one.
    cabin.settle_scheduled_run(
        Some("a-issues"),
        &grokhub_core::BgEnd::Done,
        "## Open issues\nThree opened overnight: crash on resume, tray icon, docs.",
    );
    let follow: Vec<&grokhub_core::BoardCard> = cabin
        .board
        .iter()
        .filter(|c| c.status == grokhub_core::BoardStatus::FollowUp)
        .collect();
    assert_eq!(follow.len(), 1, "{:?}", cabin.board);
    let card = follow[0].clone();
    assert_eq!(card.title, "Morning issues");
    assert_eq!(card.automation.as_deref(), Some("a-issues"));
    assert!(card.fresh && card.detail == "Open issues", "{card:?}");
    let chat = card
        .thread_id
        .clone()
        .expect("a follow up card has its own chat");
    assert_ne!(chat, mine, "its own chat, not the one you are on");
    assert!(cabin.messages.is_empty(), "nothing lands in your chat");
    let feed = cabin
        .updates
        .iter()
        .find(|c| c.kind == grokhub_core::UpdateKind::AutomationDone)
        .expect("feed card")
        .clone();
    assert_eq!(
        feed.board_id.as_deref(),
        Some(card.id.as_str()),
        "the home card leads to the Follow up card"
    );
    cabin.open_feed_card(&feed.id);
    assert!(cabin.nav == Nav::Workboard);
    assert_eq!(cabin.board_view.open.as_deref(), Some(card.id.as_str()));
    let thread = cabin.threads.iter().find(|t| t.id == chat).expect("thread");
    assert!(thread.background, "scheduled work, read on the card");
    assert!(thread.title.starts_with("Follow up · "));
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(cabin.thread_idx), true),
        Vec::<usize>::new(),
        "the Follow up chat is not a History row"
    );
    assert!(thread
        .messages
        .last()
        .unwrap()
        .1
        .contains("Three opened overnight"));

    // The next run of the same automation adds to the same card and chat.
    cabin.file_automation_follow_up(
        "a-issues",
        "Morning issues",
        "every weekday at 9, summarize my open GitHub issues",
        "Two more today: #15 and #16.",
    );
    let same: Vec<&grokhub_core::BoardCard> = cabin
        .board
        .iter()
        .filter(|c| c.automation.as_deref() == Some("a-issues"))
        .collect();
    assert_eq!(same.len(), 1);
    assert_eq!(same[0].thread_id.as_deref(), Some(chat.as_str()));
    let thread = cabin.threads.iter().find(|t| t.id == chat).unwrap();
    assert_eq!(thread.messages.len(), 2, "one message per run");
    assert!(thread.messages[1].1.contains("Two more today"));

    // A quiet chore that just did its job files nothing.
    assert!(!cabin.file_automation_follow_up(
        "a-tidy",
        "Tidy downloads",
        "every day at 18, clean the downloads folder",
        "Cleaned 12 files.",
    ));
    assert_eq!(cabin.board.len(), 1);

    // A reply in that chat does not file a second, Doing card.
    cabin.chat_job_thread = Some(chat.clone());
    cabin.note_inflight_card("yes, start with the crash on resume", "");
    assert_eq!(cabin.board.len(), 1, "{:?}", cabin.board);
    cabin.chat_job_thread = None;

    // Replying from the card hands the agent the latest report once.
    let block = grokhub_core::take_card_notes_block(&mut cabin.board, &chat).expect("report");
    assert!(block.contains("Two more today"), "{block}");
    assert_eq!(
        super::night::loop_card_name("/loop 12h summarize the workboard"),
        "Summarize the workboard"
    );
    release_isolated(&root, cabin);
}

#[test]
fn follow_up_runs_update_one_feed_card() {
    let (root, mut cabin) = isolated_cabin("follow-up-one-card");
    cabin.board.clear();
    cabin.updates.clear();
    let instructions = "every weekday at 9, summarize my open GitHub issues";
    for n in 1..=3 {
        assert!(
            cabin.file_automation_follow_up(
                "a-issues",
                "Morning issues",
                instructions,
                &format!("## Open issues\nReport {n}: crash on resume."),
            ),
            "run {n} should file follow up"
        );
    }
    let feeds: Vec<&grokhub_core::UpdateCard> = cabin
        .updates
        .iter()
        .filter(|c| {
            c.kind == grokhub_core::UpdateKind::AutomationDone
                && grokhub_core::feed_group_key(c).as_deref() == Some("run:a-issues")
        })
        .collect();
    assert_eq!(
        feeds.len(),
        1,
        "one automation keeps one feed card: {:?}",
        cabin.updates
    );
    let board = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-issues"))
        .expect("follow up card");
    assert_eq!(feeds[0].board_id.as_deref(), Some(board.id.as_str()));
    assert_eq!(
        feeds[0].action,
        Some(grokhub_core::UpdateAction::OpenWorkboard)
    );
    assert_eq!(
        feeds[0].body.as_deref(),
        Some("In Follow up on your workboard. Open it to read and reply.")
    );
    release_isolated(&root, cabin);
}

#[test]
fn board_cards_open_on_click_fold_on_drag_and_chat_like_the_chat_page() {
    use super::board_ui::{board_esc_folds, click_board_card, fold_board_on_drag, BoardView};
    let mut v = BoardView::default();
    assert!(v.open.is_none(), "nothing is open until a click");
    click_board_card(&mut v, "c1");
    assert_eq!(v.open.as_deref(), Some("c1"), "a click opens it");
    fold_board_on_drag(&mut v, true);
    assert!(v.open.is_none(), "a drag folds the card back at once");
    click_board_card(&mut v, "c1");
    assert_eq!(v.open.as_deref(), Some("c1"));
    fold_board_on_drag(&mut v, false);
    assert_eq!(v.open.as_deref(), Some("c1"), "no drag, no fold");
    click_board_card(&mut v, "c2");
    assert_eq!(
        v.open.as_deref(),
        Some("c2"),
        "a click on another card moves the open spot"
    );
    click_board_card(&mut v, "c2");
    assert!(v.open.is_none(), "a second click folds it");
    assert!(
        board_esc_folds(true, true, false),
        "a bare Esc folds the open card"
    );
    assert!(
        !board_esc_folds(false, true, false),
        "nothing open, nothing to fold"
    );
    assert!(!board_esc_folds(true, false, false), "no bare Esc, no fold");
    assert!(
        !board_esc_folds(true, true, true),
        "a field, menu or dialog takes Esc first"
    );

    let src = cabin_src();
    let chat = fn_src(&src, "paint_card_chat");
    assert!(
        chat.contains("visible_chat(")
            && chat.contains("paint_chat_block_with")
            && chat.contains("turn_log"),
        "the card chat uses the Chat page's bubbles and streams the live turn: {chat}"
    );
    let compact = fn_src(&src, "paint_board_card_compact");
    assert!(
        !compact.contains("white_pill") && !compact.contains("ghost_pill"),
        "a small card has no buttons: {compact}"
    );
    assert!(
        compact.contains("BOARD_PILL_RADIUS")
            && compact.contains("paint_board_progress_ring")
            && compact.contains("paint_status_chip")
            && compact.contains("paint_presence_stack")
            && compact.contains("sheet_shadow"),
        "compact cards keep the Flo project-card chrome: {compact}"
    );
    let say = fn_src(&src, "say_on_card");
    assert!(
        say.contains("take_card_notes_block") && say.contains("kick_model"),
        "{say}"
    );
    let saved = say
        .find("self.persist()")
        .expect("the card's chat is saved");
    assert!(
        saved < say.find("self.kick_model").unwrap(),
        "saved before the turn starts: {say}"
    );

    let (root, mut cabin) = isolated_cabin("board-say-needs-agent");
    let mut card = grokhub_core::BoardCard::new("Ship the AUR release", "", "");
    card.status = grokhub_core::BoardStatus::Todo;
    let id = card.id.clone();
    cabin.board = vec![card];
    assert!(!cabin.say_on_card(&id, "start with the PKGBUILD"));
    assert!(
        cabin
            .board_view
            .note
            .as_ref()
            .is_some_and(|(n, t)| n == &id && t.contains("Grok")),
        "{:?}",
        cabin.board_view.note
    );
    assert!(
        cabin.board[0].thread_id.is_none(),
        "nothing changes without an agent"
    );
    release_isolated(&root, cabin);
}

#[test]
fn ideas_from_a_one_time_install_go_and_new_ones_need_a_reason() {
    let (root, mut cabin) = isolated_cabin("ideas-one-off");
    cabin.messages = std::sync::Arc::new(vec![
        (
            "user".into(),
            "install the nvidia 550 driver for my 4070".into(),
        ),
        ("assistant".into(), "Installed.".into()),
    ]);
    let mut driver = grokhub_core::idea_card(
        "gen-driver",
        "Driver update check",
        "Checks for a newer nvidia driver every week.",
        1,
    );
    driver.prompt = Some("every monday at 9, check for a newer nvidia driver".into());
    driver.idea_kind = Some(grokhub_core::IdeaKind::Automation);
    let mut kept = grokhub_core::idea_card(
        "gen-stream",
        "Stream prep",
        "Gets the Friday stream scene ready.",
        1,
    );
    kept.prompt = Some("every friday at 17, open obs and load the stream scene".into());
    cabin.updates = vec![driver, kept];
    cabin.ensure_useful_ideas();
    let titles: Vec<&str> = cabin.updates.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(
        titles,
        vec!["Stream prep"],
        "the untouched one-off idea goes, the rest stays"
    );

    // The nightly review's skill for that install does not make it either.
    let mut items = vec![grokhub_core::LearnedSuggestion {
        kind: grokhub_core::SuggestionKind::Skill,
        title: "NVIDIA driver install".into(),
        body: "Installs the nvidia driver with the right kernel modules.".into(),
        seed: None,
        name: Some("nvidia-driver-install".into()),
        trigger: Some("when installing the nvidia driver".into()),
        instructions: Some("1. pacman -S nvidia".into()),
        provider: None,
        tool: None,
    }];
    cabin.keep_reasoned_suggestions(&mut items);
    assert!(items.is_empty(), "{items:?}");
    release_isolated(&root, cabin);
}

#[test]
fn board_drop_moves_to_another_column_only() {
    use grokhub_core::KanbanColumn;
    for col in KanbanColumn::ALL {
        let st = col.status();
        assert_eq!(super::pages::board_drop_move(st, col), None, "{col:?}");
        for other in KanbanColumn::ALL.into_iter().filter(|c| *c != col) {
            assert_eq!(
                super::pages::board_drop_move(st, other),
                Some(other.status())
            );
        }
    }
    let src = cabin_src();
    let board = fn_src(&src, "ui_board");
    let compact = fn_src(&src, "paint_board_card_compact");
    let open = fn_src(&src, "paint_board_card_open");
    assert!(
        compact.contains("DragAndDrop::set_payload(") && compact.contains("BoardDrag(id.clone())"),
        "a small card drags whole"
    );
    assert!(
        open.contains("DragAndDrop::set_payload(") && open.contains("self.board_view.open = None"),
        "an open card drags by its title row and folds back"
    );
    assert!(
        board.contains("dnd_release_payload::<BoardDrag>"),
        "columns must take drops"
    );
    assert!(
        board.contains("BoardAct::Move"),
        "a drop is the same Move the menu uses"
    );
    assert!(
        board.contains("KanbanColumn::FollowUp"),
        "the Follow up row takes drops too"
    );
}

#[test]
fn workboard_flo_card_progress_labels_and_presence_chips() {
    use super::board_ui::{
        board_card_progress, board_presence_chips, board_status_label, BOARD_PILL_RADIUS,
    };
    use grokhub_core::{BoardCard, BoardStatus};
    assert_eq!(BOARD_PILL_RADIUS, 22.0);
    assert_eq!(board_status_label(BoardStatus::Todo), "To do");
    assert_eq!(board_status_label(BoardStatus::InProgress), "In progress");
    assert_eq!(board_status_label(BoardStatus::Blocked), "Blocked");
    assert_eq!(board_status_label(BoardStatus::Done), "Done");
    assert_eq!(board_status_label(BoardStatus::FollowUp), "Follow up");
    assert_eq!(board_card_progress(BoardStatus::Todo), (0, 4));
    assert_eq!(board_card_progress(BoardStatus::InProgress), (2, 4));
    assert_eq!(board_card_progress(BoardStatus::Blocked), (1, 4));
    assert_eq!(board_card_progress(BoardStatus::FollowUp), (3, 4));
    assert_eq!(board_card_progress(BoardStatus::Done), (4, 4));
    let mut card = BoardCard::new("Onboarding flow", "Ship the welcome path", "");
    card.status = BoardStatus::InProgress;
    assert!(
        board_presence_chips(&card).is_empty(),
        "plain cards show no presence chips"
    );
    card.thread_id = Some("t1".into());
    card.notes = "Keep the copy short".into();
    card.automation = Some("auto-1".into());
    let chips = board_presence_chips(&card);
    assert_eq!(
        chips.iter().map(|(c, _, _)| *c).collect::<Vec<_>>(),
        vec!['C', 'N', 'A']
    );
}

fn work_card(id: &str, status: grokhub_core::BoardStatus) -> grokhub_core::BoardCard {
    let mut c = grokhub_core::BoardCard::new(id, "", "");
    c.id = id.into();
    c.status = status;
    c
}

fn steps(marks: &[grokhub_core::StepStatus]) -> Vec<grokhub_core::WorkStep> {
    marks
        .iter()
        .enumerate()
        .map(|(i, s)| grokhub_core::WorkStep {
            text: format!("Step {}", i + 1),
            status: *s,
        })
        .collect()
}

#[test]
fn card_meta_line_shows_left_off_badge_and_kind() {
    use super::board_ui::{card_badges, card_left_off, card_meta_line};
    use crate::ui_kit::BadgeTone;
    use grokhub_core::{BoardStatus, Checkpoint, StepStatus, WorkKind, WorkOrigin};
    let mut c = work_card("meta", BoardStatus::InProgress);
    c.updated_ms = 0;
    assert_eq!(card_meta_line(&c, 0), "", "a bare card says nothing");
    assert_eq!(card_badges(&c), vec![("You", BadgeTone::Neutral)]);

    // A step in flight is where it left off.
    c.steps = steps(&[StepStatus::Done, StepStatus::Doing, StepStatus::Planned]);
    assert_eq!(card_left_off(&c).as_deref(), Some("Step 2"));
    assert!(card_meta_line(&c, 0).starts_with("Left off: Step 2"));

    // The checkpoint's next step wins, and is clipped on the compact card.
    c.checkpoint = Some(Checkpoint {
        next: "Wire the tray menu to the new store and then re-run the whole app test suite".into(),
        ..Default::default()
    });
    let meta = card_meta_line(&c, 0);
    assert!(meta.starts_with("Left off: Wire the tray menu"), "{meta}");
    assert!(meta.len() <= "Left off: ".len() + 60, "{meta}");

    c.origin = Some(WorkOrigin::Agent);
    c.kind = WorkKind::Background;
    assert_eq!(
        card_badges(&c),
        vec![
            ("Agent", BadgeTone::Accent),
            ("Background", BadgeTone::Neutral)
        ]
    );

    // Finished work has nowhere left to pick up.
    c.status = BoardStatus::Done;
    assert_eq!(card_left_off(&c), None);
    assert!(!card_meta_line(&c, 0).contains("Left off"));
}

#[test]
fn ring_fraction_matches_the_steps() {
    use super::board_ui::card_progress;
    use grokhub_core::{BoardStatus, StepStatus};
    let mut c = work_card("ring", BoardStatus::InProgress);
    assert_eq!(card_progress(&c), (2, 4), "no steps: the column's ring");
    c.steps = steps(&[
        StepStatus::Done,
        StepStatus::Skipped,
        StepStatus::Doing,
        StepStatus::Failed,
        StepStatus::Planned,
    ]);
    assert_eq!(card_progress(&c), (2, 5), "done and skipped fill a segment");
    c.status = BoardStatus::Done;
    assert_eq!(card_progress(&c), (5, 5), "a Done card is full");
}

#[test]
fn resume_shows_only_on_interrupted_or_paused_cards() {
    use super::board_ui::{board_counts, card_can_resume, BoardCounts};
    use grokhub_core::{BoardStatus, Checkpoint};
    let mut paused_run = work_card("paused", BoardStatus::Blocked);
    paused_run.run = true;
    let mut interrupted = work_card("cut", BoardStatus::Blocked);
    interrupted.checkpoint = Some(Checkpoint {
        next: "Re-run the build".into(),
        ..Default::default()
    });
    let blocked_on_you = work_card("ask", BoardStatus::Blocked);
    assert!(card_can_resume(&paused_run));
    assert!(card_can_resume(&interrupted));
    assert!(
        !card_can_resume(&blocked_on_you),
        "plain Blocked waits on you"
    );
    for status in [
        BoardStatus::Todo,
        BoardStatus::InProgress,
        BoardStatus::Done,
        BoardStatus::FollowUp,
        BoardStatus::Dismissed,
    ] {
        let mut c = work_card("other", status);
        c.run = true;
        assert!(!card_can_resume(&c), "{status:?} shows no Resume");
    }
    let mut doing = work_card("doing", BoardStatus::InProgress);
    doing.run = true;
    let follow = work_card("follow", BoardStatus::FollowUp);
    assert_eq!(
        board_counts(&[paused_run, interrupted, blocked_on_you, doing, follow]),
        BoardCounts {
            doing: 1,
            interrupted: 2,
            needs_you: 2,
        }
    );
}

#[test]
fn open_paused_card_paints_steps_checkpoint_resume_and_log() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("board-work-open");
    let mut c = work_card("card-p", grokhub_core::BoardStatus::Blocked);
    c.title = "Port the tray".into();
    c.run = true;
    c.origin = Some(grokhub_core::WorkOrigin::Agent);
    c.steps = steps(&[
        grokhub_core::StepStatus::Done,
        grokhub_core::StepStatus::Doing,
    ]);
    c.checkpoint = Some(grokhub_core::Checkpoint {
        goal: "Tray uses the new store".into(),
        next: "Step 2".into(),
        blocker: "Needs a rebuild".into(),
        refs: vec!["tray.rs".into()],
        ..Default::default()
    });
    cabin.board = vec![c];
    let dir = crate::work_store::work_dir();
    for i in 0..30 {
        grokhub_core::work_item::append_work_log(
            &dir,
            "card-p",
            1_000 + i,
            "step",
            &format!("line {i}"),
        )
        .unwrap();
    }
    let mut page = BoardPage::new();
    page.step(&mut cabin, Vec::new());
    page.step(&mut cabin, Vec::new());
    let card = page.rect(("board-card", "card-p")).center();
    page.click(&mut cabin, card);
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-p"));
    assert!(
        cabin.board_view.log.is_none(),
        "the log is not read until asked"
    );

    // The drawer reads the tail once, on open: the newest 20 lines.
    assert!(!cabin.apply_board_act(Some(super::pages::BoardAct::ToggleLog("card-p".into()))));
    let (id, lines) = cabin.board_view.log.clone().expect("drawer open");
    assert_eq!(id, "card-p");
    assert_eq!(lines.len(), super::board_ui::LOG_DRAWER_LINES);
    assert_eq!(lines.first().unwrap().text, "line 10");
    assert_eq!(lines.last().unwrap().text, "line 29");
    page.step(&mut cabin, Vec::new());
    page.step(&mut cabin, Vec::new());
    assert!(!cabin.apply_board_act(Some(super::pages::BoardAct::ToggleLog("card-p".into()))));
    assert!(cabin.board_view.log.is_none(), "a second press closes it");

    // Resume (KB-03 stub) with no agent: the bell says it failed, with a way back.
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Resume("card-p".into()))));
    let n = cabin.notices.items.first().expect("a bell notice");
    assert!(
        n.text.starts_with("Resume failed: Port the tray"),
        "{}",
        n.text
    );
    assert_eq!(
        n.action,
        Some(super::notices::NoticeAction::OpenCard("card-p".into()))
    );
    assert!(cabin.board[0].touched, "Resume is your hand on the card");

    // Hand back: the card is yours, in To do.
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::HandBack("card-p".into()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert_eq!(cabin.board[0].origin, Some(grokhub_core::WorkOrigin::User));
    page.step(&mut cabin, Vec::new());
    release_isolated(&root, cabin);
}

#[test]
fn log_tail_reads_only_the_end_of_a_big_log() {
    use super::board_ui::read_log_tail;
    let root = crate::config::test_config_root("board-log-tail");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for i in 0..40 {
        grokhub_core::work_item::append_work_log(&root, "big", i, "note", &format!("event {i}"))
            .unwrap();
    }
    // 200 bytes holds a cut line and a few whole ones; the cut one is dropped.
    let tail = read_log_tail(&root, "big", 200, 20);
    assert!(!tail.is_empty() && tail.len() < 6, "{}", tail.len());
    assert_eq!(tail.last().unwrap().text, "event 39");
    assert!(read_log_tail(&root, "../escape", 200, 20).is_empty());
    assert!(read_log_tail(&root, "none-yet", 200, 20).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_user_move_or_chat_sets_touched() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("board-touched");
    let mut c = work_card("t1", grokhub_core::BoardStatus::Done);
    c.origin = Some(grokhub_core::WorkOrigin::Agent);
    cabin.board = vec![c, work_card("t2", grokhub_core::BoardStatus::Todo)];
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Move {
        id: "t1".into(),
        status: grokhub_core::BoardStatus::InProgress,
    })));
    assert!(cabin.board[0].touched);
    cabin.apply_board_act(Some(super::pages::BoardAct::Say {
        id: "t2".into(),
        text: "hello".into(),
    }));
    assert!(cabin.board[1].touched, "a line to the agent is yours too");
    release_isolated(&root, cabin);
}

#[test]
fn bell_sweep_undo_restores_the_cards_and_paused_runs_ring_once() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("board-bell");
    let mut done = work_card("d1", grokhub_core::BoardStatus::Done);
    done.origin = Some(grokhub_core::WorkOrigin::Agent);
    done.created_ms = 1;
    cabin.board = vec![done];
    let day = 24 * 60 * 60 * 1000;
    let report = grokhub_core::sweep_work_cards(&mut cabin.board, 2 * day, &[]);
    assert_eq!(report.archived_done, vec!["d1".to_string()]);
    cabin.note_board_sweep(&report);
    assert_eq!(cabin.notices.items[0].text, "Archived 1 finished card");
    assert_eq!(
        cabin.notices.items[0].action.as_ref().unwrap().label(),
        "Undo"
    );
    cabin.run_notice_action(0);
    assert_eq!(
        cabin.board[0].status,
        grokhub_core::BoardStatus::Todo,
        "Undo puts the card back"
    );
    assert_eq!(cabin.notices.items[0].action, None, "Undo runs once");

    // A run already paused when the session starts stays quiet; a new one rings once.
    let mut old = work_card("old", grokhub_core::BoardStatus::Blocked);
    old.run = true;
    cabin.board.push(old);
    let before = cabin.notices.items.len();
    cabin.tick_board_notices();
    assert_eq!(cabin.notices.items.len(), before);
    let mut fresh = work_card("new", grokhub_core::BoardStatus::Blocked);
    fresh.run = true;
    fresh.title = "Sort downloads".into();
    cabin.board.push(fresh);
    cabin.tick_board_notices();
    cabin.tick_board_notices();
    assert_eq!(cabin.notices.items.len(), before + 1);
    assert!(cabin.notices.items[0]
        .text
        .starts_with("Interrupted: Sort downloads"));
    cabin.run_notice_action(0);
    assert_eq!(cabin.board_view.open.as_deref(), Some("new"));
    assert!(matches!(cabin.nav, Nav::Workboard));
    release_isolated(&root, cabin);
}

/// Runs the cabin's sweep timer until its thread reports back.
fn run_sweep_timer(cabin: &mut Cabin, now: u64) {
    cabin.tick_board_sweep(now);
    let start = std::time::Instant::now();
    while cabin.board_view.sweep.is_some() && start.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cabin.tick_board_sweep(now);
    }
    assert!(cabin.board_view.sweep.is_none(), "the sweep thread ended");
}

#[test]
fn the_sweep_timer_archives_on_disk_without_a_model_and_undo_restores() {
    use grokhub_core::{BoardStatus, WorkOrigin};
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("board-sweep-timer");
    let day = grokhub_core::work_item::DAY_MS;
    let now = grokhub_core::now_ms();
    let mut agent = work_card("agent", BoardStatus::Done);
    agent.origin = Some(WorkOrigin::Agent);
    agent.created_ms = now - 2 * day;
    let mut mine = work_card("mine", BoardStatus::Done);
    mine.origin = Some(WorkOrigin::User);
    mine.created_ms = now - 2 * day;
    let mut touched = work_card("touched", BoardStatus::Done);
    touched.origin = Some(WorkOrigin::Agent);
    touched.created_ms = now - 2 * day;
    touched.touched = true;
    let path = crate::config::workboard_path();
    grokhub_core::work_file::write_board(&path, &[agent, mine, touched]).unwrap();
    cabin.board = crate::config::load_board();

    run_sweep_timer(&mut cabin, now);
    let status = |cabin: &Cabin, id: &str| cabin.board.iter().find(|c| c.id == id).unwrap().status;
    assert_eq!(
        status(&cabin, "agent"),
        BoardStatus::Dismissed,
        "in memory at once"
    );
    assert_eq!(
        status(&cabin, "mine"),
        BoardStatus::Done,
        "your card never auto-archives"
    );
    assert_eq!(
        status(&cabin, "touched"),
        BoardStatus::Done,
        "nor one you touched"
    );
    let disk = grokhub_core::work_file::read_board(&path).unwrap();
    assert_eq!(
        disk[0].status,
        BoardStatus::Dismissed,
        "the sweep wrote the file"
    );
    let sweeps = cabin
        .notices
        .items
        .iter()
        .filter(|n| n.text.starts_with("Archived"))
        .count();
    assert_eq!(sweeps, 1, "one bell notice per sweep");
    assert_eq!(cabin.notices.items[0].text, "Archived 1 finished card");

    // The next frame is not due: the timer waits six hours.
    cabin.tick_board_sweep(now + 1);
    assert!(cabin.board_view.sweep.is_none());

    cabin.run_notice_action(0);
    assert_eq!(
        status(&cabin, "agent"),
        BoardStatus::Todo,
        "Undo restores it"
    );
    crate::config::save_board(&cabin.board).unwrap();
    let disk = grokhub_core::work_file::read_board(&path).unwrap();
    assert_eq!(
        disk[0].status,
        BoardStatus::Todo,
        "and the restore is saved"
    );
    release_isolated(&root, cabin);
}

#[test]
fn the_archived_view_lists_newest_first_finds_and_bulk_deletes_only_archived() {
    use super::board_ui::{
        archived_older_count, archived_rows, auto_delete_label, bulk_delete_label,
        confirm_delete_text,
    };
    use grokhub_core::BoardStatus;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("board-archived-view");
    let day = grokhub_core::work_item::DAY_MS;
    let now = grokhub_core::now_ms();
    let archived = |id: &str, title: &str, ago: u64| {
        let mut c = work_card(id, BoardStatus::Dismissed);
        c.title = title.into();
        c.archived_ms = now - ago;
        c
    };
    cabin.board = vec![
        archived("old", "Flash the router", 95 * day),
        archived("mid", "Sort photos", 45 * day),
        archived("new", "Router notes", day),
        work_card("todo", BoardStatus::Todo),
        work_card("done", BoardStatus::Done),
    ];
    let ids = |rows: Vec<&grokhub_core::BoardCard>| -> Vec<String> {
        rows.into_iter().map(|c| c.id.clone()).collect()
    };
    assert_eq!(
        ids(archived_rows(&cabin.board, "")),
        vec!["new", "mid", "old"]
    );
    assert_eq!(
        ids(archived_rows(&cabin.board, "ROUTER")),
        vec!["new", "old"]
    );
    assert_eq!(archived_older_count(&cabin.board, 0, now), 3);
    assert_eq!(archived_older_count(&cabin.board, 30, now), 2);
    assert_eq!(archived_older_count(&cabin.board, 90, now), 1);
    assert_eq!(bulk_delete_label(0), "Delete all archived");
    assert_eq!(bulk_delete_label(90), "Delete archived older than 90 days");
    assert_eq!(
        confirm_delete_text(2),
        "Delete 2 archived cards and their logs for good?"
    );
    assert_eq!(auto_delete_label(0), "Off");

    let logs = crate::work_store::work_dir();
    for id in ["old", "mid", "new", "todo"] {
        grokhub_core::work_item::append_work_log(&logs, id, 1, "step", "x").unwrap();
    }
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::DeleteArchived(30))));
    assert_eq!(cabin.status, "Deleted 2 archived cards");
    assert!(!logs.join("old.jsonl").exists() && !logs.join("mid.jsonl").exists());
    assert!(logs.join("new.jsonl").exists());
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::DeleteArchived(0))));
    let left: Vec<&str> = cabin.board.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        left,
        vec!["todo", "done"],
        "bulk delete never touches the board"
    );
    assert!(logs.join("todo.jsonl").exists());
    assert!(!cabin.apply_board_act(Some(super::pages::BoardAct::DeleteArchived(0))));
    release_isolated(&root, cabin);
}

#[test]
fn activity_row_finds_the_card_for_its_chat() {
    use super::board_ui::board_card_for_chat;
    let mut a = work_card("a", grokhub_core::BoardStatus::InProgress);
    a.thread_id = Some("chat-1".into());
    let mut gone = work_card("gone", grokhub_core::BoardStatus::Dismissed);
    gone.thread_id = Some("chat-2".into());
    let cards = vec![a, gone];
    assert_eq!(board_card_for_chat(&cards, "chat-1").as_deref(), Some("a"));
    assert_eq!(
        board_card_for_chat(&cards, "chat-2"),
        None,
        "archived cards stay off"
    );
    assert_eq!(board_card_for_chat(&cards, ""), None);
}
