//! Spike-0 harness in the cabin.
//!
//! D1: a stricter layer on top of Grok Build. GB still owns its permission
//! prompts (Ask / Auto / Always) and computer use. Before GB executes, the
//! cabin may only tighten: hard floor → Deny, hard class → park a white card
//! (even under Always), desktop tools with the switch off → Deny. Nothing here
//! answers a GB ask more loosely than the pill would.
//! D2: Readonly ↔ Supervised is the Settings switch "Let Grok control the
//! desktop". Full is one inline Work-tree card. No chip, menu, or panel.
//! D3: the same checks run on Windows; the `grokhub-desktop` dispatch shares
//! them (path A lives in `desktop_mcp::run_stdio`).
//!
//! Paths handled here: B (ACP ask), E (native side ask), the cabin half of A
//! (cards for parks the desktop MCP posts), and C (headless denials).

use super::*;
use grokhub_agent::harness::{self as hx, GateOutcome, Step};
use grokhub_agent::{AccessMode, HardClass};

/// How often the cabin looks for desktop parks and refreshes the turn file.
const POLL: Duration = Duration::from_millis(250);

const HARD_NOTE: &str = "Always cannot skip this. Approve runs it once.";
const HEADLESS_NOTE: &str =
    "Grok Build's deny rule stopped this. Approve re-runs this one step with Grok's own Allow.";

/// Who is waiting on a hard card.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ParkSource {
    /// Path B / E: a Grok Build permission ask waiting on its RPC.
    Ask(grokhub_acp::PermissionAsk),
    /// Path A: the `--mcp-desktop` process is blocked on this park id.
    Desk(String),
    /// Path C: GB's `--deny` rule already stopped it. Nothing is waiting.
    Headless,
}

/// One hard-class action on the white card.
#[derive(Debug, Clone)]
pub(super) struct HardParkUi {
    pub source: ParkSource,
    pub class: HardClass,
    pub path: &'static str,
    pub tool: String,
    pub action: String,
    pub parked_at: Instant,
}

/// Path C: a hard-classified call seen in a headless stream.
#[derive(Debug, Clone)]
pub(super) struct HeadlessHit {
    pub card_id: String,
    pub tool: String,
    pub action: String,
    pub hit: GateOutcome,
    pub status: String,
}

/// Path C approve: one ACP Ask turn so GB shows its own Allow for this step.
#[derive(Debug, Clone)]
pub(super) struct OneShot {
    pub action: Option<String>,
    pub restore: PermissionMode,
    pub started: bool,
}

#[derive(Debug, Default)]
pub(super) struct HarnessState {
    /// Session-only Grant full. The Always pill never sets this.
    pub access_full: bool,
    /// The inline Grant full card, with when it was offered (TTL → Deny).
    pub full_card: Option<Instant>,
    pub full_offered: bool,
    pub park: Option<HardParkUi>,
    pub queue: VecDeque<HardParkUi>,
    pub headless: Vec<HeadlessHit>,
    pub oneshot: Option<OneShot>,
    pub last_poll: Option<Instant>,
    pub turn_ctx: Option<hx::TurnContext>,
}

/// Readonly until the desktop switch is on; Full only after Grant full.
pub(super) fn access_for(desktop_control: bool, full: bool) -> AccessMode {
    match (desktop_control, full) {
        (false, _) => AccessMode::Readonly,
        (true, false) => AccessMode::Supervised,
        (true, true) => AccessMode::Full,
    }
}

/// Grok Build computer use reaches the cabin as the `grokhub-desktop` MCP.
pub(super) fn is_desktop_ask(p: &grokhub_acp::PermissionAsk) -> bool {
    let hay = format!("{} {}", p.title, p.action).to_ascii_lowercase();
    hay.contains(grokhub_core::DESKTOP_MCP_SERVER)
}

/// Typed text never lands in a span: a desktop `type` logs its length only.
pub(super) fn span_args(tool: &str, action: &str) -> String {
    let t = tool.to_ascii_lowercase();
    if t == "type" || t.ends_with("__type") {
        format!(r#"{{"chars":{}}}"#, action.chars().count())
    } else {
        action.to_string()
    }
}

fn raw_of(card: &ToolCard) -> serde_json::Value {
    serde_json::from_str(&card.raw_input).unwrap_or(serde_json::Value::Null)
}

fn is_desktop_card(card: &ToolCard) -> bool {
    let hay = format!("{} {}", card.title, card.raw_input).to_ascii_lowercase();
    hay.contains(grokhub_core::DESKTOP_MCP_SERVER)
}

/// Path C classifier for one headless tool card: the shell command when there
/// is one, else the tool name (MCP `server__tool` or `use_tool` target).
pub(super) fn headless_hit(card: &ToolCard) -> (String, String, GateOutcome) {
    let raw = raw_of(card);
    let tool = raw
        .get("name")
        .or_else(|| raw.get("tool"))
        .and_then(|v| v.as_str())
        .unwrap_or(card.title.as_str())
        .to_string();
    if let Some(cmd) = raw.get("command").and_then(|v| v.as_str()) {
        let args = serde_json::json!({ "command": cmd }).to_string();
        let verdict = hx::decide(Step::Tool { name: "run_terminal_command", arguments: &args });
        return (tool, cmd.to_string(), verdict);
    }
    let verdict = hx::decide(Step::Tool { name: &tool, arguments: "{}" });
    (tool.clone(), tool, verdict)
}

fn ran(status: &str) -> bool {
    matches!(status.to_ascii_lowercase().as_str(), "completed" | "done" | "success")
}

impl Cabin {
    pub(super) fn access_mode(&self) -> AccessMode {
        access_for(self.cfg.desktop_control, self.harness.access_full)
    }

    /// Parked hard cards, for the needs-attention line.
    pub(super) fn hard_waiting(&self) -> usize {
        usize::from(self.harness.park.is_some()) + self.harness.queue.len()
    }

    fn trace_id(&self) -> String {
        self.threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "session".into())
    }

    fn turn_no(&self) -> u32 {
        self.messages.iter().filter(|(r, _)| r == "user").count() as u32
    }

    fn write_span(&self, span: hx::Span, path: &str) {
        let trace = self.trace_id();
        let mut span = span.on_path(path).in_turn(&trace, self.turn_no());
        if span.access.is_empty() {
            span.access = self.access_mode().as_str().into();
        }
        let _ = hx::append_span(&crate::config::config_dir(), &span);
    }

    /// Path B: runs before Grok Build's own Ask / Auto / Always answer.
    pub(super) fn harness_precheck(
        &mut self,
        p: grokhub_acp::PermissionAsk,
    ) -> Option<grokhub_acp::PermissionAsk> {
        self.harness_precheck_on(p, "B")
    }

    /// Returns the ask untouched when the harness has nothing stricter to say.
    pub(super) fn harness_precheck_on(
        &mut self,
        p: grokhub_acp::PermissionAsk,
        path: &'static str,
    ) -> Option<grokhub_acp::PermissionAsk> {
        let trace = self.trace_id();
        match hx::decide(Step::Ask { title: &p.title, action: &p.action }) {
            GateOutcome::Refuse { reason } => {
                self.write_span(hx::Span::deny(&trace, &p.title, &span_args(&p.title, &p.action), &reason, "floor"), path);
                if let Some(h) = &self.acp {
                    let _ = h.reject_permission(&p);
                }
                self.status = format!("Denied: {reason}");
                None
            }
            GateOutcome::Park { hard: Some(class), .. } => {
                if self.take_oneshot(&p.action) {
                    // Approved once on the path C card: Grok's own Allow card decides.
                    return Some(p);
                }
                self.write_span(hx::Span::hard_park(&trace, &p.title, &span_args(&p.title, &p.action), class), path);
                let (tool, action) = (p.title.clone(), p.action.clone());
                self.park_hard(ParkSource::Ask(p), class, path, tool, action);
                None
            }
            _ if is_desktop_ask(&p) && !self.access_mode().allows_computer() => {
                let why = "Access is Readonly: turn on Let Grok control the desktop first";
                self.write_span(hx::Span::deny(&trace, &p.title, &span_args(&p.title, &p.action), why, "soft"), path);
                if let Some(h) = &self.acp {
                    let _ = h.reject_permission(&p);
                }
                self.status = why.into();
                None
            }
            _ => {
                if is_desktop_ask(&p) {
                    self.offer_full();
                }
                Some(p)
            }
        }
    }

    fn take_oneshot(&mut self, action: &str) -> bool {
        let Some(shot) = self.harness.oneshot.as_mut() else {
            return false;
        };
        let hit = shot
            .action
            .as_deref()
            .is_some_and(|a| a.trim() == action.trim() || action.contains(a.trim()));
        if hit {
            shot.action = None;
        }
        hit
    }

    fn park_hard(
        &mut self,
        source: ParkSource,
        class: HardClass,
        path: &'static str,
        tool: String,
        action: String,
    ) {
        let park = HardParkUi {
            source,
            class,
            path,
            tool,
            action,
            parked_at: Instant::now(),
        };
        if self.harness.park.is_some() {
            self.harness.queue.push_back(park);
        } else {
            self.harness.park = Some(park);
        }
        if self.chrome_here() {
            self.status = crate::motion::needs_attention_summary(self.hard_waiting());
        }
    }

    /// Approve answers once (never Always). Deny, Esc, TTL, and halt reject.
    pub(super) fn resolve_hard_park(&mut self, approve: bool, why: &str) {
        let Some(park) = self.harness.park.take() else {
            return;
        };
        let trace = self.trace_id();
        let span = if approve {
            hx::Span::hard_approve(&trace, &park.tool, &span_args(&park.tool, &park.action), park.class)
        } else {
            hx::Span::deny(&trace, &park.tool, &span_args(&park.tool, &park.action), why, park.class.as_str())
        };
        self.write_span(span, park.path);
        match &park.source {
            ParkSource::Ask(p) => {
                if let Some(h) = &self.acp {
                    if approve {
                        let _ = h.answer_permission(p.rpc_id.clone(), true);
                    } else {
                        let _ = h.reject_permission(p);
                    }
                }
            }
            ParkSource::Desk(id) => {
                let _ = hx::answer_park(&crate::config::config_dir(), id, approve);
            }
            ParkSource::Headless => {
                if approve {
                    self.start_oneshot(&park.action);
                }
            }
        }
        self.status = if approve {
            format!("Approved once · {}", park.class.label())
        } else {
            format!("Denied · {}", park.class.label())
        };
        self.harness.park = self.harness.queue.pop_front();
    }

    /// A turn is stopping: parks that hold a GB ask or a desktop call fail closed.
    /// Path C cards hold nothing and stay until clicked, TTL, or halt.
    pub(super) fn withdraw_hard_parks(&mut self) {
        let mut keep = VecDeque::new();
        while let Some(park) = self.harness.park.clone() {
            if park.source == ParkSource::Headless {
                keep.push_back(park);
                self.harness.park = self.harness.queue.pop_front();
            } else {
                self.resolve_hard_park(false, "turn stopped — fail-closed Deny");
            }
        }
        self.harness.park = keep.pop_front();
        self.harness.queue = keep;
    }

    /// Halt denies every parked card with a span.
    pub(super) fn halt_hard_parks(&mut self) {
        while self.harness.park.is_some() {
            self.resolve_hard_park(false, "halted — fail-closed Deny");
        }
        if self.harness.full_card.is_some() {
            self.resolve_grant_full(false, "halted — stays Supervised");
        }
    }

    /// Path C: remember hard-classified calls from a headless stream.
    pub(super) fn harness_note_headless(&mut self, card: &ToolCard) {
        if is_desktop_card(card) {
            self.offer_full();
        }
        if let Some(h) = self.harness.headless.iter_mut().find(|h| h.card_id == card.id) {
            if !card.status.is_empty() {
                h.status = card.status.clone();
            }
            return;
        }
        let (tool, action, hit) = headless_hit(card);
        if !hit.is_allow() {
            self.harness.headless.push(HeadlessHit {
                card_id: card.id.clone(),
                tool,
                action,
                hit,
                status: card.status.clone(),
            });
        }
    }

    /// Path C at turn end: a hard call GB denied parks a card; one that ran is
    /// logged as an allow with no approve, so `approval_gate_violation` flags it.
    pub(super) fn harness_headless_end(&mut self) {
        let trace = self.trace_id();
        for h in std::mem::take(&mut self.harness.headless) {
            let done = ran(&h.status);
            match (&h.hit, done) {
                (GateOutcome::Refuse { reason }, false) => {
                    self.write_span(hx::Span::deny(&trace, &h.tool, &h.action, reason, "floor"), "C");
                }
                (GateOutcome::Park { hard: Some(class), .. }, false) => {
                    self.write_span(hx::Span::hard_park(&trace, &h.tool, &h.action, *class), "C");
                    self.park_hard(ParkSource::Headless, *class, "C", h.tool, h.action);
                }
                (GateOutcome::Refuse { .. } | GateOutcome::Park { hard: Some(_), .. }, true) => {
                    let class = match &h.hit {
                        GateOutcome::Park { hard: Some(c), .. } => *c,
                        _ => HardClass::IrreversibleOs,
                    };
                    let mut s = hx::Span::soft_allow(
                        &trace,
                        &h.tool,
                        &h.action,
                        "ran",
                        "headless run with no deny rule match",
                        self.access_mode(),
                        "grok_build",
                    );
                    s.approval_class = class.as_str().into();
                    self.write_span(s, "C");
                }
                _ => {}
            }
        }
    }

    /// Approve on a path C card: one ACP Ask turn for that step, then the pill
    /// goes back. Flags are never loosened; Grok's own Allow card decides.
    fn start_oneshot(&mut self, action: &str) {
        self.harness.oneshot = Some(OneShot {
            action: Some(action.to_string()),
            restore: self.permission_mode,
            started: false,
        });
        self.permission_mode = PermissionMode::Ask;
        self.send_chat(format!(
            "I approved one step that was blocked. Run only this, once, then stop: {action}"
        ));
    }

    fn offer_full(&mut self) {
        if self.access_mode() == AccessMode::Supervised
            && !self.harness.full_offered
            && self.harness.full_card.is_none()
        {
            self.harness.full_card = Some(Instant::now());
            self.harness.full_offered = true;
        }
    }

    pub(super) fn resolve_grant_full(&mut self, grant: bool, why: &str) {
        self.harness.full_card = None;
        let trace = self.trace_id();
        let mut span = if grant {
            self.harness.access_full = true;
            self.status = "Full for this session. Hard actions still ask.".into();
            let mut s = hx::Span::soft_allow(
                &trace,
                "grant_full",
                "{}",
                "granted",
                "Jeremy granted Full for this session",
                AccessMode::Full,
                "none",
            );
            s.decision = "approve".into();
            s
        } else {
            hx::Span::deny(&trace, "grant_full", "{}", why, "access")
        };
        span.approval_class = "access".into();
        self.write_span(span, "B");
    }

    /// Desktop parks from the MCP process, the shared turn file, and the
    /// one-shot pill restore. Throttled to [`POLL`].
    fn poll_harness(&mut self) {
        let now = Instant::now();
        if self.harness.last_poll.is_some_and(|t| now.duration_since(t) < POLL) {
            return;
        }
        self.harness.last_poll = Some(now);
        let dir = crate::config::config_dir();
        let ctx = hx::TurnContext {
            chat_id: self.trace_id(),
            turn: self.turn_no(),
            access: self.access_mode().as_str().into(),
        };
        if self.harness.turn_ctx.as_ref() != Some(&ctx) {
            let _ = hx::write_turn_context(&dir, &ctx);
            self.harness.turn_ctx = Some(ctx);
        }
        for req in hx::pending_parks(&dir) {
            let known = self
                .harness
                .park
                .iter()
                .chain(self.harness.queue.iter())
                .any(|p| p.source == ParkSource::Desk(req.id.clone()));
            if known {
                continue;
            }
            let class = HardClass::parse(&req.class).unwrap_or(HardClass::IrreversibleOs);
            self.park_hard(ParkSource::Desk(req.id), class, "A", req.tool, req.action);
        }
    }

    /// Work tree cards: hard-class park and Grant full. Same white-accent family.
    pub(super) fn paint_harness_cards(&mut self, ui: &mut egui::Ui) {
        self.poll_harness();
        if self.running && self.cfg.desktop_control {
            ui.ctx().request_repaint_after(POLL);
        }
        if !self.cfg.desktop_control {
            self.harness.access_full = false;
            self.harness.full_card = None;
        }
        if let Some(shot) = self.harness.oneshot.as_mut() {
            if self.running {
                shot.started = true;
            } else if shot.started {
                self.permission_mode = shot.restore;
                self.harness.oneshot = None;
            }
        }
        if self
            .harness
            .park
            .as_ref()
            .is_some_and(|p| p.parked_at.elapsed() >= hx::APPROVAL_TTL)
        {
            self.resolve_hard_park(false, "timed out — fail-closed Deny");
        }
        if self
            .harness
            .full_card
            .is_some_and(|t| t.elapsed() >= hx::APPROVAL_TTL)
        {
            self.resolve_grant_full(false, "timed out — stays Supervised");
        }
        let running = self.running;
        if let Some(park) = self.harness.park.clone() {
            let waiting = self.hard_waiting() + usize::from(self.perm_ask.is_some()) + self.perm_queue.len();
            let eyebrow = crate::motion::needs_attention_summary(waiting);
            let title = format!("Hard action · {}", park.class.label());
            let action = if park.action.trim().is_empty() {
                park.tool.clone()
            } else {
                park.action.clone()
            };
            let note = if park.source == ParkSource::Headless {
                HEADLESS_NOTE
            } else {
                HARD_NOTE
            };
            let overlay = self.palette_open || self.nav == Nav::Settings || self.find.focused;
            let key = hx::hard_card_key(
                super::chat_ui::bare_press(ui, egui::Key::Enter),
                super::chat_ui::bare_press(ui, egui::Key::Escape),
                overlay || super::chat_ui::overlay_over_chat(ui.ctx()),
            );
            let id = ("hard-park", format!("{}:{}", park.path, park.action));
            let text = CardText {
                eyebrow: &eyebrow,
                title: &title,
                action: &action,
                note,
                primary: "Approve",
                secondary: "Deny",
            };
            if key == Some(hx::HardAnswer::Deny) {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                self.resolve_hard_park(false, "Jeremy denied (Esc)");
            } else {
                match harness_card(ui, id, text, running) {
                    Some(true) => self.resolve_hard_park(true, ""),
                    Some(false) => self.resolve_hard_park(false, "Jeremy denied"),
                    None => {}
                }
            }
        }
        if self.harness.full_card.is_some() {
            let text = CardText {
                eyebrow: "Let Grok control the desktop is on",
                title: "Grant full access for this session?",
                action: "Desktop tools keep running under your permission pill.",
                note: "Hard actions (send, delete, money, credentials) still ask. Always does not grant this.",
                primary: "Grant full",
                secondary: "Not now",
            };
            if let Some(grant) = harness_card(ui, ("grant-full", String::new()), text, running) {
                self.resolve_grant_full(grant, "Jeremy kept Supervised");
            }
        }
    }
}

/// White accents on the dark cabin: #E7E9EA ring and title on the surface
/// fill, the approval enter motion, the thinking rim while a reply runs. No
/// Thinking label, no pulse at rest.
struct CardText<'a> {
    eyebrow: &'a str,
    title: &'a str,
    action: &'a str,
    note: &'a str,
    primary: &'a str,
    secondary: &'a str,
}

/// Some(true) primary, Some(false) secondary. Click only: no Enter.
fn harness_card(
    ui: &mut egui::Ui,
    id: (&str, String),
    text: CardText<'_>,
    running: bool,
) -> Option<bool> {
    let CardText {
        eyebrow,
        title,
        action,
        note,
        primary,
        secondary,
    } = text;
    ui.add_space(8.0);
    let card_id = egui::Id::new(("harness-card", id.0, id.1));
    let enter_t = crate::motion::approval_enter_t(ui, card_id, true);
    let y = crate::motion::approval_y(enter_t, false);
    let avail = ui.available_rect_before_wrap();
    let slot = avail.translate(egui::vec2(0.0, y));
    let mut hit = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
        ui.set_min_width(avail.width());
        ui.multiply_opacity(enter_t.clamp(0.0, 1.0));
        let framed = egui::Frame::NONE
            .fill(crate::theme::surface())
            .corner_radius(crate::theme::CHROME_RADIUS)
            .stroke(egui::Stroke::new(1.5_f32, crate::theme::fg()))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label(RichText::new(eyebrow).size(12.0).color(crate::theme::muted()));
                ui.add_space(4.0);
                ui.label(RichText::new(title).size(14.0).strong().color(crate::theme::fg()));
                if !action.trim().is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(action).size(13.0).monospace().color(crate::theme::fg()));
                }
                ui.add_space(4.0);
                ui.label(RichText::new(note).size(12.0).color(crate::theme::muted()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, primary) {
                        hit = Some(true);
                    } else if crate::cards::ghost_pill(ui, secondary) {
                        hit = Some(false);
                    }
                });
            });
        let time = ui.ctx().input(|i| i.time) as f32;
        crate::motion::paint_thinking_rim(ui.painter(), framed.response.rect, running, time);
    });
    hit
}

/// Remember where a Work-tree frame painted, for the click marker.
pub(super) fn remember_work_frame(ui: &egui::Ui, card_id: &str, rect: egui::Rect, size: [usize; 2]) {
    let id = egui::Id::new(("work-frame", card_id));
    ui.ctx().data_mut(|d| d.insert_temp(id, (rect, size)));
}

/// A finished desktop click with its screenshot-space point.
pub(super) fn click_point(card: &ToolCard) -> Option<(f32, f32)> {
    if !ran(&card.status) {
        return None;
    }
    let hay = format!("{} {}", card.title, card.raw_input).to_ascii_lowercase();
    if !hay.contains("click") {
        return None;
    }
    let raw = raw_of(card);
    let x = raw.get("x").and_then(|v| v.as_f64())?;
    let y = raw.get("y").and_then(|v| v.as_f64())?;
    Some((x as f32, y as f32))
}

/// Screenshot pixel → point on the painted frame, kept inside it.
pub(super) fn marker_pos(rect: egui::Rect, size: [usize; 2], x: f32, y: f32) -> egui::Pos2 {
    let w = size[0].max(1) as f32;
    let h = size[1].max(1) as f32;
    let p = rect.min + egui::vec2(x / w * rect.width(), y / h * rect.height());
    rect.clamp(p)
}

/// Travel 220ms, hover settle, 0.92 press, then rest at the point. Reduced motion snaps.
pub(super) fn marker_sample(
    from: egui::Pos2,
    to: egui::Pos2,
    t0: f64,
    now: f64,
    reduced: bool,
) -> (egui::Pos2, f32, bool) {
    use crate::motion::{AgentCursorAnim, AgentCursorPhase, CURSOR_CLICK_SECS, CURSOR_TRAVEL_SECS};
    const HOVER: f32 = 0.200;
    if reduced {
        return (to, 1.0, false);
    }
    let el = (now - t0) as f32;
    let travel = CURSOR_TRAVEL_SECS;
    let (phase, start, dur) = if el < travel {
        (AgentCursorPhase::Travel, t0, travel)
    } else if el < travel + HOVER {
        (AgentCursorPhase::HoverSettle, t0 + f64::from(travel), HOVER)
    } else if el < travel + HOVER + CURSOR_CLICK_SECS {
        (AgentCursorPhase::ClickPress, t0 + f64::from(travel + HOVER), CURSOR_CLICK_SECS)
    } else {
        return (to, 1.0, false);
    };
    let anim = AgentCursorAnim {
        from,
        to,
        t0: start,
        duration: dur,
        phase,
    };
    let (pos, scale, _) = crate::motion::cursor_sample(&anim, now, false);
    (pos, scale, true)
}

/// Agent cursor marker on the latest Work-tree frame at the last approved click.
pub(super) fn paint_click_marker(ui: &egui::Ui, cards: &[ToolCard]) {
    let Some(frame_card) = cards.iter().rev().find(|c| c.image_data_url.is_some()) else {
        return;
    };
    let frame_id = egui::Id::new(("work-frame", frame_card.id.as_str()));
    let Some((rect, size)) = ui.ctx().data(|d| d.get_temp::<(egui::Rect, [usize; 2])>(frame_id))
    else {
        return;
    };
    let Some((click_card, (x, y))) = cards
        .iter()
        .rev()
        .find_map(|c| click_point(c).map(|p| (c, p)))
    else {
        return;
    };
    let to = marker_pos(rect, size, x, y);
    let now = ui.ctx().input(|i| i.time);
    let t_id = egui::Id::new(("work-click", click_card.id.as_str()));
    let t0 = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(t_id, || now));
    let reduced = crate::motion::reduced_motion(ui);
    let (pos, scale, live) = marker_sample(rect.center(), to, t0, now, reduced);
    crate::motion::paint_agent_cursor(ui.painter(), pos, scale);
    if live {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn card(id: &str, status: &str, raw: &str) -> ToolCard {
        ToolCard {
            id: id.into(),
            title: "Tool".into(),
            kind: String::new(),
            status: status.into(),
            detail: String::new(),
            diff: String::new(),
            image_data_url: None,
            raw_input: raw.into(),
        }
    }

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::create_dir_all(&root);
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    #[test]
    fn access_follows_desktop_switch_and_grant() {
        assert_eq!(access_for(false, false), AccessMode::Readonly);
        assert_eq!(access_for(false, true), AccessMode::Readonly);
        assert_eq!(access_for(true, false), AccessMode::Supervised);
        assert_eq!(access_for(true, true), AccessMode::Full);
    }

    #[test]
    fn desktop_asks_are_the_grokhub_desktop_mcp() {
        assert!(is_desktop_ask(&ask("grokhub-desktop__click", "click 10,20")));
        assert!(!is_desktop_ask(&ask("Run command", "ls")));
    }

    #[test]
    fn readonly_boot_and_always_is_not_full() {
        let (_pin, root) = pinned("harness-always");
        let mut cabin = Cabin::quiet_for_test();
        assert_eq!(cabin.access_mode(), AccessMode::Readonly);
        cabin.set_permission_mode(PermissionMode::AlwaysApprove);
        assert_eq!(cabin.access_mode(), AccessMode::Readonly);
        assert!(!cabin.cfg.desktop_control, "Always must not turn on desktop control");
        cabin.cfg.desktop_control = true;
        assert_eq!(cabin.access_mode(), AccessMode::Supervised);
        assert!(!cabin.harness.access_full, "Always must not grant Full");
        let soft = ask("grokhub-desktop__click", "click 1,2");
        assert_eq!(cabin.harness_precheck(soft.clone()), Some(soft));
        assert!(cabin.harness.full_card.is_some(), "first desktop act offers the inline card");
        assert_eq!(cabin.access_mode(), AccessMode::Supervised);
        cabin.resolve_grant_full(true, "");
        assert_eq!(cabin.access_mode(), AccessMode::Full);
        let spans = hx::read_spans(&root, "session").unwrap();
        assert_eq!(spans.last().unwrap().approval_class, "access");
        assert_eq!(spans.last().unwrap().decision, "approve");
        cabin.cfg.desktop_control = false;
        assert_eq!(cabin.access_mode(), AccessMode::Readonly);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hard_class_parks_under_always_and_deny_writes_span() {
        let (_pin, root) = pinned("harness-park");
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        cabin.cfg.desktop_control = true;
        cabin.harness.access_full = true;
        let out = cabin.harness_precheck(ask("Run command", "rm -f draft.md"));
        assert_eq!(out, None, "hard class must not reach the Always auto-answer");
        let park = cabin.harness.park.clone().expect("parked");
        assert_eq!(park.class, HardClass::Delete);
        assert_eq!(park.path, "B");
        assert_eq!(cabin.perm_ask, None);
        assert_eq!(cabin.hard_waiting(), 1);
        cabin.resolve_hard_park(false, "Jeremy denied");
        assert!(cabin.harness.park.is_none());
        let spans = hx::read_spans(&root, "session").unwrap();
        let kinds: Vec<_> = spans.iter().map(|s| s.decision.as_str()).collect();
        assert_eq!(kinds, vec!["park", "deny"]);
        assert_eq!(spans[1].approval_class, "delete");
        assert_eq!(spans[1].path, "B");
        assert!(hx::approval_gate_violation(&spans).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hard_park_ttl_fails_closed() {
        let (_pin, root) = pinned("harness-ttl");
        let mut cabin = Cabin::quiet_for_test();
        let _ = cabin.harness_precheck(ask("send_email", "to someone"));
        if let Some(p) = cabin.harness.park.as_mut() {
            p.parked_at = Instant::now() - hx::APPROVAL_TTL - Duration::from_secs(1);
        }
        let ctx = egui::Context::default();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.paint_harness_cards(ui));
        });
        assert!(cabin.harness.park.is_none());
        let spans = hx::read_spans(&root, "session").unwrap();
        assert_eq!(spans.last().unwrap().decision, "deny");
        assert_eq!(spans.last().unwrap().result, "timed out — fail-closed Deny");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn halt_denies_every_park_and_the_full_offer_with_spans() {
        let (_pin, root) = pinned("harness-halt");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        let _ = cabin.harness_precheck(ask("send_email", "to someone"));
        let _ = cabin.harness_precheck(ask("bash", "rm -rf ~/old"));
        cabin.offer_full();
        assert_eq!(cabin.hard_waiting(), 2);
        assert!(cabin.harness.full_card.is_some());
        cabin.halt_hard_parks();
        assert_eq!(cabin.hard_waiting(), 0);
        assert!(cabin.harness.full_card.is_none());
        let spans = hx::read_spans(&root, "session").unwrap();
        let tail: Vec<_> = spans.iter().rev().take(3).map(|s| (s.decision.as_str(), s.result.as_str())).collect();
        assert_eq!(
            tail,
            vec![
                ("deny", "halted — stays Supervised"),
                ("deny", "halted — fail-closed Deny"),
                ("deny", "halted — fail-closed Deny"),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn floor_and_readonly_desktop_deny_with_no_card() {
        let (_pin, root) = pinned("harness-floor");
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        assert_eq!(cabin.harness_precheck(ask("Run command", "cat ~/.ssh/id_rsa")), None);
        assert!(cabin.harness.park.is_none());
        assert_eq!(cabin.status, "Denied: forbidden path: ssh keys");
        assert_eq!(cabin.harness_precheck(ask("grokhub-desktop__click", "click 1,2")), None);
        assert_eq!(cabin.status, "Access is Readonly: turn on Let Grok control the desktop first");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn coding_tools_are_not_access_gated() {
        let (_pin, root) = pinned("harness-coding");
        let mut cabin = Cabin::quiet_for_test();
        let edit = ask("Edit `src/main.rs`", "src/main.rs");
        assert_eq!(cabin.harness_precheck(edit.clone()), Some(edit));
        let run = ask("Run command", "cargo test");
        assert_eq!(cabin.harness_precheck(run.clone()), Some(run));
        assert!(cabin.harness.park.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn desk_parks_from_the_mcp_show_and_answer_the_file() {
        let (_pin, root) = pinned("harness-desk");
        let mut cabin = Cabin::quiet_for_test();
        let req = hx::ParkRequest {
            id: "d1".into(),
            path: "A".into(),
            tool: "type".into(),
            action: "rm -f disposable.txt".into(),
            class: "delete".into(),
            ts_ms: 1,
        };
        hx::post_park(&root, &req).unwrap();
        cabin.poll_harness();
        let park = cabin.harness.park.clone().expect("desk park shown");
        assert_eq!(park.source, ParkSource::Desk("d1".into()));
        assert_eq!(park.path, "A");
        cabin.harness.last_poll = None;
        cabin.poll_harness();
        assert_eq!(cabin.hard_waiting(), 1, "one park per request");
        cabin.resolve_hard_park(true, "");
        assert_eq!(hx::take_answer(&root, "d1"), Some(true));
        let spans = hx::read_spans(&root, "session").unwrap();
        let logged: Vec<_> = spans.iter().map(|s| (s.decision.as_str(), s.args_redacted.as_str())).collect();
        assert_eq!(logged, vec![("approve", r#"{"chars":20}"#)], "typed text stays out of spans");
        assert_eq!(span_args("grokhub-desktop__type", "hunter2"), r#"{"chars":7}"#);
        assert_eq!(span_args("run_terminal_command", "rm -f x"), "rm -f x");
        let turn = hx::read_turn_context(&root);
        assert_eq!(turn.chat_id, "session");
        assert_eq!(turn.access, "readonly");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn headless_denial_parks_and_a_run_is_flagged() {
        let (_pin, root) = pinned("harness-headless");
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        let denied = card("c1", "pending", r#"{"tool":"run_terminal_command","command":"rm -f disposable.txt"}"#);
        cabin.harness_note_headless(&denied);
        let ran_card = card("c2", "pending", r#"{"tool":"run_terminal_command","command":"shred notes.txt"}"#);
        cabin.harness_note_headless(&ran_card);
        cabin.harness_note_headless(&card("c2", "completed", ""));
        cabin.harness_note_headless(&card("c3", "completed", r#"{"tool":"run_terminal_command","command":"ls"}"#));
        cabin.harness_headless_end();
        let park = cabin.harness.park.clone().expect("path C park");
        assert_eq!(park.source, ParkSource::Headless);
        assert_eq!(park.action, "rm -f disposable.txt");
        assert_eq!(park.path, "C");
        cabin.withdraw_hard_parks();
        assert!(cabin.harness.park.is_some(), "a turn stop leaves path C cards");
        cabin.halt_hard_parks();
        assert!(cabin.harness.park.is_none());
        let spans = hx::read_spans(&root, "session").unwrap();
        let kinds: Vec<_> = spans.iter().map(|s| (s.path.as_str(), s.decision.as_str())).collect();
        assert_eq!(kinds, vec![("C", "park"), ("C", "allow"), ("C", "deny")]);
        assert_eq!(hx::approval_gate_violation(&spans).len(), 1, "the unapproved run is flagged");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn click_marker_maps_screenshot_pixels_onto_the_frame() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(360.0, 225.0));
        assert_eq!(marker_pos(rect, [1920, 1200], 960.0, 600.0), egui::pos2(280.0, 162.5));
        assert_eq!(marker_pos(rect, [1920, 1200], 5000.0, -10.0), egui::pos2(460.0, 50.0));
        let to = egui::pos2(280.0, 162.5);
        assert_eq!(marker_sample(rect.center(), to, 0.0, 0.0, true), (to, 1.0, false));
        assert_eq!(marker_sample(rect.center(), to, 0.0, 2.0, false), (to, 1.0, false));
        let (_, _, live) = marker_sample(rect.center(), to, 0.0, 0.1, false);
        assert!(live);
        let done = card("k1", "completed", r#"{"tool":"grokhub-desktop__click","x":960,"y":600}"#);
        assert_eq!(click_point(&done), Some((960.0, 600.0)));
        let pending = card("k2", "pending", r#"{"tool":"grokhub-desktop__click","x":1,"y":2}"#);
        assert_eq!(click_point(&pending), None);
    }
}
