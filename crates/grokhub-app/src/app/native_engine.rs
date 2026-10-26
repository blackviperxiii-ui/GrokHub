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

/// Closes the volatile voice/notes tail so cabin safety (earlier, in the stable
/// prefix) still outranks a custom personality or tuned note that lands last.
/// The agent puts each turn's talk style just before it.
const VOLATILE_SAFETY_CLOSER: &str = grokhub_core::talk::VOLATILE_SAFETY_CLOSER;

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
        for action in actions {
            self.followup_queue.push(format!(
                "[panel action] {} on {}",
                action.action, action.panel_id
            ));
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
        self.side_ask_kick = false;
        if let Err(err) = ready {
            self.fail_turn_start(&err);
            return true;
        }
        let (carried, carry_idx) = self.native_carry(last_user);
        let prompted = self
            .acp
            .as_ref()
            .map(|handle| handle.prompt_with_image(&carried, image));
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
        if self
            .acp
            .as_ref()
            .is_some_and(|handle| handle.session_id == session_id && handle.cwd == cwd)
        {
            return self.publish_native_cfg();
        }
        self.acp = None;
        let (handle, ext_rx, evt_tx) =
            crate::engine_handle::AcpHandle::external(cwd.clone(), session_id.clone());
        self.publish_native_cfg_for(&session_id)?;
        std::thread::spawn(move || serve_native(session_id, ext_rx, evt_tx));
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
        let (bearer, auth_kind) = self.native_cred()?;
        crate::desktop_mcp::sync_native_cua(&self.cfg);
        let workspace = self.native_workspace();
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        // The router picks each call's effort; this is only the fallback for an unlisted class.
        let effort =
            grokhub_agent::route::live::start_effort(grokhub_agent::route::live::DEFAULT_CLASS);
        let (stable, volatile) = self.chat_prompt_parts();
        let system = grokhub_agent::system_prompt_parts(&stable, &volatile, &workspace);
        // A side question stays read-only even in plan mode, so it never gets plan approval.
        grokhub_agent::set_plan_session(
            session_id,
            matches!(self.session_mode, grokhub_core::wire::SessionMode::Plan)
                && !self.side_ask_kick,
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
        let stable = grokhub_core::cabin_rules::cabin_static_rules(self.cfg.desktop_control);
        let mut parts: Vec<String> = Vec::new();
        let mut needs_safety_closer = false;
        let voice = self.chat_personality().prompt_block();
        if !voice.is_empty() {
            parts.push(voice);
            needs_safety_closer = true;
        }
        let notes = self.tuned_notes();
        if !notes.is_empty() {
            parts.push(notes);
            needs_safety_closer = true;
        }
        let learned = grokhub_core::cabin_rules::cabin_learned_tail(&grokhub_core::brief_for(
            &self.learning,
            "chat",
        ));
        if !learned.is_empty() {
            parts.push(learned);
        }
        if needs_safety_closer {
            parts.push(VOLATILE_SAFETY_CLOSER.to_string());
        }
        (stable, parts.join("\n\n"))
    }

    fn native_gate(&self) -> Gate {
        let readonly = self.side_ask_kick
            || matches!(
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

    pub(super) fn paint_native_badge(&mut self, ui: &mut egui::Ui) {
        let checked_id = ui.id().with("native-usage-sid");
        let native = self.threads.get(self.thread_idx).is_some();
        if !native {
            let checked = ui
                .data(|data| data.get_temp::<String>(checked_id))
                .unwrap_or_default();
            if !checked.is_empty() {
                ui.data_mut(|data| data.insert_temp(checked_id, String::new()));
            }
            return;
        }
        let sid = self
            .threads
            .get(self.thread_idx)
            .and_then(|thread| thread.grok_session.clone())
            .unwrap_or_default();
        let checked = ui
            .data(|data| data.get_temp::<String>(checked_id))
            .unwrap_or_default();
        if !sid.is_empty() && checked != sid {
            if let Ok(info) = grokhub_agent::load_session(&sid) {
                self.show_native_usage(&info);
            }
            ui.data_mut(|data| data.insert_temp(checked_id, sid));
        }
        let label = native_usage_label(&self.grok_usage);
        let text = if label.is_empty() {
            "Native".to_string()
        } else {
            format!("Native · {label}")
        };
        ui.label(
            egui::RichText::new(text)
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::muted()),
        );
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

fn serve_native(
    session_id: String,
    ext_rx: std::sync::mpsc::Receiver<ExternalCmd>,
    evt_tx: std::sync::mpsc::Sender<AcpEvent>,
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
        let Some(cfg) = live_map()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&session_id)
            .cloned()
        else {
            let _ = evt_tx.send(AcpEvent::Err(grokhub_core::XAI_NEED_SIGNIN.into()));
            let _ = evt_tx.send(AcpEvent::Done {
                stop_reason: "error".into(),
            });
            continue;
        };
        engine.set_workspace(cfg.workspace);
        engine.set_gate(cfg.gate);
        engine.set_origin(cfg.origin);
        engine.set_episode(cfg.episode.clone());
        engine.set_imagine_bearer(&cfg.bearer);
        // Brain 1: an unclear ask's Brief costs one fast call. App tests
        // script each reply, so they keep the rules-only default.
        engine.set_brief_model(!cfg!(test));
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
        let _ = engine.prompt(&text, image.as_deref(), &mut |ev| {
            if let AcpEvent::Tool(ref card) = ev {
                maybe_queue_show_panel(&panel_thread, card);
            }
            let _ = tx.send(ev);
        });
    }
    live_map()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(&session_id);
    grokhub_agent::mcp::detach_elicit(&session_id);
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
