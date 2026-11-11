//! The agent loop behind chat (cross-platform recode, Phase 3). Each
//! conversation gets a worker thread that owns a `grokhub_agent::NativeEngine`,
//! the same loop the egui cabin runs. Tool calls go through the cabin's
//! permission gate in Ask mode: a risky step parks until the UI answers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use grokhub_agent::{
    AuthKind, CancelToken, DesktopOps, ElicitInbox, ElicitNote, Engine, EngineParts, Gate,
    HaltCheck, ModelClient, NativeEngine, PermAnswer, PermMode, PermitInbox, PermitNote,
    SteerQueue,
};
use grokhub_core::wire::{AcpEvent, ElicitChoice};
use serde::Serialize;

/// The cabin's attended chat gate in the mode Settings picked. In Ask, tools
/// that change things ask first; Auto and Always approve more, but hard-class
/// steps (delete, send, money, credentials) still ask in every mode.
/// `desktop` is Settings → Let Grok control the desktop: off, the desktop
/// tools are refused; on, they go through the same hard gate. Plan and btw
/// are read-only, like the cabin's.
pub fn shell_gate(mode: PermMode, desktop: bool, session: SessionMode) -> Gate {
    Gate {
        mode,
        readonly_session: session != SessionMode::Chat,
        attended: true,
        desktop,
    }
}

/// How one message runs. The app picks Chat or Plan for each message
/// (`turns::pick_mode`); btw is a side answer's read-only run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionMode {
    /// Grok can change files and run tools, through the approvals.
    #[default]
    Chat,
    /// Read-only until you approve the plan Grok writes.
    Plan,
    /// A look-only side ask: Grok can read but changes nothing.
    Btw,
}

impl SessionMode {
    /// The name the page and the saved follow-ups use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Plan => "plan",
            Self::Btw => "btw",
        }
    }
}

/// The system prompt the cabin's native chat uses, with the desktop line
/// only while desktop control is on. `voice` is the personality's block
/// (`crate::personality::voice`). The default voice carries its values rules.
pub fn system_prompt(workspace: &std::path::Path, desktop: bool, voice: &str) -> String {
    system_prompt_for(workspace, desktop, voice, "", false)
}

/// `chips` adds the rule that ends each reply with its chips block
/// (`grokhub_core::CHIP_TRAILER_RULE`) to the cached rules, as the cabin does.
pub fn system_prompt_for(
    workspace: &std::path::Path,
    desktop: bool,
    voice: &str,
    conversation_id: &str,
    chips: bool,
) -> String {
    let volatile = volatile_tail(voice, &grokhub_agent::perm::config_dir(), conversation_id);
    let mut rules = grokhub_core::cabin_rules::cabin_static_rules(desktop);
    if chips {
        rules.push_str("\n\n");
        rules.push_str(grokhub_core::CHIP_TRAILER_RULE);
    }
    grokhub_agent::system_prompt_parts(&rules, &volatile, workspace)
}

/// Voice, then the capped SOUL and USER blocks, then open intentions, then the
/// safety closer. The closer is present only when that tail has something to outrank.
fn volatile_tail(voice: &str, config_dir: &std::path::Path, conversation_id: &str) -> String {
    let mut parts = Vec::new();
    if !voice.trim().is_empty() {
        parts.push(voice.to_string());
    }
    let identity = grokhub_agent::identity_blocks(config_dir);
    if !identity.is_empty() {
        parts.push(identity);
    }
    let intentions =
        grokhub_core::intentions_block(&grokhub_core::load_intentions(config_dir, conversation_id));
    if !intentions.is_empty() {
        parts.push(intentions);
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.push(grokhub_core::talk::VOLATILE_SAFETY_CLOSER.to_string());
    parts.join("\n\n")
}

/// The folder the agent works in: the one picked in Settings, or the
/// cabin's default session folder.
pub fn workspace_for(picked: &str) -> PathBuf {
    let home = std::env::var("HOME").ok();
    let profile = std::env::var("USERPROFILE").ok();
    PathBuf::from(grokhub_core::cabin_session_cwd(
        picked,
        cfg!(windows),
        home.as_deref(),
        profile.as_deref(),
    ))
}

/// What the UI renders. Engine events the shell does not show yet map to `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UiEvent {
    Text {
        text: String,
    },
    Tool {
        id: String,
        title: String,
        status: String,
        detail: String,
    },
    Permission {
        id: String,
        title: String,
        action: String,
        reason: String,
    },
    /// A permission card answered away from the page, such as Deny on its
    /// notification. `answer` is a word `parse_answer` reads.
    Answered {
        id: String,
        answer: String,
    },
    Error {
        message: String,
    },
    Done {
        stop: String,
    },
    /// One-tap follow-ups for the reply that just ended; sent only when one
    /// names a real action (grokhub_core::chips::reply_suggestions).
    Suggestions {
        items: Vec<crate::chips::Chip>,
    },
    /// A game goal the reply noticed, to Track or leave with Not now. Sent
    /// once per goal per chat, before `Done`.
    Goal {
        id: String,
        title: String,
    },
    /// The plan Grok wrote, before it asks to leave plan mode.
    Plan {
        text: String,
    },
    /// Grok or a connector needs an answer: a plan to approve, a question, or
    /// a connector's form or link. `server` is "Plan" for a plan.
    Elicit {
        id: String,
        server: String,
        message: String,
        /// "form" or "url".
        mode: String,
        url: String,
        field_name: Option<String>,
        field_title: String,
        secret: bool,
        /// Answers to tap for a question from Grok; the recommended one first.
        options: Vec<ElicitChoice>,
    },
}

/// The follow-ups card for a finished reply, if this turn earns one.
pub fn suggestions_for(ups: &crate::chips::FollowUps) -> Option<UiEvent> {
    let items = crate::chips::wire_follow_ups(ups);
    (!items.is_empty()).then_some(UiEvent::Suggestions { items })
}

pub fn ui_event(event: AcpEvent) -> Option<UiEvent> {
    Some(match event {
        AcpEvent::Text(text) => UiEvent::Text { text },
        AcpEvent::Tool(card) => UiEvent::Tool {
            id: card.id,
            title: card.title,
            status: card.status,
            detail: card.detail,
        },
        AcpEvent::Permission(ask) => UiEvent::Permission {
            id: ask.tool_call_id,
            title: ask.title,
            action: ask.action,
            reason: ask.reason,
        },
        AcpEvent::Err(message) => UiEvent::Error { message },
        AcpEvent::Done { stop_reason } => UiEvent::Done { stop: stop_reason },
        AcpEvent::Plan(text) => UiEvent::Plan { text },
        // The shell has no findings card yet: the card's plain list follows
        // the reply, so it never shows raw JSON or nothing (LO-51).
        AcpEvent::Findings(body) => UiEvent::Text {
            text: format!("\n\n{}", body.trim()),
        },
        AcpEvent::Elicit(ask) => UiEvent::Elicit {
            id: ask.tool_call_id,
            server: ask.server_name,
            message: ask.message,
            mode: ask.mode,
            url: ask.url,
            field_name: ask.field_name,
            field_title: ask.field_title,
            secret: ask.secret,
            options: ask.options,
        },
        _ => return None,
    })
}

pub fn parse_answer(answer: &str) -> Result<PermAnswer, String> {
    match answer {
        "allow" => Ok(PermAnswer::Allow),
        "always" => Ok(PermAnswer::Always),
        "deny" => Ok(PermAnswer::Deny),
        other => Err(format!("unknown answer: {other}")),
    }
}

/// The word [`parse_answer`] reads for `answer`; a cancel reads as deny,
/// since the step did not run. Notifications are desktop only.
#[cfg(desktop)]
pub fn answer_word(answer: PermAnswer) -> &'static str {
    match answer {
        PermAnswer::Allow => "allow",
        PermAnswer::Always => "always",
        PermAnswer::Deny | PermAnswer::Cancel => "deny",
    }
}

/// The credential and folder for one prompt. Read fresh for every prompt, so
/// a Settings change applies to the next message. It names no model and no
/// effort: the router picks both for every step (`route::live::stream_routed`).
pub struct Route {
    pub client: Arc<dyn ModelClient + Send + Sync>,
    pub auth_kind: AuthKind,
    pub workspace: PathBuf,
    /// Settings' approvals. Read on every prompt, so a change applies to the
    /// next message in an open chat.
    pub mode: PermMode,
    /// Settings → Let Grok control the desktop, read the same way.
    pub desktop: bool,
    /// The composer's chip for this message.
    pub session: SessionMode,
    /// Settings → Personality's block for the system prompt, or empty.
    pub voice: String,
    /// The chat's voice settings for the agent's per-turn voice flow (dynamic
    /// chat, quips, heat), registered under the chat's id before the turn.
    /// `None` leaves whatever the chat had.
    pub voice_session: Option<grokhub_agent::voice_session::VoiceConfig>,
    /// Settings → Behavior → Suggestions: follow-ups after each reply.
    pub suggestions: bool,
    /// A session the Code tab's agent window started: it plans first and
    /// hands its tasks to helpers.
    pub lead: bool,
}

struct Job {
    text: String,
    /// Files from the composer (`crate::attach`), in order.
    attachments: Vec<crate::attach::Staged>,
    route: Route,
    /// When the person sent it: a Halt after this stops the turn.
    asked_ms: u64,
    /// The desktop episode this message belongs to, or `None` for a plain turn.
    episode: Option<grokhub_agent::EpisodeSeed>,
    emit: Box<dyn FnMut(UiEvent) + Send>,
    done: Sender<Result<String, String>>,
}

/// The stop reason of a turn Halt ended.
pub const HALTED: &str = "halted";
/// What the window sends to go on with a desktop session after an Allow or
/// a Pause (the cabin's resume text). It is not a new goal step.
pub const RESUME_TEXT: &str = "Continue";

pub struct NeverHalted;

impl HaltCheck for NeverHalted {
    fn halted(&self) -> bool {
        false
    }
}

/// One conversation. Dropping it ends the worker once its prompt returns.
pub struct Session {
    id: String,
    jobs: Sender<Job>,
    permits: Sender<PermitNote>,
    elicits: Sender<ElicitNote>,
    cancel: CancelToken,
    /// Words for the running reply, read at its next step (Steer now).
    steer: SteerQueue,
    /// A prompt is running.
    busy: AtomicBool,
    /// Halt stopped the running prompt; its turn ends as `halted`.
    halted: Arc<AtomicBool>,
    config_dir: PathBuf,
    /// The person armed a supervised desktop session: messages with desktop
    /// control on run as an episode until it ends. Off, they run as plain
    /// turns with the hard gate asking per step.
    armed: AtomicBool,
    /// The open desktop episode on this chat.
    episode: Mutex<Option<crate::episode::Open>>,
    /// The latest screenshot, for the episode panel's thumbnail.
    frames: crate::episode::FrameSlot,
    /// Pause for the running episode turn: the kernel spends it and returns
    /// before its next step, with the episode, its goal and its cards kept.
    pause: Arc<AtomicBool>,
    /// The running turn was paused, so it ends as `cancelled` for the page
    /// (nothing queued behind it sends).
    paused_turn: Arc<AtomicBool>,
    /// An Allow with no reply running, or a Pause: the next message, when it
    /// is [`RESUME_TEXT`], goes on with the goal step kept.
    resume_due: AtomicBool,
    /// An Allow landed while a reply ran, after its loop's last look: when
    /// the reply ended the window was owed the resume ([`Self::take_resume_owed`]).
    resume_owed: AtomicBool,
    /// Halt was pressed on this chat since the app started.
    halt_seen: AtomicBool,
    /// Settings → Let Grok control the desktop, live: off refuses every
    /// later desktop call of a turn that started with it on.
    desk_on: Arc<AtomicBool>,
    /// The running prompt was sent with desktop control on.
    desk_turn: AtomicBool,
}

impl Session {
    /// `config_dir` pins the GrokHub config folder for the worker, where the
    /// engine keeps session files and the permission rules it learns.
    pub fn spawn(conversation_id: &str, config_dir: PathBuf) -> Self {
        Self::spawn_with(conversation_id, config_dir, native_desktop)
    }

    /// A side answer beside `parent`'s running reply: it starts from the
    /// parent's saved turns and works under its own `side_id`, so the
    /// parent's worker, history and inboxes are never touched.
    pub fn spawn_side(side_id: &str, parent_id: &str, config_dir: PathBuf) -> Self {
        Self::spawn_from(side_id, parent_id, config_dir, native_desktop)
    }

    /// [`Session::spawn`] with the desktop the worker builds; tests pass a fake.
    pub fn spawn_with(
        conversation_id: &str,
        config_dir: PathBuf,
        desktop: fn() -> Box<dyn DesktopOps>,
    ) -> Self {
        Self::spawn_from(conversation_id, conversation_id, config_dir, desktop)
    }

    /// A worker for `conversation_id` that resumes from `resume_id`'s saved turns.
    pub fn spawn_from(
        conversation_id: &str,
        resume_id: &str,
        config_dir: PathBuf,
        desktop: fn() -> Box<dyn DesktopOps>,
    ) -> Self {
        let (jobs, rx) = channel::<Job>();
        let (permits, inbox) = PermitInbox::pair();
        // Plan approvals, Grok's questions and connector forms wait here.
        let (elicits, answers) = ElicitInbox::pair();
        grokhub_agent::attach_elicit(conversation_id, answers);
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        let halted = Arc::new(AtomicBool::new(false));
        let steer = SteerQueue::new();
        let frames = crate::episode::FrameSlot::default();
        let worker_dir = config_dir.clone();
        let pause = Arc::new(AtomicBool::new(false));
        let paused_turn = Arc::new(AtomicBool::new(false));
        let desk_on = Arc::new(AtomicBool::new(true));
        let worker = Worker {
            pause: Arc::clone(&pause),
            paused_turn: Arc::clone(&paused_turn),
            desk_on: Arc::clone(&desk_on),
            frames: frames.clone(),
            id: conversation_id.to_string(),
            resume: resume_id.to_string(),
            config_dir,
            desktop,
            cancel: worker_cancel,
            steer: steer.clone(),
            halted: Arc::clone(&halted),
        };
        std::thread::spawn(move || serve(worker, inbox, rx));
        Self {
            id: conversation_id.to_string(),
            jobs,
            permits,
            elicits,
            cancel,
            steer,
            busy: AtomicBool::new(false),
            halted,
            config_dir: worker_dir,
            armed: AtomicBool::new(false),
            episode: Mutex::new(None),
            frames,
            pause,
            paused_turn,
            resume_due: AtomicBool::new(false),
            resume_owed: AtomicBool::new(false),
            halt_seen: AtomicBool::new(false),
            desk_on,
            desk_turn: AtomicBool::new(false),
        }
    }

    /// Arms (or disarms) the supervised desktop session: the next messages
    /// sent with desktop control on run as an episode. An open episode stays
    /// open either way.
    pub fn arm_episode(&self, on: bool) {
        self.armed.store(on, Ordering::SeqCst);
    }

    pub fn episode_armed(&self) -> bool {
        self.armed.load(Ordering::SeqCst)
    }

    /// The open desktop episode on this chat, if any.
    pub fn episode(&self) -> Option<crate::episode::Open> {
        self.episode
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn frame(&self) -> Option<String> {
        self.frames.get()
    }

    /// A message with desktop control on opens the chat's episode, or goes on
    /// in the open one (a paused one runs again). `resume`: the message is
    /// the window's [`RESUME_TEXT`], not a new goal step.
    fn open_episode(&self, resume: bool) -> grokhub_agent::EpisodeSeed {
        let now = grokhub_core::now_ms();
        let mut slot = self.episode.lock().unwrap_or_else(|e| e.into_inner());
        let open = slot.get_or_insert_with(|| crate::episode::Open {
            id: grokhub_agent::episode::new_episode_id(now),
            started_ms: now,
            turn: 0,
            paused: false,
            cua: crate::episode::cua_in_use(&self.config_dir),
        });
        open.paused = false;
        open.turn += 1;
        grokhub_agent::EpisodeSeed {
            id: open.id.clone(),
            chat_id: self.id.clone(),
            turn: open.turn,
            resume,
            config_dir: self.config_dir.clone(),
            held: Vec::new(),
            access: grokhub_agent::harness::AccessMode::Supervised,
        }
    }

    /// OK or Deny on a card the open episode posted. True when the window
    /// should send [`RESUME_TEXT`] now: an Allow with no reply running, which
    /// no loop would pick up until the next message. An Allow while a reply
    /// runs answers false: its loop takes it, or the end of the reply owes
    /// the resume ([`Self::take_resume_owed`]). After Halt an Allow is
    /// refused; a Deny still answers.
    pub fn answer_episode(&self, park_id: &str, approve: bool) -> Result<bool, String> {
        // Held over the answer and the look at `busy`, as the end of
        // `prompt` holds it: exactly one of the two says the resume is due.
        let slot = self.episode.lock().unwrap_or_else(|e| e.into_inner());
        let open = slot
            .as_ref()
            .ok_or_else(|| "No desktop session is open on this chat.".to_string())?;
        if approve && self.halt_seen.load(Ordering::SeqCst) {
            return Err((*grokhub_core::desktop_mcp::HALT_MSG).to_string());
        }
        crate::episode::answer(&self.config_dir, &open.id, park_id, approve)?;
        let due = approve && !self.busy.load(Ordering::SeqCst);
        if due {
            self.resume_due.store(true, Ordering::SeqCst);
        }
        Ok(due)
    }

    /// True once after a reply ended with an Allow nobody acted on (it landed
    /// after the loop's last look): the window sends [`RESUME_TEXT`] now.
    pub fn take_resume_owed(&self) -> bool {
        self.resume_owed.swap(false, Ordering::SeqCst)
    }

    /// Settings turned desktop control off: a running turn's later desktop
    /// calls are refused, the session ends (a running one stops, an idle one
    /// is closed and its cards denied) and the chat is disarmed, so nothing
    /// comes back when the switch does. True when a session was open.
    pub fn desktop_off(&self) -> bool {
        self.desk_on.store(false, Ordering::SeqCst);
        let stopped = self.stop_episode();
        self.armed.store(false, Ordering::SeqCst);
        stopped
    }

    /// Settings turned desktop control back on.
    pub fn desktop_on(&self) {
        self.desk_on.store(true, Ordering::SeqCst);
    }

    /// Whether this chat drives the desktop right now: a supervised session
    /// is armed or open on it, or its running reply was sent with desktop
    /// control on. A paired browser can't steer or answer it meanwhile.
    pub fn on_desktop(&self) -> bool {
        self.episode_armed()
            || self.episode().is_some()
            || (self.busy.load(Ordering::SeqCst) && self.desk_turn.load(Ordering::SeqCst))
    }

    /// Ends the open episode while no loop runs it: its end is written,
    /// every card it left (an allowed one too) is taken back, and the chat
    /// is disarmed. False when none is open.
    fn end_idle_episode(&self, why: &str) -> bool {
        let open = self
            .episode
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.armed.store(false, Ordering::SeqCst);
        self.resume_due.store(false, Ordering::SeqCst);
        let Some(open) = open else {
            return false;
        };
        crate::episode::write_end(&self.config_dir, &self.id, &open.id, why);
        true
    }

    /// Pause: the running step loop stops after the step it is on, the
    /// episode stays open with its goal and its cards, and the next message
    /// goes on with it. Nothing is cancelled or denied. False when no
    /// episode runs.
    pub fn pause_episode(&self) -> bool {
        let mut slot = self.episode.lock().unwrap_or_else(|e| e.into_inner());
        let Some(open) = slot.as_mut() else {
            return false;
        };
        if !self.busy.load(Ordering::SeqCst) {
            return false;
        }
        open.paused = true;
        self.paused_turn.store(true, Ordering::SeqCst);
        self.resume_due.store(true, Ordering::SeqCst);
        self.pause.store(true, Ordering::SeqCst);
        true
    }

    /// Stop: ends the episode. A running loop ends it itself (every open
    /// card is denied); with none running the end is written here.
    pub fn stop_episode(&self) -> bool {
        if self.episode().is_none() {
            return false;
        }
        if self.busy.load(Ordering::SeqCst) {
            if let Some(o) = self
                .episode
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_mut()
            {
                o.paused = false;
            }
            // Stop after a Pause in the same turn: the Stop wins.
            self.paused_turn.store(false, Ordering::SeqCst);
            self.cancel.cancel();
        } else {
            self.end_idle_episode("stop");
        }
        true
    }

    /// After a prompt: a stop that ends the episode closes it; a Pause's
    /// cancel keeps it open. `ran`: the prompt was a turn of the episode.
    /// Its `cancelled` is the Pause's only while `paused_turn` still stands:
    /// the worker clears it when a real Stop ended the turn, and then the
    /// engine has ended the episode, so the slot closes with it.
    fn settle_episode(&self, slot: &mut Option<crate::episode::Open>, stop: &str, ran: bool) {
        let paused = slot.as_ref().is_some_and(|o| o.paused)
            && (!ran || self.paused_turn.load(Ordering::SeqCst));
        if crate::episode::ends(stop) && !(paused && stop == "cancelled") {
            *slot = None;
            // The session is over: the next message is a plain turn again,
            // and a "Continue" is not a resume of anything.
            self.armed.store(false, Ordering::SeqCst);
            self.resume_due.store(false, Ordering::SeqCst);
        }
    }

    /// Runs one prompt and waits for it. Blocks while a tool waits for an
    /// answer, so call it off the UI thread.
    pub fn prompt(
        &self,
        text: &str,
        attachments: Vec<crate::attach::Staged>,
        route: Route,
        emit: Box<dyn FnMut(UiEvent) + Send>,
    ) -> Result<String, String> {
        if text.trim().is_empty() && attachments.is_empty() {
            return Err("Type a message first.".into());
        }
        // The resume is the window's own text; anything else the person
        // typed is a new goal step, and either way the resume is spent.
        let resume = self.resume_due.swap(false, Ordering::SeqCst) && text.trim() == RESUME_TEXT;
        let seed = (route.desktop && (self.episode_armed() || self.episode().is_some()))
            .then(|| self.open_episode(resume));
        let in_episode = seed.is_some();
        let (done, wait) = channel();
        self.resume_owed.store(false, Ordering::SeqCst);
        self.halted.store(false, Ordering::SeqCst);
        self.pause.store(false, Ordering::SeqCst);
        self.paused_turn.store(false, Ordering::SeqCst);
        self.desk_turn.store(route.desktop, Ordering::SeqCst);
        self.busy.store(true, Ordering::SeqCst);
        let ran = self
            .jobs
            .send(Job {
                text: text.to_string(),
                attachments,
                route,
                asked_ms: grokhub_core::now_ms(),
                episode: seed,
                emit,
                done,
            })
            .map_err(|_| "The chat worker stopped.".to_string())
            .and_then(|()| {
                wait.recv()
                    .map_err(|_| "The chat worker stopped.".to_string())?
            });
        // "No reply runs" and the look for an Allow no loop took are one
        // step under the episode lock, as an answer is (`answer_episode`):
        // an Allow lands wholly before (seen here) or wholly after (its own
        // return says the resume is due).
        let mut slot = self.episode.lock().unwrap_or_else(|e| e.into_inner());
        self.busy.store(false, Ordering::SeqCst);
        self.desk_turn.store(false, Ordering::SeqCst);
        if let Ok(stop) = &ran {
            self.settle_episode(&mut slot, stop, in_episode);
        }
        // A paused session waits for the person; anything else goes on.
        let owed = slot.as_ref().is_some_and(|open| {
            !open.paused && crate::episode::allowed(&self.config_dir, &open.id)
        });
        if owed {
            self.resume_due.store(true, Ordering::SeqCst);
            self.resume_owed.store(true, Ordering::SeqCst);
        }
        drop(slot);
        ran
    }

    pub fn answer(&self, call_id: &str, answer: PermAnswer) {
        let _ = self.permits.send(PermitNote {
            id: call_id.to_string(),
            answer,
        });
    }

    /// Answers a plan, a question or a connector form: "accept" with the
    /// typed value under `field`, "decline" or "cancel".
    pub fn answer_elicit(&self, id: &str, action: &str, field: Option<&str>, value: &str) {
        let content = match (action, field) {
            ("accept", Some(field)) => Some(serde_json::json!({ field: value })),
            _ => None,
        };
        let _ = self.elicits.send(ElicitNote {
            id: id.to_string(),
            action: action.to_string(),
            content,
        });
    }

    /// Steer now: hands `text` to the running reply, which reads it at its
    /// next step (after the tool that is running, or before it ends). False,
    /// and nothing kept, when no reply runs or the text is empty.
    pub fn steer(&self, text: &str) -> bool {
        if text.trim().is_empty() || !self.busy.load(Ordering::SeqCst) {
            return false;
        }
        self.steer.push(text.trim());
        true
    }

    /// Stops the running prompt. A tool that waits for an answer is cancelled
    /// and does not run.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Halt for this chat: the running prompt ends as `halted` and a waiting
    /// tool does not run. With no prompt running, an open desktop session
    /// ends here (its loop is not there to end it): its cards are taken back,
    /// an allowed one too, so nothing queued runs with a later message. True
    /// when a prompt was running.
    pub fn halt(&self) -> bool {
        self.halted.store(true, Ordering::SeqCst);
        self.halt_seen.store(true, Ordering::SeqCst);
        self.cancel.cancel();
        let busy = self.busy.load(Ordering::SeqCst);
        if !busy {
            self.end_idle_episode("halt");
        }
        busy
    }
}

/// Halt, as the cabin's tray Halt: the halt stamp stops desktop steps (one
/// mid-action included) and every prompt started before it, held keys are
/// let go, unattended runs and their tasks stop, and so does every chat.
/// Desktop steps stay off until the app restarts, as in the cabin. Nothing is
/// typed or clicked. Returns the chats that were mid-reply.
#[cfg(desktop)]
pub fn halt<'a>(
    config_dir: &std::path::Path,
    chats: impl IntoIterator<Item = &'a Arc<Session>>,
) -> Vec<String> {
    {
        let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
        grokhub_desktop::write_halt_stamp();
    }
    grokhub_desktop::note_halt();
    grokhub_agent::halt_all_sessions();
    let mut busy: Vec<String> = chats
        .into_iter()
        .filter(|session| session.halt())
        .map(|session| session.id.clone())
        .collect();
    busy.sort();
    busy
}

impl Drop for Session {
    fn drop(&mut self) {
        grokhub_agent::detach_elicit(&self.id);
        grokhub_agent::set_plan_session(&self.id, false);
        grokhub_agent::set_lead_session(&self.id, None);
    }
}

/// The real desktop tools behind the hard gate, with the shell as their host.
#[cfg(desktop)]
fn native_desktop() -> Box<dyn DesktopOps> {
    Box::new(grokhub_desktop::NativeDesktop::new())
}

/// Phones have no desktop tools; the capability report keeps them unoffered,
/// and a call that slips through is refused.
#[cfg(mobile)]
fn native_desktop() -> Box<dyn DesktopOps> {
    struct NoDesktop;
    impl DesktopOps for NoDesktop {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, _name: &str, _args: &serde_json::Value) -> grokhub_agent::ToolOutput {
            grokhub_agent::ToolOutput::err("Desktop control isn't available on this device.")
        }
    }
    Box::new(NoDesktop)
}

/// A finished turn, for its follow-ups.
struct FollowUpTurn<'a> {
    said: &'a str,
    reply: &'a str,
    block: &'a str,
    failed: bool,
    mode: SessionMode,
}

/// The reply's chips, saved with the chat (the turn was recorded before
/// Done, so its count is the chat's now), and the chips block reviewed
/// against how its chips did.
fn follow_ups(
    config_dir: &std::path::Path,
    conversation_id: &str,
    turn: FollowUpTurn<'_>,
) -> Option<crate::chips::FollowUps> {
    let now = grokhub_core::now_ms();
    crate::chips::review_trailer(config_dir, now);
    let chips = crate::chips::reply_chips(turn.said, turn.reply, turn.block, turn.failed);
    if chips.is_empty() {
        return None;
    }
    let at = grokhub_agent::load_session(conversation_id)
        .map(|info| crate::chips::user_turns(&crate::chips::chat_pairs(&info)))
        .ok();
    let ups = crate::chips::FollowUps {
        chips,
        at: at.unwrap_or(0),
        mode: turn.mode.as_str().to_string(),
        saved_ms: now,
    };
    // A chat with no saved file (scratch) still shows them, but keeps none.
    if at.is_some() {
        let _ = crate::chips::save_follow_ups(config_dir, conversation_id, Some(ups.clone()));
    }
    Some(ups)
}

/// What a chat's worker thread owns besides its queues.
struct Worker {
    id: String,
    /// Whose saved turns the engine starts from: the chat's own, or the
    /// parent's for a side answer.
    resume: String,
    config_dir: PathBuf,
    desktop: fn() -> Box<dyn DesktopOps>,
    cancel: CancelToken,
    steer: SteerQueue,
    halted: Arc<AtomicBool>,
    frames: crate::episode::FrameSlot,
    pause: Arc<AtomicBool>,
    paused_turn: Arc<AtomicBool>,
    desk_on: Arc<AtomicBool>,
}

fn serve(worker: Worker, inbox: PermitInbox, jobs: Receiver<Job>) {
    let Worker {
        id: conversation_id,
        resume,
        config_dir,
        desktop,
        cancel,
        steer,
        halted,
        frames,
        pause,
        paused_turn,
        desk_on,
    } = worker;
    let _config = grokhub_agent::perm::ConfigGuard::set(&config_dir);
    let mut engine: Option<NativeEngine> = None;
    let mut inbox = Some(inbox);
    while let Ok(job) = jobs.recv() {
        let Job {
            text,
            attachments,
            route,
            asked_ms,
            episode,
            mut emit,
            done,
        } = job;
        // Where the engine starts; the router picks each step's provider,
        // model and effort, as in the cabin.
        let (model, effort) = grokhub_agent::route::host::chat_start();
        // The first picture goes as image input; a file the model can't
        // read stops the send with the cabin's note.
        let mods = crate::attach::modalities(&config_dir, &model);
        let message = match crate::attach::compose(&text, &attachments, &mods) {
            Ok(message) => message,
            Err(err) => {
                let _ = done.send(Err(err));
                continue;
            }
        };
        // The chips block stays in the rules on btw turns too, so the cached
        // prompt doesn't change with the mode; a btw reply's chips are dropped.
        let chips_on =
            crate::chips::trailer_on(&config_dir, route.suggestions, grokhub_core::now_ms());
        // The agent steps the voice per turn by this chat's id once it is registered.
        if let Some(voice) = route.voice_session {
            grokhub_agent::voice_session::register(&conversation_id, voice);
        }
        let system = system_prompt_for(
            &route.workspace,
            route.desktop,
            &route.voice,
            &conversation_id,
            chips_on,
        );
        let gate = shell_gate(route.mode, route.desktop, route.session);
        // Plan holds the turn read-only until its plan is approved; Chat and
        // btw clear a plan left from an earlier message.
        grokhub_agent::set_plan_session(&conversation_id, route.session == SessionMode::Plan);
        // A lead session's turn plans or works; a side answer on it is no
        // lead turn. Set on every message, so an old mark never outlives it.
        let lead_phase = match route.session {
            _ if !route.lead => None,
            SessionMode::Btw => None,
            SessionMode::Plan => Some(grokhub_agent::LeadPhase::Plan),
            SessionMode::Chat => Some(grokhub_agent::LeadPhase::Work),
        };
        grokhub_agent::set_lead_session(&conversation_id, lead_phase);
        let mut plan_watch = crate::lead::PlanWatch::default();
        crate::helper_asks::turn_started(&conversation_id);
        let engine = match (&mut engine, inbox.take()) {
            (Some(engine), _) => {
                engine.set_workspace(route.workspace);
                engine.set_gate(gate);
                engine.set_route(route.client, model, effort, system, route.auth_kind);
                engine
            }
            (none, inbox) => {
                let Some(inbox) = inbox else {
                    let _ = done.send(Err("The chat worker stopped.".into()));
                    return;
                };
                let engine = none.insert(NativeEngine::new(EngineParts {
                    client: route.client,
                    workspace: route.workspace,
                    model,
                    effort,
                    system,
                    conversation_id: conversation_id.clone(),
                    auth_kind: route.auth_kind,
                    max_turns: 0,
                    cancel: cancel.clone(),
                    steer: steer.clone(),
                    halt: Box::new(NeverHalted),
                    gate,
                    desktop: Some(Box::new(crate::episode::FrameTap {
                        inner: Box::new(crate::episode::Switched {
                            inner: desktop(),
                            on: Arc::clone(&desk_on),
                        }),
                        slot: frames.clone(),
                    })),
                    permits: Arc::new(inbox),
                }));
                engine.set_pause(Arc::clone(&pause));
                // A chat saved earlier (opened from History, or the last chat
                // after a restart) goes on from its saved turns, as the cabin's does.
                if let Ok(info) = grokhub_agent::load_session(&resume) {
                    engine.resume(info.input(), info.usage);
                }
                engine
            }
        };
        // A message with desktop control on belongs to the chat's episode.
        engine.set_episode(episode.filter(|_| route.desktop));
        // The desktop Halt (the cabin's hotkey or a halt stamp) stops this prompt too.
        #[cfg(desktop)]
        engine.set_halt(Box::new(grokhub_agent::StampHalt {
            started_ms: asked_ms,
            read: grokhub_desktop::read_halt_stamp,
        }));
        #[cfg(mobile)]
        let _ = asked_ms;
        let mut stop = String::new();
        let mut error = None;
        let mut reply = String::new();
        // The reply's chips block is held back from the page and the saved
        // chat's view (`history::lines` strips it there).
        let mut trailer = grokhub_core::ChipTrailerFilter::default();
        // Game goal lines are read at the end and never shown.
        let mut markers = grokhub_core::objectives::MarkerFilter::default();
        // A tool call the model wrote as text never shows (LO-51).
        let mut tool_tags = grokhub_core::tool_tags::ToolTagFilter::new();
        let mut raw = String::new();
        let ran = engine.prompt(&message.text, message.image.as_deref(), &mut |event| {
            let Some(mut ui) = ui_event(event) else {
                return;
            };
            if lead_phase.is_some() {
                crate::helper_asks::turn_ask(&conversation_id, &ui);
                // The lead's own plan is saved with the session: the draft,
                // and the text as the person edited it.
                let plan = match &ui {
                    UiEvent::Plan { text } => Some(text.as_str()),
                    _ => None,
                };
                let planning = grokhub_agent::plan_on(&conversation_id);
                if let Some(text) = plan_watch.keep(plan, planning) {
                    let _ = crate::lead::save_plan(&config_dir, &conversation_id, text);
                }
            }
            if let UiEvent::Done { stop: s } = &mut ui {
                // Halt cancels a waiting tool too; the turn reads as halted.
                if halted.load(Ordering::SeqCst) {
                    *s = HALTED.into();
                } else if paused_turn.load(Ordering::SeqCst) {
                    if s == "end_turn" {
                        // A paused session's turn: stopped, not finished.
                        *s = "cancelled".into();
                    } else {
                        // Something else ended it (the reply's own Stop):
                        // the Pause did not, and the session is not paused.
                        paused_turn.store(false, Ordering::SeqCst);
                    }
                }
            }
            match &mut ui {
                UiEvent::Text { text } => {
                    *text = trailer.feed(text);
                    raw.push_str(text);
                    *text = markers.feed(text);
                    *text = tool_tags.feed(text);
                    if text.is_empty() {
                        return;
                    }
                    reply.push_str(text);
                }
                UiEvent::Done { stop: s } => {
                    stop = s.clone();
                    let (tail, block) = trailer.finish();
                    raw.push_str(&tail);
                    let mut tail = markers.feed(&tail);
                    tail.push_str(&markers.finish());
                    let mut tail = tool_tags.feed(&tail);
                    tail.push_str(&tool_tags.finish());
                    if !tail.is_empty() {
                        reply.push_str(&tail);
                        emit(UiEvent::Text { text: tail });
                    }
                    let btw = route.session == SessionMode::Btw;
                    if route.suggestions && crate::chips::follow_ups_wanted(btw, s) {
                        let failed = s == "error" || error.is_some();
                        let ups = follow_ups(
                            &config_dir,
                            &conversation_id,
                            FollowUpTurn {
                                said: &text,
                                reply: &reply,
                                block: &block,
                                failed,
                                mode: route.session,
                            },
                        );
                        if let Some(card) = ups.as_ref().and_then(suggestions_for) {
                            emit(card);
                        }
                    }
                    if !btw && s != "error" && error.is_none() {
                        let raw = std::mem::take(&mut raw);
                        let ask = crate::goals::read_chat_turn(
                            &config_dir,
                            &conversation_id,
                            &text,
                            &raw,
                        );
                        if let Some(card) = ask {
                            emit(UiEvent::Goal {
                                id: card.id,
                                title: card.title,
                            });
                        }
                    }
                }
                UiEvent::Error { message } => error = Some(message.clone()),
                _ => {}
            }
            emit(ui);
        });
        crate::helper_asks::turn_ended(&conversation_id);
        // A planning turn that ends out of plan mode had its plan approved.
        if lead_phase == Some(grokhub_agent::LeadPhase::Plan)
            && !grokhub_agent::plan_on(&conversation_id)
        {
            let _ = crate::lead::approve(&config_dir, &conversation_id);
        }
        let result = match (ran, error) {
            _ if halted.load(Ordering::SeqCst) => Ok(HALTED.to_string()),
            (Err(err), _) => Err(err),
            (Ok(()), Some(message)) if stop == "error" => Err(message),
            (Ok(()), _) => Ok(stop),
        };
        let _ = done.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{
        ClientError, ContentPart, FunctionCall, InputItem, ResponsesRequest, StreamEvent,
        TurnOutput,
    };
    use std::sync::Mutex;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-agent-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Asks for one tool call, then answers with the tool's output.
    struct OneTool {
        name: &'static str,
        arguments: String,
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl OneTool {
        fn new(name: &'static str, arguments: serde_json::Value) -> Arc<Self> {
            Arc::new(Self {
                name,
                arguments: arguments.to_string(),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> usize {
            self.requests.lock().expect("requests").len()
        }

        fn tool_output(&self) -> Option<String> {
            let requests = self.requests.lock().expect("requests");
            requests.last()?.input.iter().find_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
                _ => None,
            })
        }
    }

    impl ModelClient for OneTool {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut requests = self.requests.lock().expect("requests");
            requests.push(req.clone());
            if requests.len() == 1 {
                return Ok(TurnOutput {
                    calls: vec![FunctionCall {
                        call_id: "call-1".into(),
                        name: self.name.into(),
                        arguments: self.arguments.clone(),
                    }],
                    ..TurnOutput::default()
                });
            }
            sink(StreamEvent::TextDelta("All done.".into()));
            Ok(TurnOutput {
                text: "All done.".into(),
                ..TurnOutput::default()
            })
        }
    }

    struct Run {
        session: Arc<Session>,
        events: Arc<Mutex<Vec<UiEvent>>>,
        result: std::thread::JoinHandle<Result<String, String>>,
    }

    fn start(client: Arc<OneTool>, workspace: &std::path::Path, config: &std::path::Path) -> Run {
        start_in(PermMode::Ask, client, workspace, config)
    }

    fn start_in(
        mode: PermMode,
        client: Arc<OneTool>,
        workspace: &std::path::Path,
        config: &std::path::Path,
    ) -> Run {
        start_desk(mode, false, client, workspace, config)
    }

    /// Every desktop call the fake desktop got, as `tool text`.
    static DESK_CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    struct FakeDesktop;

    impl DesktopOps for FakeDesktop {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, name: &str, args: &serde_json::Value) -> grokhub_agent::ToolOutput {
            let text = args["text"].as_str().unwrap_or_default();
            DESK_CALLS
                .lock()
                .expect("desk calls")
                .push(format!("{name} {text}"));
            grokhub_agent::ToolOutput::ok("typed")
        }
    }

    fn fake_desktop() -> Box<dyn DesktopOps> {
        Box::new(FakeDesktop)
    }

    fn desk_ran(entry: &str) -> bool {
        DESK_CALLS
            .lock()
            .expect("desk calls")
            .iter()
            .any(|c| c == entry)
    }

    fn start_desk(
        mode: PermMode,
        desktop: bool,
        client: Arc<OneTool>,
        workspace: &std::path::Path,
        config: &std::path::Path,
    ) -> Run {
        start_full(mode, desktop, SessionMode::Chat, client, workspace, config)
    }

    fn start_full(
        mode: PermMode,
        desktop: bool,
        chip: SessionMode,
        client: Arc<OneTool>,
        workspace: &std::path::Path,
        config: &std::path::Path,
    ) -> Run {
        let who = Who {
            id: "gate-test",
            lead: false,
        };
        start_as(who, mode, desktop, chip, client, workspace, config)
    }

    /// Whose turn a test runs: the chat's id, and whether it is a lead session.
    struct Who {
        id: &'static str,
        lead: bool,
    }

    fn start_as(
        who: Who,
        mode: PermMode,
        desktop: bool,
        chip: SessionMode,
        client: Arc<OneTool>,
        workspace: &std::path::Path,
        config: &std::path::Path,
    ) -> Run {
        let session = Arc::new(Session::spawn_with(
            who.id,
            config.to_path_buf(),
            fake_desktop,
        ));
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let route = Route {
            client,
            auth_kind: AuthKind::ApiKey,
            workspace: workspace.to_path_buf(),
            mode,
            desktop,
            session: chip,
            voice: String::new(),
            voice_session: None,
            suggestions: true,
            lead: who.lead,
        };
        let runner = Arc::clone(&session);
        let result = std::thread::spawn(move || {
            runner.prompt(
                "do the thing",
                Vec::new(),
                route,
                Box::new(move |event| sink.lock().expect("events").push(event)),
            )
        });
        Run {
            session,
            events,
            result,
        }
    }

    fn wait_for_permission(run: &Run) -> UiEvent {
        for _ in 0..400 {
            let found = run
                .events
                .lock()
                .expect("events")
                .iter()
                .find(|event| matches!(event, UiEvent::Permission { .. }))
                .cloned();
            if let Some(ask) = found {
                return ask;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "no permission card: {:?}",
            run.events.lock().expect("events")
        );
    }

    /// The prompt is still parked, nothing ran, and the model was not called again.
    fn assert_parked(run: &Run, client: &OneTool) {
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !run.result.is_finished(),
            "the turn went on without an answer"
        );
        assert_eq!(client.calls(), 1);
    }

    #[test]
    fn write_waits_for_an_ok_and_deny_leaves_no_file() {
        let (workspace, config) = (temp_dir("deny-ws"), temp_dir("deny-cfg"));
        let client = OneTool::new(
            "write",
            serde_json::json!({"path": "note.txt", "content": "hi"}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);

        let ask = wait_for_permission(&run);
        let UiEvent::Permission { id, title, .. } = &ask else {
            unreachable!()
        };
        assert_eq!((id.as_str(), title.as_str()), ("call-1", "write"));
        assert_parked(&run, &client);
        assert!(!workspace.join("note.txt").exists());

        run.session.answer("call-1", PermAnswer::Deny);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert!(!workspace.join("note.txt").exists());
        assert_eq!(
            client.tool_output().as_deref(),
            Some("User rejected the execution for tool `write`")
        );
        let events = run.events.lock().expect("events");
        assert!(events.contains(&UiEvent::Text {
            text: "All done.".into()
        }));
        assert_eq!(
            events.last(),
            Some(&UiEvent::Done {
                stop: "end_turn".into()
            })
        );
    }

    #[test]
    fn allow_runs_the_write() {
        let (workspace, config) = (temp_dir("allow-ws"), temp_dir("allow-cfg"));
        let client = OneTool::new(
            "write",
            serde_json::json!({"path": "note.txt", "content": "hi"}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);

        wait_for_permission(&run);
        assert_parked(&run, &client);
        assert!(!workspace.join("note.txt").exists());

        run.session.answer("call-1", PermAnswer::Allow);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert_eq!(
            std::fs::read_to_string(workspace.join("note.txt")).expect("written"),
            "hi"
        );
        // The loop then nudges the model to check its change; only the
        // tool's output matters here.
        let requests = client.requests.lock().expect("requests");
        assert_eq!(
            requests.get(1).and_then(|r| r.input.last()),
            Some(&InputItem::FunctionCallOutput {
                call_id: "call-1".into(),
                output: "wrote 2 bytes to note.txt".into(),
            })
        );
    }

    fn wait_for_elicit(run: &Run) -> UiEvent {
        for _ in 0..200 {
            let found = run
                .events
                .lock()
                .expect("events")
                .iter()
                .find(|e| matches!(e, UiEvent::Elicit { .. }))
                .cloned();
            if let Some(found) = found {
                return found;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("no elicit card: {:?}", run.events.lock().expect("events"));
    }

    fn elicit_id(event: &UiEvent) -> String {
        match event {
            UiEvent::Elicit { id, .. } => id.clone(),
            other => panic!("not an elicit: {other:?}"),
        }
    }

    #[test]
    fn plan_and_btw_refuse_a_write_without_asking() {
        for chip in [SessionMode::Plan, SessionMode::Btw] {
            let (workspace, config) = (temp_dir("ro-ws"), temp_dir("ro-cfg"));
            let client = OneTool::new(
                "write",
                serde_json::json!({"path": "note.txt", "content": "hi"}),
            );
            let run = start_full(
                PermMode::Always,
                false,
                chip,
                Arc::clone(&client),
                &workspace,
                &config,
            );
            assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
            assert!(!workspace.join("note.txt").exists(), "{chip:?}");
            let events = run.events.lock().expect("events");
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, UiEvent::Permission { .. })),
                "{chip:?}: {events:?}"
            );
            let refused = client.tool_output().expect("tool output");
            assert!(refused.contains("read-only"), "{chip:?}: {refused}");
        }
    }

    #[test]
    fn a_plan_shows_then_waits_for_approve() {
        let (workspace, config) = (temp_dir("plan-ws"), temp_dir("plan-cfg"));
        let client = OneTool::new(
            "exit_plan_mode",
            serde_json::json!({"plan": "1. Read main.rs\n2. Fix the typo"}),
        );
        let run = start_full(
            PermMode::Ask,
            false,
            SessionMode::Plan,
            Arc::clone(&client),
            &workspace,
            &config,
        );
        let card = wait_for_elicit(&run);
        assert_eq!(
            card,
            UiEvent::Elicit {
                id: elicit_id(&card),
                server: "Plan".into(),
                message: "1. Read main.rs\n2. Fix the typo".into(),
                mode: "form".into(),
                url: String::new(),
                field_name: None,
                field_title: String::new(),
                secret: false,
                options: Vec::new(),
            }
        );
        assert!(run.events.lock().expect("events").contains(&UiEvent::Plan {
            text: "1. Read main.rs\n2. Fix the typo".into()
        }));
        assert_eq!(client.calls(), 1, "the turn went on before the answer");
        assert!(grokhub_agent::plan_on("gate-test"));

        run.session
            .answer_elicit(&elicit_id(&card), "accept", None, "");
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert_eq!(
            client.tool_output().as_deref(),
            Some("Plan approved. Plan mode is off.")
        );
        assert!(!grokhub_agent::plan_on("gate-test"));
    }

    const LEAD_DRAFT: &str = "Read the code.\n\nTasks\n1. Write the note";

    /// The first request's input, as text.
    fn first_ask(client: &OneTool) -> String {
        let requests = client.requests.lock().expect("requests");
        format!("{:?}", requests.first().expect("a request").input)
    }

    #[test]
    fn a_lead_plan_is_saved_and_its_approval_is_remembered() {
        let (workspace, config) = (temp_dir("lead-ws"), temp_dir("lead-cfg"));
        let id = "lead-plan-test";
        let client = OneTool::new("exit_plan_mode", serde_json::json!({"plan": LEAD_DRAFT}));
        let run = start_as(
            Who { id, lead: true },
            PermMode::Ask,
            false,
            SessionMode::Plan,
            Arc::clone(&client),
            &workspace,
            &config,
        );
        let card = wait_for_elicit(&run);
        let plan_file = config.join("sessions").join(id).join("plan.md");
        assert_eq!(
            std::fs::read_to_string(&plan_file).expect("plan.md"),
            LEAD_DRAFT
        );
        assert!(!crate::lead::approved(&config, id));
        assert!(crate::helper_asks::needs_you(id));
        let asked = first_ask(&client);
        assert!(
            asked.contains("You are the lead for this session."),
            "{asked}"
        );
        assert!(asked.contains("Plan first."), "{asked}");

        run.session.answer_elicit(
            &elicit_id(&card),
            "accept",
            Some("plan"),
            "Read the code.\n\nTasks\n1. Write the page",
        );
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert_eq!(
            std::fs::read_to_string(&plan_file).expect("plan.md"),
            "Read the code.\n\nTasks\n1. Write the page"
        );
        assert!(crate::lead::approved(&config, id));
        assert!(!crate::helper_asks::needs_you(id));
        let output = client.tool_output().expect("tool output");
        assert!(
            output.starts_with(
                "Plan approved. Plan mode is off. The user edited the plan. Follow this text, \
                 not your draft:\nRead the code.\n\nTasks\n1. Write the page\n\nThe plan is approved."
            ),
            "{output}"
        );
        drop(run.session);
        assert!(!grokhub_agent::lead_on(id));
    }

    #[test]
    fn a_chat_plan_saves_no_file() {
        let (workspace, config) = (temp_dir("chat-plan-ws"), temp_dir("chat-plan-cfg"));
        let id = "chat-plan-test";
        let client = OneTool::new("exit_plan_mode", serde_json::json!({"plan": LEAD_DRAFT}));
        let run = start_as(
            Who { id, lead: false },
            PermMode::Ask,
            false,
            SessionMode::Plan,
            Arc::clone(&client),
            &workspace,
            &config,
        );
        let card = wait_for_elicit(&run);
        assert!(!grokhub_agent::lead_on(id));
        assert!(!crate::helper_asks::needs_you(id));
        assert!(!first_ask(&client).contains("You are the lead"));
        run.session
            .answer_elicit(&elicit_id(&card), "accept", Some("plan"), "1. Edited");
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert!(!config.join("sessions").join(id).join("plan.md").exists());
        assert!(!crate::lead::approved(&config, id));
        assert_eq!(
            client.tool_output().as_deref(),
            Some("Plan approved. Plan mode is off.")
        );
    }

    #[test]
    fn a_lead_turn_needs_the_person_while_its_card_waits() {
        let (workspace, config) = (temp_dir("lead-ask-ws"), temp_dir("lead-ask-cfg"));
        let id = "lead-ask-test";
        let client = OneTool::new(
            "write",
            serde_json::json!({"path": "note.txt", "content": "hi"}),
        );
        let run = start_as(
            Who { id, lead: true },
            PermMode::Ask,
            false,
            SessionMode::Chat,
            Arc::clone(&client),
            &workspace,
            &config,
        );
        wait_for_permission(&run);
        assert!(crate::helper_asks::needs_you(id));
        // A turn that works carries the lead note, and claims no approval.
        let asked = first_ask(&client);
        assert!(
            asked.contains("You are the lead for this session."),
            "{asked}"
        );
        assert!(asked.contains("Keep working from the task list"), "{asked}");
        assert!(!asked.contains("The plan is approved"), "{asked}");

        run.session.answer("call-1", PermAnswer::Allow);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert!(!crate::helper_asks::needs_you(id));
        // A turn that worked approves nothing and saves no plan.
        assert!(!crate::lead::approved(&config, id));
        assert!(!config.join("sessions").join(id).join("plan.md").exists());
    }

    #[test]
    fn a_side_answer_on_a_lead_session_is_no_lead_turn() {
        let (workspace, config) = (temp_dir("lead-btw-ws"), temp_dir("lead-btw-cfg"));
        let id = "lead-btw-test";
        let client = OneTool::new("read", serde_json::json!({"path": "missing.txt"}));
        let run = start_as(
            Who { id, lead: true },
            PermMode::Ask,
            false,
            SessionMode::Btw,
            Arc::clone(&client),
            &workspace,
            &config,
        );
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert!(!first_ask(&client).contains("You are the lead"));
        assert!(!grokhub_agent::lead_on(id));
    }

    #[test]
    fn a_question_takes_the_typed_answer() {
        let (workspace, config) = (temp_dir("ask-ws"), temp_dir("ask-cfg"));
        let client = OneTool::new(
            "ask_user_question",
            serde_json::json!({"questions": [{"question": "Which color?"}]}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);
        let card = wait_for_elicit(&run);
        assert!(
            matches!(&card, UiEvent::Elicit { server, field_name, field_title, .. }
                if server == "Question"
                    && field_name.as_deref() == Some("answer")
                    && field_title == "Your answer"),
            "{card:?}"
        );
        run.session
            .answer_elicit(&elicit_id(&card), "accept", Some("answer"), "  blue ");
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert_eq!(client.tool_output().as_deref(), Some("answer: blue"));
    }

    #[test]
    fn a_question_with_options_offers_them_recommended_first() {
        let (workspace, config) = (temp_dir("pick-ws"), temp_dir("pick-cfg"));
        let client = OneTool::new(
            "ask_user_question",
            serde_json::json!({"questions": [{"question": "Which fix?", "options": [
                {"label": "Upgrade everything"},
                {"label": "Pin the crate", "description": "smallest change", "recommended": true}
            ]}]}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);
        let card = wait_for_elicit(&run);
        let UiEvent::Elicit { options, .. } = &card else {
            panic!("{card:?}");
        };
        assert_eq!(
            options,
            &vec![
                ElicitChoice {
                    label: "Pin the crate".into(),
                    description: "smallest change".into(),
                    recommended: true,
                },
                ElicitChoice {
                    label: "Upgrade everything".into(),
                    description: String::new(),
                    recommended: false,
                },
            ]
        );
        run.session
            .answer_elicit(&elicit_id(&card), "accept", Some("answer"), "Pin the crate");
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert_eq!(
            client.tool_output().as_deref(),
            Some("answer: Pin the crate")
        );
    }

    #[test]
    fn declining_a_question_tells_the_model() {
        let (workspace, config) = (temp_dir("no-ws"), temp_dir("no-cfg"));
        let client = OneTool::new(
            "ask_user_question",
            serde_json::json!({"questions": [{"question": "Which color?"}]}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);
        let card = wait_for_elicit(&run);
        run.session
            .answer_elicit(&elicit_id(&card), "decline", None, "");
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".into()));
        assert_eq!(
            client.tool_output().as_deref(),
            Some("User declined the question")
        );
    }

    #[test]
    fn a_shell_command_waits_and_stop_runs_nothing() {
        let (workspace, config) = (temp_dir("stop-ws"), temp_dir("stop-cfg"));
        let client = OneTool::new(
            "run_terminal_command",
            serde_json::json!({"command": "echo ran > ran.txt"}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);

        let ask = wait_for_permission(&run);
        assert!(
            matches!(&ask, UiEvent::Permission { title, action, .. }
                if title == "run_terminal_command" && action.contains("echo ran")),
            "{ask:?}"
        );
        assert_parked(&run, &client);

        run.session.cancel();
        assert_eq!(
            run.result.join().expect("join"),
            Ok("cancelled".to_string())
        );
        std::thread::sleep(Duration::from_millis(200));
        assert!(!workspace.join("ran.txt").exists());
        assert_eq!(client.calls(), 1);
    }

    #[test]
    fn a_read_runs_without_asking() {
        let (workspace, config) = (temp_dir("read-ws"), temp_dir("read-cfg"));
        std::fs::write(workspace.join("a.txt"), "alpha").expect("seed");
        let client = OneTool::new("read_file", serde_json::json!({"target_file": "a.txt"}));
        let run = start(Arc::clone(&client), &workspace, &config);

        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        let events = run.events.lock().expect("events");
        assert!(!events
            .iter()
            .any(|event| matches!(event, UiEvent::Permission { .. })));
        assert!(client.tool_output().expect("output").contains("alpha"));
    }

    #[test]
    fn engine_events_map_to_ui_events() {
        assert_eq!(
            ui_event(AcpEvent::Text("hi".into())),
            Some(UiEvent::Text { text: "hi".into() })
        );
        assert_eq!(
            ui_event(AcpEvent::Err("boom".into())),
            Some(UiEvent::Error {
                message: "boom".into()
            })
        );
        assert_eq!(ui_event(AcpEvent::Thought("hmm".into())), None);
        let json = serde_json::to_value(UiEvent::Permission {
            id: "c".into(),
            title: "write".into(),
            action: "note.txt".into(),
            reason: String::new(),
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({"kind": "permission", "id": "c", "title": "write", "action": "note.txt", "reason": ""})
        );
        assert_eq!(
            ui_event(AcpEvent::Plan("1. Read".into())),
            Some(UiEvent::Plan {
                text: "1. Read".into()
            })
        );
        // No findings card in the shell yet: its plain list follows the reply.
        assert_eq!(
            ui_event(AcpEvent::Findings(
                "Findings\n\n- Info: tokens go to the vendor".into()
            )),
            Some(UiEvent::Text {
                text: "\n\nFindings\n\n- Info: tokens go to the vendor".into()
            })
        );
        let card = serde_json::to_value(UiEvent::Elicit {
            id: "ask-1".into(),
            server: "Question".into(),
            message: "Which color?".into(),
            mode: "form".into(),
            url: String::new(),
            field_name: Some("answer".into()),
            field_title: "Your answer".into(),
            secret: false,
            options: vec![ElicitChoice {
                label: "Blue".into(),
                description: "calm".into(),
                recommended: true,
            }],
        })
        .expect("json");
        assert_eq!(
            card,
            serde_json::json!({
                "kind": "elicit", "id": "ask-1", "server": "Question", "message": "Which color?",
                "mode": "form", "url": "", "fieldName": "answer", "fieldTitle": "Your answer",
                "secret": false,
                "options": [{"label": "Blue", "description": "calm", "recommended": true}]
            })
        );
        let chip: SessionMode = serde_json::from_str("\"btw\"").expect("chip");
        assert_eq!(chip, SessionMode::Btw);
    }

    #[test]
    fn answers_parse_and_junk_is_refused() {
        assert_eq!(parse_answer("allow"), Ok(PermAnswer::Allow));
        assert_eq!(parse_answer("always"), Ok(PermAnswer::Always));
        assert_eq!(parse_answer("deny"), Ok(PermAnswer::Deny));
        assert_eq!(parse_answer("yes"), Err("unknown answer: yes".to_string()));
    }

    #[test]
    fn plan_and_btw_gates_are_read_only() {
        for (chip, readonly) in [
            (SessionMode::Chat, false),
            (SessionMode::Plan, true),
            (SessionMode::Btw, true),
        ] {
            assert_eq!(
                shell_gate(PermMode::Auto, false, chip).readonly_session,
                readonly,
                "{chip:?}"
            );
        }
    }

    #[test]
    fn the_gate_asks_and_follows_the_desktop_switch() {
        assert_eq!(
            shell_gate(PermMode::Ask, false, SessionMode::Chat),
            Gate {
                mode: PermMode::Ask,
                readonly_session: false,
                attended: true,
                desktop: false,
            }
        );
        assert_eq!(
            shell_gate(PermMode::Always, true, SessionMode::Chat),
            Gate {
                mode: PermMode::Always,
                readonly_session: false,
                attended: true,
                desktop: true,
            }
        );
        let ws = std::path::Path::new("/tmp");
        assert!(!system_prompt(ws, false, "").contains("grokhub-desktop"));
        assert!(system_prompt(ws, true, "").contains("grokhub-desktop"));
    }

    #[test]
    fn a_personality_rides_after_the_safety_rules() {
        let ws = std::path::Path::new("/tmp");
        let voice = "Personality: Pirate. Talk like a pirate.";
        let plain = system_prompt(ws, false, "");
        let with = system_prompt(ws, false, voice);
        assert!(!plain.contains(voice));
        let rules = grokhub_core::cabin_rules::cabin_static_rules(false);
        let (at_rules, at_voice) = (with.find(rules.trim()), with.find(voice));
        assert!(at_rules.is_some() && at_voice.is_some(), "{with}");
        assert!(at_rules < at_voice, "the voice must come after the rules");
    }

    #[test]
    fn the_safety_closer_appears_once_with_a_real_voice() {
        let cfg = temp_dir("closer-once");
        let _guard = grokhub_agent::perm::ConfigGuard::set(&cfg);
        let ws = std::path::Path::new("/tmp");
        let closer = grokhub_core::talk::VOLATILE_SAFETY_CLOSER;
        for stored in ["", "pirate"] {
            let settings = crate::settings::ShellSettings {
                personality: stored.into(),
                ..crate::settings::ShellSettings::default()
            };
            let voice = crate::personality::voice(&cfg, &settings);
            assert!(!voice.is_empty());
            let prompt = system_prompt(ws, false, &voice);
            assert_eq!(prompt.matches(closer).count(), 1, "{prompt}");
            assert!(prompt.trim_end().ends_with(closer), "{prompt}");
        }
    }

    #[test]
    fn steer_now_reaches_only_a_running_reply() {
        let config = temp_dir("steer-idle");
        let session = Session::spawn_with("shell-steer-test", config, fake_desktop);
        // No reply runs: nothing is kept for a later one.
        assert!(!session.steer("then deploy"));
        assert!(session.steer.drain().is_empty());
        // A reply runs: the words wait for its next step, trimmed.
        session.busy.store(true, Ordering::SeqCst);
        assert!(!session.steer("   "));
        assert!(session.steer("  then deploy "));
        assert_eq!(session.steer.drain(), vec!["then deploy".to_string()]);
        session.busy.store(false, Ordering::SeqCst);
    }

    #[test]
    fn a_chat_turn_registers_its_voice_settings() {
        let (workspace, config) = (temp_dir("voice-reg-ws"), temp_dir("voice-reg"));
        let id = "shell-voice-register-test";
        grokhub_agent::voice_session::forget(id);
        let client = Arc::new(Answers {
            requests: Mutex::new(Vec::new()),
        });
        let session = Session::spawn_with(id, config.clone(), fake_desktop);
        let settings = crate::settings::ShellSettings {
            personality: "pirate".into(),
            ..crate::settings::ShellSettings::default()
        };
        let route = Route {
            client: Arc::clone(&client) as Arc<dyn ModelClient + Send + Sync>,
            auth_kind: AuthKind::ApiKey,
            workspace,
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Chat,
            voice: crate::personality::voice(&config, &settings),
            voice_session: Some(crate::personality::voice_config(&config, &settings, 0)),
            suggestions: true,
            lead: false,
        };
        assert_eq!(grokhub_agent::voice_session::active_voice(id), None);
        let result = session.prompt("hello", Vec::new(), route, Box::new(|_| {}));
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            grokhub_agent::voice_session::active_voice(id).as_deref(),
            Some("pirate")
        );
        grokhub_agent::voice_session::forget(id);
    }

    #[test]
    fn soul_and_user_ride_before_the_safety_closer() {
        let cfg = temp_dir("soul-cfg");
        let _guard = grokhub_agent::perm::ConfigGuard::set(&cfg);
        std::fs::create_dir_all(cfg.join("memory")).unwrap();
        std::fs::write(cfg.join("app.json"), "{}\n").unwrap();
        std::fs::write(
            cfg.join("memory").join("SOUL.md"),
            "HARBOR_SOUL_MARK speaks plainly\n",
        )
        .unwrap();
        std::fs::write(
            cfg.join("memory").join("USER.md"),
            "HARBOR_USER_MARK keeps a paper chart\n",
        )
        .unwrap();
        let voice = "Personality: Pirate. Talk like a pirate.";
        let prompt = system_prompt(std::path::Path::new("/tmp"), false, voice);
        let rules = grokhub_core::cabin_rules::cabin_static_rules(false);
        let at_rules = prompt.find(rules.trim()).expect("rules");
        let at_voice = prompt.find(voice).expect("voice");
        let at_soul = prompt.find("HARBOR_SOUL_MARK").expect("soul");
        let at_user = prompt.find("HARBOR_USER_MARK").expect("user");
        let at_closer = prompt
            .find("earlier in this prompt always win")
            .expect("closer");
        assert!(
            at_rules < at_voice && at_voice < at_soul && at_soul < at_user && at_user < at_closer,
            "{prompt}"
        );
        assert!(!rules.contains("HARBOR_SOUL_MARK"));
    }

    #[test]
    fn desktop_tools_are_refused_while_the_switch_is_off() {
        let (workspace, config) = (temp_dir("desk-off-ws"), temp_dir("desk-off-cfg"));
        let client = OneTool::new("type", serde_json::json!({"text": "hello from off"}));
        let run = start_desk(
            PermMode::Always,
            false,
            Arc::clone(&client),
            &workspace,
            &config,
        );

        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert_eq!(
            client.tool_output().as_deref(),
            Some(*grokhub_core::desktop_mcp::OFF_MSG)
        );
        assert!(!desk_ran("type hello from off"));
    }

    #[test]
    fn a_risky_desktop_step_waits_for_an_ok_even_in_always() {
        let (workspace, config) = (temp_dir("desk-rm-ws"), temp_dir("desk-rm-cfg"));
        let client = OneTool::new("type", serde_json::json!({"text": "rm -rf ~/old"}));
        let run = start_desk(
            PermMode::Always,
            true,
            Arc::clone(&client),
            &workspace,
            &config,
        );

        // Always skips the card for ordinary steps; a delete still asks.
        match wait_for_permission(&run) {
            UiEvent::Permission { title, action, .. } => {
                assert_eq!((title.as_str(), action.as_str()), ("type", "rm -rf ~/old"));
            }
            other => panic!("{other:?}"),
        }
        assert_parked(&run, &client);
        assert!(!desk_ran("type rm -rf ~/old"));

        run.session.answer("call-1", PermAnswer::Deny);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert!(!desk_ran("type rm -rf ~/old"));
        assert_eq!(
            client.tool_output().as_deref(),
            Some("User rejected the execution for tool `type`")
        );
    }

    #[test]
    fn an_allowed_desktop_step_runs_on_the_desktop() {
        let (workspace, config) = (temp_dir("desk-ok-ws"), temp_dir("desk-ok-cfg"));
        let client = OneTool::new("type", serde_json::json!({"text": "hello desk"}));
        let run = start_desk(
            PermMode::Ask,
            true,
            Arc::clone(&client),
            &workspace,
            &config,
        );

        wait_for_permission(&run);
        assert!(!desk_ran("type hello desk"));
        run.session.answer("call-1", PermAnswer::Allow);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert!(desk_ran("type hello desk"));
        assert_eq!(client.tool_output().as_deref(), Some("typed"));
    }

    #[test]
    fn always_runs_a_write_without_a_card() {
        let (workspace, config) = (temp_dir("always-ws"), temp_dir("always-cfg"));
        let client = OneTool::new(
            "write",
            serde_json::json!({"path": "note.txt", "content": "hi"}),
        );
        let run = start_in(PermMode::Always, Arc::clone(&client), &workspace, &config);

        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert_eq!(
            std::fs::read_to_string(workspace.join("note.txt")).expect("written"),
            "hi"
        );
        assert!(!run
            .events
            .lock()
            .expect("events")
            .iter()
            .any(|event| matches!(event, UiEvent::Permission { .. })));
    }

    #[test]
    fn always_still_waits_for_an_ok_to_delete() {
        let (workspace, config) = (temp_dir("always-rm-ws"), temp_dir("always-rm-cfg"));
        std::fs::write(workspace.join("notes.txt"), "keep me").expect("seed");
        let client = OneTool::new(
            "run_terminal_command",
            serde_json::json!({"command": "rm notes.txt"}),
        );
        let run = start_in(PermMode::Always, Arc::clone(&client), &workspace, &config);

        wait_for_permission(&run);
        assert_parked(&run, &client);
        assert!(workspace.join("notes.txt").exists());

        run.session.answer("call-1", PermAnswer::Deny);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert_eq!(
            std::fs::read_to_string(workspace.join("notes.txt")).expect("kept"),
            "keep me"
        );
    }

    #[cfg(desktop)]
    #[test]
    fn halt_ends_a_waiting_desktop_step_and_it_never_runs() {
        let (workspace, config) = (temp_dir("halt-ws"), temp_dir("halt-cfg"));
        let client = OneTool::new("type", serde_json::json!({"text": "halt me"}));
        let run = start_desk(
            PermMode::Ask,
            true,
            Arc::clone(&client),
            &workspace,
            &config,
        );

        wait_for_permission(&run);
        assert_parked(&run, &client);
        assert_eq!(halt(&config, [&run.session]), vec!["gate-test".to_string()]);
        assert_eq!(run.result.join().expect("join"), Ok("halted".to_string()));
        std::thread::sleep(Duration::from_millis(200));
        assert!(!desk_ran("type halt me"));
        assert_eq!(client.calls(), 1);
        assert_eq!(
            run.events.lock().expect("events").last(),
            Some(&UiEvent::Done {
                stop: "halted".into()
            })
        );
        assert!(config.join("desktop-halt.stamp").is_file());
        // An idle chat is not counted as stopped.
        assert_eq!(halt(&config, [&run.session]), Vec::<String>::new());
    }

    #[cfg(desktop)]
    #[test]
    fn a_prompt_sent_after_halt_still_runs() {
        let (workspace, config) = (temp_dir("after-ws"), temp_dir("after-cfg"));
        assert_eq!(halt(&config, std::iter::empty()), Vec::<String>::new());
        assert!(config.join("desktop-halt.stamp").is_file());
        std::thread::sleep(Duration::from_millis(20));
        let client = OneTool::new("type", serde_json::json!({"text": "after halt"}));
        let run = start_desk(
            PermMode::Always,
            true,
            Arc::clone(&client),
            &workspace,
            &config,
        );

        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert!(desk_ran("type after halt"));
        assert_eq!(client.tool_output().as_deref(), Some("typed"));
    }

    /// Answers every turn with one line and keeps each request.
    struct Answers {
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl ModelClient for Answers {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.requests.lock().expect("requests").push(req.clone());
            sink(StreamEvent::TextDelta("Seen.".into()));
            Ok(TurnOutput {
                text: "Seen.".into(),
                ..TurnOutput::default()
            })
        }
    }

    /// Sends `text` with `files` through a fresh chat and returns the user
    /// message the model got.
    fn send_with(
        name: &str,
        text: &str,
        files: Vec<crate::attach::Staged>,
    ) -> (Result<String, String>, Vec<ContentPart>) {
        let (workspace, config) = (temp_dir(&format!("{name}-ws")), temp_dir(name));
        let client = Arc::new(Answers {
            requests: Mutex::new(Vec::new()),
        });
        let session = Session::spawn_with(name, config, fake_desktop);
        let route = Route {
            client: Arc::clone(&client) as Arc<dyn ModelClient + Send + Sync>,
            auth_kind: AuthKind::ApiKey,
            workspace,
            mode: PermMode::Ask,
            desktop: false,
            session: SessionMode::Chat,
            voice: String::new(),
            voice_session: None,
            suggestions: true,
            lead: false,
        };
        let result = session.prompt(text, files, route, Box::new(|_| {}));
        let requests = client.requests.lock().expect("requests");
        let user = requests
            .first()
            .and_then(|r| {
                r.input.iter().rev().find_map(|item| match item {
                    InputItem::Message { role, content } if role == "user" => Some(content.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default();
        (result, user)
    }

    fn png_bytes() -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    fn texts(parts: &[ContentPart]) -> String {
        parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::InputText(t) => Some(t.as_str()),
                ContentPart::InputImage(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_text_file_reaches_the_model_as_its_send_line() {
        let config = temp_dir("attach-text-stage");
        let trays = crate::attach::Trays::default();
        let notes = trays
            .stage(
                &config,
                "attach-text",
                "notes.md",
                b"# plan",
                &["text", "image"],
            )
            .expect("staged");
        let line = format!(
            "File notes.md is on this computer at {}. Read it yourself. Do not ask for the contents to be pasted.",
            notes.path
        );
        let (result, user) = send_with("attach-text", "summarize this", vec![notes]);
        assert_eq!(result, Ok("end_turn".to_string()));
        assert!(
            texts(&user).contains(&format!("summarize this\n{line}")),
            "{user:?}"
        );
        assert!(!user.iter().any(|p| matches!(p, ContentPart::InputImage(_))));
    }

    #[test]
    fn a_picture_reaches_the_model_as_image_input() {
        use base64::Engine as _;
        let config = temp_dir("attach-image-stage");
        let trays = crate::attach::Trays::default();
        let shot = trays
            .stage(
                &config,
                "attach-image",
                "shot.png",
                &png_bytes(),
                &["text", "image"],
            )
            .expect("staged");
        let (result, user) = send_with("attach-image", "what is this", vec![shot]);
        assert_eq!(result, Ok("end_turn".to_string()));
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png_bytes())
        );
        assert!(user.contains(&ContentPart::InputImage(url)), "{user:?}");
        assert!(texts(&user).contains("what is this\nfrom reference shot.png"));
    }

    #[test]
    fn files_alone_send_and_an_empty_message_does_not() {
        let config = temp_dir("attach-alone-stage");
        let trays = crate::attach::Trays::default();
        let notes = trays
            .stage(&config, "attach-alone", "a.txt", b"x", &["text"])
            .expect("staged");
        let (result, user) = send_with("attach-alone", "  ", vec![notes]);
        assert_eq!(result, Ok("end_turn".to_string()));
        assert!(texts(&user).contains("File a.txt is on this computer"));
        let (empty, _) = send_with("attach-none", "  ", Vec::new());
        assert_eq!(empty, Err("Type a message first.".to_string()));
    }
}

#[cfg(test)]
#[path = "agent_episode_tests.rs"]
mod episode_tests;
