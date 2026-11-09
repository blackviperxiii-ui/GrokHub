//! Native engine thread: every chat turn starts here (`kick_native_turn`) and
//! streams back through `poll_acp`.

use super::*;
use crate::engine_handle::{ExternalCmd, NativePerm};
use grokhub_agent::{
    AuthKind, Engine, EngineParts, Gate, NativeEngine, PermAnswer, PermMode, PermitNote, StampHalt,
    XaiClient,
};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
struct LiveCfg {
    workspace: std::path::PathBuf,
    model: String,
    effort: Option<String>,
    system: String,
    bearer: String,
    auth_kind: AuthKind,
    gate: Gate,
    /// Who started this turn (Spike-4c): a typed message or a heartbeat act.
    origin: grokhub_agent::harness::Origin,
    /// Spike-3b: the open desktop episode on this chat.
    episode: Option<grokhub_agent::EpisodeSeed>,
    /// Cabin chat thread that owns this native session (G4 panel drain).
    cabin_thread_id: String,
    /// `briefModelFallback`: an ask the rules and local scorer can't sort asks the model.
    brief_model_fallback: bool,
}

fn live_map() -> &'static Mutex<HashMap<String, LiveCfg>> {
    static MAP: OnceLock<Mutex<HashMap<String, LiveCfg>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Completed `show_panel` tool results queued from the native engine thread for the UI.
/// Each row is `(cabin_thread_id, call_id, detail)` so drain paints the owning chat,
/// not whichever tab is visible when the frame runs.
fn pending_show_panels() -> &'static Mutex<Vec<(String, String, String)>> {
    static Q: OnceLock<Mutex<Vec<(String, String, String)>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

/// Backstop on queued rows; the oldest are dropped first.
const PENDING_SHOW_PANELS_MAX: usize = 64;

fn maybe_queue_show_panel(cabin_thread_id: &str, card: &ToolCard) {
    if !card.status.eq_ignore_ascii_case("completed") {
        return;
    }
    if card.kind != "show_panel" && card.title != "show_panel" {
        return;
    }
    if !card.detail.contains("\"panel\"") {
        return;
    }
    if cabin_thread_id.trim().is_empty() {
        return;
    }
    pending_show_panels()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .push((
            cabin_thread_id.to_string(),
            card.id.clone(),
            card.detail.clone(),
        ));
}

impl Cabin {
    /// Drain `show_panel` tool results captured on the native engine thread into [`Self::panels`].
    /// Only the pending queue — never re-scan `tool_cards` each frame (that revived dismissals
    /// and cloned diffs/screenshots). Rows for other chats stay queued until that chat is
    /// visible so a mid-turn tab switch cannot paint or clear the wrong PanelStore.
    pub(super) fn drain_show_panel_results(&mut self) {
        let visible = self.visible_thread_id();
        self.panels.bind_thread(&visible);
        let queued = std::mem::take(
            &mut *pending_show_panels()
                .lock()
                .unwrap_or_else(|err| err.into_inner()),
        );
        let mut deferred = Vec::new();
        for (thread_id, call_id, detail) in queued {
            if thread_id == visible {
                let _ = self.panels.upsert_from_tool_result(&call_id, &detail);
            } else if self.threads.iter().any(|t| t.id == thread_id) {
                // Archived chats stay in `threads`, so they can still become visible.
                deferred.push((thread_id, call_id, detail));
            }
        }
        if !deferred.is_empty() {
            let mut q = pending_show_panels()
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            q.extend(deferred);
            let over = q.len().saturating_sub(PENDING_SHOW_PANELS_MAX);
            q.drain(..over);
        }
    }

    /// Test helper: enqueue a completed `show_panel` result as the native thread would.
    #[cfg(test)]
    pub(super) fn enqueue_show_panel_for_test(&self, call_id: &str, detail: &str) {
        self.enqueue_show_panel_for_thread_for_test(&self.visible_thread_id(), call_id, detail);
    }

    /// Test helper: enqueue for an explicit cabin thread id (cross-chat drain cases).
    #[cfg(test)]
    pub(super) fn enqueue_show_panel_for_thread_for_test(
        &self,
        thread_id: &str,
        call_id: &str,
        detail: &str,
    ) {
        maybe_queue_show_panel(
            thread_id,
            &ToolCard {
                id: call_id.to_string(),
                title: "show_panel".into(),
                kind: "show_panel".into(),
                status: "completed".into(),
                detail: detail.to_string(),
                diff: String::new(),
                image_data_url: None,
                raw_input: String::new(),
            },
        );
    }

    /// Test helper: queued `(call_id)`s plus total queue length.
    #[cfg(test)]
    pub(super) fn pending_show_panel_snapshot_for_test() -> (Vec<String>, usize) {
        let q = pending_show_panels()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        (q.iter().map(|r| r.1.clone()).collect(), q.len())
    }

    /// Queue panel button intents as user-visible follow-ups (approval-gated).
    pub(super) fn queue_panel_actions(&mut self, actions: Vec<panels::PanelAction>) {
        let here = self.visible_thread_id();
        for action in actions {
            self.followup_queue.push(
                &here,
                format!("[panel action] {} on {}", action.action, action.panel_id),
            );
        }
    }

    /// `/compact` on the open thread. False only when no thread is open.
    /// The job thread is bound after the engine, so the session id stays on this tab.
    /// `running` stays false: the compact `Done` must not append an assistant bubble.
    pub(super) fn native_compact_if_current(&mut self) -> bool {
        if self.threads.get(self.thread_idx).is_none() {
            return false;
        }
        if let Err(err) = self.ensure_native_engine() {
            self.status = err;
            return true;
        }
        let prompted = self.acp.as_ref().map(|handle| handle.prompt("/compact"));
        match prompted {
            Some(Ok(())) => {
                self.chat_job_thread = Some(self.visible_thread_id());
                self.status = "Compacting…".into();
            }
            Some(Err(err)) => self.status = err,
            None => self.status = "native engine is not running".into(),
        }
        true
    }

    pub(super) fn kick_native_turn(
        &mut self,
        last_user: &str,
        image: Option<&str>,
        raw_ask: &str,
        thread_label: &str,
    ) -> bool {
        // `ensure_native_engine` publishes the config (with the episode seed) once.
        let ready = self.ensure_native_engine();
        if let Err(err) = ready {
            self.fail_turn_start(&err);
            return true;
        }
        let (mut carried, carry_idx) = self.native_carry(last_user);
        if let Some(line) = self.take_focus_context() {
            carried = format!("{line}\n{carried}");
        }
        let mut prompted = self
            .acp
            .as_ref()
            .map(|handle| handle.prompt_with_image(&carried, image));
        // The engine died between the check and the send: start a new one
        // for this chat and send the message once more, never drop it.
        if matches!(&prompted, Some(Err(err)) if err == crate::engine_handle::ENGINE_GONE) {
            self.acp = None;
            if let Err(err) = self.ensure_native_engine() {
                self.fail_turn_start(&err);
                return true;
            }
            prompted = self
                .acp
                .as_ref()
                .map(|handle| handle.prompt_with_image(&carried, image));
        }
        match prompted {
            Some(Ok(())) => {
                self.episode_resume_sent();
                self.clear_native_carry(carry_idx);
                self.note_inflight_card(raw_ask, thread_label);
            }
            Some(Err(err)) => self.fail_turn_start(&err),
            None => self.fail_turn_start("native engine is not running"),
        }
        true
    }

    /// `/flush` and `/dream` on a native thread. `running` stays false so the
    /// following `Done` does not append an assistant bubble, and the job thread
    /// stays clear so the composer does not look busy.
    pub(super) fn prompt_native_memory(&mut self, command: &str, status: &str) -> bool {
        if self.threads.get(self.thread_idx).is_none() {
            return false;
        }
        if let Err(err) = self.ensure_native_engine() {
            self.status = err;
            return true;
        }
        let prompted = self.acp.as_ref().map(|handle| handle.prompt(command));
        match prompted {
            Some(Ok(())) => self.status = status.into(),
            Some(Err(err)) => self.status = err,
            None => self.status = "native engine is not running".into(),
        }
        true
    }

    /// Can a chat turn start? The native engine needs GrokHub's own sign-in or an
    /// API key (an unattended run carries its own).
    pub(super) fn agent_ready(&self) -> bool {
        self.scheduled_perm || self.has_key() || self.imagine_native.tokens.is_some()
    }

    /// Can a chat turn start? [`Self::agent_ready`], or no sign-in or key but
    /// Local AI is on, installed and not paused: the router sends the turn to
    /// the device (`local:no_keys`), online or offline.
    pub(super) fn chat_ready(&self) -> bool {
        self.agent_ready() || local_chat_available()
    }

    /// Signed out, and chats run on Local AI until a sign-in or key is added.
    pub(super) fn local_only_chat(&self) -> bool {
        !self.agent_ready() && local_chat_available()
    }

    /// Is this the bearer xAI last turned away?
    fn bearer_rejected(&self, bearer: &str) -> bool {
        self.rejected_bearer.as_deref() == Some(bearer)
    }

    /// Remember that xAI turned away the bearer this chat runs on, when the
    /// engine error says so. True when it did and Local AI can take the turn.
    pub(super) fn note_cloud_rejection(&mut self, err: &str) -> bool {
        if !is_cloud_rejection(err) {
            return false;
        }
        if let Ok((bearer, _)) = self.native_cred() {
            self.rejected_bearer = Some(bearer);
        }
        local_chat_available()
    }

    /// The Grok sign-in or key can carry a turn right now, as far as the cabin
    /// can tell without calling xAI: a key, a live token, or a token that can
    /// still be renewed, and nothing xAI has turned away.
    pub(super) fn cloud_cred_usable(&self) -> bool {
        let now = grokhub_core::now_ms();
        let rejected = |b: &str| self.bearer_rejected(b);
        let key = self.console_key().trim();
        if !key.is_empty() && !rejected(key) {
            return true;
        }
        if self.imagine_native.tokens.as_ref().is_some_and(|t| {
            grokhub_core::imagine_access_usable(t, now) && !rejected(&t.access_token)
        }) {
            return true;
        }
        let Some(t) = self.secrets.oauth.as_ref() else {
            return false;
        };
        if t.access_token.trim().is_empty() || rejected(&t.access_token) {
            return false;
        }
        if grokhub_core::oauth_access_live(t, now) {
            return true;
        }
        let renewable = t
            .refresh_token
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty());
        renewable && !crate::oauth::cabin_refresh_failed()
    }

    /// Why a turn did not start, naming the engine's own missing piece.
    pub(super) fn no_agent_note(&self) -> &'static str {
        grokhub_core::XAI_NEED_SIGNIN
    }

    /// The first native turn on a thread adopted from the CLI carries its earlier
    /// turns, once. Every other turn goes out as typed. The index names the
    /// thread whose carry the turn used; it is cleared only once the send is
    /// accepted, so a failed first send carries the recap on the retry.
    fn native_carry(&self, last_user: &str) -> (String, Option<usize>) {
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let Some(thread) = self.threads.get(idx).filter(|t| t.native_carry) else {
            return (last_user.to_string(), None);
        };
        let recap = if idx == self.thread_idx {
            threads::native_carry_recap(&self.messages)
        } else {
            threads::native_carry_recap(&thread.messages)
        };
        (format!("{recap}{last_user}"), Some(idx))
    }

    fn clear_native_carry(&mut self, idx: Option<usize>) {
        let Some(thread) = idx.and_then(|idx| self.threads.get_mut(idx)) else {
            return;
        };
        thread.native_carry = false;
        self.persist();
    }

    pub(super) fn ensure_native_engine(&mut self) -> Result<(), String> {
        let cwd = self.native_workspace();
        let (session_id, changed) = self.bind_native_session_id(&cwd);
        if changed {
            self.persist();
        }
        // A handle whose engine thread is gone (it panicked or returned) is
        // never reused: every send on it would fail. A new engine for the same
        // session picks the chat up from its saved turns.
        if self.acp.as_ref().is_some_and(|handle| {
            handle.session_id == session_id && handle.cwd == cwd && handle.is_alive()
        }) {
            return self.publish_native_cfg();
        }
        self.acp = None;
        let (handle, ext_rx, evt_tx, life) =
            crate::engine_handle::AcpHandle::external(cwd.clone(), session_id.clone());
        self.publish_native_cfg_for(&session_id)?;
        let owner = handle.id;
        engine_owners()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(session_id.clone(), owner);
        note_engine_panics();
        let spawned = std::thread::Builder::new()
            .name(ENGINE_THREAD.into())
            .spawn(move || serve_native(session_id, owner, ext_rx, evt_tx, life));
        if spawned.is_err() {
            return Err(crate::engine_handle::ENGINE_GONE.into());
        }
        self.acp = Some(handle);
        Ok(())
    }

    /// Keep the thread's session id across handle restarts. A new id is minted once.
    fn bind_native_session_id(&mut self, cwd: &std::path::Path) -> (String, bool) {
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let cwd_text = cwd.display().to_string();
        let Some(thread) = self.threads.get_mut(idx) else {
            return (format!("native-{}", grokhub_core::uid("n")), false);
        };
        let mut changed = threads::adopt_native(thread);
        if thread.grok_cwd.as_deref().unwrap_or("").trim().is_empty() {
            thread.grok_cwd = Some(cwd_text);
            changed = true;
        }
        if let Some(id) = thread
            .grok_session
            .clone()
            .filter(|id| !id.trim().is_empty())
        {
            return (id, changed);
        }
        let id = format!("native-{}", grokhub_core::uid("n"));
        thread.grok_session = Some(id.clone());
        (id, true)
    }

    pub(super) fn native_workspace(&self) -> std::path::PathBuf {
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        self.threads
            .get(idx)
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd())
    }

    fn publish_native_cfg(&mut self) -> Result<(), String> {
        let Some(session_id) = self.acp.as_ref().map(|handle| handle.session_id.clone()) else {
            return Err("native engine is not running".into());
        };
        self.publish_native_cfg_for(&session_id)
    }

    fn publish_native_cfg_for(&mut self, session_id: &str) -> Result<(), String> {
        let (bearer, auth_kind) = match self.native_cred() {
            Ok(cred) if !self.bearer_rejected(&cred.0) || !local_chat_available() => cred,
            // No usable sign-in or key (none, expired, its renewal failed, or
            // xAI turned it away): with Local AI ready the router answers on
            // the device, so the engine starts with no bearer at all.
            _ if local_chat_available() => {
                if self.agent_ready() {
                    self.status = local_fallback_note().into();
                }
                (String::new(), AuthKind::ApiKey)
            }
            Ok(cred) => cred,
            Err(err) => return Err(err),
        };
        crate::desktop_mcp::sync_native_cua(&self.cfg);
        let workspace = self.native_workspace();
        let model = grokhub_agent::DEFAULT_MODEL.to_string();
        // The router picks each call's effort; this is only the fallback for an unlisted class.
        let effort =
            grokhub_agent::route::live::start_effort(grokhub_agent::route::live::DEFAULT_CLASS);
        let (mut stable, volatile) = self.chat_prompt_parts_for(session_id);
        // Dynamic chat and quips step per turn from this chat's voice settings.
        grokhub_agent::voice_session::register(session_id, self.chat_voice_config());
        // Fixed text in the cached prefix: on or off, it changes rarely.
        if self.reply_chips_on() {
            stable = format!("{stable}\n\n{}", grokhub_core::CHIP_TRAILER_RULE);
        }
        let system = grokhub_agent::system_prompt_parts(&stable, &volatile, &workspace);
        grokhub_agent::set_plan_session(
            session_id,
            matches!(self.session_mode, grokhub_core::wire::SessionMode::Plan),
        );
        let cabin_thread_id = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| self.visible_thread_id());
        let cfg = LiveCfg {
            workspace,
            model,
            effort,
            system,
            bearer,
            auth_kind,
            gate: self.native_gate(),
            origin: self.harness.turn_origin,
            episode: self.native_episode_seed(),
            cabin_thread_id,
            brief_model_fallback: self.cfg.brief_model_fallback,
        };
        live_map()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(session_id.to_string(), cfg);
        Ok(())
    }

    /// UI/test helper: personality tone first, then cabin safety (legacy order).
    /// Live publishes use [`Self::chat_prompt_parts`] for a cacheable stable prefix.
    #[cfg(test)]
    pub(super) fn chat_rules(&self) -> String {
        let rules = grokhub_core::cabin_rules::cabin_rules_for(
            &grokhub_core::brief_for(&self.learning, "chat"),
            self.cfg.desktop_control,
        );
        let notes = self.tuned_notes();
        let rules = if notes.is_empty() {
            rules
        } else {
            format!("{notes}\n\n{rules}")
        };
        grokhub_core::personality::layer(&self.chat_personality(), &rules)
    }

    /// Stable cabin safety/approval rules, then volatile personality / notes / brief.
    /// When voice or notes are present, a short closer reasserts that the earlier
    /// cabin safety rules win (so a rogue tone cannot outrank them by landing last).
    pub(super) fn chat_prompt_parts(&self) -> (String, String) {
        self.chat_prompt_parts_for("")
    }

    fn chat_prompt_parts_for(&self, session_id: &str) -> (String, String) {
        let stable = grokhub_core::cabin_rules::cabin_static_rules(self.cfg.desktop_control);
        let personality = self.chat_personality();
        let config_dir = crate::config::config_dir();
        // The join lives in the agent so the live eval sends this same tail.
        let tail = grokhub_agent::chat_volatile_tail(&grokhub_agent::ChatTail {
            // With 18+ on, every voice jokes at the chosen heat.
            voice: personality.prompt_block_for(self.adult_heat()),
            notes: self.tuned_notes(),
            learned: grokhub_core::brief_for(&self.learning, "chat"),
            identity: grokhub_agent::identity_blocks(&config_dir),
            intentions: grokhub_core::intentions_block(&grokhub_core::load_intentions(
                &config_dir,
                session_id,
            )),
            explicit: personality.explicit,
        });
        (stable, tail)
    }

    fn native_gate(&self) -> Gate {
        let readonly = matches!(
            self.session_mode,
            grokhub_core::wire::SessionMode::Plan | grokhub_core::wire::SessionMode::Ask
        );
        let mode = match self.permission_mode {
            grokhub_core::wire::PermissionMode::Ask => PermMode::Ask,
            grokhub_core::wire::PermissionMode::Auto => PermMode::Auto,
            grokhub_core::wire::PermissionMode::AlwaysApprove => PermMode::Always,
        };
        Gate {
            mode,
            readonly_session: readonly,
            attended: !self.scheduled_perm,
            desktop: self.cfg.desktop_control,
        }
    }

    pub(super) fn ensure_native_listing(&mut self) {
        let workspace = self.grok_cwd();
        let stamp = grokhub_agent::plugins::listing_stamp(&workspace);
        let cwd = format!("{}\n{stamp}", workspace.display());
        if self.native_listing_cwd == cwd {
            return;
        }
        let home = grokhub_core::user_home();
        let skills = grokhub_agent::plugins::skill_dirs(&workspace);
        let hooks = grokhub_agent::plugins::hook_paths(&workspace);
        self.native_skills = grokhub_agent::discover_skills(&workspace, home.as_deref(), &skills);
        self.native_hooks = grokhub_agent::discover_hooks(&workspace, home.as_deref(), &hooks);
        self.native_hooks_trusted = grokhub_agent::folder_trusted(&workspace);
        self.native_listing_cwd = cwd;
    }

    /// The native engine's bearer, in this order: the Imagine sign-in, the Settings → Account
    /// "Sign in with Grok" (the sign-in the CLI path and the rest of the cabin use),
    /// then the console API key. The Grok CLI's own login file is never read here.
    pub(super) fn native_cred(&mut self) -> Result<(String, AuthKind), String> {
        let now = grokhub_core::now_ms();
        if let Some(tokens) = self.imagine_native.tokens.clone() {
            if grokhub_core::imagine_oauth_preferred(&tokens, now) {
                if grokhub_core::imagine_access_usable(&tokens, now) {
                    return Ok((tokens.access_token, AuthKind::OAuth));
                }
                if let Ok((access, updated)) = crate::imagine_auth::access_for_job(&tokens) {
                    if let Some(next) = updated {
                        self.imagine_native.tokens = Some(next);
                    }
                    return Ok((access, AuthKind::OAuth));
                }
            }
        }
        if let Some(access) = self.native_account_access(now) {
            return Ok((access, AuthKind::OAuth));
        }
        let key = self.console_key().trim();
        if !key.is_empty() {
            return Ok((key.to_string(), AuthKind::ApiKey));
        }
        Err(grokhub_core::XAI_NEED_SIGNIN.into())
    }

    /// The Settings → Account sign-in. A live token is used as is; inside the
    /// refresh window it is renewed off the UI thread. An expired token with a
    /// refresh token is renewed once, here, and saved back to the cabin's own store.
    pub(super) fn native_account_access(&mut self, now: u64) -> Option<String> {
        let tokens = self.secrets.oauth.clone()?;
        if tokens.access_token.trim().is_empty() {
            return None;
        }
        if grokhub_core::oauth_access_live(&tokens, now) {
            if grokhub_core::token_needs_refresh(&tokens, now) {
                if let Some(next) = crate::oauth::refresh_cabin_oauth(&tokens) {
                    let access = next.access_token.clone();
                    self.keep_account_tokens(next);
                    return Some(access);
                }
            }
            return Some(tokens.access_token);
        }
        let has_refresh = tokens
            .refresh_token
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        if !has_refresh {
            return None;
        }
        let next = crate::oauth::ensure_access_with_backoff(&tokens)?;
        let access = next.access_token.clone();
        self.keep_account_tokens(next);
        Some(access)
    }

    fn keep_account_tokens(&mut self, next: grokhub_core::XaiOAuthTokens) {
        self.secrets.oauth = Some(next);
        self.persist_oauth_secrets();
    }

    /// Refresh grok_usage from the open native session. Does not paint — the
    /// persistent "Native · tokens · pool" bar was removed (usage lives in Settings).
    pub(super) fn sync_native_usage(&mut self, ui: &mut egui::Ui) {
        let checked_id = ui.id().with("native-usage-sid");
        let sid = self
            .threads
            .get(self.thread_idx)
            .and_then(|thread| thread.grok_session.clone())
            .unwrap_or_default();
        if sid.is_empty() {
            let checked = ui
                .data(|data| data.get_temp::<String>(checked_id))
                .unwrap_or_default();
            if !checked.is_empty() {
                ui.data_mut(|data| data.insert_temp(checked_id, String::new()));
            }
            return;
        }
        let checked = ui
            .data(|data| data.get_temp::<String>(checked_id))
            .unwrap_or_default();
        if checked != sid {
            if let Ok(info) = grokhub_agent::load_session(&sid) {
                self.show_native_usage(&info);
            }
            ui.data_mut(|data| data.insert_temp(checked_id, sid));
        }
    }
}

/// The native usage line: tokens, cost, meter, and the last turn's prompt-cache
/// share ("cache N%") when the engine reported one.
pub(super) fn native_usage_label(u: &GrokUsage) -> String {
    let label = grokhub_agent::usage_label(
        u.input_tokens,
        u.output_tokens,
        u.reasoning_tokens,
        u.cost_in_usd_ticks,
        &u.meter,
    );
    match u.cache_note() {
        Some(note) if !label.is_empty() => format!("{label} · {note}"),
        _ => label,
    }
}

enum NativeJob {
    Prompt { text: String, image: Option<String> },
    Shutdown,
}

/// The engine thread's name. The panic note below only records panics here.
const ENGINE_THREAD: &str = "loadout-chat-engine";

/// What the chat shows when the engine thread panicked mid-reply. The engine
/// is started fresh from the chat's saved turns on the next send.
pub(super) const ENGINE_CRASHED: &str =
    "The chat engine hit an internal error and stopped this reply.";

/// Which engine (by [`crate::engine_handle::AcpHandle::id`]) is current for a
/// session. Only the current one may clear the session's shared config and
/// question inbox when it ends, so an old engine shutting down late never
/// takes them from the new one.
fn engine_owners() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    static LAST_ENGINE_PANIC: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Keep where an engine thread panicked (message and source line), then run
/// the usual hook. The app runs without a console, so this is the only trace.
fn note_engine_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() == Some(ENGINE_THREAD) {
                let what = info.payload_as_str().unwrap_or("panic");
                let at = info
                    .location()
                    .map(|l| format!("{}:{}", l.file(), l.line()))
                    .unwrap_or_default();
                LAST_ENGINE_PANIC.with(|slot| *slot.borrow_mut() = Some(format!("{what} at {at}")));
            }
            prev(info);
        }));
    });
}

/// One line per engine crash in `<config>/logs/engine-crash.log`.
fn log_engine_crash(session_id: &str, what: &str) {
    use std::io::Write;
    let dir = crate::config::config_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let what: String = what.replace('\n', " ").chars().take(600).collect();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("engine-crash.log"))
    {
        let _ = writeln!(file, "{} {session_id} {what}", grokhub_core::now_ms());
    }
}

fn serve_native(
    session_id: String,
    owner: u64,
    ext_rx: std::sync::mpsc::Receiver<ExternalCmd>,
    evt_tx: std::sync::mpsc::Sender<AcpEvent>,
    _life: crate::engine_handle::EngineLife,
) {
    let cancel = grokhub_agent::CancelToken::new();
    let steer = grokhub_agent::SteerQueue::new();
    let (job_tx, job_rx) = std::sync::mpsc::channel();
    let (permit_tx, permit_inbox) = grokhub_agent::PermitInbox::pair();
    let (elicit_tx, elicit_inbox) = grokhub_agent::mcp::ElicitInbox::pair();
    grokhub_agent::mcp::attach_elicit(&session_id, elicit_inbox);
    let cancel_ctl = cancel.clone();
    let steer_ctl = steer.clone();
    std::thread::spawn(move || {
        while let Ok(cmd) = ext_rx.recv() {
            match cmd {
                ExternalCmd::Cancel => cancel_ctl.cancel(),
                ExternalCmd::Steer(text) => steer_ctl.push(text),
                ExternalCmd::Shutdown => {
                    cancel_ctl.cancel();
                    let _ = job_tx.send(NativeJob::Shutdown);
                    return;
                }
                ExternalCmd::Prompt { text, image } => {
                    if job_tx.send(NativeJob::Prompt { text, image }).is_err() {
                        return;
                    }
                }
                ExternalCmd::Permission { id, answer } => {
                    let answer = match answer {
                        NativePerm::Allow => PermAnswer::Allow,
                        NativePerm::Always => PermAnswer::Always,
                        NativePerm::Deny => PermAnswer::Deny,
                        NativePerm::Cancel => PermAnswer::Cancel,
                    };
                    let _ = permit_tx.send(PermitNote { id, answer });
                }
                ExternalCmd::Elicit {
                    id,
                    action,
                    content,
                } => {
                    let _ = elicit_tx.send(grokhub_agent::mcp::ElicitNote {
                        id,
                        action,
                        content,
                    });
                }
            }
        }
    });
    let dir = std::env::temp_dir();
    let mut engine = NativeEngine::new(EngineParts {
        client: std::sync::Arc::new(XaiClient::new(
            String::new(),
            AuthKind::ApiKey,
            Duration::from_secs(120),
        )),
        workspace: dir,
        model: grokhub_core::CABIN_FAST_MODEL.to_string(),
        effort: None,
        system: String::new(),
        conversation_id: session_id.clone(),
        auth_kind: AuthKind::ApiKey,
        max_turns: 0,
        cancel: cancel.clone(),
        steer,
        halt: Box::new(StampHalt {
            started_ms: 0,
            read: || None,
        }),
        gate: Gate::phase_readonly(),
        desktop: Some(Box::new(crate::desktop_mcp::native_desktop())),
        permits: std::sync::Arc::new(permit_inbox),
    });
    grokhub_agent::watch_cancel(&session_id, cancel.clone());
    let _ = grokhub_agent::hub_for(&session_id);
    let _run = grokhub_agent::attach_run(&session_id, cancel.clone());
    if let Ok(info) = grokhub_agent::load_session(&session_id) {
        engine.resume(info.input(), info.usage);
    }
    while let Ok(job) = job_rx.recv() {
        let NativeJob::Prompt { text, image } = job else {
            break;
        };
        // A panic anywhere in the turn ends this engine, not the chat: the
        // reply says so plainly and the cabin starts a fresh engine from the
        // saved turns. The engine's state mid-panic is not trusted again.
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_native_job(&mut engine, &session_id, &evt_tx, &text, image.as_deref())
        }));
        if let Err(payload) = ran {
            let noted = LAST_ENGINE_PANIC.with(|slot| slot.borrow_mut().take());
            let what = noted.unwrap_or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panic".into())
            });
            log_engine_crash(&session_id, &what);
            let _ = evt_tx.send(AcpEvent::Err(ENGINE_CRASHED.into()));
            let _ = evt_tx.send(AcpEvent::Done {
                stop_reason: "error".into(),
            });
            break;
        }
    }
    let mut owners = engine_owners()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    if owners.get(&session_id) == Some(&owner) {
        owners.remove(&session_id);
        live_map()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&session_id);
        grokhub_agent::mcp::detach_elicit(&session_id);
    }
}

/// One chat message on the engine thread, with the config the cabin published.
fn run_native_job(
    engine: &mut NativeEngine,
    session_id: &str,
    evt_tx: &std::sync::mpsc::Sender<AcpEvent>,
    text: &str,
    image: Option<&str>,
) {
    let Some(cfg) = live_map()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(session_id)
        .cloned()
    else {
        let _ = evt_tx.send(AcpEvent::Err(grokhub_core::XAI_NEED_SIGNIN.into()));
        let _ = evt_tx.send(AcpEvent::Done {
            stop_reason: "error".into(),
        });
        return;
    };
    engine.set_workspace(cfg.workspace);
    engine.set_gate(cfg.gate);
    engine.set_origin(cfg.origin);
    engine.set_episode(cfg.episode.clone());
    engine.set_imagine_bearer(&cfg.bearer);
    // Brain 1: the rules and the local scorer sort the ask in-process; a
    // model call for what they can't sort is opt-in (off by default).
    engine.set_brief_model(cfg.brief_model_fallback);
    let client = XaiClient::new(cfg.bearer, cfg.auth_kind, Duration::from_secs(120));
    #[cfg(test)]
    let client = match test_responses_url() {
        Some(url) => client.with_loopback_url(&url).expect("loopback test URL"),
        None => client,
    };
    engine.set_route(
        std::sync::Arc::new(client),
        cfg.model,
        cfg.effort,
        cfg.system,
        cfg.auth_kind,
    );
    let started = grokhub_core::now_ms();
    engine.set_halt(Box::new(StampHalt {
        started_ms: started,
        read: || crate::desktop_mcp::read_halt_stamp(),
    }));
    let tx = evt_tx.clone();
    let panel_thread = cfg.cabin_thread_id.clone();
    #[cfg(test)]
    let crash_here = take_mid_turn_panic(session_id);
    let _ = engine.prompt(text, image, &mut |ev| {
        if let AcpEvent::Tool(ref card) = ev {
            maybe_queue_show_panel(&panel_thread, card);
        }
        #[cfg(test)]
        let streamed = matches!(ev, AcpEvent::Text(_));
        let _ = tx.send(ev);
        #[cfg(test)]
        if crash_here && streamed {
            panic!("test: the engine panics mid-turn");
        }
    });
}

#[cfg(test)]
fn mid_turn_panics() -> &'static Mutex<Vec<String>> {
    static Q: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(test)]
fn take_mid_turn_panic(session_id: &str) -> bool {
    let mut q = mid_turn_panics()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let Some(at) = q.iter().position(|id| id == session_id) else {
        return false;
    };
    q.remove(at);
    true
}

/// Test-only: the next turn on `session_id` panics on the engine thread right
/// after its first streamed text, as a bug deep in a turn would.
#[cfg(test)]
pub(super) fn panic_mid_turn_for_test(session_id: &str) {
    mid_turn_panics()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .push(session_id.to_string());
}

/// Local AI is on, installed and not paused, so a chat can answer on the device.
/// Shown when a sign-in or key is on file but can't be used and Local AI
/// takes the chat instead of the turn failing.
pub(super) const LOCAL_FALLBACK_NOTE: &str =
    "Your Grok sign-in needs renewing, so Local AI is answering. Sign in again in Settings → AI models to use Grok.";

/// The same, when the sign-in is fine but xAI's plan gate refused it: the
/// Grok plan doesn't include app use, or its pool is used up. Signing in
/// again wouldn't help, so this note doesn't ask for it.
pub(super) const PLAN_FALLBACK_NOTE: &str =
    "Your Grok plan can't take chats from apps right now (it doesn't include app use, or its pool is used up), so Local AI is answering. Your sign-in is fine.";

/// The note for a turn Local AI took after xAI turned the cloud bearer away.
pub(super) fn local_fallback_note() -> &'static str {
    let now = grokhub_core::now_ms();
    if grokhub_agent::plan_room::refused_since()
        .is_some_and(|t| now.saturating_sub(t) <= grokhub_agent::plan_room::FRESH_MS)
    {
        PLAN_FALLBACK_NOTE
    } else {
        LOCAL_FALLBACK_NOTE
    }
}

/// xAI turned the bearer away (`client::ClientError` 401/403 wording).
fn is_cloud_rejection(err: &str) -> bool {
    err.contains("sign-in was rejected (HTTP")
        || err.contains("Grok plan refused this (HTTP")
        || err.contains("API key rejected (HTTP")
}

fn local_chat_available() -> bool {
    #[cfg(test)]
    if let Some(on) = LOCAL_READY_TEST.with(std::cell::Cell::get) {
        return on;
    }
    grokhub_agent::route::local::available()
}

#[cfg(test)]
thread_local! {
    static LOCAL_READY_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Test-only: say Local AI is (or isn't) ready on this thread; `None` reads the real gate.
#[cfg(test)]
pub(super) fn set_local_ready_for_test(on: Option<bool>) {
    LOCAL_READY_TEST.with(|c| c.set(on));
}

/// Test-only: the bearer the native engine was handed for `session_id`.
#[cfg(test)]
pub(super) fn published_bearer_for_test(session_id: &str) -> Option<String> {
    live_map()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(session_id)
        .map(|cfg| cfg.bearer.clone())
}

#[cfg(test)]
static TEST_RESPONSES_URL: Mutex<Option<String>> = Mutex::new(None);

#[cfg(test)]
fn test_responses_url() -> Option<String> {
    TEST_RESPONSES_URL
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

/// Test-only. Production builds do not compile this, so native turns always go to xAI.
#[cfg(test)]
pub(super) fn set_responses_url_for_test(url: Option<&str>) {
    *TEST_RESPONSES_URL
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = url.map(str::to_string);
}

#[cfg(test)]
mod usage_label_tests {
    use super::*;

    fn usage(pct: Option<u8>) -> GrokUsage {
        GrokUsage {
            input_tokens: 1200,
            output_tokens: 80,
            meter: "API credits".into(),
            turn_cache_pct: pct,
            ..GrokUsage::default()
        }
    }

    #[test]
    fn native_usage_shows_the_turn_cache_share_when_reported() {
        assert_eq!(
            native_usage_label(&usage(Some(87))),
            "1200 in · 80 out · API credits · cache 87%"
        );
    }

    #[test]
    fn native_usage_hides_cache_when_absent_or_empty() {
        assert_eq!(
            native_usage_label(&usage(None)),
            "1200 in · 80 out · API credits"
        );
        let empty = GrokUsage {
            turn_cache_pct: Some(0),
            ..GrokUsage::default()
        };
        assert_eq!(native_usage_label(&empty), "");
    }
}
