//! Quick chips and the empty-home greeting.

use super::*;
use grokhub_core::{ChipAction, ChipSignals};

/// Where a feature's offer ranks: above the PC's plain signals and seeds,
/// under a reply's own follow-ups.
const FEATURE_SCORE: f32 = 100.0;
/// The app you just came from leads the empty row, as it did before.
const FEATURE_FOCUS_SCORE: f32 = 110.0;
/// A skill matching the draft is what the person is typing toward.
const FEATURE_SKILL_SCORE: f32 = 108.0;

/// Chips on Home's row.
pub(super) const HOME_CHIP_MAX: usize = grokhub_core::CHIP_ROW_MAX;

/// The plain chip signals, kept apart so they join the paint key: chips
/// redraw when a game starts or the day turns, not every frame.
#[derive(Debug, Clone, Copy)]
struct ChipSignalsKey {
    low_disk: bool,
    weekday: u8,
    update_waiting: bool,
    quiet_hours: bool,
    gaming: bool,
    anticheat: bool,
}

/// Chip state that lives between taps: the stream filter that keeps a
/// reply's `<chips>` block off screen, and the last Do chip's undo.
#[derive(Debug, Default)]
pub(super) struct ChipLive {
    filter: grokhub_core::ChipTrailerFilter,
    undo: Option<ChipUndo>,
}

/// How long the Undo chip stays after a Do chip.
const UNDO_MS: u64 = 30_000;

/// What the last Do chip changed, so its Undo chip can put it back.
#[derive(Debug, Clone)]
pub(super) struct ChipUndo {
    label: String,
    at_ms: u64,
    kind: UndoKind,
}

#[derive(Debug, Clone)]
pub(super) enum UndoKind {
    /// The backup state and reminder from before the snooze. The state is
    /// boxed so the undo stays small.
    BackupSnooze(
        Box<grokhub_core::backup_nudge::NudgeState>,
        Option<grokhub_core::backup_nudge::Reminder>,
    ),
    /// The game whose release-day reminder was just added.
    RadarRemind(u32),
}

fn user_turns(messages: &[(String, String)]) -> usize {
    messages.iter().filter(|(role, _)| role == "user").count()
}

/// True when a chat chip should run through `send_chat` (slash parse or empty-composer send).
fn chip_chat_runs_immediately(chip: &QuickChip, composer_draft: &str) -> bool {
    let value = chip.value.trim();
    if value.starts_with('/') {
        return true;
    }
    if composer_draft.trim().is_empty() {
        return true;
    }
    false
}

/// Status after the `/plan` chip fills the box.
pub(super) const PLAN_CHIP_NOTE: &str =
    "Say what to plan, then press Enter. You get a plan to approve before anything changes";

fn is_bare_plan_chip(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("/plan")
}

/// The box after the `/plan` chip: `/plan ` then whatever was typed, so
/// Enter plans that ask and nothing typed is lost.
fn plan_chip_draft(typed: &str) -> String {
    let typed = typed.trim();
    let typed = typed
        .strip_prefix("/plan")
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        .map(str::trim)
        .unwrap_or(typed);
    format!("/plan {typed}")
}

/// The composer text a shell chip leaves: one `/sh` line, ready to check
/// and run with Enter.
fn shell_chip_draft(value: &str) -> String {
    let cmd = value.trim().trim_start_matches('$').trim();
    let cmd = cmd.strip_prefix("/sh ").unwrap_or(cmd).trim();
    format!("/sh {cmd}")
}

/// A chat chip picked while a draft is typed: the chip's text goes after
/// the draft on its own line, so nothing typed is lost. A chip whose text
/// already starts with the draft (expand-draft) replaces it.
fn draft_with_chip(draft: &str, value: &str) -> String {
    let draft = draft.trim_end();
    let value = value.trim();
    if draft.is_empty() || value.starts_with(draft) {
        // Empty box, or the chip already builds on the draft (expand-draft).
        value.to_string()
    } else if value.is_empty() || draft.ends_with(value) {
        draft.to_string()
    } else {
        format!("{draft}\n{value}")
    }
}

/// Only the chip in the first slot carries the primary style, after every
/// insert and re-order.
fn mark_first_primary(chips: &mut [QuickChip]) {
    for (i, c) in chips.iter_mut().enumerate() {
        c.primary = i == 0;
    }
}

fn chip_action_tip(chip: &QuickChip, composer_draft: &str) -> String {
    match chip.kind {
        ChipKind::Nav => "Opens this page — nothing is sent".into(),
        ChipKind::Shell => "Puts the command in the box — check it, then press Enter to run".into(),
        ChipKind::Chat => {
            if chip_chat_runs_immediately(chip, composer_draft) {
                if chip.value.trim().starts_with('/') {
                    "Runs this slash command".into()
                } else {
                    "Sends when you are signed in — edit in the composer first if you need to"
                        .into()
                }
            } else {
                "Adds to what you typed — edit and send when ready (never auto-sends)".into()
            }
        }
    }
}

fn polish_chip_hints(chips: &mut [QuickChip], composer_draft: &str) {
    for c in chips {
        if c.hint.trim().is_empty() {
            c.hint = chip_action_tip(c, composer_draft);
        } else if !c.hint.contains("send")
            && !c.hint.contains("Inserts")
            && !c.hint.contains("Opens")
        {
            let action = chip_action_tip(c, composer_draft);
            if action != c.hint {
                c.hint = format!("{}\n{}", c.hint.trim(), action);
            }
        }
    }
}

impl Cabin {
    /// Chat pairs for chip rebuild: scan a 4KB prefix so an 8MB complete
    /// does not get cloned into `chip_thread_from_messages`.
    pub(super) fn chip_chat_pairs(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.0.clone(), chip_scan(&m.1).to_string()))
            .collect()
    }

    pub(super) fn chip_hour() -> u8 {
        Self::local_clock().hour as u8
    }

    pub(super) fn other_chip_threads(&self) -> Vec<ChipThread> {
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.as_str())
            .unwrap_or("");
        collect_other_chip_threads(&self.threads, current)
    }

    pub(super) fn poll_greeting(&mut self) {
        if let Some(rx) = self.greeting_files_rx.take() {
            match rx.try_recv() {
                Ok((user_at, user, memory_at, memory)) => {
                    self.greeting_user_md = user;
                    self.greeting_user_at = user_at;
                    self.greeting_memory_md = memory;
                    self.greeting_memory_at = memory_at;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.greeting_files_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        let Some(rx) = self.greeting_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                self.greeting_busy = false;
                let mut sources = vec![
                    self.greeting_user_md.clone(),
                    self.greeting_memory_md.clone(),
                    self.last_night_hint(),
                ];
                if self.mem_name == "USER.md" || self.mem_name == "MEMORY.md" {
                    sources.push(self.mem_body.clone());
                }
                for insight in &self.learning.insights {
                    sources.push(insight.text.clone());
                }
                self.greeting = pick_greeting_against(&self.greeting, Some(&raw), &sources);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.greeting_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.greeting_busy = false;
            }
        }
    }

    pub(super) fn refresh_greeting(&mut self) {
        let empty = self.messages.is_empty();
        let scratch = self.scratch();
        if !should_paint_greeting(empty, scratch) {
            if !self.greeting.is_empty() {
                self.greeting.clear();
            }
            return;
        }
        if !self.scratch()
            && (self.greeting_flush_name != self.mem_name
                || self.greeting_flush_len != self.mem_body.len())
        {
            let name = self.mem_name.clone();
            let body = self.mem_body.clone();
            std::thread::spawn(move || {
                if config::read_memory(&name) != body {
                    let _ = config::write_memory(&name, &body);
                }
            });
            self.greeting_flush_name = self.mem_name.clone();
            self.greeting_flush_len = self.mem_body.len();
        }
        let user_at = config::memory_updated_at("USER.md");
        let memory_at = config::memory_updated_at("MEMORY.md");
        if self.greeting_user_at != user_at || self.greeting_memory_at != memory_at {
            if self.greeting_user_at == 0 && self.greeting_memory_at == 0 {
                self.greeting_user_md = config::read_memory("USER.md");
                self.greeting_memory_md = config::read_memory("MEMORY.md");
                self.greeting_user_at = user_at;
                self.greeting_memory_at = memory_at;
            } else if self.greeting_files_rx.is_none() {
                let (tx, rx) = mpsc::channel();
                self.greeting_files_rx = Some(rx);
                std::thread::spawn(move || {
                    let user = config::read_memory("USER.md");
                    let memory = config::read_memory("MEMORY.md");
                    let user_at = config::memory_updated_at("USER.md");
                    let memory_at = config::memory_updated_at("MEMORY.md");
                    let _ = tx.send((user_at, user, memory_at, memory));
                });
            }
        }
        let insights: Vec<String> = self
            .learning
            .insights
            .iter()
            .take(6)
            .map(|i| i.text.clone())
            .collect();
        let display_name = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.name.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_default();
        let hour = Self::chip_hour();
        let last_night = self.last_night_hint();
        let (local, fp, warm) = {
            let user_md = if self.mem_name == "USER.md" {
                self.mem_body.as_str()
            } else {
                self.greeting_user_md.as_str()
            };
            let memory_md = if self.mem_name == "MEMORY.md" {
                self.mem_body.as_str()
            } else {
                self.greeting_memory_md.as_str()
            };
            let last_project =
                Self::last_project_title_from(&last_night, &self.continue_hint, &self.cfg.goal_pin);
            let first_run = is_cabin_first_run(self.cfg.get_started_done, self.has_real_history());
            let signed_in = self.cabin_signed_in();
            let paused = self
                .board
                .iter()
                .rev()
                .find(|c| c.run && c.status == BoardStatus::Blocked)
                .map(|c| c.title.clone())
                .unwrap_or_default();
            let input = GreetingInput {
                user_md,
                memory_md,
                insights: &insights,
                display_name: &display_name,
                hour,
                last_night: &last_night,
                signed_in,
                first_run,
                last_project: &last_project,
                paused_task: &paused,
            };
            let fp = greeting_fingerprint(&input);
            let local = local_greeting(&input);
            // Greeting stays local. Debounce fields stay warm so a future
            // Settings opt-in can re-enable paraphrase without a schema bump.
            let warm = grokhub_core::should_refresh_greeting(
                &self.greeting_llm_fp,
                &fp,
                self.greeting_llm_at,
                now_ms(),
                self.llm_ready(),
                self.greeting_busy,
            );
            (local, fp, warm)
        };
        if self.greeting_fp != fp {
            self.greeting = local;
            self.greeting_fp = fp.clone();
        }
        if warm {
            self.greeting_llm_fp = fp;
            self.greeting_llm_at = now_ms();
        }
    }

    /// Test-only Fast-model greeting path. Empty-home paint uses `local_greeting` only.
    #[cfg(test)]
    pub(super) fn spawn_greeting_llm(&mut self, prompt: String) {
        if self.greeting_busy {
            return;
        }
        self.spawn_native_greeting(prompt);
    }

    pub(super) fn poll_goals(&mut self) {
        let Some(rx) = self.goal_rx.take() else {
            if self.goal_stale {
                self.spawn_thread_goal();
            }
            return;
        };
        match rx.try_recv() {
            Ok((tid, reply)) => {
                self.goal_busy = false;
                self.apply_thread_goal(&tid, &reply);
                if self.goal_stale {
                    self.spawn_thread_goal();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.goal_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.goal_busy = false;
                if self.goal_stale {
                    self.spawn_thread_goal();
                }
            }
        }
    }

    pub(super) fn apply_thread_goal(&mut self, tid: &str, reply: &str) {
        let topics = parse_fast_topics(reply);
        if topics.is_empty() {
            return;
        }
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let renaming = self
            .rename_idx
            .and_then(|i| self.threads.get(i))
            .is_some_and(|r| r.id == tid);
        {
            let Some(t) = self.threads.iter_mut().find(|t| t.id == tid) else {
                return;
            };
            if t.scratch {
                return;
            }
            t.goal = blend_thread_goal(&t.goal, &topics, GOAL_DROP_AFTER);
            if !t.goal.label.is_empty() {
                let mut tab = ThreadTab {
                    title: t.title.clone(),
                    pinned: t.pinned,
                    title_locked: t.title_locked,
                };
                if apply_auto_title_in(&mut tab, &t.goal.label, renaming) {
                    t.title = tab.title;
                    t.accessed_ms = now_ms();
                }
                if tid == current {
                    self.cfg.goal_pin = t.goal.label.clone();
                }
            }
        }
        self.persist();
    }

    pub(super) fn spawn_thread_goal(&mut self) {
        self.spawn_thread_goal_on(None);
    }

    pub(super) fn spawn_thread_goal_on(&mut self, thread_id: Option<&str>) {
        if self.goal_busy {
            self.goal_stale = true;
            return;
        }
        let vis = self.visible_thread_id();
        let tid = thread_id
            .map(|s| s.to_string())
            .or_else(|| self.threads.get(self.thread_idx).map(|t| t.id.clone()))
            .unwrap_or_default();
        let on_visible = tid == vis || tid.is_empty();
        let scratch = if on_visible {
            self.scratch()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == tid)
                .map(|t| t.scratch)
                .unwrap_or(false)
        };
        let pairs = if on_visible {
            self.chip_chat_pairs()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == tid)
                .map(|t| {
                    t.messages
                        .iter()
                        .map(|(r, c)| (r.clone(), chip_scan(c).to_string()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let user_turns = visible_turn_count(&pairs);
        let locked = self
            .threads
            .iter()
            .find(|t| t.id == tid)
            .map(|t| t.title_locked)
            .unwrap_or(false);
        if locked || !should_name_thread(scratch, user_turns) {
            self.goal_stale = false;
            return;
        }
        if tid.is_empty() {
            self.goal_stale = false;
            return;
        }
        // Built-in topics from the chat — no key, network, or GOAL: parse.
        let topics = topics_from_turns(&pairs);
        self.goal_stale = false;
        if topics.is_empty() {
            return;
        }
        let reply = format!("GOAL: {}", topics.join(", "));
        self.apply_thread_goal(&tid, &reply);
    }

    /// Whether this chat's prompt asks the model for its own chips: never for
    /// a side question or an unwatched run, and not while they keep losing.
    pub(super) fn reply_chips_on(&self) -> bool {
        self.cfg.suggestions
            && !self.scheduled_perm
            && grokhub_core::chip_trailer_on(&self.chip_memory, now_ms())
    }

    /// A new turn: no block half read.
    pub(super) fn begin_reply_chips(&mut self) {
        self.chip_live.filter = Default::default();
    }

    /// The part of a streamed chunk that is shown: a reply's chips block is
    /// held back, thinking passes as it came.
    pub(super) fn shown_chunk<'a>(
        &mut self,
        kind: LiveKind,
        chunk: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        match kind {
            LiveKind::Say => self.chip_live.filter.feed(chunk).into(),
            _ => chunk.into(),
        }
    }

    /// The finished reply without its chips block. The block's chips are
    /// saved with the chat the reply belongs to, with the turn's mode, and
    /// the rule is reviewed against how they did.
    pub(super) fn take_reply_chips(&mut self, mut text: String) -> String {
        let (tail, mut block) = self.chip_live.filter.finish();
        text.push_str(&tail);
        let (text, late) = grokhub_core::split_chip_trailer(&text);
        if block.trim().is_empty() {
            block = late;
        }
        grokhub_core::review_chip_trailer(&mut self.chip_memory, now_ms());
        let chips = grokhub_core::parse_chip_trailer(&block);
        if self.job_is_idea_talk() || chips.is_empty() {
            return text;
        }
        let visible = self.visible_thread_id();
        let id = self
            .chat_job_thread
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| visible.clone());
        let live_turns = user_turns(&self.messages);
        let mode = self.session_mode.as_str().to_string();
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
            let at = if id == visible {
                live_turns
            } else {
                user_turns(&t.messages)
            };
            t.follow_ups = Some(crate::threads::FollowUps { chips, at, mode });
        }
        text
    }

    /// The saved follow-ups of the chat on screen, while they still belong
    /// to its last reply.
    fn follow_ups_here(&self) -> Option<&crate::threads::FollowUps> {
        let id = self.visible_thread_id();
        let turns = user_turns(&self.messages);
        self.threads
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.follow_ups.as_ref())
            .filter(|f| f.at == turns)
    }

    /// The model's chips for the reply on screen; none once the chat moved on.
    pub(super) fn reply_chips_here(&self) -> Vec<QuickChip> {
        self.follow_ups_here()
            .map(|f| f.chips.clone())
            .unwrap_or_default()
    }

    /// A follow-up runs in the mode of the turn it came from.
    fn take_follow_up_mode(&mut self) {
        let Some(mode) = self
            .follow_ups_here()
            .and_then(|f| SessionMode::parse(&f.mode))
        else {
            return;
        };
        if mode != self.session_mode {
            self.set_session_mode(mode);
            let name = match mode {
                SessionMode::Plan => "Plan",
                SessionMode::Ask => "Ask",
                SessionMode::Chat => "Chat",
            };
            self.status = format!("Back in {name} for this follow-up.");
        }
    }

    /// Note what a Do chip changed; its Undo chip leads the row for a while.
    pub(super) fn set_chip_undo(&mut self, label: &str, kind: UndoKind) {
        self.chip_live.undo = Some(ChipUndo {
            label: label.to_string(),
            at_ms: now_ms(),
            kind,
        });
        self.chip_paint_key.clear();
    }

    /// The Undo chip, while the last Do chip can still be taken back.
    fn undo_chip(&self) -> Option<QuickChip> {
        let u = self.chip_live.undo.as_ref()?;
        (now_ms().saturating_sub(u.at_ms) < UNDO_MS).then(|| QuickChip {
            id: grokhub_core::UNDO_CHIP_ID.into(),
            label: "Undo".into(),
            value: String::new(),
            kind: ChipKind::Chat,
            score: 0.0,
            hint: format!("Takes back: {}", u.label),
            primary: false,
        })
    }

    /// A row with the Undo chip first, when there is one.
    pub(super) fn with_undo(&self, mut chips: Vec<QuickChip>) -> Vec<QuickChip> {
        if let Some(undo) = self.undo_chip() {
            chips.insert(0, undo);
            chips.truncate(grokhub_core::CHIP_ROW_MAX);
        }
        chips
    }

    fn take_undo(&mut self) {
        let Some(u) = self.chip_live.undo.take() else {
            return;
        };
        match u.kind {
            UndoKind::BackupSnooze(state, reminder) => self.undo_backup_snooze(*state, reminder),
            UndoKind::RadarRemind(id) => self.undo_radar_remind(id),
        }
        self.chip_paint_key.clear();
    }

    pub(super) fn refresh_chips(&mut self) {
        if self.running {
            return;
        }
        if !self.cfg.suggestions {
            self.visible_chips.clear();
            self.home_chips.clear();
            return;
        }
        let hour = Self::chip_hour();
        let n = self.messages.len();
        let last = self.messages.last().map(|m| m.1.len()).unwrap_or(0);
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let last_failed = self.last_receipt_ok == Some(false);
        let draft_head: String = self.composer.chars().take(16).collect();
        let draft_tail: String = self.composer.chars().rev().take(16).collect();
        let last_slash = self.chip_memory.last_slash.clone().unwrap_or_default();
        let last_surface = self.chip_memory.last_surface.clone().unwrap_or_default();
        let first_run = is_cabin_first_run(self.cfg.get_started_done, self.has_real_history());
        let last_project = Self::last_project_title_from(
            &self.last_night_hint(),
            &self.continue_hint,
            &self.cfg.goal_pin,
        );
        let skill_count = self.skill_list.len();
        let session_mode = self.session_mode.as_str().to_string();
        let game = self
            .game_running()
            .and_then(|id| self.steam_cached_name(id))
            .map(str::to_string);
        let game_ended = self.game_just_ended().map(str::to_string);
        let features = self.feature_chips();
        let feature_ids: Vec<&str> = features.iter().map(|c| c.id.as_str()).collect();
        let guard = self.chip_guard();
        let attachment = self.attached_file_name();
        let reply = self.reply_chips_here();
        let reply_ids: Vec<&str> = reply.iter().map(|c| c.id.as_str()).collect();
        let signals_key = ChipSignalsKey {
            low_disk: super::home::cached_drives()
                .iter()
                .any(|d| grokhub_core::disk_is_low(d.free, d.total)),
            weekday: Self::local_clock().weekday,
            update_waiting: self.cabin_update_available(),
            quiet_hours: self.quiet_now(),
            gaming: guard.gaming,
            anticheat: guard.anticheat.is_some(),
        };
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            self.thread_idx,
            n,
            last,
            self.composer.len(),
            draft_head,
            draft_tail,
            hour,
            last_failed,
            title,
            self.chip_dismissed.len(),
            self.has_key(),
            self.llm_ready(),
            self.cabin_signed_in(),
            self.usage.messages,
            last_slash,
            last_surface,
            first_run,
            last_project,
            session_mode,
            persistable_cabin_lane(&self.cfg.cabin_lane),
            game,
            game_ended,
            signals_key,
            feature_ids,
            attachment,
            reply_ids
        );
        if self.chip_paint_key == key && !self.visible_chips.is_empty() {
            return;
        }
        self.chip_paint_key = key;
        let chat = self.chip_chat_pairs();
        let others = self.other_chip_threads();
        let input = ChipInput {
            chat: &chat,
            draft: &self.composer,
            grok_connected: self.cabin_signed_in(),
            // The router alone picks the model: chips always see Auto.
            mode: "auto",
            thread_title: &title,
            usage_messages: self.usage.messages,
            memory: &self.chip_memory,
            dismissed: &self.chip_dismissed,
            reply_chips: &reply,
            last_failed,
            hour,
            now_ms: now_ms(),
            // The engine gathers a pool; the ranker picks the row from it.
            max: grokhub_core::CHIP_HARD_MAX,
            other_threads: &others,
            first_run,
            last_slash: &last_slash,
            last_surface: &last_surface,
            last_project: &last_project,
            skill_count: skill_count as u32,
            session_mode: &session_mode,
            signals: ChipSignals {
                game: game.as_deref(),
                game_ended: game_ended.as_deref(),
                low_disk: signals_key.low_disk,
                weekday: signals_key.weekday,
                update_waiting: signals_key.update_waiting,
                quiet_hours: signals_key.quiet_hours,
                gaming: signals_key.gaming,
                anticheat: signals_key.anticheat,
                attachment: attachment.as_deref(),
            },
            extra: &features,
        };
        let pace = grokhub_core::cabin_pace(&input);
        self.offer_repeated = matches!(
            pace,
            grokhub_core::CabinPace::With | grokhub_core::CabinPace::Quiet
        );
        // Home starts a new chat, so its row is the same engine for an empty
        // one: no transcript, no draft, the PC's signals, seeds to fill.
        let home = ChipInput {
            chat: &[],
            draft: "",
            thread_title: "",
            last_failed: false,
            max: grokhub_core::CHIP_HARD_MAX,
            // Feature offers, the reply's chips and the tray's file belong
            // to the open chat.
            extra: &[],
            reply_chips: &[],
            signals: ChipSignals {
                attachment: None,
                ..input.signals
            },
            ..input.clone()
        };
        // Chips are local only: no model ask, cloud or local (LO-28).
        let home_pool = build_quick_chips(home);
        let mut pool = build_quick_chips(input);
        bias_chips_for_lane(&mut pool, cabin_lane(&self.cfg.cabin_lane));
        // One draw per boundary (thread, turn, hour), never per keystroke,
        // so the row holds still while the person types.
        let mut seed = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.thread_idx, n, last, hour).hash(&mut h);
            h.finish()
        };
        // The seed takes the hour, so 1 boundary in 20 lands in the holdout
        // at some hours and not others. Tests here need a row every time;
        // the holdout itself is proven in core's chip health tests.
        while cfg!(test) && grokhub_core::chip_health::in_holdout(seed) {
            seed = seed.wrapping_add(1);
        }
        let ctx = |surface, max| grokhub_core::RankCtx {
            surface,
            hour,
            now_ms: now_ms(),
            seed,
            max,
        };
        self.home_chips = grokhub_core::rank_quick_chips(
            &mut self.chip_memory,
            home_pool,
            ctx("home", HOME_CHIP_MAX),
        );
        self.visible_chips = grokhub_core::rank_quick_chips(
            &mut self.chip_memory,
            pool,
            ctx("composer", grokhub_core::CHIP_ROW_MAX),
        );
    }

    /// Every feature's offer for the open chat, with the score it ranks at.
    /// Each was spliced in front of the row before; now the engine weighs
    /// it against the chat's own chips. All read cached state only.
    pub(super) fn feature_chips(&self) -> Vec<QuickChip> {
        fn scored(chips: Vec<QuickChip>, score: f32, hint: &str) -> Vec<QuickChip> {
            chips
                .into_iter()
                .map(|mut c| {
                    c.score = score;
                    if c.hint.trim().is_empty() {
                        c.hint = hint.to_string();
                    }
                    c
                })
                .collect()
        }
        let mut out: Vec<QuickChip> = Vec::new();
        out.extend(scored(
            skill_offer_chip(&self.composer, &self.skill_list)
                .into_iter()
                .collect(),
            FEATURE_SKILL_SCORE,
            "A skill fits what you're typing",
        ));
        out.extend(scored(
            self.focus_chip().into_iter().collect(),
            FEATURE_FOCUS_SCORE,
            "The app you were just in",
        ));
        out.extend(scored(
            self.model_chips(),
            FEATURE_SCORE + 4.0,
            "New model file",
        ));
        out.extend(scored(
            self.resume_chips(),
            FEATURE_SCORE + 3.0,
            "Code you left",
        ));
        out.extend(scored(
            self.backup_chips(),
            FEATURE_SCORE + 2.0,
            "Backup due",
        ));
        out.extend(scored(
            self.tidy_chips(),
            FEATURE_SCORE,
            "Downloads cleanup",
        ));
        out.extend(scored(self.goal_chips(), FEATURE_SCORE + 1.0, "Your goal"));
        out.extend(scored(self.game_chips(), FEATURE_SCORE, "Your game"));
        out.extend(scored(
            self.radar_chips(),
            FEATURE_SCORE - 2.0,
            "Release radar",
        ));
        // Order inside one feature is its own: keep it among equal scores.
        for (i, c) in out.iter_mut().enumerate() {
            c.score -= i as f32 * 0.001;
        }
        out.retain(|c| !self.chip_hidden(c));
        out
    }

    /// The first file waiting in the attach tray, by name only.
    fn attached_file_name(&self) -> Option<String> {
        let item = self.attach_tray.queue.items().first()?;
        std::path::Path::new(&item.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.trim().is_empty())
    }

    /// Ranked composer chips. Empty home and mid-thread share `visible_chips`
    /// (habit / static / stage pool when the LLM row is not ready). A thread
    /// with messages must not clear that pool.
    pub(super) fn composer_chips(&self) -> Vec<QuickChip> {
        // The fixed slide-02 four are for deck screenshots only. Real chats get
        // the ranked pool (those four live on as seeds inside it).
        if super::loadout_home::deck_shot_requested()
            && super::loadout_home::hangout_composer_layout(self.messages.is_empty())
        {
            return super::loadout_home::hangout_quick_chips();
        }
        // Feature offers (focus help, model drop, code resume, tidy, backup,
        // radar, game, skill offer) are already in the ranked pool.
        let mut chips = self.visible_chips.clone();
        polish_chip_hints(&mut chips, &self.composer);
        // A shell chip fills the box with its command; with a draft typed
        // there is no room for it, so it waits until the box is empty.
        if !self.composer.trim().is_empty() {
            chips.retain(|c| c.kind != ChipKind::Shell);
        }
        chips.truncate(grokhub_core::CHIP_ROW_MAX);
        let mut chips = self.with_undo(chips);
        mark_first_primary(&mut chips);
        chips
    }

    /// True when the person hid this chip, this session or for good.
    fn chip_hidden(&self, chip: &QuickChip) -> bool {
        grokhub_core::chip_dismissed_for_good(&self.chip_memory, chip)
            || self
                .chip_dismissed
                .iter()
                .any(|d| d == &chip.id || d == &chip.value)
    }

    /// Home's row. A chat chip starts its own chat, like an ask typed on
    /// Home; a page or mode chip acts as it does under the composer. Both
    /// are learned, and the × dismisses for Home and the composer alike.
    pub(super) fn take_home_chip_act(
        &mut self,
        act: crate::cards::ChipRowAct,
        chips: &[QuickChip],
    ) {
        if let crate::cards::ChipRowAct::Apply(i) = act {
            if chips.get(i).is_some_and(|c| c.kind == ChipKind::Chat) {
                self.new_thread(false);
                self.nav = Nav::Chat;
                self.composer.clear();
            }
        }
        self.take_chip_act_on("home", act, chips);
    }

    pub(super) fn take_chip_act(&mut self, act: crate::cards::ChipRowAct, chips: &[QuickChip]) {
        self.take_chip_act_on("composer", act, chips);
    }

    /// Note a shown row in the local chip log (no chip text, see
    /// `grokhub_core::chip_store`). Same ids as last time on this surface:
    /// nothing to do, so a repaint costs one hash.
    pub(super) fn note_chips_shown(&mut self, surface: &'static str, chips: &[QuickChip]) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for c in chips {
            c.id.hash(&mut h);
        }
        let sig = h.finish();
        match self.chips_logged.iter_mut().find(|(s, _)| *s == surface) {
            Some((_, last)) if *last == sig => return,
            Some((_, last)) => *last = sig,
            None => self.chips_logged.push((surface, sig)),
        }
        grokhub_core::log_chips_shown(&mut self.chip_memory, surface, chips, now_ms());
    }

    fn take_chip_act_on(
        &mut self,
        surface: &'static str,
        act: crate::cards::ChipRowAct,
        chips: &[QuickChip],
    ) {
        let i = match act {
            crate::cards::ChipRowAct::Apply(i) | crate::cards::ChipRowAct::Dismiss(i) => i,
        };
        // The Undo chip is not a suggestion: it is not logged or learned.
        if chips
            .get(i)
            .is_some_and(|c| c.id == grokhub_core::UNDO_CHIP_ID)
        {
            match act {
                crate::cards::ChipRowAct::Apply(_) => self.take_undo(),
                crate::cards::ChipRowAct::Dismiss(_) => self.chip_live.undo = None,
            }
            self.chip_paint_key.clear();
            return;
        }
        match act {
            crate::cards::ChipRowAct::Apply(i) => {
                if let Some(c) = chips.get(i).cloned() {
                    grokhub_core::log_chip_outcome(
                        &mut self.chip_memory,
                        surface,
                        &c,
                        true,
                        now_ms(),
                    );
                    // A shown row that was acted on is closed: the next showing is new.
                    self.chips_logged.retain(|(s, _)| *s != surface);
                    self.apply_chip(c);
                }
            }
            crate::cards::ChipRowAct::Dismiss(i) => {
                if let Some(c) = chips.get(i).cloned() {
                    grokhub_core::log_chip_outcome(
                        &mut self.chip_memory,
                        surface,
                        &c,
                        false,
                        now_ms(),
                    );
                    self.chips_logged.retain(|(s, _)| *s != surface);
                    self.dismiss_chip(c);
                    self.refresh_chips();
                }
            }
        }
    }

    pub(super) fn apply_chip(&mut self, chip: QuickChip) {
        // A retired model/effort chip from old memory does nothing at all:
        // the router alone picks the model and effort.
        if grokhub_core::retired_mode_value(&chip.value) {
            return;
        }
        // A new tap closes the last Do chip's undo.
        self.chip_live.undo = None;
        // One-shot chips act here and are not learned: replayed later they would mean nothing.
        // The tidy review chip opens a window; it is not sent or learned either.
        if self.take_tidy_chip(&chip)
            || self.take_backup_chip(&chip)
            || self.take_radar_chip(&chip)
            || self.take_goal_chip(&chip)
            || self.take_game_chip(&chip)
        {
            return;
        }
        let hour = Self::chip_hour();
        let mode = "auto";
        let tag = context_fingerprint(
            &self.chip_chat_pairs(),
            &self.composer,
            self.last_receipt_ok == Some(false),
            hour,
            mode,
        );
        remember_chip_click(&mut self.chip_memory, &chip, Some(&tag), now_ms(), hour);
        grokhub_core::remember_intent(
            &mut self.chip_memory,
            &chip.label,
            &chip.value,
            now_ms(),
            hour,
            Self::local_clock().weekday,
        );
        self.flush_chips();
        // One tap rule per kind (see `grokhub_core::ChipAction`). The Do
        // one-taps already ran above.
        match grokhub_core::chip_action(&chip) {
            ChipAction::Do => {}
            ChipAction::Go => {
                if nav_from_chip_value(&chip.value) == Some("update") {
                    self.open_update_overlay();
                } else if let Some(id) = nav_from_chip_value(&chip.value) {
                    self.nav = Self::nav_from_id(id);
                    if id == "imagine" {
                        remember_home_surface(&mut self.chip_memory, "imagine", now_ms());
                    } else if id == "skills" {
                        remember_home_surface(&mut self.chip_memory, "skills", now_ms());
                    }
                }
            }
            // A shell chip never runs on tap: it puts the command in the box
            // so the person sees it, and Enter runs it through the same path
            // (and the same destructive-command confirm) as a typed `/sh`.
            ChipAction::Review => {
                self.composer = shell_chip_draft(&chip.value);
                self.composer_want_focus = true;
            }
            ChipAction::Ask => {
                if chip
                    .id
                    .starts_with(grokhub_core::chip_trailer::REPLY_CHIP_PREFIX)
                {
                    self.take_follow_up_mode();
                }
                // A bare `/plan` chip used to flip a hidden flag and clear the
                // box: it looked like nothing happened. It now puts `/plan`
                // in the box, before what was typed, ready for Enter.
                if is_bare_plan_chip(&chip.value) {
                    self.composer = plan_chip_draft(&self.composer);
                    self.composer_chip_text = self.composer.clone();
                    self.composer_want_focus = true;
                    self.status = PLAN_CHIP_NOTE.into();
                    return;
                }
                // Slash chips use the typed send path so `/learn` click and Enter
                // both parse locally instead of inserting a no-op or hitting Grok.
                if chip_chat_runs_immediately(&chip, &self.composer) {
                    self.composer.clear();
                    // The send below is the chip's turn, not a typed one.
                    self.chip_send_key = Some(grokhub_core::chip_memory_key(&chip));
                    // A send that can't go out (signed out) puts the text back
                    // in the box; it is still the chip's text, not a draft.
                    self.composer_chip_text = chip.value.clone();
                    self.send_chat(chip.value);
                    self.chip_send_key = None;
                } else {
                    // Never replace what the person typed: add to it. Text a
                    // chip put there (and nobody edited) is swapped instead.
                    let typed = if self.composer == self.composer_chip_text {
                        ""
                    } else {
                        self.composer.as_str()
                    };
                    self.composer = draft_with_chip(typed, &chip.value);
                    self.composer_chip_text = self.composer.clone();
                    self.composer_want_focus = true;
                }
            }
        }
    }

    pub(super) fn dismiss_chip(&mut self, chip: QuickChip) {
        remember_chip_dismiss(&mut self.chip_memory, &chip, now_ms(), Self::chip_hour());
        self.chip_dismissed.push(chip.id);
        self.chip_dismissed.push(chip.value);
        self.flush_chips();
    }

    pub(super) fn flush_chips(&self) {
        let chips = self.chip_memory.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_chips(&chips);
            }
        });
    }

    /// Run start. `kick_native_turn` calls this after the prompt succeeds.
    /// A Doing card is filed only when the ask is a task. Ordinary chat does not.
    /// Writes `workboard.json` through `flush_board` when the card changes.
    pub(super) fn note_inflight_card(&mut self, ask: &str, thread_label: &str) {
        if self.job_is_idea_talk() {
            return;
        }
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
        // KB-03: a resume turn runs on the card it resumes, already in Doing.
        if self.work_resume.pending.take().is_some() {
            self.inflight_open = true;
            self.begin_run_card(&id);
            self.flush_board();
            return;
        }
        // A reply in a Follow up card's chat is that card's work, not a new Doing card.
        if self
            .board
            .iter()
            .any(|c| c.automation.is_some() && c.thread_id.as_deref() == Some(id.as_str()))
        {
            return;
        }
        let title = inflight_card_title(ask, thread_label);
        if title.trim().is_empty() {
            return;
        }
        if !self.inflight_open {
            release_inflight_card(&mut self.board, &id);
        }
        self.inflight_open = true;
        if upsert_inflight_card(&mut self.board, &id, &title) {
            self.begin_run_card(&id);
            self.flush_board();
        }
    }

    /// Run complete. `finish_acp_turn` calls this before it drops `chat_job_thread`.
    /// Doing → done, then `WORK_PIN:` / `WORK_UPDATE:` lines in the assistant text.
    pub(super) fn settle_turn_card(&mut self, assistant: &str) {
        self.work_resume.follow = None;
        let Some(id) = self.chat_job_thread.clone() else {
            self.inflight_open = false;
            return;
        };
        if self.inflight_open {
            self.end_run_card(&id, &grokhub_core::work_run::RunEnd::Done);
        }
        self.inflight_open = false;
        let mut changed = settle_inflight_card(&mut self.board, &id);
        if apply_assistant_work_marks(&mut self.board, assistant, &id) {
            changed = true;
        }
        if changed {
            self.flush_board();
            if let Some(title) = self
                .board
                .iter()
                .find(|c| {
                    c.thread_id.as_deref() == Some(id.as_str())
                        && c.status == grokhub_core::BoardStatus::Done
                })
                .map(|c| c.title.clone())
            {
                let key = format!("done:{}", grokhub_core::engine_slug(&title));
                self.engine_note("workboard", &key, &title);
            }
        }
    }

    /// The turn never finished. Undoes the Doing card this attempt filed.
    pub(super) fn abandon_turn_card(&mut self) {
        self.drop_pending_resume();
        if !self.inflight_open {
            return;
        }
        self.inflight_open = false;
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
        self.end_run_card(&id, &grokhub_core::work_run::RunEnd::Stopped);
        if abandon_inflight_card(&mut self.board, &id) {
            self.flush_board();
            if let Some(title) = self
                .board
                .iter()
                .find(|c| {
                    c.thread_id.as_deref() == Some(id.as_str())
                        && c.status == grokhub_core::BoardStatus::Blocked
                })
                .map(|c| c.title.clone())
            {
                let key = format!("paused:{}", grokhub_core::engine_slug(&title));
                self.engine_note("workboard", &key, &title);
            }
        }
    }

    /// Each frame, cheaply: pick up another writer's board save (checked every
    /// two seconds), and run the local sweep at start and every six hours. No
    /// model is involved.
    pub(super) fn tick_board_store(&mut self) {
        let path = config::workboard_path();
        crate::work_store::reload_if_changed(&path, &mut self.board);
        let now = grokhub_core::now_ms();
        self.tick_board_sweep(now);
        self.tick_board_queue(now);
    }

    pub(super) fn flush_board(&mut self) {
        let board = self.board.clone();
        let io = self.persist_io.clone();
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = config::save_board(&board);
            }
        });
    }
}

/// Composer quick-chip row (paint + layout). Restyled with ui_kit tokens.
pub mod row {
    use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
    use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, StrokeKind};
    use std::sync::Arc;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ChipRowAct {
        Apply(usize),
        Dismiss(usize),
    }

    pub fn chip_paint_label(label: &str) -> String {
        label
            .split_whitespace()
            .flat_map(|w| w.split('…'))
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub const CHIP_LABEL_MAX_W: f32 = 200.0;
    pub const CHIP_PAD_X: f32 = crate::theme::SPACING_12;
    #[cfg(test)]
    pub const CHIP_PAD_Y: f32 = 8.0;
    pub const CHIP_GAP: f32 = crate::theme::SPACING_8;
    pub const CHIP_DISMISS_SLOT: f32 = crate::theme::MIN_HIT * 0.55;
    pub const CHIP_DISMISS_GAP: f32 = crate::theme::SPACING_4;
    /// One fixed chip height, a full touch target. Does not grow when a
    /// label is long.
    pub const CHIP_ROW_H: f32 = 48.0;
    /// Chip text: 16 px, easy to read without leaning in.
    pub const CHIP_FONT: f32 = crate::theme::TYPE_TITLE;

    pub fn chip_row_width_lock(avail: f32) -> f32 {
        avail.clamp(120.0, crate::theme::CHAT_COL_W)
    }

    pub fn chip_row_visible_w(ui: &egui::Ui) -> f32 {
        let left = ui.cursor().left();
        let clip_w = (ui.clip_rect().right() - left).max(0.0);
        let avail = ui.available_width().min(clip_w);
        let col = crate::cards::composer_pill_w(ui.ctx().content_rect().width());
        chip_row_width_lock(avail.min(col))
    }

    pub fn chip_pill_width(label_w: f32) -> f32 {
        CHIP_PAD_X + label_w.max(0.0) + CHIP_DISMISS_GAP + CHIP_DISMISS_SLOT + CHIP_PAD_X
    }

    pub fn chip_dismiss_rect(pill: egui::Rect) -> egui::Rect {
        egui::Rect::from_center_size(
            egui::pos2(
                pill.right() - CHIP_PAD_X - CHIP_DISMISS_SLOT * 0.5,
                pill.center().y,
            ),
            egui::vec2(CHIP_DISMISS_SLOT, CHIP_DISMISS_SLOT),
        )
    }

    pub fn chip_hit_act(index: usize, dismiss: egui::Rect, pointer: egui::Pos2) -> ChipRowAct {
        if dismiss.contains(pointer) {
            ChipRowAct::Dismiss(index)
        } else {
            ChipRowAct::Apply(index)
        }
    }

    pub fn chip_label_budget(row_w: f32) -> f32 {
        let chrome = chip_pill_width(0.0);
        (row_w - chrome).clamp(32.0, CHIP_LABEL_MAX_W)
    }

    pub fn fluid_chip_count(widths: &[f32], row_w: f32, gap: f32) -> usize {
        if widths.is_empty() || row_w <= 0.0 {
            return 0;
        }
        let mut used = 0.0;
        let mut n = 0usize;
        for (i, w) in widths.iter().enumerate() {
            let add = if i == 0 { *w } else { gap + *w };
            if used + add > row_w + 0.5 {
                break;
            }
            used += add;
            n += 1;
        }
        n
    }

    pub fn chip_cluster_w(widths: &[f32], fit: usize, gap: f32) -> f32 {
        let n = fit.min(widths.len());
        let pills: f32 = widths[..n].iter().sum();
        pills + gap * n.saturating_sub(1) as f32
    }

    pub fn chip_cluster_lead(row_w: f32, cluster_w: f32, ppp: f32) -> f32 {
        let lead = ((row_w - cluster_w) * 0.5).max(0.0);
        let ppp = if ppp > 0.0 { ppp } else { 1.0 };
        (lead * ppp).round() / ppp
    }

    #[cfg(test)]
    pub(crate) fn begin_chip_dismiss_hits(ui: &egui::Ui) {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("qchip-dismiss-hits"),
                Vec::<egui::Rect>::new(),
            );
            d.insert_temp(egui::Id::new("qchip-pills"), Vec::<egui::Rect>::new());
        });
    }

    #[cfg(test)]
    pub(crate) fn remember_chip_dismiss_hit(ui: &egui::Ui, rect: egui::Rect) {
        ui.ctx().data_mut(|d| {
            let id = egui::Id::new("qchip-dismiss-hits");
            let mut hits = d.get_temp::<Vec<egui::Rect>>(id).unwrap_or_default();
            hits.push(rect);
            d.insert_temp(id, hits);
        });
    }

    #[cfg(test)]
    pub(crate) fn remember_chip_pill(ui: &egui::Ui, rect: egui::Rect) {
        ui.ctx().data_mut(|d| {
            let id = egui::Id::new("qchip-pills");
            let mut pills = d.get_temp::<Vec<egui::Rect>>(id).unwrap_or_default();
            pills.push(rect);
            d.insert_temp(id, pills);
        });
    }

    pub fn layout_chip_label(
        ui: &egui::Ui,
        text: &str,
        font: FontId,
        color: Color32,
        max_w: f32,
    ) -> Arc<egui::Galley> {
        let mut job = LayoutJob::single_section(
            text.to_owned(),
            TextFormat {
                font_id: font,
                color,
                ..Default::default()
            },
        );
        job.wrap = TextWrapping::truncate_at_width(max_w.max(1.0));
        job.break_on_newline = false;
        ui.fonts_mut(|f| f.layout_job(job))
    }

    pub const CHIP_EMPTY_LABEL: &str = "Nothing queued";

    fn chip_fill(primary: bool, hovered: bool) -> Color32 {
        if primary {
            crate::theme::blend_color(
                crate::theme::selection(),
                crate::theme::surface_hover(),
                if hovered { 0.35 } else { 0.15 },
            )
        } else if hovered {
            crate::theme::surface_hover()
        } else {
            crate::theme::elevated()
        }
    }

    pub fn quick_chip_fill(primary: bool) -> Color32 {
        chip_fill(primary, false)
    }

    pub fn quick_chip_fg(primary: bool) -> Color32 {
        if primary {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        }
    }

    pub fn quick_chip_stroke_w(primary: bool) -> f32 {
        if primary {
            1.5
        } else {
            1.0
        }
    }

    pub fn quick_chip_stroke(primary: bool) -> Color32 {
        if primary {
            crate::theme::border_strong()
        } else {
            crate::theme::border()
        }
    }

    fn quick_chip_stroke_paint(primary: bool) -> Stroke {
        Stroke::new(quick_chip_stroke_w(primary), quick_chip_stroke(primary))
    }

    pub fn quick_chip_strong(primary: bool) -> bool {
        primary
    }

    /// One hint segment in plain words. The engine's hints were written for
    /// debugging ("Predicted · finish · Mid-thread"); the tooltip is for people.
    fn plain_why_part(part: &str) -> Option<String> {
        let part = part.trim();
        let intent = |s: &str| -> Option<&'static str> {
            Some(match s.trim() {
                "finish" | "finish work" => "finish the job",
                "fix" | "fix something" => "fix something",
                "ship" | "build/ship" => "build or ship it",
                "explain" => "understand it",
                "host" | "run on host" => "run something on your PC",
                "decide" => "make a call",
                "create" => "make something",
                "continue" => "keep going",
                "review" => "check the work",
                "chat" => "keep chatting",
                _ => return None,
            })
        };
        if part.is_empty() || part == "Predicted" {
            return None;
        }
        if let Some(i) = intent(part) {
            return Some(format!("Looks like you want to {i}"));
        }
        if let Some(rest) = part.strip_prefix("Predicted next:") {
            return Some(match intent(rest) {
                Some(i) => format!("Looks like you want to {i}"),
                None => "A likely next step".into(),
            });
        }
        let plain = match part {
            "Highest-ranked next action" => "A likely next step",
            "Mid-thread" => "You're mid-conversation",
            "Predicted for this time of day" => "You often do this around now",
            "Predicted — last attempt failed" => "The last try didn't work",
            "Predicted incomplete turn" => "The last reply looks unfinished",
            "Predicted: you need action, not another plan" => {
                "The reply was a plan; this acts on it"
            }
            _ if part.starts_with("Predicted match") => "Matches what you're typing",
            _ if part.starts_with("Predicted") => "Based on what you're typing",
            _ => return Some(part.to_string()),
        };
        Some(plain.to_string())
    }

    /// A chip hint in plain words, one reason per line, no repeats.
    fn plain_why(hint: &str) -> String {
        let mut lines: Vec<String> = Vec::new();
        for part in hint.split(['\n', '·']) {
            if let Some(p) = plain_why_part(part) {
                if !lines.contains(&p) {
                    lines.push(p);
                }
            }
        }
        lines.join("\n")
    }

    pub fn chip_why_tip(hint: &str, label: &str) -> String {
        let why = hint.trim();
        if why == "New here" {
            return "Suggested because you're new here".into();
        }
        let why = plain_why(why);
        if !why.is_empty() {
            return format!("Why this?\n{why}");
        }
        let label = label.trim();
        if label.is_empty() {
            "Why this?".into()
        } else {
            format!("Why this?\n{label}")
        }
    }

    fn paint_chip_pill(
        ui: &mut egui::Ui,
        rect: egui::Rect,
        fill: Color32,
        primary: bool,
        resp: &egui::Response,
    ) {
        let stroke = quick_chip_stroke_paint(primary);
        let body = rect.shrink(stroke.width * 0.5);
        let radius = crate::theme::RADIUS_MD;
        ui.painter()
            .rect(body, radius, fill, stroke, StrokeKind::Inside);
        crate::theme::paint_focus_ring(ui, body, radius, resp);
    }

    pub fn paint_empty_chip_state(ui: &mut egui::Ui) {
        let max_w = chip_row_visible_w(ui);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, CHIP_ROW_H),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_wrap(false)
                .with_main_align(egui::Align::Min),
            |ui| {
                ui.set_max_width(max_w);
                #[cfg(test)]
                begin_chip_dismiss_hits(ui);
                let color = quick_chip_fg(false);
                let budget = chip_label_budget(max_w);
                let galley = layout_chip_label(
                    ui,
                    CHIP_EMPTY_LABEL,
                    FontId::proportional(CHIP_FONT),
                    color,
                    budget,
                );
                let pill_w = (CHIP_PAD_X + galley.size().x.max(8.0) + CHIP_PAD_X).min(max_w);
                ui.add_space(chip_cluster_lead(
                    max_w,
                    pill_w,
                    ui.ctx().pixels_per_point(),
                ));
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(pill_w, CHIP_ROW_H), Sense::hover());
                #[cfg(test)]
                remember_chip_pill(ui, rect);
                let fill = if resp.hovered() {
                    chip_fill(false, true)
                } else {
                    quick_chip_fill(false)
                };
                paint_chip_pill(ui, rect, fill, false, &resp);
                let text_pos = egui::pos2(
                    rect.left() + CHIP_PAD_X,
                    rect.center().y - galley.size().y * 0.5,
                );
                ui.painter().galley(text_pos, galley, color);
            },
        );
    }

    /// Alt+1, Alt+2 and Alt+3 pick the chip in that slot.
    const CHIP_KEYS: [egui::Key; 3] = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3];

    fn chip_key_pick(ui: &egui::Ui, shown: usize) -> Option<usize> {
        CHIP_KEYS
            .iter()
            .take(shown)
            .position(|k| ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, *k)))
    }

    pub fn quick_chip_row(
        ui: &mut egui::Ui,
        chips: &[grokhub_core::QuickChip],
    ) -> Option<ChipRowAct> {
        if chips.is_empty() {
            paint_empty_chip_state(ui);
            return None;
        }
        let mut act = None;
        let max_w = chip_row_visible_w(ui);
        let budget = chip_label_budget(max_w);
        let measured: Vec<Arc<egui::Galley>> = chips
            .iter()
            .map(|c| {
                let font = if quick_chip_strong(c.primary) {
                    crate::theme::title_font(CHIP_FONT)
                } else {
                    FontId::proportional(CHIP_FONT)
                };
                let color = quick_chip_fg(c.primary);
                layout_chip_label(ui, &chip_paint_label(&c.label), font, color, budget)
            })
            .collect();
        let widths: Vec<f32> = measured
            .iter()
            .map(|g| chip_pill_width(g.size().x.min(budget)).min(max_w))
            .collect();
        let fit = fluid_chip_count(&widths, max_w, CHIP_GAP);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, CHIP_ROW_H),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_wrap(false)
                .with_main_align(egui::Align::Min),
            |ui| {
                ui.set_max_width(max_w);
                ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, 0.0);
                crate::theme::glide_paint(ui, "quick-chips");
                let mut chip_hover: Option<egui::Rect> = None;
                #[cfg(test)]
                begin_chip_dismiss_hits(ui);
                let cluster_w = chip_cluster_w(&widths, fit, CHIP_GAP);
                ui.add_space(chip_cluster_lead(
                    max_w,
                    cluster_w,
                    ui.ctx().pixels_per_point(),
                ));
                let mut used = 0.0f32;
                // Tab reaches the chips, egui's arrow keys move between them
                // and Enter or Space picks one; Alt+1/2/3 picks by slot.
                let mut shown = 0usize;
                for (i, c) in chips.iter().enumerate().take(fit) {
                    let galley = measured[i].clone();
                    let pill_w = widths[i];
                    let add = if used == 0.0 {
                        pill_w
                    } else {
                        CHIP_GAP + pill_w
                    };
                    if used + add > max_w + 0.5 {
                        break;
                    }
                    used += add;
                    let chip_id = ui.id().with(("qchip", i));
                    let color = quick_chip_fg(c.primary);
                    let paint = chip_paint_label(&c.label);
                    let why = if c.hint.is_empty() && paint != c.label {
                        c.label.as_str()
                    } else {
                        c.hint.as_str()
                    };
                    let tip = match CHIP_KEYS.get(i) {
                        Some(_) => format!("{}  (Alt+{})", chip_why_tip(why, &c.label), i + 1),
                        None => chip_why_tip(why, &c.label),
                    };
                    let (rect, hit_resp) =
                        ui.allocate_exact_size(egui::vec2(pill_w, CHIP_ROW_H), Sense::click());
                    let hit_resp = crate::theme::pointing(hit_resp);
                    shown += 1;
                    let hovered = hit_resp.hovered();
                    if hovered {
                        chip_hover = Some(rect);
                    }
                    let fill = chip_fill(c.primary, hovered);
                    let (_, felt, painted) =
                        crate::theme::feel_response(ui, hit_resp.clone(), fill);
                    paint_chip_pill(ui, felt, painted, c.primary, &hit_resp);
                    #[cfg(test)]
                    remember_chip_pill(ui, rect);
                    let text_pos = egui::pos2(
                        rect.left() + CHIP_PAD_X,
                        rect.center().y - galley.size().y * 0.5,
                    );
                    ui.painter().galley(text_pos, galley, color);
                    let x_rect = chip_dismiss_rect(rect);
                    #[cfg(test)]
                    remember_chip_dismiss_hit(ui, x_rect);
                    let dismiss_t = ui.ctx().animate_bool_with_time(
                        chip_id.with("dismiss"),
                        hovered,
                        grokhub_core::SELECT_SECS,
                    );
                    let over_x = hit_resp.hover_pos().is_some_and(|p| x_rect.contains(p));
                    let hit = if over_x {
                        hit_resp.on_hover_text("Hide this suggestion")
                    } else {
                        hit_resp.on_hover_text(tip)
                    };
                    let hit_resp = hit;
                    if dismiss_t > 0.01 {
                        if over_x {
                            ui.painter().rect_filled(
                                x_rect,
                                crate::theme::RADIUS_SM,
                                crate::theme::lift_fill(
                                    Color32::TRANSPARENT,
                                    grokhub_core::HOVER_WASH * dismiss_t,
                                ),
                            );
                        }
                        let x_color = crate::theme::blend_color(
                            Color32::TRANSPARENT,
                            crate::theme::subtle(),
                            dismiss_t,
                        );
                        ui.painter().text(
                            x_rect.center(),
                            Align2::CENTER_CENTER,
                            "×",
                            FontId::proportional(CHIP_FONT),
                            x_color,
                        );
                    }
                    if hit_resp.clicked() {
                        act = Some(match hit_resp.interact_pointer_pos() {
                            Some(pos) => chip_hit_act(i, x_rect, pos),
                            None => ChipRowAct::Apply(i),
                        });
                    }
                    if hit_resp.has_focus() {
                        crate::theme::paint_focus_ring(
                            ui,
                            felt,
                            crate::theme::RADIUS_MD,
                            &hit_resp,
                        );
                    }
                }
                if let Some(i) = chip_key_pick(ui, shown) {
                    act = Some(ChipRowAct::Apply(i));
                }
                if let Some(rect) = chip_hover {
                    crate::theme::glide_candidate(ui, "quick-chips", rect, true, false);
                }
                crate::theme::glide_aim(ui, "quick-chips");
            },
        );
        act
    }
    // quick_chip_row_end

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::theme::{hold_paint_test, set_paint_dark};

        #[test]
        fn chip_row_source_layout_contract() {
            let src = include_str!("chips.rs");
            let chip_row = src
                .split("pub fn quick_chip_row(")
                .nth(1)
                .and_then(|s| s.split("#[cfg(test)]").next())
                .expect("quick_chip_row");
            assert!(
                chip_row.contains("with_main_wrap(false)")
                    && chip_row.contains("fluid_chip_count")
                    && chip_row.contains("layout_chip_label")
                    && chip_row.contains("CHIP_ROW_H")
                    && chip_row.contains("chip_row_visible_w")
                    && !chip_row.contains("with_main_wrap(true)"),
                "chips are one fixed line, ellipsized, and drop overflow inside Ask anything"
            );
        }

        #[test]
        fn chip_row_height_meets_caption_pad() {
            let _hold = hold_paint_test();
            const {
                assert!(CHIP_ROW_H >= CHIP_FONT + CHIP_PAD_Y * 2.0);
                // A full touch target and text that reads without leaning in.
                assert!(CHIP_ROW_H >= 48.0 && CHIP_FONT >= 16.0 && CHIP_GAP >= 8.0);
            }
        }

        #[test]
        fn chip_pill_contrast_dark_and_light() {
            let _hold = hold_paint_test();
            for dark in [true, false] {
                set_paint_dark(dark);
                let fill = quick_chip_fill(true);
                let fg = quick_chip_fg(true);
                assert!(
                    crate::theme::contrast_ratio(fg, fill) >= 4.5,
                    "primary chip contrast dark={dark}"
                );
            }
        }
    }
}

#[cfg(test)]
mod chip_apply_tests {
    use super::*;
    use grokhub_core::{AttachKind, ChipKind, QuickChip};

    fn chat_chip(id: &str, label: &str, value: &str) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    #[test]
    fn chip_insert_keeps_draft_and_attachments() {
        let _hold = crate::config::hold_test_config();
        let root = crate::config::test_config_root("chip-insert");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };

        let mut cabin = Cabin::quiet_for_test();
        cabin.composer = "fix the leak".into();
        cabin.attach_path = Some("/tmp/leak.png".into());
        cabin.attach_name = Some("leak.png".into());
        cabin.attach_kind = Some(AttachKind::Image);

        let expanded = "fix the leak. Be concrete and complete the goal.";
        cabin.apply_chip(chat_chip("pred-expand-draft", "Expand & send", expanded));

        assert_eq!(cabin.composer, expanded);
        assert_eq!(cabin.attach_path.as_deref(), Some("/tmp/leak.png"));
        assert_eq!(cabin.attach_name.as_deref(), Some("leak.png"));
        assert_eq!(cabin.attach_kind, Some(AttachKind::Image));
        assert!(!cabin.running);
        assert!(cabin.messages.is_empty());

        let _ = std::fs::remove_dir_all(&root);
        unsafe { std::env::remove_var("GROKHUB_CONFIG") };
    }

    fn chip_of(id: &str, kind: ChipKind, score: f32) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: id.into(),
            value: id.into(),
            kind,
            score,
            hint: String::new(),
            primary: false,
        }
    }

    #[test]
    fn shell_chip_fills_the_box_and_never_runs() {
        assert_eq!(shell_chip_draft("$ cargo test"), "/sh cargo test");
        assert_eq!(shell_chip_draft("/sh git status"), "/sh git status");
        assert_eq!(shell_chip_draft("  ls -la "), "/sh ls -la");

        let mut cabin = Cabin::quiet_for_test();
        let mut chip = chip_of("shell", ChipKind::Shell, 1.0);
        chip.value = "rm -rf build".into();
        cabin.apply_chip(chip);
        assert_eq!(cabin.composer, "/sh rm -rf build");
        assert!(!cabin.running, "a tap only fills the box");
        assert!(cabin.messages.is_empty());
        assert!(cabin.composer_want_focus);
    }

    #[test]
    fn the_plan_chip_fills_the_box_before_what_was_typed() {
        assert_eq!(plan_chip_draft(""), "/plan ");
        assert_eq!(plan_chip_draft("  fix my fps "), "/plan fix my fps");
        assert_eq!(plan_chip_draft("/plan fix my fps"), "/plan fix my fps");
        assert_eq!(plan_chip_draft("/planet facts"), "/plan /planet facts");
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        cabin.composer = "speed up boot".into();
        cabin.apply_chip(chat_chip("home-slash-plan", "/plan", "/plan"));
        assert_eq!(cabin.composer, "/plan speed up boot");
        assert!(cabin.composer_want_focus);
        assert_eq!(cabin.status, PLAN_CHIP_NOTE);
        assert!(!cabin.plan_next, "nothing is armed out of sight");
        // A second tap does not stack another `/plan`.
        cabin.apply_chip(chat_chip("home-slash-plan", "/plan", "/plan"));
        assert_eq!(cabin.composer, "/plan speed up boot");
    }

    #[test]
    fn chat_chip_adds_to_a_typed_draft() {
        assert_eq!(draft_with_chip("", "Explain this"), "Explain this");
        assert_eq!(
            draft_with_chip("the build broke ", "Fix it"),
            "the build broke\nFix it"
        );
        assert_eq!(draft_with_chip("Fix it", "Fix it"), "Fix it");
        assert_eq!(
            draft_with_chip("fix the leak", "fix the leak. Be concrete."),
            "fix the leak. Be concrete."
        );

        let mut cabin = Cabin::quiet_for_test();
        cabin.composer = "my notes on the wall".into();
        cabin.apply_chip(chat_chip("a", "A", "Summarize it"));
        assert_eq!(cabin.composer, "my notes on the wall\nSummarize it");
        assert!(!cabin.running);
    }

    #[test]
    fn a_second_chip_swaps_text_the_first_chip_put_there() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.composer = "typed".into();
        cabin.composer_chip_text = "typed".into();
        cabin.apply_chip(chat_chip("b", "B", "Plan it"));
        assert_eq!(cabin.composer, "Plan it");
        // The person edits the chip's text: now it is theirs and is kept.
        cabin.composer.push_str(" today");
        cabin.apply_chip(chat_chip("c", "C", "Be brief"));
        assert_eq!(cabin.composer, "Plan it today\nBe brief");
    }

    #[test]
    fn only_the_first_chip_is_primary() {
        let mut chips = vec![
            QuickChip {
                primary: true,
                ..chip_of("a", ChipKind::Chat, 1.0)
            },
            QuickChip {
                primary: true,
                ..chip_of("b", ChipKind::Chat, 2.0)
            },
            chip_of("c", ChipKind::Nav, 3.0),
        ];
        mark_first_primary(&mut chips);
        assert_eq!(
            chips.iter().map(|c| c.primary).collect::<Vec<_>>(),
            vec![true, false, false]
        );
    }

    #[test]
    fn shell_chips_wait_for_an_empty_box() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.visible_chips = vec![
            chip_of("shell", ChipKind::Shell, 2.0),
            chip_of("chat", ChipKind::Chat, 1.0),
        ];
        cabin.composer = "half a thought".into();
        assert!(cabin
            .composer_chips()
            .iter()
            .all(|c| c.kind != ChipKind::Shell));
        cabin.composer.clear();
        assert!(cabin.composer_chips().iter().any(|c| c.id == "shell"));
    }

    #[test]
    fn a_hidden_chip_stays_hidden() {
        let mut cabin = Cabin::quiet_for_test();
        let chip = chip_of("radar:new", ChipKind::Nav, 1.0);
        assert!(!cabin.chip_hidden(&chip));
        cabin.chip_dismissed.push("radar:new".into());
        assert!(cabin.chip_hidden(&chip));
    }

    #[test]
    fn why_tip_speaks_plainly() {
        let tip = |h: &str| row::chip_why_tip(h, "x");
        assert_eq!(
            tip("Predicted · finish · Mid-thread"),
            "Why this?\nLooks like you want to finish the job\nYou're mid-conversation"
        );
        assert_eq!(
            tip("Predicted match"),
            "Why this?\nMatches what you're typing"
        );
        assert_eq!(
            tip("Highest-ranked next action"),
            "Why this?\nA likely next step"
        );
        assert_eq!(
            tip("Predicted next: fix"),
            "Why this?\nLooks like you want to fix something"
        );
        assert_eq!(
            tip("Predicted for this time of day"),
            "Why this?\nYou often do this around now"
        );
        assert_eq!(tip("Last slash"), "Why this?\nLast slash");
        for h in ["Predicted · ship", "Predicted match · Predicted next: x"] {
            assert!(!tip(h).contains("Predicted"), "{h} -> {}", tip(h));
        }
    }

    #[test]
    fn lane_only_breaks_near_ties() {
        let mut chips = vec![
            QuickChip {
                label: "Open Imagine".into(),
                value: "__nav:imagine".into(),
                ..chip_of("imagine", ChipKind::Nav, 100.0)
            },
            QuickChip {
                label: "Ship it".into(),
                value: "Ship a minimal solid slice.".into(),
                ..chip_of("ship", ChipKind::Chat, 40.0)
            },
        ];
        super::super::glance::bias_chips_for_lane(&mut chips, super::super::CabinLane::Coding);
        assert_eq!(chips[0].id, "imagine", "a far stronger chip keeps its spot");
        chips[1].score = 95.0;
        super::super::glance::bias_chips_for_lane(&mut chips, super::super::CabinLane::Coding);
        assert_eq!(chips[0].id, "ship", "inside one band the lane decides");
    }

    #[test]
    fn a_shown_row_and_its_outcome_reach_the_chip_log() {
        let mut cabin = Cabin::quiet_for_test();
        let row = vec![
            chip_of("pred-a", ChipKind::Nav, 2.0),
            chip_of("learn-b", ChipKind::Nav, 1.0),
        ];
        cabin.note_chips_shown("composer", &row);
        cabin.note_chips_shown("composer", &row);
        assert_eq!(
            cabin.chip_memory.events.len(),
            1,
            "a repaint is one showing"
        );
        cabin.take_chip_act(crate::cards::ChipRowAct::Dismiss(1), &row);
        assert!(matches!(
            cabin.chip_memory.events[0].outcome,
            Some(grokhub_core::ChipOutcome::Dismissed { slot: 1, .. })
        ));
        cabin.note_chips_shown("composer", &row);
        assert_eq!(
            cabin.chip_memory.events.len(),
            2,
            "after an act the next showing is new"
        );
        let json = serde_json::to_string(&cabin.chip_memory.events).expect("json");
        assert!(
            !json.contains("pred-a") && !json.contains("learn-b"),
            "{json}"
        );
    }
}
