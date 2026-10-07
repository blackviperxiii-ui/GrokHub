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
//! (cards for parks the desktop MCP posts), C (headless denials), and D
//! (Spike-1c: a watchdog on Grok Build's own computer-use frames that no ask
//! covered; the `--deny` rules on every spawn are the lock, this is the
//! belt). Spike-4a
//! adds the `egress` path: a cabin-owned send the EgressGuard parked as hard
//! Send (today only `/sync`; see `privacy_ui.rs`).

use super::*;
use grokhub_agent::harness::{self as hx, GateOutcome, Step};
use grokhub_agent::{AccessMode, HardClass};

/// How often the cabin looks for desktop parks and refreshes the turn file.
const POLL: Duration = Duration::from_millis(250);

const HARD_NOTE: &str = "Always can't skip this. Approve runs it once. Esc denies.";
const HEADLESS_NOTE: &str =
    "Grok Build's deny rule stopped this. Approve re-runs this one step with Grok's own Allow. Esc denies.";
/// Path D: Grok Build's own computer use ran a hard step with no ask.
const UNASKED_NOTE: &str =
    "Grok Build did this without asking, so the turn was stopped. Approve re-runs this one step with Grok's own Allow. Esc denies.";
const HARD_EYEBROW: &str = "Hard action";
/// Why a ladder pause or an ask was denied from a card or the inbox.
pub(super) const SOFT_DENY: &str = "Jeremy denied";
pub(super) const HALT_DENY: &str = "halted — fail-closed Deny";
const SOFT_EYEBROW: &str = "Paused";
const SOFT_NOTE: &str = "Grok stopped after a step didn't work. Approve lets it try again; Deny stops it.";
/// What a path B credential card and its spans say instead of GB's action line.
const CREDENTIAL_ACTION: &str = "type into a credential field (value hidden)";
/// The span tool and args for a `/sync` publish (no content).
const HUB_SYNC_TOOL: &str = "hub_sync";
const HUB_SYNC_ARGS: &str = r#"{"dest":"hub","data":["chat","personal"]}"#;
const FULL_EYEBROW: &str = "Session access · Desktop control is on";
const FULL_TITLE: &str = "Let Grok click and type without asking for this session?";
const FULL_NOTE: &str = "Deletes, sends, money and credentials still ask.";

/// One outer width for every approval card. Short content does not hug; long commands wrap.
pub(super) const APPROVAL_CARD_MAX_W: f32 = 520.0;
const APPROVAL_CARD_MARGIN_PX: i8 = 12;
const APPROVAL_CARD_MARGIN: f32 = APPROVAL_CARD_MARGIN_PX as f32;

pub(super) fn approval_card_width(column: f32) -> f32 {
    column.min(APPROVAL_CARD_MAX_W)
}

/// Frame inner width so the painted outer edge is [`approval_card_width`].
pub(super) fn approval_card_inner(column: f32, stroke_w: f32) -> f32 {
    (approval_card_width(column) - 2.0 * APPROVAL_CARD_MARGIN - 2.0 * stroke_w).max(0.0)
}

pub(super) use crate::cards::PillStyle as PillKind;

#[derive(Clone, Copy, Debug, PartialEq)]
struct CardStyle {
    stroke_w: f32,
    stroke_color: egui::Color32,
    primary: PillKind,
}

/// Hard Approve is the danger pill with a 2px foreground stroke.
/// Grant full stays the white pill on a 1px border, same family as a permission card.
fn card_style(hard: bool) -> CardStyle {
    if hard {
        CardStyle {
            stroke_w: 2.0,
            stroke_color: crate::theme::fg(),
            primary: PillKind::Danger,
        }
    } else {
        CardStyle {
            stroke_w: 1.0,
            stroke_color: crate::theme::border(),
            primary: PillKind::Solid,
        }
    }
}

/// `GROKHUB_GRANT_FULL=1` shows the Grant full card. `Cabin::new` reads this.
/// `quiet_for_test` does not, so a test that wants the card sets `full_card_on`.
pub(super) fn grant_full_card_on() -> bool {
    std::env::var("GROKHUB_GRANT_FULL").ok().as_deref() == Some("1")
}

/// Who is waiting on a hard card.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ParkSource {
    /// Path B / E: a Grok Build permission ask waiting on its RPC.
    Ask(grokhub_acp::PermissionAsk),
    /// Path A: the `--mcp-desktop` process is blocked on this park id.
    Desk(String),
    /// Path C: GB's `--deny` rule already stopped it. Nothing is waiting.
    Headless,
    /// A cabin-owned send to this destination (`hub`) the EgressGuard parked.
    /// Nothing left; Approve sends it once.
    Egress(String),
    /// A path A / B park whose turn was steered (Spike-1a). Its call was
    /// denied with the old turn; Approve re-runs the step once, like path C.
    Held,
    /// Path D (Spike-1c): Grok Build's own computer use tried a hard step
    /// with no ask, and the cabin stopped the turn. Nothing is waiting;
    /// Approve re-runs the step once, like path C.
    Unasked,
}

/// A path D frame the watchdog already checked (Spike-1c).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Watched {
    /// A pending frame on a live ACP session: Grok Build may still ask.
    Waiting { tool: String, args: serde_json::Value },
    /// Allowed; its span is written once the step finishes (or at turn end).
    Allowed { tool: String, args: String },
    Done,
}

/// A recovery-ladder pause waiting on the user (Spike-1a). Counts in the
/// needs-attention line; the user's next message (not a Steer or a queued
/// one) answers it.
#[derive(Debug, Clone)]
pub(super) struct SoftPark {
    pub detector: String,
    pub reason: String,
    pub evidence: Vec<String>,
    /// The chat whose turn paused (Spike-1b inbox).
    pub chat_id: String,
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
    /// The chat that was visible when it parked (Spike-1b inbox).
    pub chat_id: String,
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
    /// A path D card's tool and class. Its re-run comes back as a path B ask
    /// whose action is Grok's own words, so the ask matches by tool name.
    pub step: Option<(String, HardClass)>,
    pub restore: PermissionMode,
    pub started: bool,
}

#[derive(Debug, Default)]
pub(super) struct HarnessState {
    /// Session-only Grant full. The Always pill never sets this.
    pub access_full: bool,
    /// Show the inline Grant full card. Off unless `GROKHUB_GRANT_FULL=1`.
    ///
    /// In this build `AccessMode::Full` is only recorded on spans and `turn.json`.
    /// No gate reads `is_full`, `AccessMode::Full`, or `access_full` to skip an ask:
    /// `decide` ignores access, `decide_harness` treats Full like Supervised
    /// (`allows_computer` is true for both, and a hard class still parks), and the
    /// desktop `precheck` calls `decide` only. `access_now` copies the turn-file
    /// string onto the span.
    pub full_card_on: bool,
    /// The inline Grant full card, with when it was offered (TTL → Deny).
    pub full_card: Option<Instant>,
    pub full_offered: bool,
    pub park: Option<HardParkUi>,
    pub queue: VecDeque<HardParkUi>,
    pub headless: Vec<HeadlessHit>,
    /// Tool call ids a path B / E ask covered this turn: the cabin decided
    /// those, so the path D watchdog leaves their frames alone (Spike-1c).
    pub asked: HashSet<String>,
    /// Path D: Grok Build computer-use frames already checked this turn.
    pub watched: HashMap<String, Watched>,
    /// Hash of the last computer-use frame image, for `ui_changed`.
    pub last_cu_frame: Option<u64>,
    pub oneshot: Option<OneShot>,
    pub last_poll: Option<Instant>,
    pub turn_ctx: Option<hx::TurnContext>,
    /// The consent ledger as last read. `None` means read it again.
    pub consent: Option<hx::ConsentLedger>,
    /// When `consent` was read. A locked ledger is read again at most once
    /// per `LOCK_RECHECK`, not on every painted frame.
    pub consent_at: Option<Instant>,
    /// The last lock answer Settings painted, and when it was asked.
    pub lock_seen: Option<(Instant, Option<hx::Locked>)>,
    /// `/privacy` output on its way from the reader thread.
    pub privacy_rx: Option<mpsc::Receiver<String>>,
    /// Settings → Permissions: the folder typed for a new files scope.
    pub scope_folder: String,
    /// Settings → Permissions: the browser picked for a history scope.
    pub scope_browser: usize,
    /// Why the last scope Allow was refused, shown under the heading.
    pub scope_note: Option<String>,
    /// The native folder dialog's answer on its way (SB-04).
    pub scope_pick_rx: Option<mpsc::Receiver<super::scope_ui::FolderPick>>,
    /// When Try again last asked the keyring again (SB-02).
    pub lock_retry_at: Option<u64>,
    /// The keyring hasn't answered the last lock check yet.
    pub lock_pending: bool,
    /// Undo / Restore rows under the newest `/skills changes` bubble, as last
    /// read from the ChangeLedger. `None` means read it again.
    pub skill_rows: Option<Vec<super::skill_undo::SkillRow>>,
    /// True only while `send_from_composer` hands the user's own typed line
    /// to `send_chat`. `/skills undo` and `/skills restore` act on it; from
    /// anywhere else they only show the Undo rows.
    pub typed_send: bool,
    /// Spike-1a recovery ladder for this cabin. The user's own message resets it.
    pub ladder: hx::Ladder,
    /// Ladder pauses waiting on the user.
    pub soft_parks: Vec<SoftPark>,
    /// A retry / backtrack turn, sent once the live reply and the user's
    /// queued messages are done.
    pub repair: Option<String>,
    /// The last finished reply's prose, for the turn-end audit. `None` when
    /// that turn was not on the visible chat.
    pub last_reply: Option<String>,
    /// The decision inbox rows are open under the needs-attention line.
    pub inbox_open: bool,
    /// The card an inbox row asked to scroll into view, painted once.
    pub jump: Option<&'static str>,
}

/// Readonly until the desktop switch is on; Full only after Grant full.
pub(super) fn access_for(desktop_control: bool, full: bool) -> AccessMode {
    match (desktop_control, full) {
        (false, _) => AccessMode::Readonly,
        (true, false) => AccessMode::Supervised,
        (true, true) => AccessMode::Full,
    }
}

/// Grok Build computer use reaches the cabin as the `grokhub-desktop` MCP
/// (or the Spike-2a `grokhub-cua` proxy).
pub(super) fn is_desktop_ask(p: &grokhub_acp::PermissionAsk) -> bool {
    let hay = format!("{} {}", p.title, p.action).to_ascii_lowercase();
    grokhub_core::CABIN_CU_SERVERS.iter().any(|s| hay.contains(s))
}

/// Permission-card eyebrow: the kind of ask, not the decision count.
pub(super) fn perm_card_eyebrow(p: &grokhub_acp::PermissionAsk) -> &'static str {
    if is_desktop_ask(p) { "Desktop" } else { "Tool" }
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
    grokhub_core::CABIN_CU_SERVERS.iter().any(|s| hay.contains(s))
}

/// The tool name of a computer-use frame: the stream's `toolName` when it
/// sent one, else the frame title.
fn cu_name(card: &ToolCard, raw: &serde_json::Value) -> String {
    raw.get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or(card.title.as_str())
        .trim()
        .to_string()
}

/// Grok Build's own computer use (path D): not `grokhub-desktop`, not a
/// browser tool, and named like a screen, mouse, or keyboard tool. The name
/// is the stream's `toolName` when sent, so a generic title still counts.
pub(super) fn is_builtin_cu_card(card: &ToolCard) -> bool {
    !is_desktop_card(card) && hx::builtin_cu(&cu_name(card, &raw_of(card)))
}

/// What a path D check reads: the frame's kept keys without the tool name.
fn cu_args(raw: &serde_json::Value) -> serde_json::Value {
    let mut args = match raw {
        serde_json::Value::Object(m) => serde_json::Value::Object(m.clone()),
        _ => serde_json::json!({}),
    };
    if let Some(m) = args.as_object_mut() {
        m.remove("tool");
    }
    args
}

fn frame_hash(url: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    h.finish()
}

fn pending(status: &str) -> bool {
    matches!(status.trim().to_ascii_lowercase().as_str(), "" | "pending")
}

fn failed(status: &str) -> bool {
    matches!(status.to_ascii_lowercase().as_str(), "failed" | "error" | "cancelled")
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

    /// Everything on the approval stack that has buttons: the hard card and its
    /// queue, the Grant full card, the permission ask and its queue, and an
    /// elicit. Recovery-ladder pauses count too.
    pub(super) fn decisions_waiting(&self) -> usize {
        self.hard_waiting()
            + self.harness.soft_parks.len()
            + usize::from(self.harness.full_card.is_some())
            + usize::from(self.perm_ask.is_some())
            + self.perm_queue.len()
            + usize::from(self.elicit_ask.is_some())
    }

    pub(super) fn trace_id(&self) -> String {
        self.threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "session".into())
    }

    pub(super) fn turn_no(&self) -> u32 {
        self.messages.iter().filter(|(r, _)| r == "user").count() as u32
    }

    /// The allow span for one hub publish, tagged with its consent. Returns
    /// the `{session}:{ts_ms}` ref the `egress.jsonl` line points at.
    pub(super) fn hub_sync_span(&self, send: &super::privacy_ui::HubSend) -> String {
        let mut span = hx::Span::soft_allow(
            &self.trace_id(),
            HUB_SYNC_TOOL,
            HUB_SYNC_ARGS,
            "sent",
            "published chats and memory to paired computers",
            self.access_mode(),
            "none",
        )
        .with_consent(send.consent_ref());
        span.approval_class = HardClass::Send.as_str().into();
        let at = span.span_ref();
        self.write_span(span, "egress");
        at
    }

    /// No hub grant: park a hard Send card. Nothing has left.
    pub(super) fn park_egress(&mut self, dest: &str, class: HardClass) {
        let trace = self.trace_id();
        self.write_span(hx::Span::hard_park(&trace, HUB_SYNC_TOOL, HUB_SYNC_ARGS, class), "egress");
        let action = super::privacy_ui::HUB_CARD_ACTION.to_string();
        self.park_hard(ParkSource::Egress(dest.into()), class, "egress", HUB_SYNC_TOOL.into(), action);
    }

    pub(super) fn write_span(&self, span: hx::Span, path: &str) {
        self.write_span_at(span, path, self.turn_no());
    }

    fn write_span_at(&self, span: hx::Span, path: &str, turn: u32) {
        let trace = self.trace_id();
        let mut span = span.on_path(path).in_turn(&trace, turn);
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
        if !p.tool_call_id.is_empty() {
            self.harness.asked.insert(p.tool_call_id.clone());
        }
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
                if self.take_oneshot(&p.action) || self.take_oneshot_step(&p.title, &p.action, class) {
                    // Approved once on a path C or D card: Grok's own Allow card decides.
                    return Some(p);
                }
                // A credential field's typed value never reaches a span or the card.
                let action = if class == HardClass::Credentials {
                    CREDENTIAL_ACTION.to_string()
                } else {
                    p.action.clone()
                };
                self.write_span(hx::Span::hard_park(&trace, &p.title, &span_args(&p.title, &action), class), path);
                let tool = p.title.clone();
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
            shot.step = None;
        }
        hit
    }

    /// The path B ask for a path D re-run: same hard class, and its title or
    /// action names the tool the card was for.
    fn take_oneshot_step(&mut self, title: &str, action: &str, class: HardClass) -> bool {
        let Some(shot) = self.harness.oneshot.as_mut() else {
            return false;
        };
        let hit = shot.step.as_ref().is_some_and(|(tool, c)| {
            let tool = tool.to_ascii_lowercase();
            *c == class
                && (title.to_ascii_lowercase().contains(&tool) || action.to_ascii_lowercase().contains(&tool))
        });
        if hit {
            shot.action = None;
            shot.step = None;
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
            chat_id: self.trace_id(),
        };
        if self.harness.park.is_some() {
            self.harness.queue.push_back(park);
        } else {
            self.harness.park = Some(park);
        }
        if self.chrome_here() {
            self.status = crate::motion::needs_attention_summary(self.decisions_waiting());
        }
    }

    /// Approve answers once (never Always). Deny, Esc, TTL, and halt reject.
    pub(super) fn resolve_hard_park(&mut self, approve: bool, why: &str) {
        let Some(park) = self.harness.park.take() else {
            return;
        };
        let mut sync_once = false;
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
            ParkSource::Headless | ParkSource::Held => {
                if approve {
                    self.start_oneshot(&park.action, None);
                }
            }
            ParkSource::Unasked => {
                if approve {
                    self.start_oneshot(&park.action, Some((park.tool.clone(), park.class)));
                }
            }
            ParkSource::Egress(dest) => {
                sync_once = approve && dest == hx::HUB_DEST;
            }
        }
        self.status = if approve {
            format!("Approved once · {}", park.class.label())
        } else {
            format!("Denied · {}", park.class.label())
        };
        self.harness.park = self.harness.queue.pop_front();
        if sync_once {
            self.run_hub_sync(super::privacy_ui::HubSend::Once);
        }
    }

    /// Put the `i`th hard park (0 is the card on screen, then the queue) on the
    /// card. The one it replaces goes back to the front of the queue.
    pub(super) fn promote_hard_park(&mut self, i: usize) {
        if i == 0 {
            return;
        }
        if let Some(p) = self.harness.queue.remove(i - 1) {
            if let Some(head) = self.harness.park.take() {
                self.harness.queue.push_front(head);
            }
            self.harness.park = Some(p);
        }
    }

    /// Answer the `i`th hard park: the same span and effect as its card.
    pub(super) fn resolve_hard_park_at(&mut self, i: usize, approve: bool, why: &str) {
        self.promote_hard_park(i);
        self.resolve_hard_park(approve, why);
    }

    /// A ladder pause answered from its card or the inbox. Approve writes the
    /// same `resume` span as the user's next message; Deny drops the pause
    /// and the pending repair turn. Either way the ladder starts over.
    pub(super) fn answer_soft_park(&mut self, i: usize, approve: bool, why: &str) {
        if i >= self.harness.soft_parks.len() {
            return;
        }
        let park = self.harness.soft_parks.remove(i);
        let trace = self.trace_id();
        let args = serde_json::json!({ "detector": park.detector, "evidence": park.evidence }).to_string();
        let mut span = if approve {
            let mut s = hx::Span::soft_allow(
                &trace,
                hx::RECOVERY_TOOL,
                &args,
                "resume",
                &format!("you answered the pause ({})", park.reason),
                self.access_mode(),
                "none",
            );
            s.decision = "resume".into();
            s
        } else {
            hx::Span::deny(&trace, hx::RECOVERY_TOOL, &args, why, "soft")
        };
        span.origin = hx::Origin::Repair;
        self.write_span(span, "audit");
        self.harness.repair = None;
        self.harness.ladder.reset();
        self.status = if approve { "Resumed".into() } else { "Stopped that step".into() };
    }

    /// Tray Halt and the halt hotkeys: every inbox row is denied with a span,
    /// not only the hard cards. A Steer does not come here.
    pub(super) fn halt_inbox(&mut self) {
        self.halt_hard_parks();
        while self.perm_ask.is_some() || !self.perm_queue.is_empty() {
            if self.perm_ask.is_none() {
                self.next_perm_ask();
            }
            self.answer_perm_at(0, false, HALT_DENY);
        }
        while !self.harness.soft_parks.is_empty() {
            self.answer_soft_park(0, false, HALT_DENY);
        }
    }

    /// A turn is stopping: parks that hold a GB ask or a desktop call fail closed.
    /// Path C and egress cards hold nothing and stay until clicked, TTL, or halt.
    pub(super) fn withdraw_hard_parks(&mut self) {
        let mut keep = VecDeque::new();
        while let Some(park) = self.harness.park.clone() {
            if matches!(
                park.source,
                ParkSource::Headless | ParkSource::Egress(_) | ParkSource::Held | ParkSource::Unasked
            ) {
                keep.push_back(park);
                self.harness.park = self.harness.queue.pop_front();
            } else {
                self.resolve_hard_park(false, "turn stopped — fail-closed Deny");
            }
        }
        self.harness.park = keep.pop_front();
        self.harness.queue = keep;
    }

    /// A Steer stops the turn, not the decisions on screen (Spike-1a). Take the
    /// hard parks out before the halt: a park holding the stopped turn's GB ask
    /// or desktop call is denied there with a span (that call is gone) and
    /// kept as a held card that re-runs the step once on Approve. Path C and
    /// egress cards hold nothing and keep as they are. Ladder pauses are not
    /// touched by a halt.
    pub(super) fn take_parks_for_steer(&mut self) -> VecDeque<HardParkUi> {
        self.take_parks_for_stop("turn steered — the call is gone, the card stays for a fresh approval")
    }

    /// [`Self::take_parks_for_steer`] with the reason the deny spans give.
    fn take_parks_for_stop(&mut self, why: &str) -> VecDeque<HardParkUi> {
        let dir = crate::config::config_dir();
        let trace = self.trace_id();
        let mut kept: VecDeque<HardParkUi> = self.harness.park.take().into_iter().collect();
        kept.extend(self.harness.queue.drain(..));
        for park in kept.iter_mut() {
            match &park.source {
                ParkSource::Ask(p) => {
                    if let Some(h) = &self.acp {
                        let _ = h.reject_permission(p);
                    }
                }
                ParkSource::Desk(id) => {
                    let _ = hx::answer_park(&dir, id, false);
                }
                ParkSource::Headless | ParkSource::Egress(_) | ParkSource::Held | ParkSource::Unasked => continue,
            }
            let args = span_args(&park.tool, &park.action);
            self.write_span(hx::Span::deny(&trace, &park.tool, &args, why, park.class.as_str()), park.path);
            self.write_span(hx::Span::hard_park(&trace, &park.tool, &args, park.class), park.path);
            park.source = ParkSource::Held;
        }
        kept
    }

    /// Put the steered parks back, ahead of anything parked since.
    pub(super) fn restore_parks_after_steer(&mut self, mut kept: VecDeque<HardParkUi>) {
        kept.extend(self.harness.park.take());
        kept.extend(self.harness.queue.drain(..));
        self.harness.park = kept.pop_front();
        self.harness.queue = kept;
    }

    /// Spike-1a, at the end of a turn: write the reply's claim, audit this turn
    /// in two passes, and take one ladder step for the first finding. Only a
    /// turn that has harness spans (or says `GOAL_COMPLETE`) is audited, so a
    /// plain coding chat writes nothing. While a pause waits on the user, the
    /// ladder waits too.
    pub(super) fn harness_turn_end(&mut self, reply: &str, turn: u32) {
        let dir = crate::config::config_dir();
        let trace = self.trace_id();
        let this_turn = |a: &hx::Audit| a.windows.iter().any(|w| w.chat_id == trace && w.turn == turn);
        let stepped = hx::audit_file(&dir, &trace, Some((&trace, turn))).is_ok_and(|a| this_turn(&a));
        if !stepped && !grokhub_core::verify::has_goal_complete(reply) {
            return;
        }
        self.write_span_at(hx::Span::reply(&trace, reply, &self.secret_hold), "audit", turn);
        if !self.harness.soft_parks.is_empty() {
            return;
        }
        let Ok(audit) = hx::audit_file(&dir, &trace, Some((&trace, turn))) else {
            return;
        };
        let Some(window) = audit.flagged.iter().find(|w| !w.findings.is_empty()) else {
            return;
        };
        let step = self.harness.ladder.next(&window.findings[0], &window.spans);
        self.write_span_at(hx::ladder_span(&trace, &step), "audit", turn);
        match step.rung {
            hx::Rung::Retry | hx::Rung::Backtrack => self.harness.repair = step.prompt,
            hx::Rung::Pause => {
                self.harness.repair = None;
                self.harness.soft_parks.push(SoftPark {
                    detector: step.detector,
                    reason: step.reason,
                    evidence: step.evidence,
                    chat_id: trace.clone(),
                });
                if self.chrome_here() {
                    self.status = crate::motion::needs_attention_summary(self.decisions_waiting());
                }
            }
        }
    }

    /// [`Self::harness_turn_end`] on the reply `finish_acp_turn` kept. `turn`
    /// is the user turn that just ended (taken before a queued message could start).
    pub(super) fn harness_turn_end_last_reply(&mut self, turn: u32) {
        if let Some(reply) = self.harness.last_reply.take() {
            self.harness_turn_end(&reply, turn);
        }
    }

    /// The repair turn goes once nothing of the user's is waiting. Returns
    /// true when it was sent.
    pub(super) fn kick_repair(&mut self) -> bool {
        match self.harness.repair.take() {
            Some(prompt) => {
                self.send_chat(prompt);
                true
            }
            None => false,
        }
    }

    /// A message the user queued goes first; the pending repair turn is
    /// dropped with a span. Parks and pauses stay.
    pub(super) fn harness_user_queued(&mut self) {
        if self.harness.repair.take().is_some() {
            let trace = self.trace_id();
            let mut span = hx::Span::deny(&trace, hx::RECOVERY_TOOL, "{}", "skip", "soft")
                .from_origin(hx::Origin::Repair);
            span.decision = "skip".into();
            span.claim = "your queued message goes first".into();
            self.write_span(span, "audit");
        }
    }

    /// The user typed a new message with no reply running: that answers the
    /// ladder's pauses and starts every target over. A Steer or a queued
    /// message does not come here.
    pub(super) fn harness_user_sent(&mut self) {
        let trace = self.trace_id();
        for park in std::mem::take(&mut self.harness.soft_parks) {
            let args = serde_json::json!({ "detector": park.detector, "evidence": park.evidence }).to_string();
            let mut span = hx::Span::soft_allow(
                &trace,
                hx::RECOVERY_TOOL,
                &args,
                "resume",
                &format!("you answered the pause ({})", park.reason),
                self.access_mode(),
                "none",
            );
            span.decision = "resume".into();
            self.write_span(span, "audit");
        }
        self.harness.repair = None;
        self.harness.ladder.reset();
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
        // Grok Build's own computer use is path D's (`harness_watch_cu`).
        if is_builtin_cu_card(card) {
            return;
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

    /// Path D (Spike-1c): a Grok Build computer-use frame the cabin made no
    /// decision for (no path B ask). This is the belt; the `--deny` rules on
    /// every spawn (`BUILTIN_CU_DENY`) are the lock. A fast step can finish
    /// before its first frame arrives, so this can stop the turn only after
    /// that step. On a live ACP session (`from_acp`) a pending frame waits:
    /// Grok Build sends its ask after the first frame. Returns true when it
    /// stopped the turn.
    pub(super) fn harness_watch_cu(&mut self, card: &ToolCard, from_acp: bool) -> bool {
        // An update frame may carry no title or args: use what the first one had.
        let (name, args) = match self.harness.watched.get(&card.id).cloned() {
            Some(Watched::Done) => return false,
            Some(Watched::Allowed { tool, args }) => {
                if ran(&card.status) || failed(&card.status) {
                    self.write_cu_allow(card, &tool, &args);
                }
                return false;
            }
            Some(Watched::Waiting { tool, args }) => (tool, args),
            None => {
                if !is_builtin_cu_card(card) {
                    return false;
                }
                let raw = raw_of(card);
                (cu_name(card, &raw), cu_args(&raw))
            }
        };
        if self.harness.asked.contains(&card.id) {
            // Path B decided this one.
            self.harness.watched.insert(card.id.clone(), Watched::Done);
            return false;
        }
        if !self.running {
            return false;
        }
        if from_acp && pending(&card.status) {
            self.harness.watched.insert(card.id.clone(), Watched::Waiting { tool: name, args });
            return false;
        }
        match hx::decide_unasked(&name, &args, self.access_mode()) {
            GateOutcome::Allow => {
                let kept = hx::redact_args(&args.to_string());
                if ran(&card.status) || failed(&card.status) {
                    self.write_cu_allow(card, &name, &kept);
                } else {
                    self.harness.watched.insert(card.id.clone(), Watched::Allowed { tool: name, args: kept });
                }
                false
            }
            GateOutcome::Park { hard: Some(class), .. } => self.stop_unasked_hard(card, &name, &args, class),
            GateOutcome::Refuse { reason } | GateOutcome::Park { reason, hard: None, .. } => {
                // Readonly or the floor: stop the turn, no card, no bypass.
                self.harness.watched.insert(card.id.clone(), Watched::Done);
                let trace = self.trace_id();
                let class = if self.access_mode().allows_computer() { "floor" } else { "soft" };
                let span = hx::Span::deny(&trace, &name, &hx::redact_args(&args.to_string()), &reason, class);
                self.write_span(span, "D");
                self.stop_turn_unasked();
                self.status = format!("Stopped: {reason}");
                true
            }
        }
    }

    /// A soft path D step finished: its allow span, with `ui_changed` when the
    /// frame carries an image. Looks (screenshots, snapshots) write no span,
    /// like path A.
    fn write_cu_allow(&mut self, card: &ToolCard, name: &str, kept: &str) {
        self.harness.watched.insert(card.id.clone(), Watched::Done);
        let changed = card.image_data_url.as_deref().map(frame_hash).and_then(|h| {
            let was = self.harness.last_cu_frame.replace(h);
            was.map(|w| w != h)
        });
        if hx::cu_look_only(name) {
            return;
        }
        let trace = self.trace_id();
        let span = if failed(&card.status) {
            hx::Span::deny(&trace, name, kept, &card.status, "soft")
        } else {
            hx::Span::soft_allow(&trace, name, kept, "ran", "Grok Build computer use, no ask", self.access_mode(), "grok_build")
        };
        self.write_span(span.with_ui_changed(changed), "D");
    }

    /// A hard path D step: the turn stops at once, the raw step is logged as
    /// an allow with no approve when it already ran (`approval_gate_violation`
    /// flags it), then a deny span and a hard card. Approve re-runs that one
    /// step once through a one-shot ACP Ask, like a path C card.
    fn stop_unasked_hard(&mut self, card: &ToolCard, name: &str, args: &serde_json::Value, class: HardClass) -> bool {
        self.harness.watched.insert(card.id.clone(), Watched::Done);
        let trace = self.trace_id();
        let kept = hx::redact_args(&args.to_string());
        let action = hx::unasked_action(name, args, class);
        if self.take_oneshot(&action) {
            // The one step the user approved on this card, re-run once.
            let mut s = hx::Span::soft_allow(&trace, name, &kept, "ran", "approved once on the card", self.access_mode(), "grok_build");
            s.approval_class = class.as_str().into();
            s.hard_approved = true;
            self.write_span(s, "D");
            return false;
        }
        if ran(&card.status) {
            let mut s = hx::Span::soft_allow(&trace, name, &kept, "ran", "Grok Build ran a hard step with no ask", self.access_mode(), "grok_build");
            s.approval_class = class.as_str().into();
            self.write_span(s, "D");
        }
        let why = "stopped: Grok Build tried a hard step without asking";
        self.write_span(hx::Span::deny(&trace, name, &kept, why, class.as_str()), "D");
        self.stop_turn_unasked();
        self.write_span(hx::Span::hard_park(&trace, name, &kept, class), "D");
        self.park_hard(ParkSource::Unasked, class, "D", name.to_string(), action);
        self.status = hx::unasked_title(class);
        true
    }

    /// Stop the turn with the existing stop path. Parked cards stay, like a
    /// Steer: a path A / B park is denied with a span and kept as held.
    fn stop_turn_unasked(&mut self) {
        let parks = self.take_parks_for_stop("turn stopped by the path D check — the card stays for a fresh approval");
        self.halt_in_flight();
        self.restore_parks_after_steer(parks);
    }

    /// Turn end (or a stop): allowed path D steps that never sent a finished
    /// frame still get their span; the turn's asks and checks start over.
    pub(super) fn harness_watch_end(&mut self) {
        let trace = self.trace_id();
        let mut left: Vec<(String, Watched)> = self.harness.watched.drain().collect();
        left.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, w) in left {
            if let Watched::Allowed { tool, args } = w {
                if hx::cu_look_only(&tool) {
                    continue;
                }
                let span = hx::Span::soft_allow(&trace, &tool, &args, "ran", "Grok Build computer use, no ask", self.access_mode(), "grok_build");
                self.write_span(span, "D");
            }
        }
        self.harness.asked.clear();
    }

    /// Approve on a path C card: one ACP Ask turn for that step, then the pill
    /// goes back. Flags are never loosened; Grok's own Allow card decides.
    fn start_oneshot(&mut self, action: &str, step: Option<(String, HardClass)>) {
        self.harness.oneshot = Some(OneShot {
            action: Some(action.to_string()),
            step,
            restore: self.permission_mode,
            started: false,
        });
        self.permission_mode = PermissionMode::Ask;
        self.send_chat(format!(
            "I approved one step that was blocked. Run only this, once, then stop: {action}"
        ));
    }

    fn offer_full(&mut self) {
        // Full changes nothing a gate reads, so the card stays hidden until the flag is on.
        if !self.harness.full_card_on {
            return;
        }
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
            let title = match park.source {
                ParkSource::Unasked => hx::unasked_title(park.class),
                _ => park.class.label().to_string(),
            };
            let action = if park.action.trim().is_empty() {
                park.tool.clone()
            } else {
                park.action.clone()
            };
            let note = match park.source {
                ParkSource::Headless => HEADLESS_NOTE,
                ParkSource::Unasked => UNASKED_NOTE,
                ParkSource::Egress(_) => super::privacy_ui::HUB_CARD_NOTE,
                _ => HARD_NOTE,
            };
            let overlay = self.palette_open || self.nav == Nav::Settings || self.find.focused;
            let key = hx::hard_card_key(
                super::chat_ui::bare_press(ui, egui::Key::Enter),
                super::chat_ui::bare_press(ui, egui::Key::Escape),
                overlay || super::chat_ui::overlay_over_chat(ui.ctx()),
            );
            let id = ("hard-park", format!("{}:{}", park.path, park.action));
            let text = CardText {
                eyebrow: HARD_EYEBROW,
                title: &title,
                action: &action,
                note,
                primary: "Approve",
                secondary: "Deny",
                hard: true,
            };
            if key == Some(hx::HardAnswer::Deny) {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                self.resolve_hard_park(false, "Jeremy denied (Esc)");
            } else {
                let top = ui.cursor().min.y;
                let hit = harness_card(ui, id, text, running);
                self.scroll_if_jumped(ui, "hard", top);
                match hit {
                    Some(true) => self.resolve_hard_park(true, ""),
                    Some(false) => self.resolve_hard_park(false, SOFT_DENY),
                    None => {}
                }
            }
        }
        if let Some(park) = self.harness.soft_parks.first() {
            let text = CardText {
                eyebrow: SOFT_EYEBROW,
                title: &park.reason.clone(),
                action: "",
                note: SOFT_NOTE,
                primary: "Approve",
                secondary: "Deny",
                hard: false,
            };
            let top = ui.cursor().min.y;
            let hit = harness_card(ui, ("soft-park", park.detector.clone()), text, running);
            self.scroll_if_jumped(ui, "soft", top);
            if let Some(approve) = hit {
                self.answer_soft_park(0, approve, SOFT_DENY);
            }
        }
        if self.harness.full_card.is_some() {
            let text = CardText {
                eyebrow: FULL_EYEBROW,
                title: FULL_TITLE,
                action: "",
                note: FULL_NOTE,
                primary: "Grant full",
                secondary: "Not now",
                hard: false,
            };
            let top = ui.cursor().min.y;
            let hit = harness_card(ui, ("grant-full", String::new()), text, running);
            self.scroll_if_jumped(ui, "full", top);
            if let Some(grant) = hit {
                self.resolve_grant_full(grant, "Jeremy kept Supervised");
            }
        }
    }

    /// An inbox row asked for this card: bring what was just painted (from
    /// `top` to the cursor) into view once.
    pub(super) fn scroll_if_jumped(&mut self, ui: &egui::Ui, card: &str, top: f32) {
        if self.harness.jump != Some(card) {
            return;
        }
        self.harness.jump = None;
        let bottom = ui.cursor().min.y.max(top + 1.0);
        let rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=bottom);
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
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
    /// Hard: danger Approve, 2px stroke, monospace command. Soft: white primary, proportional.
    hard: bool,
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
        hard,
    } = text;
    let style = card_style(hard);
    ui.add_space(8.0);
    let card_id = egui::Id::new(("harness-card", id.0, id.1));
    let enter_t = crate::motion::approval_enter_t(ui, card_id, true);
    let y = crate::motion::approval_y(enter_t, false);
    let avail = ui.available_rect_before_wrap();
    let slot = avail.translate(egui::vec2(0.0, y));
    let mut hit = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
        ui.multiply_opacity(enter_t.clamp(0.0, 1.0));
        // Transparent rest fill keeps muted text at the permission card's contrast.
        // Hard is a 2px foreground stroke; Grant full is the 1px border.
        let column = ui.available_width();
        let inner = approval_card_inner(column, style.stroke_w);
        let framed = egui::Frame::NONE
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(crate::theme::CHROME_RADIUS)
            .stroke(egui::Stroke::new(style.stroke_w, style.stroke_color))
            .inner_margin(egui::Margin::same(APPROVAL_CARD_MARGIN_PX))
            .show(ui, |ui| {
                ui.set_min_width(inner);
                ui.set_max_width(inner);
                ui.add(
                    egui::Label::new(
                        RichText::new(eyebrow)
                            .size(12.0)
                            .color(crate::theme::muted()),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(14.0)
                            .strong()
                            .color(crate::theme::fg()),
                    )
                    .wrap(),
                );
                // Mono only for the literal command. Grant full passes an empty action.
                if !action.trim().is_empty() {
                    ui.add_space(4.0);
                    let mut line = RichText::new(action).size(13.0).color(crate::theme::fg());
                    if hard {
                        line = line.monospace();
                    }
                    ui.add(egui::Label::new(line).wrap());
                }
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(RichText::new(note).size(12.0).color(crate::theme::muted()))
                        .wrap(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let clicked = match style.primary {
                        // A hard Approve takes a pointer click: Enter or Space on a focused pill does nothing.
                        PillKind::Danger => crate::cards::felt_pill_at(ui, None, primary, PillKind::Danger)
                            .clicked_by(egui::PointerButton::Primary),
                        PillKind::Solid => crate::cards::white_pill(ui, primary),
                        PillKind::Ghost => crate::cards::ghost_pill(ui, primary),
                    };
                    if clicked {
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
    use crate::motion::{
        AgentCursorAnim, AgentCursorPhase, AGENT_CURSOR_MOTION, CURSOR_CLICK_SECS, CURSOR_TRAVEL_SECS,
    };
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
        motion: AGENT_CURSOR_MOTION,
    };
    let (pos, scale, _) = crate::motion::cursor_sample(&anim, now, false);
    (pos, scale, true)
}

/// Agent cursor marker on the latest Work-tree frame at the last approved click.
pub(super) fn paint_click_marker(ui: &egui::Ui, cards: &[&ToolCard]) {
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
        cabin.harness.full_card_on = true;
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
        cabin.harness.full_card_on = true;
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

    /// The plain-text send path with no Grok Build on PATH, so a send stays local.
    struct NoGrok;
    impl NoGrok {
        fn set(root: &std::path::Path) -> Self {
            std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));
            Self
        }
    }
    impl Drop for NoGrok {
        fn drop(&mut self) {
            std::env::remove_var("GROKHUB_GROK");
        }
    }

    fn decisions(root: &std::path::Path) -> Vec<(String, String, String)> {
        hx::read_spans(root, "session")
            .unwrap()
            .into_iter()
            .map(|s| (s.path, s.tool, s.decision))
            .collect()
    }

    fn row(path: &str, tool: &str, decision: &str) -> (String, String, String) {
        (path.into(), tool.into(), decision.into())
    }

    #[test]
    fn parked_cards_and_spans_survive_a_steer_and_a_queued_message() {
        let (_pin, root) = pinned("harness-steer");
        let _grok = NoGrok::set(&root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        // Path B hard ask, path A desk park, path C headless park, one ladder pause.
        assert_eq!(cabin.harness_precheck(ask("Run command", "rm -f draft.md")), None);
        hx::post_park(
            &root,
            &hx::ParkRequest {
                id: "d9".into(),
                path: "A".into(),
                tool: "type".into(),
                action: "rm -f disposable.txt".into(),
                class: "delete".into(),
                ts_ms: 1,
            },
        )
        .unwrap();
        cabin.poll_harness();
        cabin.harness_note_headless(&card("c1", "pending", r#"{"tool":"run_terminal_command","command":"shred notes.txt"}"#));
        cabin.harness_headless_end();
        cabin.harness.soft_parks.push(SoftPark {
            detector: "action_loop".into(),
            reason: "action_loop: `click` ran 3 times".into(),
            evidence: vec!["session:1".into()],
            chat_id: "session".into(),
        });
        assert_eq!(cabin.decisions_waiting(), 4);
        let before = decisions(&root);

        // A Steer: the live turn on this chat stops and carries the new message.
        let (_tx, rx) = mpsc::channel();
        cabin.grok_p_rx = Some(rx);
        cabin.running = true;
        cabin.chat_job_thread = Some(cabin.visible_thread_id());
        assert!(cabin.can_steer_live_turn());
        cabin.send_from_composer("go left instead".into());
        assert!(cabin.grok_p_rx.is_none(), "the old turn stopped");

        assert_eq!(cabin.hard_waiting(), 3, "every hard card is still parked");
        let sources: Vec<ParkSource> = cabin
            .harness
            .park
            .iter()
            .chain(cabin.harness.queue.iter())
            .map(|p| p.source.clone())
            .collect();
        assert_eq!(sources, vec![ParkSource::Held, ParkSource::Held, ParkSource::Headless]);
        assert_eq!(cabin.harness.soft_parks.len(), 1, "a Steer does not answer a pause");
        assert_eq!(cabin.decisions_waiting(), 4);
        assert_eq!(hx::take_answer(&root, "d9"), Some(false), "the stopped desk call was denied");
        let after = decisions(&root);
        assert_eq!(after[..before.len()], before[..], "earlier spans are untouched");
        assert_eq!(
            after[before.len()..],
            [
                row("B", "Run command", "deny"),
                row("B", "Run command", "park"),
                row("A", "type", "deny"),
                row("A", "type", "park"),
            ]
        );
        let spans = hx::read_spans(&root, "session").unwrap();
        assert!(hx::approval_gate_violation(&spans).is_empty());
        assert_eq!(
            spans[before.len()].result,
            "turn steered — the call is gone, the card stays for a fresh approval"
        );

        // A queued message runs after the reply: the cards and the pause stay,
        // and a pending repair turn gives way with a span.
        cabin.running = false;
        cabin.harness.repair = Some("GrokHub's check: retry".into());
        cabin.followup_queue = vec!["also rename the folder".into()];
        cabin.drain_followup_queue();
        assert!(cabin.followup_queue.is_empty());
        assert_eq!(cabin.harness.repair, None);
        assert_eq!(cabin.hard_waiting(), 3);
        assert_eq!(cabin.harness.soft_parks.len(), 1);
        let last = hx::read_spans(&root, "session").unwrap().pop().unwrap();
        assert_eq!((last.tool.as_str(), last.decision.as_str()), (hx::RECOVERY_TOOL, "skip"));
        assert_eq!(last.origin, hx::Origin::Repair);

        // Approve on a held card re-runs that one step under Grok's own Allow.
        cabin.resolve_hard_park(true, "");
        assert_eq!(cabin.permission_mode, PermissionMode::Ask);
        assert_eq!(cabin.harness.oneshot.as_ref().and_then(|o| o.action.clone()).as_deref(), Some("rm -f draft.md"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn turn_end_audit_retries_backtracks_then_pauses_with_spans() {
        let (_pin, root) = pinned("harness-ladder");
        let _grok = NoGrok::set(&root);
        let mut cabin = Cabin::quiet_for_test();
        let turn = cabin.turn_no();
        let click = |ts: u64| {
            let mut s = hx::Span::soft_allow("session", "click", r#"{"x":40,"y":12}"#, "clicked", "grokhub-desktop click", AccessMode::Supervised, "grok_build")
                .on_path("A")
                .in_turn("session", turn)
                .with_ui_changed(Some(false));
            s.ts_ms = ts;
            hx::append_span(&root, &s).unwrap();
        };
        // A plain chat turn with no harness span writes nothing.
        cabin.harness_turn_end("I've updated the README.", turn);
        assert!(hx::read_spans(&root, "session").unwrap().is_empty());

        click(1);
        cabin.harness_turn_end("I clicked Save and it worked.", turn);
        let repair = cabin.harness.repair.clone().expect("retry turn queued");
        assert!(repair.starts_with("GrokHub's check: `click` changed nothing on screen"), "{repair}");
        assert!(cabin.kick_repair(), "the retry goes once nothing else waits");
        assert_eq!(cabin.harness.repair, None);

        click(2);
        cabin.harness_turn_end("Clicked Save again, done.", turn);
        assert!(cabin.harness.repair.as_deref().unwrap().contains("try a different target"));
        cabin.harness.repair = None;

        click(3);
        cabin.harness_turn_end("Saved.", turn);
        assert_eq!(cabin.harness.repair, None);
        assert_eq!(cabin.harness.soft_parks.len(), 1);
        assert_eq!(cabin.decisions_waiting(), 1);
        assert_eq!(cabin.status, "1 thing needs a decision");

        // Paused: the ladder waits for the user.
        click(4);
        cabin.harness_turn_end("Saved it.", turn);
        assert_eq!(cabin.harness.soft_parks.len(), 1);

        let got: Vec<(String, String)> = hx::read_spans(&root, "session")
            .unwrap()
            .into_iter()
            .map(|s| (s.tool, s.decision))
            .collect();
        let pair = |t: &str, d: &str| (t.to_string(), d.to_string());
        assert_eq!(
            got,
            vec![
                pair("click", "allow"),
                pair("reply", "say"),
                pair("harness_recovery", "retry"),
                pair("click", "allow"),
                pair("reply", "say"),
                pair("harness_recovery", "backtrack"),
                pair("click", "allow"),
                pair("reply", "say"),
                pair("harness_recovery", "pause"),
                pair("click", "allow"),
                pair("reply", "say"),
            ]
        );

        // The user's own next message answers the pause and starts over.
        cabin.send_from_composer("ok, use the File menu".into());
        assert!(cabin.harness.soft_parks.is_empty());
        let last = hx::read_spans(&root, "session").unwrap().pop().unwrap();
        assert_eq!((last.tool.as_str(), last.decision.as_str()), ("harness_recovery", "resume"));
        assert!(last.claim.starts_with("you answered the pause (claimed_click_no_change:"), "{}", last.claim);
        assert_eq!(cabin.harness.ladder, hx::Ladder::new());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn denied_hard_send_claimed_as_sent_pauses_and_never_retries() {
        let (_pin, root) = pinned("harness-ladder-hard");
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        let turn = cabin.turn_no();
        assert_eq!(cabin.harness_precheck(ask("send_email", "to sam")), None);
        cabin.resolve_hard_park(false, "Jeremy denied");
        cabin.harness_turn_end("I sent the email to Sam.", turn);
        assert_eq!(cabin.harness.repair, None, "a hard step is never retried on its own");
        assert_eq!(cabin.harness.soft_parks.len(), 1);
        let last = hx::read_spans(&root, "session").unwrap().pop().unwrap();
        assert_eq!((last.decision.as_str(), last.approval_class.as_str()), ("pause", "send"));
        assert_eq!(
            last.claim,
            "unsupported_assurance: hard-class send is never retried on its own; it needs your approval"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn credential_ask_parks_hard_and_keeps_the_value_out() {
        let (_pin, root) = pinned("harness-cred");
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        assert_eq!(cabin.harness_precheck(ask("Type text", "into the Password field: hunter2222")), None);
        let park = cabin.harness.park.clone().expect("hard card");
        assert_eq!(park.class, HardClass::Credentials);
        assert_eq!(park.action, CREDENTIAL_ACTION);
        cabin.secret_hold = vec!["hunter2222".into()];
        cabin.resolve_hard_park(false, "Jeremy denied");
        cabin.harness_turn_end("I typed hunter2222 into the field and logged in.", cabin.turn_no());
        let trace = std::fs::read_to_string(hx::span_path(&root, "session")).unwrap();
        assert!(!trace.contains("hunter2222"), "{trace}");
        assert!(trace.contains("I typed [redacted] into the field and logged in."), "{trace}");
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

    #[test]
    fn card_style_splits_hard_from_grant_full() {
        let hard = card_style(true);
        assert_eq!(hard.primary, PillKind::Danger);
        assert_eq!(hard.stroke_w, 2.0);
        assert_eq!(hard.stroke_color, crate::theme::fg());
        let soft = card_style(false);
        assert_eq!(soft.primary, PillKind::Solid);
        assert_eq!(soft.stroke_w, 1.0);
        assert_eq!(soft.stroke_color, crate::theme::border());
    }

    #[test]
    fn approval_card_width_clamps_at_520() {
        assert_eq!(approval_card_width(900.0), 520.0);
        assert_eq!(approval_card_width(300.0), 300.0);
        assert_eq!(approval_card_width(520.0), 520.0);
    }

    #[test]
    fn approval_copy_names_the_card_and_the_floor() {
        assert_eq!(
            HARD_NOTE,
            "Always can't skip this. Approve runs it once. Esc denies."
        );
        assert!(HEADLESS_NOTE.ends_with(" Esc denies."));
        assert_eq!(
            HEADLESS_NOTE,
            "Grok Build's deny rule stopped this. Approve re-runs this one step with Grok's own Allow. Esc denies."
        );
        assert_eq!(HARD_EYEBROW, "Hard action");
        assert_eq!(FULL_EYEBROW, "Session access · Desktop control is on");
        assert_eq!(
            FULL_TITLE,
            "Let Grok click and type without asking for this session?"
        );
        assert_eq!(FULL_NOTE, "Deletes, sends, money and credentials still ask.");
        assert_eq!(
            perm_card_eyebrow(&ask("grokhub-desktop__click", "click 1,2")),
            "Desktop"
        );
        assert_eq!(perm_card_eyebrow(&ask("bash", "ls")), "Tool");
        assert_eq!(
            super::super::settings::DESKTOP_CONTROL_HINT,
            "Grok can see the screen and use the mouse and keyboard through GrokHub. Ask still asks first. Deletes, sends, money and credentials always ask."
        );
        assert_eq!(
            crate::motion::CURSOR_OUTLINE,
            egui::Color32::from_rgb(0x0b, 0x0b, 0x0c)
        );
    }

    #[test]
    fn needs_attention_summary_is_only_on_the_stack() {
        let chat = include_str!("chat_ui.rs");
        let stack = chat
            .split("fn paint_approval_stack(")
            .nth(1)
            .and_then(|s| s.split("fn paint_perm_ask(").next())
            .expect("paint_approval_stack");
        assert!(
            stack.contains("paint_inbox"),
            "the stack line is the one count (Spike-1b: the inbox line): {stack}"
        );
        let perm = chat
            .split("fn paint_perm_ask(")
            .nth(1)
            .and_then(|s| s.split("fn paint_elicit_ask(").next())
            .expect("paint_perm_ask");
        assert!(
            !perm.contains("needs_attention_summary"),
            "the permission card must not repeat the count: {perm}"
        );
        let here = include_str!("harness_ui.rs");
        let card = here
            .split("fn harness_card(")
            .nth(1)
            .and_then(|s| s.split("fn remember_work_frame(").next())
            .expect("harness_card");
        assert!(!card.contains("needs_attention_summary"));
        assert!(
            !card.contains("\"Always\""),
            "the hard card has no Always pill: {card}"
        );
        let paint = here
            .split("fn paint_harness_cards(")
            .nth(1)
            .and_then(|s| s.split("struct CardText").next())
            .expect("paint_harness_cards");
        assert!(!paint.contains("needs_attention_summary"));
        assert!(paint.contains("primary: \"Approve\"") && paint.contains("secondary: \"Deny\""));
    }

    #[test]
    fn three_cards_count_as_three_decisions_once() {
        let (_pin, root) = pinned("decisions-waiting");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.harness.full_card_on = true;
        let _ = cabin.harness_precheck(ask("Run command", "rm -f draft.md"));
        cabin.perm_ask = Some(ask("grokhub-desktop__click", "click 4,5"));
        cabin.offer_full();
        assert!(cabin.harness.park.is_some());
        assert!(cabin.harness.full_card.is_some());
        assert!(cabin.perm_ask.is_some());
        assert_eq!(cabin.decisions_waiting(), 3);
        assert_eq!(
            crate::motion::needs_attention_summary(cabin.decisions_waiting()),
            "3 things need a decision"
        );
        let (texts, _) = paint_stack(&mut cabin, Vec::new(), 1400.0);
        let counts = texts.iter().filter(|t| t.contains("need a decision")).count();
        assert_eq!(counts, 1, "one count line, got {texts:?}");
        assert!(texts.iter().any(|t| t == "3 things need a decision. Everything else is on track."));
        assert!(texts.iter().any(|t| t == "Hard action"));
        assert!(texts.iter().any(|t| t == "Delete"));
        assert!(texts.iter().any(|t| t == "Desktop"));
        assert!(texts.iter().any(|t| t == "Grok wants permission"));
        assert!(texts.iter().any(|t| t == "Session access · Desktop control is on"));
        assert!(texts
            .iter()
            .any(|t| t == "Let Grok click and type without asking for this session?"));
        assert!(texts
            .iter()
            .any(|t| t == "Deletes, sends, money and credentials still ask."));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hard_and_perm_cards_share_one_width() {
        let (_pin, root) = pinned("card-width");
        let mut cabin = Cabin::quiet_for_test();
        let _ = cabin.harness_precheck(ask("Run command", "rm -f draft.md"));
        cabin.perm_ask = Some(ask("bash", "ls"));
        let (_, rects) = paint_stack(&mut cabin, Vec::new(), 1400.0);
        let hard = rects
            .iter()
            .find(|(r, s)| (s.width - 2.0).abs() < 0.01 && r.width() > 400.0)
            .map(|(r, _)| r.width());
        let perm = rects
            .iter()
            .find(|(r, s)| (s.width - 1.0).abs() < 0.01 && r.width() > 400.0 && r.height() > 40.0)
            .map(|(r, _)| r.width());
        let hard = hard.expect("hard frame");
        let perm = perm.expect("perm frame");
        assert_eq!(hard, perm);
        assert_eq!(hard, 520.0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hard_card_buttons_are_approve_and_deny() {
        let (_pin, root) = pinned("hard-buttons");
        let mut cabin = Cabin::quiet_for_test();
        let _ = cabin.harness_precheck(ask("Run command", "rm -f draft.md"));
        let (texts, _) = paint_stack(&mut cabin, Vec::new(), 900.0);
        let buttons: Vec<_> = texts
            .iter()
            .filter(|t| *t == "Approve" || *t == "Deny" || *t == "Always" || *t == "Allow")
            .cloned()
            .collect();
        assert_eq!(buttons, vec!["Approve".to_string(), "Deny".to_string()]);
        let _ = std::fs::remove_dir_all(root);
    }

    /// `grokhub-fake-acp`, built next to this test binary by
    /// `cargo test --workspace` (grokhub-acp's integration tests need it).
    fn fake_acp() -> std::path::PathBuf {
        let file = format!("grokhub-fake-acp{}", std::env::consts::EXE_SUFFIX);
        let exe = std::env::current_exe().expect("test exe");
        exe.ancestors()
            .take(4)
            .map(|dir| dir.join(&file))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("{file} not built: run `cargo build -p grokhub-acp --bin grokhub-fake-acp` (cargo test --workspace builds it)"))
    }

    /// A live ACP session on the fake agent under Always, prompted once, with
    /// the cabin mid-turn. The fake replays one tool frame per `env`.
    fn fake_turn(cabin: &mut Cabin, env: &[(&str, &str)]) {
        let opts = grokhub_acp::SpawnOpts {
            program: fake_acp(),
            args: vec![],
            cwd: std::env::temp_dir(),
            api_key: None,
            xai_api_key: None,
            always_approve: true,
            auto: false,
            session_mode: SessionMode::Chat,
            reasoning_effort: None,
            extra_env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            handshake_timeout: None,
            resume: None,
            skip_cabin_home: false,
            worktree: false,
        };
        let h = grokhub_acp::connect(opts).expect("connect fake acp");
        h.prompt("do it").expect("prompt");
        cabin.acp = Some(h);
        cabin.running = true;
        cabin.permission_mode = PermissionMode::AlwaysApprove;
    }

    /// Poll the real ACP event loop until the fake's turn is over.
    fn drive(cabin: &mut Cabin, until: impl Fn(&Cabin) -> bool) {
        let t = Instant::now();
        while !until(cabin) {
            assert!(t.elapsed() < Duration::from_secs(20), "fake ACP turn did not finish");
            cabin.poll_acp();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn path_d(root: &std::path::Path) -> Vec<hx::Span> {
        hx::read_spans(root, "session").unwrap().into_iter().filter(|s| s.path == "D").collect()
    }

    #[test]
    fn unasked_soft_computer_use_logs_a_path_d_allow_under_always() {
        let (_pin, root) = pinned("path-d-soft");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.harness.last_cu_frame = Some(frame_hash("data:image/jpeg;base64,AAAA"));
        fake_turn(
            &mut cabin,
            &[("FAKE_ACP_TOOL", "computer_click"), ("FAKE_ACP_RAW_INPUT", r#"{"x":5,"y":6}"#), ("FAKE_ACP_IMAGE", "QQ==")],
        );
        drive(&mut cabin, |c| !c.running);
        let spans = path_d(&root);
        assert_eq!(spans.len(), 1, "{spans:?}");
        let s = &spans[0];
        assert_eq!((s.tool.as_str(), s.decision.as_str(), s.approval_class.as_str()), ("computer_click", "allow", "soft"));
        assert_eq!(s.args_redacted, r#"{"x":5,"y":6}"#);
        assert_eq!(s.access, "supervised");
        assert_eq!(s.ui_changed, Some(true), "the frame differs from the last one");
        assert!(cabin.harness.park.is_none());
        assert!(!cabin.status.starts_with("Stopped"), "a soft step does not stop the turn: {}", cabin.status);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unasked_hard_computer_use_stops_the_turn_parks_and_approve_reruns_once() {
        let (_pin, root) = pinned("path-d-hard");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        fake_turn(
            &mut cabin,
            &[
                ("FAKE_ACP_TOOL", "computer_type"),
                ("FAKE_ACP_RAW_INPUT", r#"{"label":"Password","text":"hunter2222"}"#),
            ],
        );
        drive(&mut cabin, |c| c.harness.park.is_some());
        assert!(!cabin.running, "the turn stops at once");
        let park = cabin.harness.park.clone().unwrap();
        assert_eq!(park.source, ParkSource::Unasked);
        assert_eq!(park.class, HardClass::Credentials);
        assert_eq!(park.path, "D");
        assert_eq!(park.action, "computer_type: type into a credential field (value hidden)");
        assert_eq!(cabin.status, "Grok tried to type into a credential field without asking");
        let spans = path_d(&root);
        let rows: Vec<_> = spans.iter().map(|s| (s.decision.as_str(), s.approval_class.as_str())).collect();
        assert_eq!(rows, vec![("allow", "credentials"), ("deny", "credentials"), ("park", "credentials")]);
        assert_eq!(spans[1].result, "stopped: Grok Build tried a hard step without asking");
        let flagged = hx::approval_gate_violation(&spans);
        assert_eq!(flagged.len(), 1, "the raw step that ran with no approve is flagged");
        assert_eq!(flagged[0].tool, "computer_type");
        let all = std::fs::read_to_string(hx::span_path(&root, "session")).unwrap();
        assert!(!all.contains("hunter2222"), "the typed value never reaches a span");

        // The card: no Always, Enter leaves it.
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let (texts, _) = paint_stack(&mut cabin, vec![enter], 900.0);
        assert!(texts.iter().any(|t| t == "Grok tried to type into a credential field without asking"), "{texts:?}");
        assert!(texts.iter().any(|t| t == UNASKED_NOTE), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Approve") && texts.iter().any(|t| t == "Deny"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("Always")), "a hard card has no Always: {texts:?}");
        assert!(cabin.harness.park.is_some(), "Enter must not approve");

        // Approve re-runs that one step once through a one-shot ACP Ask.
        cabin.resolve_hard_park(true, "");
        let shot = cabin.harness.oneshot.clone().expect("one-shot");
        assert_eq!(shot.action.as_deref(), Some(park.action.as_str()));
        assert_eq!(shot.restore, PermissionMode::AlwaysApprove);
        assert_eq!(cabin.permission_mode, PermissionMode::Ask);
        let card = ToolCard {
            id: "tool-2".into(),
            title: "computer_type".into(),
            kind: "other".into(),
            status: "completed".into(),
            detail: String::new(),
            diff: String::new(),
            image_data_url: None,
            raw_input: r#"{"label":"Password"}"#.into(),
        };
        cabin.running = true;
        assert!(!cabin.harness_watch_cu(&card, true), "the approved step runs once");
        assert!(cabin.harness.park.is_none());
        let again = ToolCard { id: "tool-3".into(), ..card };
        assert!(cabin.harness_watch_cu(&again, true), "a second run parks again");
        let spans = path_d(&root);
        let approved: Vec<_> = spans.iter().filter(|s| s.hard_approved).map(|s| s.decision.as_str()).collect();
        assert_eq!(approved, vec!["approve", "allow"]);
        let raw_runs: Vec<_> = spans
            .iter()
            .filter(|s| s.decision == "allow" && !s.hard_approved)
            .map(|s| s.claim.as_str())
            .collect();
        assert_eq!(raw_runs, vec!["Grok Build ran a hard step with no ask"; 2], "each unapproved run keeps its raw span");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_generic_titled_keyboard_step_is_watched_and_its_approve_matches_the_rerun_ask() {
        let (_pin, root) = pinned("path-d-rerun");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        cabin.running = true;
        // The title says nothing; the stream's toolName says keyboard.
        let card = card("tool-1", "completed", r#"{"tool":"keyboard_type","id":"login-password"}"#);
        assert!(cabin.harness_watch_cu(&card, false), "a keyboard step is path D computer use");
        let park = cabin.harness.park.clone().expect("hard card");
        assert_eq!((park.source.clone(), park.class), (ParkSource::Unasked, HardClass::Credentials));

        cabin.resolve_hard_park(true, "");
        assert_eq!(cabin.permission_mode, PermissionMode::Ask);
        // The re-run comes back as Grok's own path B ask, in its own words.
        let rerun = ask("keyboard_type", "type into the password field");
        assert!(matches!(
            hx::decide(Step::Ask { title: &rerun.title, action: &rerun.action }),
            GateOutcome::Park { hard: Some(HardClass::Credentials), .. }
        ));
        assert!(cabin.harness_precheck(rerun.clone()).is_some(), "the approved step reaches Grok's Allow once");
        assert!(cabin.harness.park.is_none());
        assert!(cabin.harness_precheck(rerun).is_none(), "a second ask parks again");
        assert_eq!(cabin.harness.park.as_ref().map(|p| p.class), Some(HardClass::Credentials));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unasked_computer_use_under_readonly_stops_with_a_deny_and_no_card() {
        let (_pin, root) = pinned("path-d-readonly");
        let mut cabin = Cabin::quiet_for_test();
        assert_eq!(cabin.access_mode(), AccessMode::Readonly);
        fake_turn(
            &mut cabin,
            &[("FAKE_ACP_TOOL", "computer_click"), ("FAKE_ACP_RAW_INPUT", r#"{"x":1,"y":2}"#), ("FAKE_ACP_BLOCK_UNTIL_CANCEL", "1")],
        );
        drive(&mut cabin, |c| !c.running);
        assert!(cabin.harness.park.is_none(), "Readonly refuses with no card");
        assert_eq!(cabin.hard_waiting(), 0);
        let spans = path_d(&root);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert_eq!((spans[0].tool.as_str(), spans[0].decision.as_str()), ("computer_click", "deny"));
        let reason = "Access is Readonly — Grok Build's own computer use ran with desktop control off";
        assert_eq!(spans[0].result, reason);
        assert_eq!(cabin.status, format!("Stopped: {reason}"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn path_b_asks_are_unchanged_and_their_frames_are_not_rechecked() {
        let (_pin, root) = pinned("path-d-asked");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.running = true;
        // A soft built-in CU ask still goes to Grok Build's own card.
        let soft = ask("computer_click", "click 4,5");
        assert_eq!(cabin.harness_precheck(soft.clone()), Some(soft));
        // A hard one still parks a hard card on path B.
        assert_eq!(cabin.harness_precheck(ask("computer_type", "into the Password field")), None);
        assert_eq!(cabin.harness.park.as_ref().map(|p| (p.class, p.path)), Some((HardClass::Credentials, "B")));
        // Its frame (tool call `t`) was decided on path B: the watchdog leaves it.
        let card = ToolCard {
            id: "t".into(),
            title: "computer_type".into(),
            kind: "other".into(),
            status: "completed".into(),
            detail: String::new(),
            diff: String::new(),
            image_data_url: None,
            raw_input: r#"{"label":"Password"}"#.into(),
        };
        assert!(!cabin.harness_watch_cu(&card, true));
        assert!(cabin.running);
        assert!(path_d(&root).is_empty());
        // A pending ACP frame waits for the ask that may follow it.
        let pending = ToolCard { id: "t2".into(), status: "pending".into(), ..card };
        assert!(!cabin.harness_watch_cu(&pending, true));
        assert!(path_d(&root).is_empty() && cabin.harness.park.as_ref().is_some_and(|p| p.path == "B"));
        // Browser and path A frames are not path D's.
        for title in ["browser_tab", "grokhub-desktop__click", "Read `src/click.rs`"] {
            let other = ToolCard { id: title.into(), title: title.into(), status: "completed".into(), ..pending.clone() };
            assert!(!cabin.harness_watch_cu(&other, false), "{title}");
        }
        assert!(path_d(&root).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn enter_leaves_a_hard_park_and_esc_denies_it() {
        let (_pin, root) = pinned("hard-keys");
        let mut cabin = Cabin::quiet_for_test();
        let _ = cabin.harness_precheck(ask("Run command", "rm -f draft.md"));
        assert!(cabin.harness.park.is_some());
        let before = hx::read_spans(&root, "session").unwrap();
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = paint_stack(&mut cabin, vec![enter], 900.0);
        assert!(cabin.harness.park.is_some(), "Enter must not approve a hard card");
        let after_enter = hx::read_spans(&root, "session").unwrap();
        assert_eq!(after_enter.len(), before.len(), "Enter writes no span");
        assert_eq!(after_enter.last().unwrap().decision, "park");
        let esc = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = paint_stack(&mut cabin, vec![esc], 900.0);
        assert!(cabin.harness.park.is_none());
        let spans = hx::read_spans(&root, "session").unwrap();
        assert_eq!(spans.last().unwrap().decision, "deny");
        assert_eq!(spans.last().unwrap().result, "Jeremy denied (Esc)");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn grant_full_stays_hidden_and_supervised_without_the_flag() {
        let (_pin, root) = pinned("grant-full-off");
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.harness.full_card_on);
        cabin.cfg.desktop_control = true;
        let soft = ask("grokhub-desktop__click", "click 1,2");
        assert_eq!(cabin.harness_precheck(soft.clone()), Some(soft));
        assert!(cabin.harness.full_card.is_none());
        assert_eq!(cabin.access_mode(), AccessMode::Supervised);
        cabin.offer_full();
        assert!(cabin.harness.full_card.is_none());
        assert_eq!(cabin.access_mode(), AccessMode::Supervised);
        let _ = std::fs::remove_dir_all(root);
    }

    /// SY-01: on the empty chat the composer column is centered and justified.
    /// The /sync hard card must still be the #520 chat-column card: left edge on
    /// the column, min(column, 520) wide, every line left-aligned and ragged
    /// right, and the buttons on the left.
    #[test]
    fn sync_card_is_the_left_aligned_chat_column_card_on_the_empty_chat() {
        let (_pin, root) = pinned("sync-card-left");
        let mut cabin = Cabin::quiet_for_test();
        cabin.park_egress(hx::HUB_DEST, HardClass::Send);
        assert_eq!(cabin.hard_waiting(), 1);
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1200.0))),
            ..Default::default()
        };
        let column = egui::Rect::from_min_size(egui::pos2(300.0, 100.0), egui::vec2(700.0, 900.0));
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                // The same nesting as the empty chat's composer column.
                ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                        ui.set_width(column.width());
                        cabin.paint_approval_stack(ui);
                    });
                });
            });
        });
        let mut texts: Vec<(String, f32, bool, egui::Align)> = Vec::new();
        let mut frames = Vec::new();
        fn walk(
            shape: &egui::Shape,
            texts: &mut Vec<(String, f32, bool, egui::Align)>,
            frames: &mut Vec<(egui::Rect, egui::Stroke)>,
        ) {
            match shape {
                egui::Shape::Text(t) => texts.push((
                    t.galley.text().to_string(),
                    t.pos.x + t.galley.rect.min.x,
                    t.galley.job.justify,
                    t.galley.job.halign,
                )),
                egui::Shape::Rect(r) => frames.push((r.rect, r.stroke)),
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, texts, frames)),
                _ => {}
            }
        }
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts, &mut frames);
        }
        let frame = frames
            .iter()
            .find(|(r, s)| (s.width - 2.0).abs() < 0.01 && r.width() > 400.0)
            .map(|(r, _)| *r)
            .expect("hard frame");
        assert_eq!(frame.width(), approval_card_width(column.width()));
        assert!((frame.left() - column.left()).abs() < 1.0, "card sits on the column's left edge: {frame:?}");
        let x_of = |want: &str| {
            texts
                .iter()
                .find(|t| t.0 == want)
                .unwrap_or_else(|| panic!("{want} not painted: {texts:?}"))
                .clone()
        };
        let lines = [
            HARD_EYEBROW,
            "Send",
            super::super::privacy_ui::HUB_CARD_ACTION,
            super::super::privacy_ui::HUB_CARD_NOTE,
        ];
        let left = frame.left() + 2.0 + APPROVAL_CARD_MARGIN;
        for want in lines {
            let (_, x, justify, halign) = x_of(want);
            assert!((x - left).abs() < 1.0, "{want} starts at the card's left inset ({left}), got {x}");
            assert!(!justify, "{want} is ragged right, not justified");
            assert_eq!(halign, egui::Align::LEFT, "{want}");
        }
        let (_, approve_x, _, _) = x_of("Approve");
        let (_, deny_x, _, _) = x_of("Deny");
        assert!(approve_x < left + 40.0 && approve_x < deny_x, "buttons start on the left: {approve_x} {deny_x}");
        let (_, count_x, _, _) = x_of("1 thing needs a decision. Everything else is on track.");
        assert!((count_x - column.left()).abs() < 1.0, "the count line is left-aligned too: {count_x}");
        let _ = std::fs::remove_dir_all(root);
    }

    fn paint_stack(
        cabin: &mut Cabin,
        events: Vec<egui::Event>,
        width: f32,
    ) -> (Vec<String>, Vec<(egui::Rect, egui::Stroke)>) {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 1600.0),
            )),
            events,
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.paint_approval_stack(ui));
        });
        let mut texts = Vec::new();
        let mut rects = Vec::new();
        for clipped in &out.shapes {
            collect_paint(&clipped.shape, &mut texts, &mut rects);
        }
        (texts, rects)
    }

    fn collect_paint(
        shape: &egui::Shape,
        texts: &mut Vec<String>,
        rects: &mut Vec<(egui::Rect, egui::Stroke)>,
    ) {
        match shape {
            egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
            egui::Shape::Rect(r) => rects.push((r.rect, r.stroke)),
            egui::Shape::Vec(v) => {
                for child in v {
                    collect_paint(child, texts, rects);
                }
            }
            _ => {}
        }
    }
}
