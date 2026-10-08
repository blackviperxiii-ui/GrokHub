//! Route unattended cabin work onto the native engine when Settings → Labs is on.
//! The CLI path stays in the existing runners. This file only changes the transport.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;

struct ReadyModel {
    client: Arc<dyn grokhub_agent::ModelClient + Send + Sync>,
    kind: grokhub_agent::AuthKind,
    bearer: String,
}

struct Prepared {
    ready: ReadyModel,
    workspace: PathBuf,
    model: String,
    effort: Option<String>,
    system: String,
    session_id: String,
    resume: bool,
    gate: grokhub_agent::Gate,
    prompt: String,
    image: Option<String>,
}

#[cfg(test)]
fn test_slot(
) -> &'static std::sync::Mutex<Option<Arc<dyn grokhub_agent::ModelClient + Send + Sync>>> {
    static SLOT: std::sync::OnceLock<
        std::sync::Mutex<Option<Arc<dyn grokhub_agent::ModelClient + Send + Sync>>>,
    > = std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
pub(super) fn set_unattended_client_for_test(
    client: Option<Arc<dyn grokhub_agent::ModelClient + Send + Sync>>,
) {
    *test_slot().lock().unwrap_or_else(|err| err.into_inner()) = client;
}

#[cfg(test)]
fn test_client() -> Option<Arc<dyn grokhub_agent::ModelClient + Send + Sync>> {
    test_slot()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

/// Tokens spent by one-shot runs (chips, greeting, ideas, digest, review) that
/// have no chat to report to. The cabin frame adds them to today's usage.
static EPHEMERAL_SPENT: std::sync::Mutex<Option<grokhub_agent::Usage>> =
    std::sync::Mutex::new(None);

/// Count a one-shot run's tokens toward today's usage, then delete its session
/// file so these runs don't fill History.
fn settle_ephemeral(done: &grokhub_agent::UnattendedDone) {
    if done.usage != grokhub_agent::Usage::default() {
        let mut spent = EPHEMERAL_SPENT
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        spent
            .get_or_insert_with(grokhub_agent::Usage::default)
            .add(&done.usage);
    }
    let _ = grokhub_agent::delete_session(&done.session_id);
}

fn take_ephemeral_spent() -> Option<grokhub_agent::Usage> {
    EPHEMERAL_SPENT
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .take()
}

fn unattended_model(cabin: &mut Cabin) -> Result<ReadyModel, String> {
    #[cfg(test)]
    {
        let _ = cabin;
        if let Some(client) = test_client() {
            return Ok(ReadyModel {
                client,
                kind: grokhub_agent::AuthKind::ApiKey,
                bearer: String::new(),
            });
        }
        #[allow(clippy::needless_return)]
        return Err(grokhub_core::XAI_NEED_SIGNIN.to_string());
    }
    #[cfg(not(test))]
    {
        let (bearer, kind) = cabin.native_cred()?;
        let client = Arc::new(grokhub_agent::XaiClient::new(
            bearer.clone(),
            kind,
            std::time::Duration::from_secs(120),
        ));
        Ok(ReadyModel {
            client,
            kind,
            bearer,
        })
    }
}

fn scheduled_gate(cabin: &Cabin, mode: SessionMode) -> grokhub_agent::Gate {
    let readonly = matches!(mode, SessionMode::Plan | SessionMode::Ask);
    let perm = match cabin.permission_mode {
        PermissionMode::Ask => grokhub_agent::PermMode::Ask,
        PermissionMode::Auto => grokhub_agent::PermMode::Auto,
        PermissionMode::AlwaysApprove => grokhub_agent::PermMode::Always,
    };
    grokhub_agent::Gate {
        mode: perm,
        readonly_session: readonly,
        attended: false,
        desktop: cabin.cfg.desktop_control,
    }
}

fn fast_gate() -> grokhub_agent::Gate {
    grokhub_agent::Gate {
        mode: grokhub_agent::PermMode::Ask,
        readonly_session: false,
        attended: false,
        desktop: false,
    }
}

fn execute(job: Prepared) -> grokhub_agent::UnattendedDone {
    let cancel = grokhub_agent::CancelToken::new();
    let session = job.session_id.clone();
    let _ = grokhub_agent::hub_for(&session);
    grokhub_agent::watch_cancel(&session, cancel.clone());
    let desktop_ops = if job.gate.desktop {
        Some(Box::new(crate::desktop_mcp::NativeDesktop::new())
            as Box<dyn grokhub_agent::DesktopOps>)
    } else {
        None
    };
    grokhub_agent::run_unattended(grokhub_agent::UnattendedRun {
        client: job.ready.client,
        workspace: job.workspace,
        model: job.model,
        effort: job.effort,
        system: job.system,
        session_id: session,
        auth_kind: job.ready.kind,
        prompt: job.prompt,
        image: job.image,
        mode: job.gate.mode,
        readonly_session: job.gate.readonly_session,
        desktop: job.gate.desktop,
        cancel,
        halt: Box::new(grokhub_agent::StampHalt {
            started_ms: now_ms(),
            read: crate::desktop_mcp::read_halt_stamp,
        }),
        desktop_ops,
        imagine_bearer: job.ready.bearer,
        resume: job.resume,
    })
}

fn grok_usage_of(done: &grokhub_agent::UnattendedDone) -> GrokUsage {
    GrokUsage {
        input_tokens: done.usage.input_tokens,
        output_tokens: done.usage.output_tokens,
        reasoning_tokens: done.usage.reasoning_tokens,
        total_tokens: done
            .usage
            .input_tokens
            .saturating_add(done.usage.output_tokens)
            .saturating_add(done.usage.reasoning_tokens),
        cost_in_usd_ticks: done.usage.cost_in_usd_ticks,
        meter: done.meter.clone(),
        context_tokens_used: done.context_tokens_used,
        context_window_tokens: done.context_window_tokens,
        stop_reason: done.stop_reason.clone(),
        ..GrokUsage::default()
    }
}

fn emit_grok_p(tx: &mpsc::Sender<GrokPEvent>, done: grokhub_agent::UnattendedDone) {
    if done.stop_reason == "halted" || done.stop_reason == "cancelled" {
        return;
    }
    if !done.thought.is_empty() {
        let _ = tx.send(GrokPEvent::Thought(done.thought.clone()));
    }
    if !done.text.is_empty() {
        let _ = tx.send(GrokPEvent::Text(done.text.clone()));
    }
    let usage = grok_usage_of(&done);
    if !usage.is_empty() {
        let _ = tx.send(GrokPEvent::Usage(usage.clone()));
    }
    if done.stop_reason == "error" && done.text.trim().is_empty() {
        let err = if done.error.is_empty() {
            "unattended run failed".to_string()
        } else {
            done.error
        };
        let _ = tx.send(GrokPEvent::Err(err));
        return;
    }
    let _ = tx.send(GrokPEvent::End(grokhub_acp::SingleTurn {
        session_id: done.session_id,
        text: done.text,
        thought: done.thought,
        usage,
        stop_reason: done.stop_reason,
    }));
}

pub(super) fn single_turn_json(done: &grokhub_agent::UnattendedDone) -> String {
    serde_json::json!({
        "sessionId": done.session_id,
        "text": done.text,
        "thought": done.thought,
        "stopReason": done.stop_reason,
        "input_tokens": done.usage.input_tokens,
        "output_tokens": done.usage.output_tokens,
        "reasoning_tokens": done.usage.reasoning_tokens,
        "total_tokens": done.usage.input_tokens
            .saturating_add(done.usage.output_tokens)
            .saturating_add(done.usage.reasoning_tokens),
        "cost_in_usd_ticks": done.usage.cost_in_usd_ticks,
        "meter": done.meter,
        "context_tokens_used": done.context_tokens_used,
        "context_window_tokens": done.context_window_tokens,
    })
    .to_string()
}

fn fast_once(
    ready: &ReadyModel,
    workspace: &Path,
    prompt: &str,
    model: &str,
) -> Result<String, String> {
    let done = execute(Prepared {
        ready: ReadyModel {
            client: Arc::clone(&ready.client),
            kind: ready.kind,
            bearer: ready.bearer.clone(),
        },
        workspace: workspace.to_path_buf(),
        model: model.to_string(),
        effort: Some(grokhub_core::BACKGROUND_EFFORT.to_string()),
        system: String::new(),
        session_id: format!("native-fast-{}", uid("f")),
        resume: false,
        gate: fast_gate(),
        prompt: prompt.to_string(),
        image: None,
    });
    settle_ephemeral(&done);
    if done.stop_reason == "halted" || done.stop_reason == "cancelled" {
        return Err(done.stop_reason);
    }
    if done.stop_reason == "error" && done.text.trim().is_empty() {
        return Err(if done.error.is_empty() {
            "unattended run failed".into()
        } else {
            done.error
        });
    }
    Ok(done.text)
}

fn fast_text(ready: &ReadyModel, workspace: &Path, prompt: &str) -> Result<String, String> {
    let primary = fast_once(ready, workspace, prompt, CABIN_FAST_MODEL)?;
    if !primary.trim().is_empty() {
        return Ok(primary);
    }
    fast_once(ready, workspace, prompt, CABIN_FAST_FALLBACK)
}

fn native_rules(cabin: &Cabin, workspace: &std::path::Path) -> String {
    let rules = grokhub_acp::cabin_rules_for(
        &grokhub_core::brief_for(&cabin.learning, "chat"),
        cabin.cfg.desktop_control,
    );
    grokhub_agent::system_prompt(&rules, workspace)
}

fn scheduled_session(cabin: &Cabin) -> (String, bool) {
    let idx = cabin
        .chat_job_thread
        .as_deref()
        .and_then(|id| cabin.threads.iter().position(|t| t.id == id))
        .unwrap_or(cabin.thread_idx);
    let existing = cabin
        .threads
        .get(idx)
        .and_then(|t| t.grok_session.clone())
        .unwrap_or_default();
    if existing.starts_with("native-") {
        (existing, true)
    } else {
        (format!("native-u-{}", uid("u")), false)
    }
}

impl Cabin {
    /// Add one-shot native runs' tokens to today's usage. Flag off: nothing to drain.
    pub(super) fn drain_native_unattended_usage(&mut self) {
        let Some(spent) = take_ephemeral_spent() else {
            return;
        };
        self.roll_today();
        add_tokens(
            &mut self.usage,
            spent.input_tokens,
            spent.output_tokens,
            spent.reasoning_tokens,
        );
        self.note_token_budget();
        self.persist_usage();
    }

    /// Drop one-shot native receivers on Halt and quit. Flag off leaves the CLI receivers alone.
    pub(super) fn stop_native_unattended(&mut self) {
        if !self.cfg.native_engine {
            return;
        }
        grokhub_agent::halt_all_sessions();
        grokhub_agent::mcp::shutdown_all();
        self.grok_loop_rx = None;
        self.review_rx = None;
        self.chip_rx = None;
        self.greeting_rx = None;
        self.ideas_rx = None;
        self.digest_rx = None;
        self.review_busy = false;
        self.chip_busy = false;
        self.greeting_busy = false;
        self.digest_busy = false;
    }

    pub(super) fn start_native_scheduled(
        &mut self,
        prompt: &str,
        cwd: PathBuf,
        model: &str,
        effort: Option<String>,
        mode: SessionMode,
        image: Option<String>,
    ) {
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(GrokPEvent::Err(err));
                self.grok_p_rx = Some(rx);
                return;
            }
        };
        let (session_id, resume) = scheduled_session(self);
        let gate = scheduled_gate(self, mode);
        let system = native_rules(self, &cwd);
        let job = Prepared {
            ready,
            workspace: cwd,
            model: model.to_string(),
            effort,
            system,
            session_id,
            resume,
            gate,
            prompt: prompt.to_string(),
            image,
        };
        let (tx, rx) = mpsc::channel();
        self.grok_p_rx = Some(rx);
        std::thread::spawn(move || {
            let done = execute(job);
            emit_grok_p(&tx, done);
        });
    }

    pub(super) fn spawn_native_loop(&mut self, row: GrokLoop) {
        bump_usage(&mut self.usage, "automation");
        self.daily_auto_used = self.usage.automation;
        self.daily_auto_day = self.usage.day.clone();
        self.persist_usage();
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let resume = row
            .session_id
            .as_deref()
            .is_some_and(|id| id.starts_with("native-"));
        let session_id = if resume {
            row.session_id.clone().unwrap_or_default()
        } else {
            format!("native-loop-{}", uid("l"))
        };
        let cwd = self.grok_cwd();
        let gate = scheduled_gate(self, self.session_mode);
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        let system = native_rules(self, &cwd);
        let prompt = row.prompt.clone();
        let id = row.id.clone();
        let (tx, rx) = mpsc::channel();
        self.grok_loop_rx = Some((id, rx));
        let job = Prepared {
            ready,
            workspace: cwd,
            model,
            effort: Some(grokhub_core::BACKGROUND_EFFORT.to_string()),
            system,
            session_id,
            resume,
            gate,
            prompt,
            image: None,
        };
        std::thread::spawn(move || {
            let done = execute(job);
            if done.stop_reason == "halted" || done.stop_reason == "cancelled" {
                return;
            }
            let text = if done.text.trim().is_empty() && !done.error.is_empty() {
                done.error
            } else if done.text.trim().is_empty() && done.thought.trim().is_empty() {
                return;
            } else {
                single_turn_json(&done)
            };
            let _ = tx.send(text);
        });
    }

    pub(super) fn spawn_native_review(&mut self) {
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(Err(err));
                self.review_rx = Some(rx);
                self.review_busy = true;
                return;
            }
        };
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let (thread_lines, host_receipts) = self.review_chat_digest();
        let insight = insight_pin(&self.learning);
        let skill_names: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let automation_names: Vec<String> =
            self.automations.iter().map(|a| a.name.clone()).collect();
        let github_pat = !self.secrets.github_token.trim().is_empty();
        let turned_down: Vec<String> = self
            .cfg
            .feed_pulse
            .turned_down
            .iter()
            .rev()
            .cloned()
            .collect();
        let chip_habits = top_habit_labels(&self.chip_memory, 6);
        let now = now_ms();
        let model = model_for_mode("balanced").to_string();
        let prompt = review_system_prompt().to_string();
        let workspace = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.review_rx = Some(rx);
        self.review_busy = true;
        std::thread::spawn(move || {
            if config::read_memory(&mem_name) != mem_body {
                let _ = config::write_memory(&mem_name, &mem_body);
            }
            let digest = build_review_digest(&ReviewDigest {
                insight_pin: insight,
                user_md: config::read_memory("USER.md"),
                memory_md: config::read_memory("MEMORY.md"),
                skill_names,
                automation_names,
                turned_down,
                github_pat,
                host_receipts,
                chip_habits,
                thread_lines,
                trajectory: summarize_trajectory(
                    &parse_trajectory_jsonl(&crate::store::read_trajectory()),
                    yesterday_ms(now),
                    12,
                ),
            });
            let done = execute(Prepared {
                ready,
                workspace,
                model,
                effort: Some(grokhub_core::BACKGROUND_EFFORT.to_string()),
                system: prompt,
                session_id: format!("native-review-{}", uid("r")),
                resume: false,
                gate: fast_gate(),
                prompt: digest,
                image: None,
            });
            settle_ephemeral(&done);
            let out = if done.stop_reason == "halted" || done.stop_reason == "cancelled" {
                Err(done.stop_reason)
            } else if done.stop_reason == "error" && done.text.trim().is_empty() {
                Err(if done.error.is_empty() {
                    "review failed".into()
                } else {
                    done.error
                })
            } else {
                Ok(done.text)
            };
            let _ = tx.send(out);
        });
    }

    pub(super) fn spawn_native_chips(&mut self) {
        let chat = self.chip_chat_pairs();
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let habits = top_habit_labels(&self.chip_memory, 6);
        let others = self.other_chip_threads();
        let mut dismissed = self.chip_dismissed.clone();
        for hit in &self.chip_memory.hits {
            if hit.dismisses >= 1 {
                dismissed.push(hit.value.clone());
                dismissed.push(hit.label.clone());
            }
        }
        let prompt =
            chip_suggest_prompt(&chat, &title, &self.composer, &habits, &dismissed, &others);
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let workspace = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.chip_rx = Some(rx);
        self.chip_busy = true;
        std::thread::spawn(move || {
            let text = fast_text(&ready, &workspace, &prompt).unwrap_or_default();
            let _ = tx.send(parse_llm_chips(&text));
        });
    }

    pub(super) fn spawn_native_greeting(&mut self, prompt: String) {
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let workspace = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.greeting_rx = Some(rx);
        self.greeting_busy = true;
        std::thread::spawn(move || {
            let text = fast_text(&ready, &workspace, &prompt).unwrap_or_default();
            let _ = tx.send(text);
        });
    }

    pub(super) fn spawn_native_ideas(&mut self, prompt: String, tx: mpsc::Sender<String>) {
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                self.status = err;
                let _ = tx.send(String::new());
                return;
            }
        };
        let workspace = self.grok_cwd();
        std::thread::spawn(move || {
            let text = fast_text(&ready, &workspace, &prompt).unwrap_or_default();
            let _ = tx.send(text);
        });
    }

    pub(super) fn spawn_native_digest(&mut self) {
        let prompt = grokhub_core::pulse::feed_prompt(&self.digest_steer, &self.cfg.feed_instructions);
        let ready = match unattended_model(self) {
            Ok(ready) => ready,
            Err(err) => {
                self.status = err;
                self.digest_pending = Some("NONE".into());
                self.digest_wants_lookup = false;
                return;
            }
        };
        let workspace = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.digest_rx = Some(rx);
        self.digest_busy = true;
        std::thread::spawn(move || {
            let out = fast_once(&ready, &workspace, &prompt, CABIN_FAST_MODEL);
            let _ = tx.send(out);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use grokhub_agent::{ClientError, ModelClient, StreamEvent, TurnOutput, Usage};

    struct Say {
        text: std::sync::Mutex<String>,
        calls: AtomicUsize,
    }

    impl ModelClient for Say {
        fn stream(
            &self,
            _req: &grokhub_agent::ResponsesRequest,
            _cancel: &grokhub_agent::CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let text = self
                .text
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone();
            sink(StreamEvent::TextDelta(text.clone()));
            Ok(TurnOutput {
                text,
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 4,
                    output_tokens: 3,
                    reasoning_tokens: 1,
                    cost_in_usd_ticks: 5,
                    cached_tokens: 0,
                },
            })
        }
    }

    fn fn_body(src: &str, name: &str) -> String {
        let marker = format!("fn {name}");
        let rest = src.split(&marker).nth(1).expect(name);
        let end = ["\n    pub(super) fn ", "\n    fn ", "\nfn "]
            .iter()
            .filter_map(|sep| rest.find(sep))
            .min()
            .unwrap_or(rest.len());
        rest[..end].to_string()
    }

    fn wait_until(mut ready: impl FnMut() -> bool) {
        let start = Instant::now();
        while !ready() {
            if start.elapsed() > Duration::from_secs(4) {
                panic!("timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    struct Env {
        prev: Option<String>,
    }

    impl Env {
        fn set(root: &std::path::Path) -> Self {
            let prev = std::env::var("GROKHUB_CONFIG").ok();
            std::env::set_var("GROKHUB_CONFIG", root);
            Self { prev }
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            match &self.prev {
                Some(value) => std::env::set_var("GROKHUB_CONFIG", value),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
    }

    fn isolated(label: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf, Env) {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let env = Env::set(&root);
        set_unattended_client_for_test(None);
        (lock, root, env)
    }

    #[test]
    fn native_engine_defaults_off_and_each_runner_keeps_its_cli_path() {
        assert!(!AppConfig::default().native_engine);
        let kick = fn_body(include_str!("chat_kick.rs"), "kick_model");
        assert!(kick.contains("spawn_grok_p_stream"));
        let native_at = kick
            .find("cfg.native_engine && self.scheduled_perm")
            .expect("native branch");
        let spawn_at = kick.find("spawn_grok_p_stream").expect("cli spawn");
        assert!(native_at < spawn_at);
        let sched = kick.find("!self.scheduled_perm").expect("sched");
        let acp = kick.find("uses_acp").expect("acp");
        assert!(sched < acp && acp < spawn_at);
        assert!(kick.contains("scheduled_flags"));
        assert!(kick.contains("fail_ask_without_acp"));
        let send = fn_body(include_str!("chat_kick.rs"), "send_chat");
        assert!(send.contains("persist_user_turn"));
        assert!(send.contains("self.can_agent()"));
        assert!(send.contains("cfg.native_engine && self.scheduled_perm"));

        let fire = fn_body(include_str!("night.rs"), "fire_loop");
        assert!(fire.contains("grok_user_stdout_wait"));
        assert!(fire.contains("-p"));
        assert!(fire.contains("scheduled_args"));
        assert!(fire.contains("self.cfg.native_engine"));
        assert!(!fire.contains("\"--always-approve\""));
        let tick = fn_body(include_str!("night.rs"), "tick_loops");
        assert!(tick.contains("find_grok()"));
        assert!(tick.contains("self.cfg.native_engine"));
        let night = fn_body(include_str!("night.rs"), "fire_night");
        assert!(night.contains("start_scheduled_run"));
        assert!(night.contains("night_unauth_should_skip"));
        assert!(night.contains("self.can_agent()"));
        assert!(night.contains("Night skipped {} ({why})"));
        let start = fn_body(include_str!("night.rs"), "start_scheduled_run");
        assert!(start.contains("self.cfg.native_engine"));
        assert!(start.contains("start_bg_task"));
        assert!(start.contains("The run did not start"));
        let review = fn_body(include_str!("night.rs"), "spawn_review");
        assert!(review.contains("grok_chat"));
        assert!(review.contains("model_for_mode(\"balanced\")"));
        assert!(review.contains("self.cfg.native_engine"));
        let review_tick = fn_body(include_str!("night.rs"), "tick_review");
        assert!(review_tick.contains("self.llm_ready()"));
        assert!(!review_tick.contains("send_chat"));

        let drain = fn_body(include_str!("mod.rs"), "drain_inbox");
        assert!(drain.contains("send_scheduled_chat"));
        assert!(drain.contains("self.can_agent()"));
        assert!(!drain.contains("self.llm_ready()"));
        assert!(drain.contains("self.cfg.native_engine"));

        let ideas = fn_body(include_str!("feed_ui.rs"), "maybe_suggest_ideas");
        assert!(ideas.contains("cabin_fast_llm"));
        assert!(ideas.contains("self.cfg.native_engine"));
        let digest = fn_body(include_str!("feed_ui.rs"), "follow_feed_lookup");
        assert!(digest.contains("grok_chat"));
        assert!(digest.contains("BACKGROUND_EFFORT"));
        assert!(digest.contains("self.cfg.native_engine"));
        let native_at = digest
            .find("self.cfg.native_engine")
            .expect("digest native");
        let test_at = digest.find("cfg!(test)").expect("digest test gate");
        assert!(native_at < test_at);

        let chips = fn_body(include_str!("chips.rs"), "spawn_chip_llm");
        assert!(chips.contains("cabin_fast_llm"));
        assert!(chips.contains("find_grok"));
        assert!(chips.contains("self.cfg.native_engine"));
        let greet = fn_body(include_str!("chips.rs"), "spawn_greeting_llm");
        assert!(greet.contains("cabin_fast_llm"));
        assert!(greet.contains("find_grok"));
        assert!(greet.contains("self.cfg.native_engine"));

        let here = include_str!("native_unattended.rs")
            // Not "\n#[cfg(test)]\n...": a Windows checkout has CRLF line endings.
            .split(concat!("mod tests", " {"))
            .next()
            .unwrap_or_default();
        assert!(here.contains("fn execute("), "runtime half of the file");
        assert!(!here.contains("grok_cli_key"));
        assert!(!here.contains("find_grok"));
        assert!(!here.contains("spawn_grok_p_stream"));
        assert!(!here.contains("grok_stdout"));
        for banned in [
            concat!("auth", ".json"),
            concat!("config", ".toml"),
            concat!("cli-chat-", "proxy"),
            concat!("GROK", "_HOME"),
        ] {
            assert!(!here.contains(banned), "{banned}");
        }
    }

    #[test]
    fn native_single_turn_json_matches_the_cli_parser() {
        let done = grokhub_agent::UnattendedDone {
            text: "Loop report is ready".into(),
            thought: String::new(),
            stop_reason: "end_turn".into(),
            error: String::new(),
            usage: Usage {
                input_tokens: 4,
                output_tokens: 3,
                reasoning_tokens: 1,
                cost_in_usd_ticks: 5,
                cached_tokens: 0,
            },
            meter: "API credits".into(),
            context_tokens_used: 9,
            context_window_tokens: 100,
            session_id: "native-loop-shape".into(),
            permission_cards: 0,
            elicit_cards: 0,
        };
        let turn = grokhub_acp::parse_single_turn(&single_turn_json(&done)).expect("json");
        assert_eq!(turn.session_id, "native-loop-shape");
        assert_eq!(turn.text, "Loop report is ready");
        assert_eq!(turn.usage.input_tokens, 4);
        assert!(!turn.usage.is_empty());
        let ideas = grokhub_core::parse_ideas(
            "IDEA: skill | Standup note | Write the standup | They ask every morning | Keep a short note | draft the standup",
            &[],
            &[],
        );
        assert_eq!(ideas.len(), 1);
        assert_eq!(ideas[0].title, "Standup note");
        let chips = parse_llm_chips(r#"[{"label":"Check status","value":"status please"}]"#);
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].label, "Check status");
        let greet = pick_greeting_against("", Some("Good to see you today"), &[]);
        assert_eq!(greet, "Good to see you today");
        let review = parse_suggest_lines(
            "SUGGEST_AUTO: Night wrap | Close the day | every day at 21, say good night",
        );
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].title, "Night wrap");
        let lookup =
            grokhub_core::parse_lookup("A note about the work.\nhttps://example.com/story");
        assert!(lookup.found);
        let a = grokhub_core::automation_done_card("loop-a", "Write a status report", "ready", 1);
        let b = grokhub_core::automation_done_card("loop-b", "Write a status report", "ready", 2);
        assert_eq!(
            grokhub_core::feed_group_key(&a).as_deref(),
            Some("run:loop-a")
        );
        assert_eq!(
            grokhub_core::feed_group_key(&b).as_deref(),
            Some("run:loop-b")
        );
    }

    #[test]
    fn native_runners_on_the_fake_server_keep_card_shape_and_missing_cred_fails() {
        let (_lock, root, _env) = isolated("native-runners");
        let say = Arc::new(Say {
            text: std::sync::Mutex::new("Loop report is ready".into()),
            calls: AtomicUsize::new(0),
        });
        set_unattended_client_for_test(Some(say.clone()));
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.native_engine = true;
        let mut row = new_loop("30m".into(), "Write a status report".into(), now_ms());
        row.id = "loop-a".into();
        cabin.grok_loops.push(row.clone());
        cabin.fire_loop(row);
        wait_until(|| {
            cabin.poll_grok_loop();
            cabin.grok_loop_rx.is_none()
        });
        assert!(say.calls.load(Ordering::SeqCst) >= 1);
        let card = cabin
            .updates
            .iter()
            .find(|card| grokhub_core::feed_group_key(card).as_deref() == Some("run:loop-a"))
            .expect("loop card");
        assert_eq!(
            card.kind,
            grokhub_core::UpdateKind::AutomationDone,
            "{card:?}"
        );
        assert_eq!(card.source_id, "loop-a");
        assert!(
            cabin
                .board
                .iter()
                .any(|item| item.report.contains("Loop report is ready")),
            "the loop reply lands in Follow up"
        );
        let saved = cabin
            .grok_loops
            .iter()
            .find(|row| row.id == "loop-a")
            .and_then(|row| row.session_id.clone())
            .unwrap_or_default();
        assert!(saved.starts_with("native-"), "{saved}");

        say.text
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone_from(&"Good to see you today".to_string());
        cabin.spawn_greeting_llm("say hello".into());
        wait_until(|| {
            cabin.poll_greeting();
            !cabin.greeting_busy
        });
        assert_eq!(cabin.greeting, "Good to see you today");
        let before = cabin.usage.tokens_in;
        cabin.drain_native_unattended_usage();
        assert!(
            cabin.usage.tokens_in >= before + 4,
            "one-shot tokens count today"
        );
        assert!(
            grokhub_agent::list_sessions()
                .iter()
                .all(|info| !info.id.starts_with("native-fast-")),
            "one-shot runs don't fill History"
        );

        say.text
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone_from(&r#"[{"label":"Check status","value":"status please"}]"#.to_string());
        cabin.spawn_chip_llm();
        wait_until(|| {
            cabin.poll_chips();
            !cabin.chip_busy
        });
        assert!(cabin
            .llm_chips
            .iter()
            .any(|chip| chip.label == "Check status"));

        say.text
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone_from(
                &"SUGGEST_AUTO: Night wrap | Close the day | every day at 21, say good night"
                    .to_string(),
            );
        cabin.spawn_review();
        wait_until(|| {
            cabin.poll_review();
            !cabin.review_busy
        });
        assert!(
            !cabin.status.contains("Nightly review held"),
            "{}",
            cabin.status
        );

        say.text
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone_from(&"A note about the work.\nhttps://example.com/story".to_string());
        cabin.digest_wants_lookup = true;
        cabin.digest_busy = false;
        cabin.digest_pending = None;
        cabin.follow_feed_lookup();
        wait_until(|| {
            cabin.poll_digest_lookup();
            cabin.digest_pending.is_some()
        });
        let pending = cabin.digest_pending.clone().unwrap_or_default();
        assert!(grokhub_core::parse_lookup(&pending).found, "{pending}");

        say.text.lock().unwrap_or_else(|err| err.into_inner()).clone_from(
            &"IDEA: skill | Standup note | Write the standup | They ask every morning | Keep a short note | draft the standup".to_string(),
        );
        cabin.maybe_suggest_ideas(true);
        wait_until(|| {
            cabin.poll_ideas();
            cabin.ideas_rx.is_none()
        });
        assert!(!cabin.status.contains(grokhub_core::XAI_NEED_SIGNIN));

        set_unattended_client_for_test(None);
        let mut missed = new_loop("30m".into(), "Write a status report".into(), now_ms());
        missed.id = "loop-miss".into();
        cabin.grok_loops.push(missed.clone());
        cabin.fire_loop(missed);
        assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
        assert!(cabin.grok_loop_rx.is_none());
        cabin.spawn_review();
        wait_until(|| {
            cabin.poll_review();
            !cabin.review_busy
        });
        assert!(
            cabin.status.contains(grokhub_core::XAI_NEED_SIGNIN),
            "{}",
            cabin.status
        );
        let _ = std::fs::remove_dir_all(&root);
        set_unattended_client_for_test(None);
    }

    #[test]
    fn native_scheduled_run_ends_like_the_cli_stream() {
        let (_lock, root, _env) = isolated("native-scheduled");
        let say = Arc::new(Say {
            text: std::sync::Mutex::new("Automation report".into()),
            calls: AtomicUsize::new(0),
        });
        set_unattended_client_for_test(Some(say.clone()));
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.native_engine = true;
        cabin.permission_mode = PermissionMode::Ask;
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        cabin.start_native_scheduled(
            "write the report",
            work,
            "grok-4.7",
            None,
            SessionMode::Chat,
            None,
        );
        let rx = cabin.grok_p_rx.take().expect("native stream");
        let mut text = String::new();
        let mut ended = None;
        let start = Instant::now();
        while ended.is_none() && start.elapsed() < Duration::from_secs(4) {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(GrokPEvent::Text(t)) => text.push_str(&t),
                Ok(GrokPEvent::End(turn)) => ended = Some(turn),
                Ok(GrokPEvent::Err(err)) => panic!("{err}"),
                _ => {}
            }
        }
        let turn = ended.expect("End event");
        assert_eq!(text, "Automation report");
        assert_eq!(turn.text, "Automation report");
        assert!(
            turn.session_id.starts_with("native-"),
            "{}",
            turn.session_id
        );
        assert_eq!(turn.usage.input_tokens, 4);
        assert_eq!(say.calls.load(Ordering::SeqCst), 1);

        set_unattended_client_for_test(None);
        cabin.start_native_scheduled(
            "again",
            root.clone(),
            "grok-4.7",
            None,
            SessionMode::Chat,
            None,
        );
        let rx = cabin.grok_p_rx.take().expect("error stream");
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(GrokPEvent::Err(err)) => assert_eq!(err, grokhub_core::XAI_NEED_SIGNIN),
            _ => panic!("missing credential must fail with a clear status"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    struct HoldUntilCancel {
        started: std::sync::atomic::AtomicBool,
        saw_cancel: std::sync::atomic::AtomicBool,
        session: std::sync::Mutex<String>,
        calls: AtomicUsize,
    }

    impl ModelClient for HoldUntilCancel {
        fn stream(
            &self,
            req: &grokhub_agent::ResponsesRequest,
            cancel: &grokhub_agent::CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.session.lock().unwrap_or_else(|err| err.into_inner()) =
                req.conversation_id.clone();
            self.started.store(true, Ordering::SeqCst);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                if cancel.is_cancelled() {
                    self.saw_cancel.store(true, Ordering::SeqCst);
                    return Err(ClientError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(ClientError::Protocol("halt never arrived".into()))
        }
    }

    #[test]
    fn halt_stops_an_in_flight_native_scheduled_run() {
        let (_lock, root, _env) = isolated("native-halt");
        let hold = Arc::new(HoldUntilCancel {
            started: std::sync::atomic::AtomicBool::new(false),
            saw_cancel: std::sync::atomic::AtomicBool::new(false),
            session: std::sync::Mutex::new(String::new()),
            calls: AtomicUsize::new(0),
        });
        set_unattended_client_for_test(Some(hold.clone()));
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.native_engine = true;
        cabin.permission_mode = PermissionMode::Ask;
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        cabin.start_native_scheduled(
            "long chore",
            work,
            "grok-4.7",
            None,
            SessionMode::Chat,
            None,
        );
        let rx = cabin.grok_p_rx.take().expect("native stream");
        wait_until(|| hold.started.load(Ordering::SeqCst));
        let session = hold
            .session
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert!(session.starts_with("native-"), "{session}");
        assert!(grokhub_agent::session_is_live(&session), "run is in flight");

        cabin.halt_in_flight();

        let start = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(GrokPEvent::End(turn)) => panic!("a halted run must not finish: {}", turn.text),
                Ok(GrokPEvent::Text(text)) => panic!("a halted run must not reply: {text}"),
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    assert!(
                        start.elapsed() < Duration::from_secs(4),
                        "run thread did not stop"
                    );
                }
            }
        }
        assert!(
            hold.saw_cancel.load(Ordering::SeqCst),
            "Halt reached the model call"
        );
        assert_eq!(hold.calls.load(Ordering::SeqCst), 1, "no turn after Halt");
        assert!(
            !grokhub_agent::session_is_live(&session),
            "no running task is left"
        );
        assert!(cabin.grok_p_rx.is_none());
        assert!(!cabin.running);
        set_unattended_client_for_test(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Pulse: three likes and dislikes and the cabin rewrites the Feed
    /// instructions itself, on the fake model.
    #[test]
    fn pulse_rewrites_feed_instructions_after_likes_and_dislikes() {
        let (_lock, root, _env) = isolated("pulse-rewrite");
        let after = "Keep my feed short.\n\nShow more of:\n- Rust and egui releases.\n\nShow less of:\n- Crypto price swings.";
        let say = Arc::new(Say {
            text: std::sync::Mutex::new(after.into()),
            calls: AtomicUsize::new(0),
        });
        set_unattended_client_for_test(Some(say.clone()));
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.native_engine = true;
        cabin.cfg.feed_instructions = "Keep my feed short.".into();
        cabin.updates = vec![
            grokhub_core::digest_card("d1", "Rust 1.92 ships", "Faster builds.", 1),
            grokhub_core::digest_card("d2", "Crypto prices jump", "A big week.", 2),
            grokhub_core::digest_card("d3", "egui 0.37 is out", "New layout tools.", 3),
        ];
        let id = |cabin: &Cabin, title: &str| {
            cabin
                .updates
                .iter()
                .find(|c| c.title == title)
                .unwrap()
                .id
                .clone()
        };
        let rust = id(&cabin, "Rust 1.92 ships");
        let crypto = id(&cabin, "Crypto prices jump");
        let egui_post = id(&cabin, "egui 0.37 is out");
        cabin.pulse_like(&rust, "2026-10-04");
        cabin.pulse_not_this(&crypto, "2026-10-04");
        assert_eq!(cabin.cfg.feed_pulse.taste_since_rewrite, 2);
        assert!(
            cabin.pulse_view.rewrite_rx.is_none(),
            "no rewrite before the third"
        );
        assert_eq!(cabin.cfg.feed_instructions, "Keep my feed short.");
        cabin.pulse_like(&egui_post, "2026-10-04");
        assert!(cabin.pulse_view.rewrite_rx.is_some());
        wait_until(|| {
            cabin.poll_pulse_rewrite();
            cabin.pulse_view.rewrite_rx.is_none()
        });
        assert_eq!(cabin.cfg.feed_instructions, after);
        assert_eq!(cabin.cfg.feed_pulse.taste_since_rewrite, 0);
        assert_eq!(say.calls.load(Ordering::SeqCst), 1);
        let memory = crate::config::read_memory("MEMORY.md");
        for line in [
            "- [2026-10-04] pulse: liked \"Rust 1.92 ships\" reason=liked",
            "- [2026-10-04] pulse: dismissed \"Crypto prices jump\" reason=not-this",
            "- [2026-10-04] pulse: liked \"egui 0.37 is out\" reason=liked",
        ] {
            assert!(memory.contains(line), "{line} in {memory}");
        }
        // The next lookup carries the rewritten text.
        let prompt = grokhub_core::pulse::feed_prompt("Rust", &cabin.cfg.feed_instructions);
        assert!(prompt.ends_with("- Crypto price swings."), "{prompt}");
        // A reply that is not instructions keeps the current text.
        say.text
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone_from(&"NONE".to_string());
        cabin.spawn_pulse_rewrite();
        wait_until(|| {
            cabin.poll_pulse_rewrite();
            cabin.pulse_view.rewrite_rx.is_none()
        });
        assert_eq!(cabin.cfg.feed_instructions, after);
        set_unattended_client_for_test(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pulse_idea_titles_come_from_the_model_capped_at_70_with_a_plain_fallback() {
        let (_lock, root, _env) = isolated("pulse-titles");
        std::fs::create_dir_all(crate::config::memory_dir()).unwrap();
        std::fs::write(
            crate::config::memory_dir().join("USER.md"),
            "- Keeps the GrokHub cargo suite green before standup each morning.\n- Ships a GrokHub release most weeks and writes the notes by hand.\n- Has a car registration renewal coming up at the county office.\n",
        )
        .unwrap();
        let reply = [
            "IDEA: automation | I can run your GrokHub tests before you sit down | Your tests are done before you start. | You keep the suite green before standup. | Every weekday at 8 the cabin runs cargo test in ~/GrokHub and posts failures to your feed. | every weekday at 8, run cargo test in ~/GrokHub and summarize failures",
            "IDEA: reminder | I can remind you about the county car registration renewal before it lapses this month | A nudge before the renewal lapses. | Your renewal is coming up. | A reminder a week before the county renewal date, with the office hours. | every monday at 9, remind me to renew the car registration",
            "IDEA: skill | Release notes from merged PRs | Notes drafted from what merged. | You write every release's notes by hand. | A saved procedure that reads the merged pull requests and drafts grouped notes for your review. | draft release notes from the merged pull requests since the last tag",
        ]
        .join("\n");
        let say = Arc::new(Say {
            text: std::sync::Mutex::new(reply),
            calls: AtomicUsize::new(0),
        });
        set_unattended_client_for_test(Some(say.clone()));
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.native_engine = true;
        cabin.updates.clear();
        cabin.maybe_suggest_ideas(true);
        wait_until(|| {
            cabin.poll_ideas();
            cabin.ideas_rx.is_none()
        });
        assert_eq!(say.calls.load(Ordering::SeqCst), 1);
        let mut rows: Vec<(String, String)> = cabin
            .updates
            .iter()
            .filter(|c| c.kind == grokhub_core::UpdateKind::Idea)
            .map(|c| (c.title.clone(), grokhub_core::pulse::i_can_title(c)))
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                (
                    "I can run your GrokHub tests before you sit down".to_string(),
                    "I can run your GrokHub tests before you sit down".to_string()
                ),
                (
                    "Release notes from merged PRs".to_string(),
                    "I can learn your release notes from merged PRs".to_string()
                ),
            ],
            "the model's own line is kept as written; a title over 70 is dropped; an old-style title gets the plain fallback"
        );
        for (_, line) in &rows {
            assert!(
                line.chars().count() <= grokhub_core::pulse::I_CAN_MAX,
                "{line}"
            );
            assert!(!line.contains("help with") && !line.contains('"'), "{line}");
        }
        set_unattended_client_for_test(None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
