//! `/bg`, `/send` and `/consult`: one prompt run beside the chat on the
//! engine's unattended path. Ask mode, read-only session: nobody can answer
//! a permission card, so anything that would change a file is refused, the
//! same as the cabin's `native_unattended.rs`. A run's result stays here
//! until the app closes; a bare `/bg` lists them.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use grokhub_agent::PermMode;
use grokhub_agent::{AuthKind, CancelToken, ModelClient, UnattendedDone, UnattendedRun};
use grokhub_core::{bg_task_prompt, bg_task_title, BgEnd, BG_TASK_MAX};

#[derive(Clone)]
enum Phase {
    Running,
    Ended(BgEnd, String),
}

struct Run {
    id: u64,
    title: String,
    cancel: CancelToken,
    /// `/bg stop` asked for it; the engine can also end a token on its own.
    stop_asked: bool,
    phase: Phase,
}

static RUNS: Mutex<Vec<Run>> = Mutex::new(Vec::new());

fn runs() -> std::sync::MutexGuard<'static, Vec<Run>> {
    RUNS.lock().unwrap_or_else(|e| e.into_inner())
}

/// One unattended prompt, read-only, in Ask mode.
pub fn ask_once(
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: PathBuf,
    session_id: String,
    prompt: String,
    cancel: CancelToken,
) -> UnattendedDone {
    let system = crate::agent::system_prompt(&workspace, false, "");
    grokhub_agent::run_unattended(UnattendedRun {
        client,
        workspace,
        model: model.to_string(),
        effort: None,
        system,
        session_id,
        auth_kind: AuthKind::ApiKey,
        prompt,
        image: None,
        mode: PermMode::Ask,
        readonly_session: true,
        desktop: false,
        cancel,
        halt: Box::new(crate::agent::NeverHalted),
        desktop_ops: None,
        imagine_bearer: String::new(),
        resume: false,
    })
}

/// Background runs going now (the Workboard queue waits for room).
pub fn live_count() -> usize {
    runs()
        .iter()
        .filter(|r| matches!(r.phase, Phase::Running))
        .count()
}

/// Starts `task` on its own thread and returns its title at once.
pub fn start(
    config_dir: PathBuf,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: String,
    workspace: PathBuf,
    task: &str,
) -> Result<String, String> {
    let task = task.trim();
    if task.is_empty() {
        return Err("Nothing to run".into());
    }
    let title = bg_task_title(task);
    let cancel = CancelToken::new();
    let id = {
        let mut all = runs();
        let live = all
            .iter()
            .filter(|r| matches!(r.phase, Phase::Running))
            .count();
        if live >= BG_TASK_MAX {
            return Err(format!(
                "{BG_TASK_MAX} background tasks are already running. Stop one first."
            ));
        }
        let id = all.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        all.push(Run {
            id,
            title: title.clone(),
            cancel: cancel.clone(),
            stop_asked: false,
            phase: Phase::Running,
        });
        id
    };
    let prompt = bg_task_prompt(task);
    std::thread::spawn(move || {
        let _config = grokhub_agent::perm::ConfigGuard::set(&config_dir);
        let session = format!("shell-bg-{id}-{}", grokhub_core::now_ms());
        let done = ask_once(client, &model, workspace, session, prompt, cancel);
        let end = if done.stop_reason == "halted" {
            BgEnd::Stopped
        } else if !done.error.is_empty() {
            BgEnd::Failed(done.error.clone())
        } else {
            BgEnd::Done
        };
        finish(id, end, &done.text);
    });
    Ok(title)
}

fn finish(id: u64, end: BgEnd, reply: &str) {
    let body = grokhub_core::bg_result_body(&end, reply);
    if let Some(run) = runs().iter_mut().find(|r| r.id == id) {
        let end = if run.stop_asked { BgEnd::Stopped } else { end };
        let body = if run.stop_asked {
            grokhub_core::bg_result_body(&end, reply)
        } else {
            body
        };
        run.phase = Phase::Ended(end, body);
    }
}

/// Bare `/bg`: what is running and what ended.
pub fn status() -> String {
    let all = runs();
    if all.is_empty() {
        return "No background tasks. /bg <task> starts one.".into();
    }
    all.iter()
        .map(|r| match &r.phase {
            Phase::Running => format!("Running · {}", r.title),
            Phase::Ended(end, body) => {
                let head = match end {
                    BgEnd::Done => "Done",
                    BgEnd::Failed(_) => "Failed",
                    BgEnd::Stopped => "Stopped",
                };
                if body.is_empty() {
                    format!("{head} · {}", r.title)
                } else {
                    format!("{head} · {}\n{body}", r.title)
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `/bg stop`: cancels every run that is going. How many it stopped.
pub fn stop_all() -> usize {
    let mut all = runs();
    let mut stopped = 0;
    for run in all.iter_mut().filter(|r| matches!(r.phase, Phase::Running)) {
        run.stop_asked = true;
        run.cancel.cancel();
        stopped += 1;
    }
    stopped
}

/// `/bg stop`'s answer.
pub fn stop_text() -> String {
    match stop_all() {
        0 => "No background tasks running".into(),
        1 => "Stopped 1 background task".into(),
        n => format!("Stopped {n} background tasks"),
    }
}

/// What `/send` (`sent`) and `/bg <task>` answer once the run is going.
pub fn started_text(sent: bool, title: &str) -> String {
    let lead = if sent { "Sent" } else { "Background" };
    format!(
        "{lead} · {title}. It reads and reports, and can't change files. /bg shows how it went."
    )
}

/// Whether anything is still running (for the status line).
pub fn any_live() -> bool {
    live_count() > 0
}

#[cfg(test)]
pub fn clear_for_test() {
    runs().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{
        ClientError, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
    };

    /// The tests share the one run list.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Asks to write a file, then answers with text.
    struct WantsToWrite {
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl ModelClient for WantsToWrite {
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
                        call_id: "w".into(),
                        name: "write".into(),
                        arguments: r#"{"path":"out.txt","content":"x"}"#.into(),
                    }],
                    ..TurnOutput::default()
                });
            }
            sink(StreamEvent::TextDelta("Could not write.".into()));
            Ok(TurnOutput {
                text: "Could not write.".into(),
                ..TurnOutput::default()
            })
        }
    }

    /// A test's scratch folder, removed when the test ends, pass or fail.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-bg-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Scratch(dir)
    }

    #[test]
    fn a_tests_scratch_folder_is_gone_when_the_test_ends() {
        let dir = temp_dir("gone");
        let path = dir.0.clone();
        std::fs::write(path.join("left.txt"), "x").expect("write");
        assert!(path.join("left.txt").is_file());
        drop(dir);
        assert!(!path.exists(), "{}", path.display());
    }

    #[test]
    fn send_and_bg_say_what_started_and_that_it_cannot_change_files() {
        assert_eq!(
            started_text(true, "count the files"),
            "Sent · count the files. It reads and reports, and can't change files. /bg shows how it went."
        );
        assert_eq!(
            started_text(false, "index the repo"),
            "Background · index the repo. It reads and reports, and can't change files. /bg shows how it went."
        );
    }

    #[test]
    fn a_background_run_cannot_write_and_its_result_is_listed() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear_for_test();
        let (cfg, ws) = (temp_dir("cfg"), temp_dir("ws"));
        let client = Arc::new(WantsToWrite {
            requests: Mutex::new(Vec::new()),
        });
        let title = start(
            cfg.0.clone(),
            client.clone(),
            "grok-4.7".into(),
            ws.0.clone(),
            "write out.txt with x",
        )
        .expect("started");
        assert_eq!(title, "write out.txt with x");
        for _ in 0..200 {
            if !any_live() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!any_live(), "the run ended");
        assert!(!ws.0.join("out.txt").exists(), "the write was refused");
        let requests = client.requests.lock().expect("requests");
        let refused = requests[1]
            .input
            .iter()
            .find_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
                _ => None,
            })
            .expect("tool output");
        assert!(refused.contains("read-only"), "{refused}");
        assert_eq!(status(), "Done · write out.txt with x\nCould not write.");
    }

    #[test]
    fn nothing_running_says_so_and_a_blank_task_is_refused() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear_for_test();
        assert_eq!(status(), "No background tasks. /bg <task> starts one.");
        assert_eq!(stop_text(), "No background tasks running");
        let client = Arc::new(WantsToWrite {
            requests: Mutex::new(Vec::new()),
        });
        let (cfg, ws) = (temp_dir("blank"), temp_dir("w"));
        assert_eq!(
            start(cfg.0.clone(), client, "m".into(), ws.0.clone(), "  ").err(),
            Some("Nothing to run".to_string())
        );
    }

    #[test]
    fn a_fourth_live_run_is_refused_and_stop_cancels_the_three() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear_for_test();
        let tokens: Vec<CancelToken> = (0..3).map(|_| CancelToken::new()).collect();
        for (i, token) in tokens.iter().enumerate() {
            runs().push(Run {
                id: i as u64 + 1,
                title: format!("job {i}"),
                cancel: token.clone(),
                stop_asked: false,
                phase: Phase::Running,
            });
        }
        let client = Arc::new(WantsToWrite {
            requests: Mutex::new(Vec::new()),
        });
        let (cfg, ws) = (temp_dir("max"), temp_dir("w"));
        assert_eq!(
            start(cfg.0.clone(), client, "m".into(), ws.0.clone(), "one more").err(),
            Some("3 background tasks are already running. Stop one first.".to_string())
        );
        assert_eq!(stop_text(), "Stopped 3 background tasks");
        assert!(tokens.iter().all(CancelToken::is_cancelled));
        clear_for_test();
    }
}
