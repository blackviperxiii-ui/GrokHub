//! Pactwatch decision inbox (Spike-1b).
//!
//! The needs-attention line ("N things need a decision. Everything else is on
//! track.") opens into one row per decision on Home (the chat's approval
//! stack) and the Workboard: hard parks, Grok Build asks, recovery-ladder
//! pauses, the Grant full offer, an MCP input ask, and a Spike-8a scope ask
//! (its Approve opens the card, where only a click grants). Each row says what Grok
//! wants in plain words and which chat it is from, with Approve and Deny.
//! A row answers through the same function as its card, so both write the
//! same span and both clear. Clicking a row's words jumps to its card.
//!
//! D2: no new chrome. The rows live under the existing line; no Nav variant,
//! page, panel, or chip. Hard rows follow the hard-card keys: no Always,
//! Enter never approves (a pointer click only), Esc on a focused row denies.

use super::*;
use grokhub_agent::harness as hx;
use grokhub_agent::HardClass;

use super::harness_ui::SOFT_DENY;

/// What the line adds after the count.
pub(super) const INBOX_TAIL: &str = "Everything else is on track.";
/// Longest row text before it is cut with an ellipsis.
const ROW_CHARS: usize = 120;

/// The needs-attention line with `n` decisions waiting.
pub(super) fn inbox_line(n: usize) -> String {
    format!("{}. {INBOX_TAIL}", crate::motion::needs_attention_summary(n))
}

/// Which decision a row answers. Indexes count the card on screen first,
/// then its queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InboxKey {
    Hard(usize),
    Ask(usize),
    Soft(usize),
    Full,
    Elicit,
    /// A Spike-8a scope ask. Approve opens its card: only a click there grants.
    Scope(usize),
}

impl InboxKey {
    /// The card painter that answers to a jump.
    fn card(self) -> &'static str {
        match self {
            Self::Hard(_) => "hard",
            Self::Ask(_) => "ask",
            Self::Soft(_) => "soft",
            Self::Full => "full",
            Self::Elicit => "elicit",
            Self::Scope(_) => "scope",
        }
    }

    fn salt(self) -> String {
        format!("{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct InboxRow {
    pub key: InboxKey,
    pub hard: bool,
    /// What Grok wants to do, in plain words.
    pub what: String,
    /// The chat it is from, by title.
    pub chat: String,
    pub chat_id: String,
}

fn cut(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= ROW_CHARS {
        return flat;
    }
    let mut out: String = flat.chars().take(ROW_CHARS - 1).collect();
    out.push('…');
    out
}

fn capitalized(text: &str) -> String {
    let mut c = text.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Keys and values you typed into a secret prompt never show on a row. The park
/// file is scrubbed when it's written; this catches older files and Grok Build asks.
fn scrub(text: &str, held: &[String]) -> String {
    grokhub_core::redact_held_secrets(&grokhub_core::redact_secret_words(text), held)
}

/// A hard park in plain words. A delete names its paths and count.
pub(super) fn hard_words(class: HardClass, tool: &str, action: &str, held: &[String]) -> String {
    let action = scrub(action, held);
    let action = if action.trim().is_empty() { tool } else { action.trim() };
    let text = match class {
        HardClass::Delete => {
            let paths = hx::delete_targets(action);
            if action.starts_with("delete ") || action.starts_with("move to the trash ") || action.starts_with("press ") {
                capitalized(action)
            } else if paths.is_empty() {
                format!("Delete: {action}")
            } else {
                let noun = if paths.len() == 1 { "path" } else { "paths" };
                format!("Delete {} {noun}: {}", paths.len(), paths.join(", "))
            }
        }
        HardClass::Send => format!("Send: {action}"),
        HardClass::Money => format!("Spend money: {action}"),
        HardClass::Credentials => capitalized(action),
        HardClass::IrreversibleOs => format!("Irreversible: {action}"),
    };
    cut(&text)
}

/// A Grok Build ask in plain words.
pub(super) fn ask_words(p: &grokhub_acp::PermissionAsk, held: &[String]) -> String {
    let (title, action) = (scrub(&p.title, held), scrub(&p.action, held));
    let (title, action) = (title.trim(), action.trim());
    let text = if super::harness_ui::is_desktop_ask(p) {
        format!("Use the desktop: {}", if action.is_empty() { title } else { action })
    } else if action.is_empty() {
        title.to_string()
    } else {
        format!("{title}: {action}")
    };
    cut(&text)
}

impl Cabin {
    fn chat_title(&self, chat_id: &str) -> String {
        self.threads
            .iter()
            .find(|t| t.id == chat_id)
            .map(|t| t.title.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "this chat".into())
    }

    /// The chat a live ask or offer belongs to: the one running the turn.
    fn turn_chat(&self) -> String {
        self.chat_job_thread.clone().unwrap_or_else(|| self.visible_thread_id())
    }

    /// One row per decision, in the order the cards stack. Its length is
    /// [`Self::decisions_waiting`].
    pub(super) fn inbox_rows(&self) -> Vec<InboxRow> {
        let mut rows = Vec::new();
        let row = |key, hard, what: String, chat_id: String| InboxRow {
            key,
            hard,
            what,
            chat: self.chat_title(&chat_id),
            chat_id,
        };
        for (i, p) in self.harness.park.iter().chain(self.harness.queue.iter()).enumerate() {
            rows.push(row(InboxKey::Hard(i), true, hard_words(p.class, &p.tool, &p.action, &self.secret_hold), p.chat_id.clone()));
        }
        for (i, p) in self.perm_ask.iter().chain(self.perm_queue.iter()).enumerate() {
            rows.push(row(InboxKey::Ask(i), false, ask_words(p, &self.secret_hold), self.turn_chat()));
        }
        for (i, p) in self.harness.soft_parks.iter().enumerate() {
            let what = cut(&format!("Paused and needs you: {}", p.reason));
            rows.push(row(InboxKey::Soft(i), false, what, p.chat_id.clone()));
        }
        if self.harness.full_card.is_some() {
            rows.push(row(InboxKey::Full, false, "Click and type without asking for this session".into(), self.turn_chat()));
        }
        if let Some(e) = &self.elicit_ask {
            rows.push(row(InboxKey::Elicit, false, cut(&format!("{} wants input", e.server_name)), self.turn_chat()));
        }
        for (i, a) in self.harness.indexer.asks.all().iter().enumerate() {
            let label = super::scope_ui::scope_label(&a.scope.key());
            rows.push(row(InboxKey::Scope(i), false, cut(&format!("Learn from {label}: {}", a.why)), self.visible_thread_id()));
        }
        rows
    }

    /// Approve or Deny from a row: the same call its card makes.
    pub(super) fn inbox_answer(&mut self, key: InboxKey, approve: bool) {
        match key {
            InboxKey::Hard(i) => self.resolve_hard_park_at(i, approve, if approve { "" } else { SOFT_DENY }),
            InboxKey::Ask(i) => self.answer_perm_at(i, approve, SOFT_DENY),
            InboxKey::Soft(i) => self.answer_soft_park(i, approve, SOFT_DENY),
            InboxKey::Full => self.resolve_grant_full(approve, "Jeremy kept Supervised"),
            InboxKey::Elicit if approve => self.inbox_jump(key),
            InboxKey::Elicit => {
                if let Some(p) = self.elicit_ask.take() {
                    if let Some(h) = &self.acp {
                        let _ = h.answer_elicit(p.rpc_id, "cancel", None);
                    }
                }
                self.elicit_draft.clear();
            }
            InboxKey::Scope(_) if approve => self.inbox_jump(key),
            InboxKey::Scope(i) => {
                if let Some(a) = self.harness.indexer.asks.all().get(i).cloned() {
                    self.harness.indexer.asks.remove(&a.scope);
                    self.status = format!("{} stays off.", super::scope_ui::scope_label(&a.scope.key()));
                }
            }
        }
    }

    /// Open the row's chat and bring its card up: a queued card moves onto
    /// the card slot, and the card scrolls into view once.
    pub(super) fn inbox_jump(&mut self, key: InboxKey) {
        let rows = self.inbox_rows();
        let Some(row) = rows.iter().find(|r| r.key == key) else {
            return;
        };
        if row.chat_id != self.visible_thread_id() {
            if let Some(idx) = self.threads.iter().position(|t| t.id == row.chat_id) {
                self.switch_thread(idx);
            }
        }
        self.nav = Nav::Chat;
        match key {
            InboxKey::Hard(i) => self.promote_hard_park(i),
            InboxKey::Ask(i) if i > 0 => {
                if let Some(p) = self.perm_queue.remove(i - 1) {
                    if let Some(head) = self.perm_ask.take() {
                        self.perm_queue.push_front(head);
                    }
                    self.perm_ask = Some(p);
                    self.perm_always_confirm = None;
                }
            }
            InboxKey::Soft(i) if i > 0 => {
                let p = self.harness.soft_parks.remove(i);
                self.harness.soft_parks.insert(0, p);
            }
            InboxKey::Scope(i) => self.harness.indexer.asks.promote(i),
            _ => {}
        }
        self.harness.jump = Some(key.card());
    }

    /// The needs-attention line, and the rows under it when open. Nothing
    /// when no decision waits.
    pub(super) fn paint_inbox(&mut self, ui: &mut egui::Ui) {
        let n = self.decisions_waiting();
        if n == 0 {
            self.harness.inbox_open = false;
            return;
        }
        let open = self.harness.inbox_open;
        let line = egui::Label::new(
            RichText::new(inbox_line(n)).size(12.0).color(crate::theme::muted()),
        )
        .wrap()
        .sense(egui::Sense::click());
        let resp = ui.add(line).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            self.harness.inbox_open = !open;
        }
        let t = inbox_open_t(ui, self.harness.inbox_open);
        if t <= 0.0 {
            return;
        }
        let rows = self.inbox_rows();
        let mut act: Option<(InboxKey, Option<bool>)> = None;
        ui.scope(|ui| {
            ui.multiply_opacity(t);
            for row in &rows {
                ui.add_space(6.0);
                if let Some(a) = paint_row(ui, row) {
                    act = Some((row.key, a));
                }
            }
        });
        match act {
            Some((key, Some(approve))) => self.inbox_answer(key, approve),
            Some((key, None)) => self.inbox_jump(key),
            None => {}
        }
    }
}

/// Rows open over [`crate::motion::INBOX_SETTLE_SECS`] and then stop; reduced
/// motion snaps.
fn inbox_open_t(ui: &egui::Ui, open: bool) -> f32 {
    if crate::motion::reduced_motion(ui) {
        return if open { 1.0 } else { 0.0 };
    }
    ui.ctx().animate_bool_with_time_and_easing(
        egui::Id::new("inbox-open"),
        open,
        crate::motion::INBOX_SETTLE_SECS,
        egui::emath::easing::cubic_out,
    )
}

/// Ids of a row's two buttons, so a key handler can tell when they have focus.
pub(super) fn row_button_ids(key: InboxKey) -> (egui::Id, egui::Id) {
    (egui::Id::new(("inbox-approve", key.salt())), egui::Id::new(("inbox-deny", key.salt())))
}

/// Keys on a focused row. Esc denies a hard row. Enter never approves one;
/// on a soft row the focused button's own click answers.
pub(super) fn row_key(hard: bool, focused: bool, esc: bool) -> Option<bool> {
    (hard && focused && esc).then_some(false)
}

/// One row: the words (click to jump) and the chat, then Approve and Deny.
/// `Some(Some(approve))` answers, `Some(None)` jumps.
fn paint_row(ui: &mut egui::Ui, row: &InboxRow) -> Option<Option<bool>> {
    let stroke = if row.hard {
        egui::Stroke::new(1.0, crate::theme::fg())
    } else {
        egui::Stroke::new(1.0, crate::theme::border())
    };
    let column = ui.available_width();
    let inner = super::harness_ui::approval_card_inner(column, stroke.width);
    let mut out = None;
    let (approve_id, deny_id) = row_button_ids(row.key);
    egui::Frame::NONE
        .fill(egui::Color32::TRANSPARENT)
        .corner_radius(crate::theme::CHROME_RADIUS)
        .stroke(stroke)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(inner);
            ui.set_max_width(inner);
            let words = ui
                .add(
                    egui::Label::new(RichText::new(&row.what).size(13.0).color(crate::theme::fg()))
                        .wrap()
                        .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let from = ui
                .add(
                    egui::Label::new(
                        RichText::new(format!("From {}", row.chat)).size(12.0).color(crate::theme::muted()),
                    )
                    .wrap()
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if words.clicked() || from.clicked() {
                out = Some(None);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let primary = if row.key == InboxKey::Elicit { "Open" } else { "Approve" };
                let approve = if row.hard {
                    crate::cards::felt_pill_at(ui, Some(approve_id), primary, crate::cards::PillStyle::Danger)
                        .clicked_by(egui::PointerButton::Primary)
                } else {
                    crate::cards::felt_pill_at(ui, Some(approve_id), primary, crate::cards::PillStyle::Solid).clicked()
                };
                let deny = crate::cards::felt_pill_at(ui, Some(deny_id), "Deny", crate::cards::PillStyle::Ghost).clicked();
                if approve {
                    out = Some(Some(true));
                } else if deny {
                    out = Some(Some(false));
                }
            });
            // egui drops focus when Esc arrives, so last frame's focus counts.
            let focus_key = approve_id.with("focused");
            let focused = ui.memory(|m| m.has_focus(approve_id) || m.has_focus(deny_id));
            let was = ui.data_mut(|d| std::mem::replace(d.get_temp_mut_or_default::<bool>(focus_key), focused));
            if let Some(answer) = row_key(row.hard, focused || was, super::chat_ui::bare_press(ui, egui::Key::Escape)) {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                out = Some(Some(answer));
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::harness_ui::SoftPark;

    fn ask(title: &str, action: &str) -> grokhub_acp::PermissionAsk {
        grokhub_acp::PermissionAsk {
            rpc_id: serde_json::json!(7),
            session_id: "s".into(),
            title: title.into(),
            tool_call_id: "t".into(),
            action: action.into(),
            reason: String::new(),
            reject_option: None,
        }
    }

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::create_dir_all(&root);
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    /// Always + Full: the loosest the cabin gets.
    fn loose_cabin() -> Cabin {
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        cabin.cfg.desktop_control = true;
        cabin.harness.access_full = true;
        cabin
    }

    fn pause(chat_id: &str) -> SoftPark {
        SoftPark {
            detector: "action_loop".into(),
            reason: "`click` ran 3 times".into(),
            evidence: vec!["session:1".into()],
            chat_id: chat_id.into(),
        }
    }

    /// A hard rm, a desktop ask, and a ladder pause.
    fn three_mixed(cabin: &mut Cabin) {
        assert_eq!(cabin.harness_precheck(ask("Run command", "rm -f /tmp/a.txt /tmp/b.txt")), None);
        cabin.perm_ask = Some(ask("grokhub-desktop__click", "click 10,20"));
        let chat = cabin.trace_id();
        cabin.harness.soft_parks.push(pause(&chat));
    }

    fn spans(root: &std::path::Path, trace: &str) -> Vec<(String, String, String, String, String)> {
        hx::read_spans(root, trace)
            .unwrap()
            .into_iter()
            .map(|s| (s.path, s.tool, s.decision, s.result, s.approval_class))
            .collect()
    }

    fn row(path: &str, tool: &str, decision: &str, result: &str, class: &str) -> (String, String, String, String, String) {
        (path.into(), tool.into(), decision.into(), result.into(), class.into())
    }

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, cabin: &mut Cabin) {
        let raw = egui::RawInput { events, ..Default::default() };
        let _ = crate::theme::test_pass(ctx, raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.paint_inbox(ui));
        });
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect()
    }

    fn snapped() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        ctx
    }

    #[test]
    fn line_reads_as_the_spec_and_zero_shows_nothing_new() {
        assert_eq!(inbox_line(1), "1 thing needs a decision. Everything else is on track.");
        assert_eq!(inbox_line(3), "3 things need a decision. Everything else is on track.");
        let (_pin, root) = pinned("inbox-zero");
        let mut cabin = Cabin::quiet_for_test();
        cabin.harness.inbox_open = true;
        assert_eq!(cabin.decisions_waiting(), 0);
        assert!(cabin.inbox_rows().is_empty());
        frame(&egui::Context::default(), Vec::new(), &mut cabin);
        assert!(!cabin.harness.inbox_open, "with nothing waiting the line is not there to open");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn three_mixed_parks_give_three_rows_with_words_and_chat() {
        let (_pin, root) = pinned("inbox-three");
        let mut cabin = loose_cabin();
        cabin.threads.push(crate::threads::ChatThread::new("Tidy downloads", false));
        three_mixed(&mut cabin);
        let rows = cabin.inbox_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.len(), cabin.decisions_waiting());
        let shown: Vec<_> = rows.iter().map(|r| (r.key, r.hard, r.what.as_str(), r.chat.as_str())).collect();
        assert_eq!(
            shown,
            vec![
                (InboxKey::Hard(0), true, "Delete 2 paths: /tmp/a.txt, /tmp/b.txt", "Tidy downloads"),
                (InboxKey::Ask(0), false, "Use the desktop: click 10,20", "Tidy downloads"),
                (InboxKey::Soft(0), false, "Paused and needs you: `click` ran 3 times", "Tidy downloads"),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn approve_and_deny_from_the_inbox_clear_the_card_and_log_its_span() {
        let (_pin, root) = pinned("inbox-answer");
        let mut cabin = loose_cabin();
        three_mixed(&mut cabin);
        let before = spans(&root, "session").len();
        cabin.inbox_answer(InboxKey::Hard(0), false);
        assert!(cabin.harness.park.is_none(), "the hard card cleared too");
        cabin.inbox_answer(InboxKey::Ask(0), true);
        assert!(cabin.perm_ask.is_none(), "the ask card cleared too");
        cabin.inbox_answer(InboxKey::Soft(0), false);
        assert!(cabin.harness.soft_parks.is_empty(), "the pause card cleared too");
        assert!(cabin.inbox_rows().is_empty());
        let logged = spans(&root, "session");
        assert_eq!(
            logged[before..].to_vec(),
            vec![
                row("B", "Run command", "deny", "Jeremy denied", "delete"),
                row("B", "grokhub-desktop__click", "approve", "allowed once", "soft"),
                row("audit", hx::RECOVERY_TOOL, "deny", "Jeremy denied", "soft"),
            ]
        );
        // The card's own answer writes the same span.
        let (_pin2, card_root) = pinned("inbox-answer-card");
        let mut card = loose_cabin();
        three_mixed(&mut card);
        card.resolve_hard_park(false, SOFT_DENY);
        assert_eq!(spans(&card_root, "session").last(), Some(&logged[before]));
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(card_root);
    }

    #[test]
    fn enter_on_a_hard_row_never_approves_and_esc_denies() {
        let (_pin, root) = pinned("inbox-keys");
        let mut cabin = loose_cabin();
        three_mixed(&mut cabin);
        cabin.harness.inbox_open = true;
        let ctx = snapped();
        frame(&ctx, Vec::new(), &mut cabin);
        let (approve, _) = row_button_ids(InboxKey::Hard(0));
        assert!(ctx.read_response(approve).is_some(), "the hard row's Approve is on screen");
        ctx.memory_mut(|m| m.request_focus(approve));
        frame(&ctx, key(egui::Key::Enter), &mut cabin);
        frame(&ctx, key(egui::Key::Space), &mut cabin);
        assert!(cabin.harness.park.is_some(), "Enter or Space on a focused hard Approve must not approve");
        assert!(!spans(&root, "session").iter().any(|s| s.2 == "approve"));
        ctx.memory_mut(|m| m.request_focus(approve));
        frame(&ctx, key(egui::Key::Escape), &mut cabin);
        assert!(cabin.harness.park.is_none(), "Esc on the focused hard row denies");
        assert_eq!(spans(&root, "session").last().map(|s| s.2.as_str()), Some("deny"));
        // A soft row's focused Approve does answer from the keyboard.
        frame(&ctx, Vec::new(), &mut cabin);
        let (soft_approve, _) = row_button_ids(InboxKey::Ask(0));
        ctx.memory_mut(|m| m.request_focus(soft_approve));
        frame(&ctx, key(egui::Key::Enter), &mut cabin);
        assert!(cabin.perm_ask.is_none(), "Enter answers a soft row");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn halt_clears_every_row_with_deny_spans() {
        let (_pin, root) = pinned("inbox-halt");
        let mut cabin = loose_cabin();
        three_mixed(&mut cabin);
        cabin.perm_queue.push_back(ask("grokhub-desktop__type", "type hello"));
        let _ = cabin.harness_precheck(ask("send_email", "to someone"));
        assert_eq!(cabin.inbox_rows().len(), 5);
        let before = spans(&root, "session").len();
        cabin.halt_everything("Halted");
        assert!(cabin.inbox_rows().is_empty());
        assert_eq!(cabin.decisions_waiting(), 0);
        let halt = super::super::harness_ui::HALT_DENY;
        assert_eq!(
            spans(&root, "session")[before..].to_vec(),
            vec![
                row("B", "Run command", "deny", halt, "delete"),
                row("B", "send_email", "deny", halt, "send"),
                row("B", "grokhub-desktop__click", "deny", halt, "soft"),
                row("B", "grokhub-desktop__type", "deny", halt, "soft"),
                row("audit", hx::RECOVERY_TOOL, "deny", halt, "soft"),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_row_jumps_to_its_chat_and_card() {
        let (_pin, root) = pinned("inbox-jump");
        let mut cabin = loose_cabin();
        cabin.threads.push(crate::threads::ChatThread::new("Notes", false));
        cabin.threads.push(crate::threads::ChatThread::new("Cleanup", false));
        let cleanup = cabin.threads[1].id.clone();
        let _ = cabin.harness_precheck(ask("send_email", "to someone"));
        cabin.thread_idx = 1;
        let _ = cabin.harness_precheck(ask("Run command", "rm -f /tmp/old.log"));
        cabin.thread_idx = 0;
        cabin.nav = Nav::Workboard;
        let rows = cabin.inbox_rows();
        assert_eq!((rows[1].chat.as_str(), rows[1].chat_id.as_str()), ("Cleanup", cleanup.as_str()));
        cabin.inbox_jump(InboxKey::Hard(1));
        assert_eq!(cabin.nav, Nav::Chat);
        assert_eq!(cabin.visible_thread_id(), cleanup);
        assert_eq!(cabin.harness.park.as_ref().map(|p| p.action.as_str()), Some("rm -f /tmp/old.log"));
        assert_eq!(cabin.harness.jump, Some("hard"));
        let ctx = snapped();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.paint_harness_cards(ui));
        });
        assert_eq!(cabin.harness.jump, None, "the card scrolled into view once");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workboard_shows_the_same_rows() {
        let (_pin, root) = pinned("inbox-board");
        let mut cabin = loose_cabin();
        three_mixed(&mut cabin);
        cabin.harness.inbox_open = true;
        cabin.nav = Nav::Workboard;
        let ctx = snapped();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.ui_board(ui));
        });
        for key in [InboxKey::Hard(0), InboxKey::Ask(0), InboxKey::Soft(0)] {
            let (approve, deny) = row_button_ids(key);
            assert!(ctx.read_response(approve).is_some() && ctx.read_response(deny).is_some(), "{key:?}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// Spike-8a: a scope ask gets a row, but its Approve only opens the card;
    /// no key or row click grants. Deny leaves the scope off.
    #[test]
    fn a_scope_ask_row_opens_its_card_and_never_grants() {
        let (_pin, root) = pinned("inbox-scope");
        let mut cabin = Cabin::quiet_for_test();
        let ledger = hx::ConsentLedger::load(&root);
        let asks = &mut cabin.harness.indexer.asks;
        assert!(asks.ask(hx::Scope::Apps, "To suggest the right app I'd read your installed apps.", &ledger, now_ms()));
        assert!(asks.ask(hx::Scope::SystemState, "To spot a full disk early I'd read your disk.", &ledger, now_ms()));
        let rows = cabin.inbox_rows();
        assert_eq!(rows.len(), cabin.decisions_waiting());
        let shown: Vec<_> = rows.iter().map(|r| (r.key, r.hard, r.what.as_str())).collect();
        assert_eq!(
            shown,
            vec![
                (InboxKey::Scope(0), false, "Learn from Installed apps: To suggest the right app I'd read your installed apps."),
                (InboxKey::Scope(1), false, "Learn from System state: To spot a full disk early I'd read your disk."),
            ]
        );
        cabin.harness.inbox_open = true;
        let ctx = snapped();
        frame(&ctx, Vec::new(), &mut cabin);
        let (approve, _) = row_button_ids(InboxKey::Scope(1));
        ctx.memory_mut(|m| m.request_focus(approve));
        frame(&ctx, key(egui::Key::Enter), &mut cabin);
        assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0, "a row never grants");
        assert_eq!(cabin.harness.jump, Some("scope"));
        assert_eq!(cabin.harness.indexer.asks.first().map(|a| a.scope.key()).as_deref(), Some("system_state"), "the jumped ask is on the card");
        cabin.inbox_answer(InboxKey::Scope(1), false);
        assert_eq!(cabin.status, "Installed apps stays off.");
        assert_eq!(cabin.inbox_rows().len(), 1);
        assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    /// D2: the inbox adds no Nav variant or page. A new variant fails to
    /// compile here, and the count pins the list.
    #[test]
    fn inbox_adds_no_nav_variant_or_page() {
        fn page(nav: Nav) -> &'static str {
            match nav {
                Nav::Chat => "chat",
                Nav::Devices => "devices",
                Nav::Memory => "memory",
                Nav::Workboard => "workboard",
                Nav::Pulse => "pulse",
                Nav::Imagine => "imagine",
                Nav::Skills => "skills",
                Nav::Night => "night",
                Nav::History => "history",
                Nav::Command => "command",
                Nav::Agents => "agents",
                Nav::Settings => "settings",
            }
        }
        let all = [
            Nav::Chat,
            Nav::Devices,
            Nav::Memory,
            Nav::Workboard,
            Nav::Pulse,
            Nav::Imagine,
            Nav::Skills,
            Nav::Night,
            Nav::History,
            Nav::Command,
            Nav::Agents,
            Nav::Settings,
        ];
        assert_eq!(all.len(), 12);
        assert!(all.iter().all(|n| !page(*n).contains("inbox")));
        let cards: Vec<_> = [InboxKey::Hard(0), InboxKey::Ask(0), InboxKey::Soft(0), InboxKey::Full, InboxKey::Elicit]
            .into_iter()
            .map(InboxKey::card)
            .collect();
        assert_eq!(cards, vec!["hard", "ask", "soft", "full", "elicit"]);
    }

    #[test]
    fn every_delete_parks_hard_under_always_and_full_naming_its_paths() {
        let (_pin, root) = pinned("inbox-delete");
        let mut cabin = loose_cabin();
        for action in [
            "rm -f /tmp/a.txt /tmp/b.txt",
            "gio trash /tmp/old.txt",
            "trash-put /tmp/old.txt",
            r"powershell -c Remove-Item 'C:\Users\me\old.txt'",
            r"powershell -c [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('C:\old.txt','OnlyErrorDialogs','SendToRecycleBin')",
            r"cmd /c del C:\old.txt",
        ] {
            assert_eq!(cabin.harness_precheck(ask("Run command", action)), None, "{action}");
        }
        let words: Vec<_> = cabin.inbox_rows().into_iter().map(|r| (r.hard, r.what)).collect();
        assert_eq!(
            words,
            vec![
                (true, "Delete 2 paths: /tmp/a.txt, /tmp/b.txt".to_string()),
                (true, "Delete 1 path: /tmp/old.txt".to_string()),
                (true, "Delete 1 path: /tmp/old.txt".to_string()),
                (true, r"Delete 1 path: C:\Users\me\old.txt".to_string()),
                (true, words[4].1.clone()),
                (true, r"Delete 1 path: C:\old.txt".to_string()),
            ]
        );
        assert!(words[4].1.starts_with("Delete"), "{}", words[4].1);
        // Copy and open stay soft.
        let cp = ask("Run command", "cp /tmp/a.txt /tmp/c.txt");
        assert_eq!(cabin.harness_precheck(cp.clone()), Some(cp));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn inbox_rows_scrub_keys_tokens_and_held_secrets() {
        let held = vec!["hunter2-pin-0042".to_string()];
        let raw = "curl -H 'Authorization: Bearer abcdefghijklmnopqrstuv' -d sk-abcdefghijklmnopqrstuv ghp_abcdefghijklmnopqrstuvwx hunter2-pin-0042";
        assert_eq!(
            hard_words(HardClass::Send, "type", raw, &held),
            "Send: curl -H 'Authorization: [redacted]' -d [redacted] [redacted] [redacted]"
        );
        let p = ask("Run sk-abcdefghijklmnopqrstuv", raw);
        assert_eq!(
            ask_words(&p, &held),
            "Run [redacted]: curl -H 'Authorization: [redacted]' -d [redacted] [redacted] [redacted]"
        );
    }

    #[test]
    fn hard_words_say_the_action_plainly() {
        assert_eq!(hard_words(HardClass::Delete, "delete_files", "delete 2 paths: /a, /b", &[]), "Delete 2 paths: /a, /b");
        assert_eq!(hard_words(HardClass::Send, "send_email", "to someone", &[]), "Send: to someone");
        assert_eq!(hard_words(HardClass::Money, "buy", "", &[]), "Spend money: buy");
        assert_eq!(hard_words(HardClass::Delete, "key", "press Delete on the files selected in Files (the cabin can't see which)", &[]), "Press Delete on the files selected in Files (the cabin can't see which)");
        let long = "x".repeat(300);
        assert_eq!(hard_words(HardClass::Send, "t", &long, &[]).chars().count(), ROW_CHARS);
    }
}
