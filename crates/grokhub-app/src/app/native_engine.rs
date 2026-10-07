//! Native engine thread. The CLI launch path stays in `acp` and `chat_kick`.

use super::*;
use grokhub_acp::{ExternalCmd, NativePerm};
use grokhub_agent::{
    AuthKind, Engine, EngineParts, Gate, NativeEngine, PermAnswer, PermMode, PermitNote, StampHalt, XaiClient,
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
}

/// The Grok CLI is signed in but GrokHub is not. Lab mode only uses GrokHub's own sign-in.
pub(super) const NATIVE_NEEDS_CABIN_SIGNIN: &str =
    "Lab mode uses GrokHub's sign-in, not the Grok CLI's. Sign in with Grok in Settings → Account, or add an API key.";

fn live_map() -> &'static Mutex<HashMap<String, LiveCfg>> {
    static MAP: OnceLock<Mutex<HashMap<String, LiveCfg>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Cabin {
    pub(super) fn drop_stale_native_handle(&mut self) {
        if self.native_engine_for_current() {
            return;
        }
        if self
            .acp
            .as_ref()
            .is_some_and(|handle| handle.session_id.starts_with("native-"))
        {
            self.acp = None;
        }
    }

    pub(super) fn native_engine_for_current(&self) -> bool {
        if !self.cfg.native_engine {
            return false;
        }
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        self.threads.get(idx).is_some_and(|t| t.native)
    }

    /// `/compact` on a native thread while Settings → Labs is on.
    /// Lab off, or a CLI thread, returns false so the CLI command stays as it is.
    /// The job thread is bound after the engine, so the session id stays on this tab.
    /// `running` stays false: the compact `Done` must not append an assistant bubble.
    pub(super) fn native_compact_if_current(&mut self) -> bool {
        let thread_native = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|thread| thread.native);
        if !grokhub_agent::manual_compact_targets_native(self.cfg.native_engine, thread_native) {
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
        if !self.native_engine_for_current() {
            return false;
        }
        let ready = match self.ensure_native_engine() {
            Ok(()) => self.publish_native_cfg(),
            Err(err) => Err(err),
        };
        self.side_ask_kick = false;
        if let Err(err) = ready {
            self.fail_native(&err);
            return true;
        }
        let prompted = self
            .acp
            .as_ref()
            .map(|handle| handle.prompt_with_image(last_user, image));
        match prompted {
            Some(Ok(())) => self.note_inflight_card(raw_ask, thread_label),
            Some(Err(err)) => self.fail_native(&err),
            None => self.fail_native("native engine is not running"),
        }
        true
    }

    /// `/flush` and `/dream` on a native thread. `running` stays false so the
    /// following `Done` does not append an assistant bubble, and the job thread
    /// stays clear so the composer does not look busy.
    pub(super) fn prompt_native_memory(&mut self, command: &str, status: &str) -> bool {
        let thread_native = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|thread| thread.native);
        if !grokhub_agent::manual_compact_targets_native(self.cfg.native_engine, thread_native) {
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
            grokhub_acp::AcpHandle::external(cwd.clone(), session_id.clone());
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
        let Some(thread) = self.threads.get_mut(idx).filter(|thread| thread.native) else {
            return (format!("native-{}", grokhub_core::uid("n")), false);
        };
        let mut changed = false;
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

    fn fail_native(&mut self, err: &str) {
        self.abandon_turn_card();
        self.running = false;
        self.scheduled_perm = false;
        self.status = self.apply_job_fail(err);
        self.chat_job_thread = None;
    }

    fn native_workspace(&self) -> std::path::PathBuf {
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
        let workspace = self.native_workspace();
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        let effort = grokhub_core::parse_reasoning_effort(&self.cfg.reasoning_effort).map(str::to_string);
        let rules = grokhub_acp::cabin_rules_for(
            &grokhub_core::brief_for(&self.learning, "chat"),
            self.cfg.desktop_control,
        );
        let system = grokhub_agent::system_prompt(&rules, &workspace);
        // A side question stays read-only even in plan mode, so it never gets plan approval.
        grokhub_agent::set_plan_session(
            session_id,
            matches!(self.session_mode, grokhub_acp::SessionMode::Plan) && !self.side_ask_kick,
        );
        let cfg = LiveCfg {
            workspace,
            model,
            effort,
            system,
            bearer,
            auth_kind,
            gate: self.native_gate(),
            origin: self.harness.turn_origin,
        };
        live_map()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(session_id.to_string(), cfg);
        Ok(())
    }

    fn native_gate(&self) -> Gate {
        let readonly = self.side_ask_kick
            || matches!(self.session_mode, grokhub_acp::SessionMode::Plan | grokhub_acp::SessionMode::Ask);
        let mode = match self.permission_mode {
            grokhub_acp::PermissionMode::Ask => PermMode::Ask,
            grokhub_acp::PermissionMode::Auto => PermMode::Auto,
            grokhub_acp::PermissionMode::AlwaysApprove => PermMode::Always,
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

    /// Lab mode's bearer, in this order: the Imagine sign-in, the Settings → Account
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
        if self.secrets.oauth.is_none() && self.grok_cli_login_present() {
            return Err(NATIVE_NEEDS_CABIN_SIGNIN.into());
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
        let io = self.persist_io.clone();
        let secrets = self.secrets.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = secrets::save(&secrets);
            }
        });
    }

    pub(super) fn paint_native_badge(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.native_engine {
            return;
        }
        let checked_id = ui.id().with("native-usage-sid");
        let native = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|thread| thread.native);
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
        let label = grokhub_agent::usage_label(
            self.grok_usage.input_tokens,
            self.grok_usage.output_tokens,
            self.grok_usage.reasoning_tokens,
            self.grok_usage.cost_in_usd_ticks,
            &self.grok_usage.meter,
        );
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

enum NativeJob {
    Prompt { text: String, image: Option<String> },
    Shutdown,
}

fn serve_native(session_id: String, ext_rx: std::sync::mpsc::Receiver<ExternalCmd>, evt_tx: std::sync::mpsc::Sender<AcpEvent>) {
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
        halt: Box::new(StampHalt { started_ms: 0, read: || None }),
        gate: Gate::phase_readonly(),
        desktop: Some(Box::new(crate::desktop_mcp::NativeDesktop::new())),
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
            let _ = evt_tx.send(AcpEvent::Done { stop_reason: "error".into() });
            continue;
        };
        engine.set_workspace(cfg.workspace);
        engine.set_gate(cfg.gate);
        engine.set_origin(cfg.origin);
        engine.set_imagine_bearer(&cfg.bearer);
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
        let _ = engine.prompt(&text, image.as_deref(), &mut |ev| {
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
