//! Hub sync, privacy, permissions and scope tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// Landed from PR #123.
#[test]
fn sync_writes_a_local_hub_snapshot() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("sync-local");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    // Spike-4a: /sync publishes chats and memory, so it needs the hub grant
    // (the Settings → Permissions Allow click) or a hard-card click.
    let grant = grokhub_agent::harness::grant_destination(
        &root,
        grokhub_agent::harness::HUB_DEST,
        grokhub_agent::harness::HUB_SYNC_DATA,
        grokhub_agent::harness::UserClick::from_click(),
    )
    .unwrap();
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.device_name = "harbor".into();
    // SY-03: /sync only sends when a computer is paired.
    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert_eq!(cabin.status, "Syncing…");
    assert!(cabin.sync_rx.is_some());
    cabin.run_slash_line("/sync");
    assert_eq!(cabin.status, "Syncing…");
    wait_sync(&mut cabin);
    // SY-03: one result line in the chat; the pane stays on Chat.
    assert!(matches!(cabin.nav, Nav::Chat), "nav={:?}", cabin.nav);
    assert_eq!(cabin.status, "Synced chats and memory to 1 computer.");
    let (role, body) = cabin.messages.last().cloned().expect("result line");
    assert_eq!(role, "assistant");
    assert!(
        grokhub_core::is_cabin_slash_turn(&role, &body),
        "stays out of the next model kick"
    );
    assert_eq!(
        grokhub_core::strip_slash_result(&body),
        "Synced chats and memory to 1 computer."
    );
    assert!(cabin.hub.lock().unwrap().snapshot.is_some());
    let log = grokhub_agent::harness::read_egress(&root);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].dest, "hub");
    assert_eq!(log[0].grant_id, grant.id);
    assert_eq!(log[0].basis, "grant");
    let spans = harness_spans(&root);
    let sent = spans
        .iter()
        .find(|s| s.tool == "hub_sync")
        .expect("hub_sync span");
    assert_eq!(sent.decision, "allow");
    assert_eq!(sent.consent_ref, grant.id);
    assert_eq!(sent.path, "egress");
    assert_eq!(log[0].span_id, sent.span_ref());
    std::env::remove_var("GROKHUB_CONFIG");
}

/// Every span line under `{root}/spans/`, any session.
fn harness_spans(root: &std::path::Path) -> Vec<grokhub_agent::harness::Span> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root.join("spans"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str(line).expect("span line"));
        }
    }
    out.sort_by_key(|s: &grokhub_agent::harness::Span| s.ts_ms);
    out
}

/// Pair a fake computer with the cabin's hub (no network: just the peer row).
fn pair_test_peer(cabin: &Cabin, id: &str) {
    cabin
        .hub
        .lock()
        .unwrap()
        .peers
        .push(grokhub_core::state::Peer {
            id: id.into(),
            name: format!("box-{id}"),
            token: format!("t-{id}"),
            last_seen: 0,
        });
}

fn wait_sync(cabin: &mut Cabin) {
    // Sync waits for the full snapshot's disk writes before egress. Drain that
    // FIFO job first so Windows ACL/antivirus latency doesn't consume the poll budget.
    cabin.persist_svc.flush();
    let start = std::time::Instant::now();
    while cabin.sync_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(8) {
        cabin.poll_sync();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cabin.sync_rx.is_none(),
        "/sync must land the hub snapshot; status={:?} after {:?}",
        cabin.status,
        start.elapsed()
    );
}

// Spike-4a: chats and memory to a not-yet-granted destination are hard Send.
#[test]
fn sync_without_a_hub_grant_parks_a_hard_send_card_and_approve_sends_once() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("sync-park");
    std::fs::create_dir_all(&root).unwrap();
    // A paired computer and no grant: still the hard Send card (SY-03).
    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert!(cabin.sync_rx.is_none(), "nothing leaves without a grant");
    let park = cabin.harness.park.clone().expect("hard card");
    assert_eq!(
        park.source,
        super::harness_ui::ParkSource::Egress("hub".into())
    );
    assert_eq!(park.class, grokhub_agent::HardClass::Send);
    assert_eq!(park.path, "egress");
    assert_eq!(park.tool, "hub_sync");
    assert_eq!(park.action, "/sync → paired computers (chats, memory)");
    assert_eq!(cabin.hard_waiting(), 1);
    assert!(cabin.hub.lock().unwrap().snapshot.is_none());
    assert!(hx::read_egress(&root).is_empty());
    // The same hard card: Enter never approves, Esc denies.
    assert_eq!(hx::hard_card_key(true, false, false), None);
    assert_eq!(
        hx::hard_card_key(false, true, false),
        Some(hx::HardAnswer::Deny)
    );

    cabin.resolve_hard_park(false, "user denied (Esc)");
    assert!(cabin.sync_rx.is_none());
    assert!(cabin.harness.park.is_none());
    assert_eq!(cabin.status, "Denied · Send");
    assert!(hx::read_egress(&root).is_empty());

    cabin.run_slash_line("/sync");
    assert!(cabin.harness.park.is_some());
    cabin.resolve_hard_park(true, "");
    assert!(cabin.sync_rx.is_some(), "Approve sends once");
    wait_sync(&mut cabin);
    assert_eq!(cabin.status, "Synced chats and memory to 1 computer.");
    assert!(cabin.hub.lock().unwrap().snapshot.is_some());
    let log = hx::read_egress(&root);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].dest, "hub");
    assert_eq!(log[0].grant_id, "approved-once");
    assert_eq!(log[0].basis, "approved_once");
    assert_eq!(
        hx::ConsentLedger::load(&root),
        hx::ConsentLedger::empty(),
        "a hard card never writes a standing grant"
    );
    let spans = harness_spans(&root);
    let decisions: Vec<&str> = spans.iter().map(|s| s.decision.as_str()).collect();
    assert_eq!(decisions, vec!["park", "deny", "park", "approve", "allow"]);
    assert!(spans
        .iter()
        .all(|s| s.tool == "hub_sync" && s.path == "egress"));
    assert_eq!(spans[4].consent_ref, "approved-once");
    assert_eq!(log[0].span_id, spans[4].span_ref());
    assert!(hx::approval_gate_violation(&spans).is_empty());

    // Still no grant: the next /sync asks again.
    cabin.run_slash_line("/sync");
    assert!(cabin.sync_rx.is_none());
    assert!(cabin.harness.park.is_some());
    release_isolated(&root, cabin);
}

#[test]
fn revoking_the_hub_grant_blocks_the_next_sync_and_drops_the_share() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("sync-revoke");
    std::fs::create_dir_all(&root).unwrap();
    pair_test_peer(&cabin, "p1");
    let g = hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    assert!(cabin
        .consent()
        .destination_grant("hub", hx::HUB_SYNC_DATA)
        .is_some());
    cabin.hub.lock().unwrap().snapshot = Some(std::sync::Arc::new(
        serde_json::json!({"kind": "grokhub-hub-v1"}),
    ));
    cabin.revoke_hub_grant(&g.id);
    assert_eq!(
        cabin.status,
        "Sync to paired computers revoked. /sync asks again."
    );
    assert!(
        cabin.hub.lock().unwrap().snapshot.is_none(),
        "revoke stops serving the old share"
    );
    assert_eq!(cabin.consent().active().count(), 0);
    cabin.run_slash_line("/sync");
    assert!(cabin.sync_rx.is_none());
    assert_eq!(
        cabin.harness.park.as_ref().map(|p| p.source.clone()),
        Some(super::harness_ui::ParkSource::Egress("hub".into()))
    );
    assert!(hx::read_egress(&root).is_empty());
    release_isolated(&root, cabin);
}

#[test]
fn privacy_slash_lists_grants_scopes_and_egress_without_content() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("privacy-slash");
    std::fs::create_dir_all(&root).unwrap();
    let before = cabin.messages.len();
    assert_eq!(
        hx::guard_egress(
            &root,
            &hx::EgressReq::new("https://api.x.ai/v1/responses", &[hx::DataClass::Chat])
        ),
        hx::GateOutcome::Allow
    );
    cabin.run_slash_line("/privacy");
    assert!(!cabin.running, "/privacy never reaches the model");
    let start = std::time::Instant::now();
    while cabin.harness.privacy_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(4)
    {
        cabin.poll_privacy();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(cabin.messages.len(), before + 1);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert_eq!(role, "assistant");
    assert!(
        grokhub_core::is_cabin_slash_turn(&role, &body),
        "stays out of the next model kick"
    );
    let text = grokhub_core::strip_slash_result(&body);
    assert!(
        text.starts_with("/privacy — what leaves this computer\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "- Off: Sync to paired computers (/sync asks each time) · Files in a folder · "
        ),
        "{text}"
    );
    assert!(text.contains("Granted folders, apps, browser history and system state are read on this computer only, never on battery or in quiet hours; calendar and mail aren't read yet."), "{text}");
    assert!(
        text.contains("- api.x.ai · 1 time · chats · default · last just now"),
        "{text}"
    );
    release_isolated(&root, cabin);
}

// SY-03: nothing paired means nothing to send. No card, no egress line, no
// span, with or without the grant; one plain result line instead.
#[test]
fn sync_with_no_paired_computer_posts_a_line_and_sends_nothing() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("sync-no-peers");
    std::fs::create_dir_all(&root).unwrap();
    assert!(cabin.hub.lock().unwrap().peers.is_empty());
    let before = cabin.messages.len();
    cabin.run_slash_line("/sync");
    assert!(cabin.harness.park.is_none(), "no peer: no hard card");
    assert_eq!(cabin.hard_waiting(), 0);
    assert!(cabin.sync_rx.is_none(), "no peer: nothing is sent");
    assert_eq!(
        cabin.status,
        "Nothing paired yet. Start share to pair a computer."
    );
    assert_eq!(cabin.messages.len(), before + 1);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert!(grokhub_core::is_cabin_slash_turn(&role, &body));
    assert_eq!(
        grokhub_core::strip_slash_result(&body),
        "Nothing paired yet. Start share to pair a computer."
    );
    assert!(hx::read_egress(&root).is_empty(), "no peer: no egress line");
    assert!(harness_spans(&root).is_empty(), "no peer: no span");
    assert!(cabin.hub.lock().unwrap().snapshot.is_none());

    // With the grant it is the same: nobody to send to.
    hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    cabin.run_slash_line("/sync");
    assert!(cabin.harness.park.is_none());
    assert!(cabin.sync_rx.is_none());
    assert!(hx::read_egress(&root).is_empty());
    assert!(harness_spans(&root).is_empty());
    assert!(cabin.hub.lock().unwrap().snapshot.is_none());

    // A card approved after the last computer unpaired sends nothing either.
    hx::revoke_grant(&root, &cabin.consent().active().next().unwrap().id.clone()).unwrap();
    cabin.harness.consent = None;
    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert!(
        cabin.harness.park.is_some(),
        "a peer and no grant parks the hard Send card"
    );
    cabin.hub.lock().unwrap().peers.clear();
    cabin.resolve_hard_park(true, "");
    assert!(cabin.sync_rx.is_none());
    assert!(hx::read_egress(&root).is_empty());
    assert_eq!(
        cabin.status,
        "Nothing paired yet. Start share to pair a computer."
    );
    release_isolated(&root, cabin);
}

// SY-05/SY-06: Permissions groups the trust rows under "Leaving this computer"
// and the rule editor under "Command rules". Allow is filled; Revoke is a ghost.
#[test]
fn permissions_page_groups_trust_rows_and_rules_with_a_ghost_revoke() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-headings");
    std::fs::create_dir_all(&root).unwrap();
    struct Painted {
        texts: Vec<(String, egui::Pos2)>,
        filled: Vec<egui::Rect>,
    }
    fn walk(shape: &egui::Shape, p: &mut Painted) {
        match shape {
            egui::Shape::Text(t) => p.texts.push((
                t.galley.text().to_string(),
                t.pos + t.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Rect(r) if r.fill == crate::theme::fg() => p.filled.push(r.rect),
            egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, p)),
            _ => {}
        }
    }
    let paint = |cabin: &mut Cabin| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 2400.0),
            )),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
        });
        let mut p = Painted {
            texts: Vec::new(),
            filled: Vec::new(),
        };
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut p);
        }
        p
    };
    let p = paint(&mut cabin);
    let at = |p: &Painted, want: &str| {
        p.texts.iter().position(|t| t.0 == want).unwrap_or_else(|| {
            panic!(
                "{want} not painted: {:?}",
                p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
            )
        })
    };
    let order = [
        "Leaving this computer",
        super::privacy_ui::PRIVACY_NOTE.as_str(),
        "Sync to paired computers",
        "Allow",
        "Command rules",
        "Deny wins over ask, and ask wins over allow. Dangerous commands still ask, including after Allow always.",
        "Rule",
    ];
    let idx: Vec<usize> = order.iter().map(|w| at(&p, w)).collect();
    assert!(
        idx.windows(2).all(|w| w[0] < w[1]),
        "order {order:?} got {idx:?}"
    );
    let ys: Vec<f32> = idx.iter().map(|&i| p.texts[i].1.y).collect();
    assert!(
        ys[4] > ys[2] && ys[6] > ys[5],
        "headings sit above their rows: {ys:?}"
    );
    let allow = p.texts[at(&p, "Allow")].1;
    assert!(
        !p.filled.iter().any(|r| r.contains(allow)),
        "SB-08: Allow is an outline, not filled"
    );

    hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    let p = paint(&mut cabin);
    let revoke = p.texts[at(&p, "Revoke")].1;
    assert!(
        !p.filled.iter().any(|r| r.contains(revoke)),
        "Revoke is a ghost, not filled"
    );
    assert!(
        p.texts
            .iter()
            .any(|t| t.0.ends_with("Sends chats, memory.")),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    release_isolated(&root, cabin);
}

// SY-10: both Permissions headings open their group with SECTION_HEAD_GAP of
// room above (same heading size), so "Command rules" no longer sits as close
// to the Sync row as rows sit to each other.
#[test]
fn permissions_headings_get_room_above_them() {
    let rows = include_str!("privacy_ui.rs")
        .split("fn ui_privacy_rows(")
        .nth(1)
        .and_then(|s| s.split("fn privacy_revoke_rows(").next())
        .expect("ui_privacy_rows");
    assert!(rows.contains("section_heading(ui, LEAVING_HEAD)"), "{rows}");
    let src = cabin_src();
    let editor = fn_src(&src, "ui_permission_editor");
    assert!(
        editor.contains("section_heading(ui, super::privacy_ui::RULES_HEAD)"),
        "{editor}"
    );
    assert!(
        !editor.contains("section_label(ui, super::privacy_ui::RULES_HEAD)"),
        "{editor}"
    );
    assert!((8.0..=12.0).contains(&crate::cards::SECTION_HEAD_GAP));

    // Painted: the gap from the Sync row to "Command rules" beats the
    // row-to-row gap by at least SECTION_HEAD_GAP (before: 21px vs 15px).
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-head-gap");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 2400.0),
        )),
        ..Default::default()
    };
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
    });
    let mut tops: Vec<(String, f32, f32)> = Vec::new();
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            tops.push((
                t.galley.text().to_string(),
                t.pos.y + t.galley.rect.top(),
                t.pos.y + t.galley.rect.bottom(),
            ));
        }
    }
    let find = |want: &str| {
        tops.iter()
            .find(|t| t.0 == want)
            .cloned()
            .unwrap_or_else(|| panic!("{want}: {tops:?}"))
    };
    let hub_hint = tops
        .iter()
        .find(|t| t.0.starts_with("Off. /sync asks"))
        .cloned()
        .expect("hub hint");
    let rules = find("Command rules");
    let gap = rules.1 - hub_hint.2;
    // Row rhythm: a help line to the next row title in the same group.
    let note = tops
        .iter()
        .find(|t| t.0.starts_with("Deny wins over ask"))
        .cloned()
        .expect("rules note");
    let row_gap = find("Rule").1 - note.2;
    assert!(
        gap - row_gap >= crate::cards::SECTION_HEAD_GAP - 0.5,
        "Command rules sits {gap}px under the Sync row; rows sit {row_gap}px apart"
    );
    release_isolated(&root, cabin);
}

// GL-05: aged slash and system results keep the result bubble. They never
// paint in the thought frame and never carry the "Thought process" label or
// its fold control, even with the session's thoughts collapsed.
#[test]
fn aged_slash_results_never_render_in_the_thought_frame() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("gl05-result-style");
    std::fs::create_dir_all(&root).unwrap();
    let report = super::privacy_ui::privacy_report(
        &grokhub_agent::harness::ConsentLedger::empty(),
        &grokhub_agent::harness::EgressRead::default(),
        None,
        false,
        1_000 * 86_400_000,
    );
    cabin.live_mut().push(("user".into(), "hello".into()));
    cabin.live_mut().push(("assistant".into(), "Hi.".into()));
    cabin.live_mut().push((
        "assistant".into(),
        mark_slash_result("Synced chats and memory to 1 computer."),
    ));
    cabin
        .live_mut()
        .push(("assistant".into(), mark_slash_result(&report)));
    cabin.live_mut().push((
        "assistant".into(),
        mark_slash_result("Sync to paired computers revoked. /sync asks again."),
    ));
    let views = cabin.cached_chat_views().to_vec();
    let kinds: Vec<ChatKind> = views.iter().map(|v| v.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ChatKind::User,
            ChatKind::Assistant,
            ChatKind::Result,
            ChatKind::Result,
            ChatKind::Result
        ]
    );
    // The aged /privacy report is still the one the ghost Revoke goes under.
    assert_eq!(super::privacy_ui::newest_privacy_row(&views), Some(3));

    let paint = |view: &ChatView, fold: ThoughtFold| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            ..Default::default()
        };
        let mut drawn = false;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                drawn = super::chat_ui::paint_chat_block_with(
                    ui, view, true, true, fold, true, false, 1,
                )
                .drawn;
            });
        });
        let mut texts: Vec<(String, bool)> = Vec::new();
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, bool)>) {
            match shape {
                egui::Shape::Text(t) => {
                    let italic = t.galley.job.sections.iter().any(|s| s.format.italics);
                    out.push((t.galley.text().to_string(), italic));
                }
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, out)),
                _ => {}
            }
        }
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
        (drawn, texts)
    };
    for view in views.iter().filter(|v| v.kind == ChatKind::Result) {
        // Expanded is what the pane passes; Minimized and Hidden are what an aged,
        // collapsed thought would get. A result ignores all three.
        for fold in [
            ThoughtFold::Expanded,
            ThoughtFold::Minimized,
            ThoughtFold::Hidden,
        ] {
            let (drawn, texts) = paint(view, fold);
            assert!(drawn, "a result is never folded away");
            for (t, italic) in &texts {
                assert!(!t.contains("Thought process"), "{t}");
                assert_ne!(
                    t,
                    grokhub_core::THOUGHT_ROW_LABEL,
                    "no thought row: {texts:?}"
                );
                assert_ne!(t, "Collapse", "no thought fold control: {texts:?}");
                assert_ne!(t, "Expand", "no thought fold control: {texts:?}");
                assert!(!italic, "a result is upright, not thought italics: {t}");
            }
            let first = view.body.lines().next().unwrap_or_default();
            assert!(
                texts.iter().any(|(t, _)| t.contains(first)),
                "{first} painted: {texts:?}"
            );
        }
    }
    // The pane paints a result with the result bubble, not the thought bubble.
    let src = include_str!("chat_ui.rs");
    let arm = src
        .split("ChatKind::Result => {")
        .nth(1)
        .and_then(|s| s.split("ChatKind::Thought => {").next())
        .expect("Result arm");
    assert!(arm.contains("paint_result_bubble"), "{arm}");
    assert!(
        !arm.contains("paint_thought_bubble")
            && !arm.contains("Thought process")
            && !arm.contains("italics"),
        "{arm}"
    );
    release_isolated(&root, cabin);
}

// GL-05 guard: aged model reasoning still collapses exactly as before: the
// "Thought process" label and Collapse while expanded, the "Thought" row and
// Expand when minimized, nothing when hidden.
#[test]
fn aged_model_reasoning_still_collapses() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("gl05-thought");
    std::fs::create_dir_all(&root).unwrap();
    cabin.live_mut().push(("user".into(), "fix it".into()));
    cabin.live_mut().push((
        "assistant".into(),
        grokhub_core::merge_thinking("Plan: read the file first.", "Looking now."),
    ));
    cabin
        .live_mut()
        .push(("assistant".into(), "Fixed the typo in main.rs.".into()));
    cabin.live_mut().push((
        "assistant".into(),
        mark_slash_result("Synced chats and memory to 1 computer."),
    ));
    let views = cabin.cached_chat_views().to_vec();
    let kinds: Vec<ChatKind> = views.iter().map(|v| v.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ChatKind::User,
            ChatKind::Thought,
            ChatKind::Thought,
            ChatKind::Assistant,
            ChatKind::Result
        ]
    );
    let thought = &views[1];
    let paint = |fold: ThoughtFold| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            ..Default::default()
        };
        let mut painted = None;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                painted = Some(super::chat_ui::paint_chat_block_with(
                    ui, thought, true, true, fold, true, false, 1,
                ));
            });
        });
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                texts.push(t.galley.text().to_string());
            }
        }
        (painted.expect("painted").drawn, texts)
    };
    let (drawn, texts) = paint(ThoughtFold::Expanded);
    assert!(drawn);
    assert!(texts.iter().any(|t| t == "Thought process"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Collapse"), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Plan: read the file first.")),
        "{texts:?}"
    );
    let (drawn, texts) = paint(ThoughtFold::Minimized);
    assert!(drawn);
    assert!(
        texts.iter().any(|t| t == grokhub_core::THOUGHT_ROW_LABEL),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t == "Expand"), "{texts:?}");
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("Plan: read the file first.")),
        "{texts:?}"
    );
    let (drawn, _) = paint(ThoughtFold::Hidden);
    assert!(!drawn, "a hidden thought still paints nothing");
    release_isolated(&root, cabin);
}

// SY-04: the ghost Revoke under /privacy clears the grant and drops the share.
// Only a pointer click answers it; Enter and typed text never do.
#[test]
fn revoke_from_privacy_clears_the_grant_on_a_click_only() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("privacy-revoke");
    std::fs::create_dir_all(&root).unwrap();
    let g = hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    cabin.hub.lock().unwrap().snapshot = Some(std::sync::Arc::new(
        serde_json::json!({"kind": "grokhub-hub-v1"}),
    ));
    let rows = cabin.privacy_revoke_rows();
    assert_eq!(
        rows,
        vec![super::privacy_ui::PrivacyRevoke {
            id: g.id.clone(),
            label: "Sync to paired computers".to_string(),
            detail: None,
        }]
    );

    // Slash text cannot revoke (or grant): it is not a click.
    cabin.run_slash_line("/privacy revoke");
    cabin.harness.consent = None;
    assert_eq!(cabin.consent().active().count(), 1);

    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 400.0));
    let pass = |events: Vec<egui::Event>| {
        let mut hit = None;
        let mut texts = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                hit = super::privacy_ui::paint_privacy_revokes(ui, &rows, None);
            });
        });
        let mut revoke_at = None;
        for clipped in &out.shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                let label = t.galley.text().to_string();
                if label == "Revoke" {
                    revoke_at = Some(t.pos + t.galley.rect.center().to_vec2());
                }
                texts.push(label);
            }
        }
        (hit, revoke_at, texts)
    };
    let (hit, at, texts) = pass(vec![]);
    assert_eq!(hit, None);
    assert!(
        texts.iter().all(|t| !t.contains(&g.id)),
        "no grant id on screen: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "Sync to paired computers"),
        "{texts:?}"
    );
    let at = at.expect("Revoke pill");
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let (hit, _, _) = pass(vec![enter, egui::Event::Text("revoke".into())]);
    assert_eq!(hit, None, "keys never revoke");
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = pass(vec![egui::Event::PointerMoved(at)]);
    let _ = pass(vec![press(true)]);
    let (hit, _, _) = pass(vec![press(false)]);
    assert_eq!(
        hit.as_deref(),
        Some(g.id.as_str()),
        "a click names the grant"
    );

    let before = cabin.messages.len();
    cabin.revoke_from_privacy(&g.id);
    assert_eq!(cabin.consent().active().count(), 0, "the grant is gone");
    assert!(hx::ConsentLedger::load(&root)
        .destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA)
        .is_none());
    assert!(
        cabin.hub.lock().unwrap().snapshot.is_none(),
        "revoke drops the share"
    );
    assert_eq!(
        cabin.status,
        "Sync to paired computers revoked. /sync asks again."
    );
    assert_eq!(cabin.messages.len(), before + 1);
    assert!(
        cabin.privacy_revoke_rows().is_empty(),
        "no grant, no Revoke"
    );
    // An unknown or already revoked id is a no-op.
    cabin.revoke_from_privacy(&g.id);
    assert_eq!(cabin.messages.len(), before + 1);
    release_isolated(&root, cabin);
}

#[test]
fn scope_rows_are_all_off_under_what_grokhub_can_read() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-rows");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let ys: Vec<f32> = [
        "Leaving this computer",
        "Sync to paired computers",
        "What Loadout can read",
        super::scope_ui::SCOPES_NOTE,
        "Files in a folder",
        "Installed apps",
        "Browser history",
        "Calendar",
        "Mail",
        "System state",
        "Command rules",
    ]
    .iter()
    .map(|w| p.at(w).y)
    .collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "rows in order: {ys:?}");
    for (kind, label) in grokhub_agent::harness::SCOPE_KINDS {
        let allow = p.pill_by(label, "Allow");
        // The folder row's field, dialog button and Allow share a line under its hint.
        let reach = if *kind == "files" { 56.0 } else { 24.0 };
        assert!(
            (allow.y - p.at(label).y).abs() < reach,
            "{kind}: Allow sits on its row"
        );
        // SB-08: an outline Allow, so the off rows read as the calm default.
        assert!(
            !p.filled.iter().any(|r| r.contains(allow)),
            "{kind}: Allow is an outline, not filled"
        );
    }
    assert!(
        !p.filled
            .iter()
            .any(|r| r.contains(p.pill_by("Sync to paired computers", "Allow"))),
        "Sync's Allow too"
    );
    assert!(!p.has("Revoke"), "nothing granted, nothing to revoke");
    assert!(
        p.has("Off. Not read yet: Loadout has no calendar reader, so allowing it reads nothing today."),
        "plain hints"
    );
    assert!(
        p.has("Off. Not read yet: Loadout has no mail reader, so allowing it reads nothing today."),
        "the hint says mail is not read yet"
    );
    // SB-04: the folder row offers the native dialog next to the typed path.
    let choose = p.pill_by("Files in a folder", super::scope_ui::CHOOSE_FOLDER);
    assert!(
        (choose.y - p.pill_by("Files in a folder", "Allow").y).abs() < 2.0,
        "the dialog button sits by the folder's Allow"
    );
    assert!(
        !root.join("consent.jsonl").exists(),
        "painting writes nothing"
    );
    release_isolated(&root, cabin);
}

#[test]
fn a_scope_is_granted_by_a_pointer_click_only_and_revoked_from_settings_or_privacy() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-grant");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let _ = paint_permissions(&ctx, &mut cabin, vec![]);

    // Keys never grant: walk focus over every widget and press Enter and Space.
    for _ in 0..40 {
        let _ = paint_permissions(&ctx, &mut cabin, vec![key(egui::Key::Tab)]);
        let _ = paint_permissions(&ctx, &mut cabin, vec![key(egui::Key::Enter)]);
        let _ = paint_permissions(&ctx, &mut cabin, vec![key(egui::Key::Space)]);
    }
    assert_eq!(
        hx::ConsentLedger::load(&root).active().count(),
        0,
        "keyboard never grants"
    );
    // Neither does slash text.
    cabin.run_slash_line("/privacy allow apps");
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);

    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    click_at(&ctx, &mut cabin, p.pill_by("Installed apps", "Allow"));
    assert_eq!(
        cabin.status,
        "Installed apps allowed. Loadout learns from it on this computer. Revoke it here any time."
    );
    let ledger = hx::ConsentLedger::load(&root);
    let g = ledger
        .scope_grant(&hx::Scope::Apps)
        .cloned()
        .expect("apps granted");
    assert_eq!(ledger.active().count(), 1, "one click, one scope");
    let raw = std::fs::read_to_string(root.join("consent.jsonl")).unwrap();
    assert!(
        !raw.contains("apps") && !raw.contains(&g.id),
        "the ledger is sealed: {raw}"
    );

    // The row turns into a ghost Revoke; the others stay off.
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let revoke = p.pill_by("Installed apps", "Revoke");
    assert!(
        !p.filled.iter().any(|r| r.contains(revoke)),
        "Revoke is a ghost"
    );
    assert!(p
        .texts
        .iter()
        .any(|t| t.0.starts_with("On since ") && t.0.ends_with("how often you open them.")));
    assert!(
        p.has("Off. Not read yet: Loadout has no mail reader, so allowing it reads nothing today.")
    );

    // A folder: typed, then clicked. Home itself is refused even with a click.
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        cabin.harness.scope_folder = home.clone();
        let p = paint_permissions(&ctx, &mut cabin, vec![]);
        click_at(&ctx, &mut cabin, p.pill_by("Files in a folder", "Allow"));
        assert_eq!(
            cabin.status,
            "Not allowed: files: one folder at a time, never all of your home folder"
        );
        let p = paint_permissions(&ctx, &mut cabin, vec![]);
        assert!(
            p.has(&cabin.status),
            "the refusal shows in Settings, not only on the hidden status line"
        );
        assert!(p.at(&cabin.status).y > p.at("What Loadout can read").y);
    }
    cabin.harness.scope_folder = "/srv/notes/".into();
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    click_at(&ctx, &mut cabin, p.pill_by("Files in a folder", "Allow"));
    assert_eq!(
        cabin.status,
        "Files in notes allowed. Loadout learns from it on this computer. Revoke it here any time."
    );
    assert!(cabin.harness.scope_folder.is_empty());
    // SB-03 / SB-06: the folder name titles the row, the path is in the hint,
    // and the next folder row reads "Add a folder".
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        p.has("Files in notes") && p.has(super::scope_ui::ADD_FOLDER),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert!(p
        .texts
        .iter()
        .any(|t| t.0.starts_with("On since ") && t.0.ends_with(". /srv/notes")));
    assert!(!p.has("Files in a folder"));

    // /privacy lists both by plain name, and its ghost Revoke works for scopes.
    cabin.harness.consent = None;
    let rows = cabin.privacy_revoke_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(names, vec!["Installed apps", "Files in notes"]);
    assert_eq!(
        rows[1].detail.as_deref(),
        Some("/srv/notes"),
        "the whole path rides along for hover"
    );
    let report = super::privacy_ui::privacy_report(
        &hx::ConsentLedger::load(&root),
        &hx::EgressRead::default(),
        None,
        false,
        g.granted_at + 60_000,
    );
    assert!(
        report.contains("\n- Installed apps: on since 1m ago\n- Files in notes: on since "),
        "{report}"
    );
    assert!(report.contains("\n- Off: Sync to paired computers (/sync asks each time) · Browser history · Calendar · Mail · System state\n"), "{report}");
    assert!(
        !report.contains("Read apps") && !report.contains(&g.id),
        "{report}"
    );
    let files_id = rows[1].id.clone();
    let before = cabin.messages.len();
    cabin.revoke_from_privacy(&files_id);
    assert_eq!(
        cabin.status,
        "Files in notes revoked. It's off until you allow it again."
    );
    assert_eq!(cabin.messages.len(), before + 1);

    // Settings Revoke.
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    click_at(&ctx, &mut cabin, p.pill_by("Installed apps", "Revoke"));
    assert_eq!(
        cabin.status,
        "Installed apps revoked. It's off until you allow it again."
    );
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(!p.has("Revoke"), "all off again");
    release_isolated(&root, cabin);
}

#[test]
fn locked_private_data_shows_one_message_and_grants_or_syncs_nothing() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-locked");
    std::fs::create_dir_all(&root).unwrap();
    hx::use_key_store_for(
        &root,
        std::sync::Arc::new(hx::MemoryKeyStore::unavailable()),
    );
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let msg = hx::Locked::Unavailable.message();
    let next =
        super::privacy_ui::lock_next_step_short(&hx::Locked::Unavailable, hx::KeyringOs::current());
    assert!(
        msg.starts_with("Your private data is locked because Loadout can't access your "),
        "{msg}"
    );
    assert!(
        p.has(&msg),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert_eq!(
        p.texts.iter().filter(|t| t.0 == msg).count(),
        1,
        "said once"
    );
    assert!(
        p.at(&msg).y < p.at("Sync to paired computers").y,
        "said at the top, above every row it holds"
    );
    // SB-01: every Allow is disabled, so a click does nothing at all.
    cabin.status.clear();
    click_at(&ctx, &mut cabin, p.pill_by("Calendar", "Allow"));
    assert_eq!(cabin.status, "", "a disabled Allow takes no click");
    click_at(
        &ctx,
        &mut cabin,
        p.pill_by("Sync to paired computers", "Allow"),
    );
    assert_eq!(cabin.status, "");
    assert!(
        !root.join("consent.jsonl").exists(),
        "no plaintext fallback"
    );
    assert!(!root.join(hx::KEY_ID_FILE).exists());

    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert!(
        cabin.harness.park.is_none() && cabin.sync_rx.is_none(),
        "nothing is sent"
    );
    assert_eq!(cabin.status, format!("Not synced. {msg} {next}"));
    let ledger = hx::ConsentLedger::load(&root);
    let lock = super::privacy_ui::private_lock(&root, &ledger, true);
    let report = super::privacy_ui::privacy_report(
        &ledger,
        &hx::read_egress_report(&root),
        lock.as_ref(),
        false,
        1_000,
    );
    assert!(
        report.contains(&format!("\n\n{msg}\n{next}\n\nGrants\n")),
        "{report}"
    );
    assert!(!root.join("egress.jsonl").exists());
    release_isolated(&root, cabin);
}

// SB-01: while locked, each "Allow" (and the folder dialog button) is painted
// faded by egui's disabled look; returns (label, opacity) for each pill text.
// The opacity is the painted text colour's alpha (egui multiplies it by
// `disabled_alpha`). Only rows above "Command rules" count.
fn pill_opacity(
    ctx: &egui::Context,
    cabin: &mut Cabin,
    events: Vec<egui::Event>,
) -> (Vec<(String, f32)>, Vec<String>) {
    fn walk(
        shape: &egui::Shape,
        pills: &mut Vec<(String, f32, f32)>,
        all: &mut Vec<(String, f32)>,
    ) {
        match shape {
            egui::Shape::Text(t) => {
                let text = t.galley.text().to_string();
                if [
                    "Allow",
                    "Revoke",
                    super::scope_ui::CHOOSE_FOLDER,
                    super::privacy_ui::TRY_AGAIN,
                ]
                .contains(&text.as_str())
                {
                    pills.push((text.clone(), t.fallback_color.a() as f32 / 255.0, t.pos.y));
                }
                all.push((text, t.pos.y));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, pills, all)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 2400.0),
        )),
        events,
        ..Default::default()
    };
    let out = crate::theme::test_pass(ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
    });
    let (mut pills, mut all) = (Vec::new(), Vec::new());
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut pills, &mut all);
    }
    let rules = all
        .iter()
        .find(|t| t.0 == "Command rules")
        .map(|t| t.1)
        .unwrap_or(f32::MAX);
    let pills = pills
        .into_iter()
        .filter(|p| p.2 < rules)
        .map(|p| (p.0, p.1))
        .collect();
    (pills, all.into_iter().map(|t| t.0).collect())
}

#[test]
fn locked_state_disables_every_allow_with_the_lock_on_hover() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-locked-disabled");
    std::fs::create_dir_all(&root).unwrap();
    hx::use_key_store_for(
        &root,
        std::sync::Arc::new(hx::MemoryKeyStore::unavailable()),
    );
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    ctx.global_style_mut(|s| s.interaction.tooltip_delay = 0.0);
    let (pills, texts) = pill_opacity(&ctx, &mut cabin, vec![]);
    let allows: Vec<f32> = pills
        .iter()
        .filter(|p| p.0 == "Allow")
        .map(|p| p.1)
        .collect();
    assert_eq!(allows.len(), 7, "six scope rows and Sync: {pills:?}");
    assert!(
        allows.iter().all(|o| *o < 1.0),
        "every Allow takes the disabled look: {pills:?}"
    );
    let choose = pills
        .iter()
        .find(|p| p.0 == super::scope_ui::CHOOSE_FOLDER)
        .expect("dialog button");
    assert!(
        choose.1 < 1.0,
        "the folder dialog is disabled too: {pills:?}"
    );
    let retry = pills
        .iter()
        .find(|p| p.0 == super::privacy_ui::TRY_AGAIN)
        .expect("Try again");
    assert!(
        (retry.1 - 1.0).abs() < f32::EPSILON,
        "Try again stays live: {pills:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Private data is unavailable"),
        "no hover yet"
    );

    // Hover a disabled Allow: the lock shows.
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let at = p.pill_by("Calendar", "Allow");
    let mut shown = false;
    for _ in 0..4 {
        let (_, texts) = pill_opacity(&ctx, &mut cabin, vec![egui::Event::PointerMoved(at)]);
        shown |= texts.iter().any(|t| t == "Private data is unavailable");
    }
    assert!(shown, "hover names the lock");
    assert_eq!(
        super::privacy_ui::lock_hover(&hx::Locked::Unavailable),
        "Private data is unavailable"
    );

    // A click on any of them writes nothing and says nothing new.
    cabin.status.clear();
    for title in [
        "Calendar",
        "Installed apps",
        "Sync to paired computers",
        "Files in a folder",
    ] {
        click_at(&ctx, &mut cabin, p.pill_by(title, "Allow"));
    }
    assert_eq!(cabin.status, "");
    assert!(
        cabin.harness.scope_pick_rx.is_none(),
        "the dialog never opened"
    );
    assert!(!root.join("consent.jsonl").exists());

    // Unlocked, the same pills are live.
    hx::use_key_store_for(&root, std::sync::Arc::new(hx::MemoryKeyStore::new()));
    cabin.retry_keyring();
    let (pills, _) = pill_opacity(&ctx, &mut cabin, vec![]);
    assert!(
        pills
            .iter()
            .filter(|p| p.0 == "Allow")
            .all(|p| (p.1 - 1.0).abs() < f32::EPSILON),
        "{pills:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn try_again_asks_the_keyring_again_and_refreshes_permissions() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-try-again");
    std::fs::create_dir_all(&root).unwrap();
    let store = std::sync::Arc::new(hx::MemoryKeyStore::unavailable());
    hx::use_key_store_for(&root, store.clone());
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let msg = hx::Locked::Unavailable.message();
    let step =
        super::privacy_ui::lock_next_step(&hx::Locked::Unavailable, hx::KeyringOs::current());
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        p.has(&msg) && p.has(&step),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    let retry = p.pill_by(&step, super::privacy_ui::TRY_AGAIN);
    assert!(
        (retry.y - p.at(&step).y).abs() < 24.0,
        "Try again sits on the next-step line"
    );

    // The keyring comes back, but its "down" answer is reused for 30 s, so
    // the page alone stays locked.
    store.set_available(true);
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(p.has(&msg), "the cached answer still holds");

    // Try again asks now, and the page refreshes: no lock, live Allow.
    click_at(
        &ctx,
        &mut cabin,
        p.pill_by(&step, super::privacy_ui::TRY_AGAIN),
    );
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        !p.has(&msg) && !p.has(&step),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert_eq!(cabin.status, "Private data is available again.");
    let (pills, _) = pill_opacity(&ctx, &mut cabin, vec![]);
    assert!(
        pills
            .iter()
            .filter(|p| p.0 == "Allow")
            .all(|p| (p.1 - 1.0).abs() < f32::EPSILON),
        "{pills:?}"
    );
    // It granted nothing on its own.
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);

    // Down again: Try again says it is still locked, and when it checked.
    store.set_available(false);
    cabin.retry_keyring();
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(p.has(&msg));
    assert!(
        p.has("Still locked. Checked just now."),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    release_isolated(&root, cabin);
}

// SB-05: /privacy names each scope once: on ones as bullets with a since-time,
// the rest on one "Off:" line, all under one Grants heading.
#[test]
fn privacy_lists_each_scope_once_under_one_grants_heading() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, cabin) = isolated_cabin("privacy-once");
    std::fs::create_dir_all(&root).unwrap();
    let home = std::path::Path::new("/home/someone");
    for scope in [
        hx::Scope::Calendar,
        hx::Scope::Files("/home/someone/Documents/notes".into()),
    ] {
        hx::grant_scope(&root, &scope, Some(home), hx::UserClick::from_click()).unwrap();
    }
    let ledger = hx::ConsentLedger::load(&root);
    let at = ledger
        .active()
        .map(|g| g.granted_at)
        .max()
        .unwrap_or_default();
    let report = super::privacy_ui::privacy_report(
        &ledger,
        &hx::EgressRead::default(),
        None,
        false,
        at + 60_000,
    );
    let grants = report
        .split("\nGrants\n")
        .nth(1)
        .and_then(|r| r.split("\n\n").next())
        .expect("Grants");
    assert_eq!(
        grants,
        [
            "- Calendar: on since 1m ago",
            "- Files in notes: on since 1m ago",
            "- Off: Sync to paired computers (/sync asks each time) · Installed apps · Browser history · Mail · System state",
            "- Screen: \"Let Grok control the desktop\" in Settings → Defaults (off)",
            "Granted folders, apps, browser history and system state are read on this computer only, never on battery or in quiet hours; calendar and mail aren't read yet.",
        ]
        .join("\n")
    );
    for word in [
        "Calendar",
        "Files in",
        "Installed apps",
        "Browser history",
        "Mail",
        "System state",
        "Sync to paired computers",
    ] {
        assert_eq!(report.matches(word).count(), 1, "{word} once: {report}");
    }
    assert_eq!(report.matches("Grants").count(), 1, "one heading: {report}");
    assert!(
        !report.contains(super::scope_ui::SCOPES_HEAD.as_str()),
        "no second grants heading: {report}"
    );
    assert!(
        !report.contains("/home/someone/Documents"),
        "the short label, not the path: {report}"
    );
    release_isolated(&root, cabin);
}

// The on-device model lines on the Privacy page, in PRIVACY.md and in the
// installer's privacy.txt say the same thing, each line once.
#[test]
fn local_model_routing_lines_match_the_privacy_docs() {
    let lines = super::privacy_page::LOCAL_ROUTING;
    assert_eq!(
        lines[0],
        "Online, the cloud takes every step while any cloud model answers. This PC's model is the last resort, never for hard or long work."
    );
    assert_eq!(
        lines[4],
        "Explicit 18+ chats go only to xAI or this PC's model."
    );
    for (name, doc) in [
        ("PRIVACY.md", include_str!("../../../../PRIVACY.md")),
        (
            "privacy.txt",
            include_str!("../../../../packaging/windows/privacy.txt"),
        ),
    ] {
        assert_eq!(
            doc.matches("Loadout sends data only to the AI services and accounts the user configures, and only when the user asks.").count(),
            1,
            "{name} names the product"
        );
        for line in lines {
            assert_eq!(
                doc.matches(&format!("- {line}")).count(),
                1,
                "{name} lists {line:?} once"
            );
        }
    }
}
