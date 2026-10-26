//! Batched read-only tool round (Reason harness R5): consecutive gate-readonly
//! calls in one model reply may execute dispatch in parallel (bounded pool).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::FunctionCall;

use super::show_panel::SHOW_PANEL;
use super::{bind_deferred_session, is_readonly, ToolOutput};

const CONFIG_NAME: &str = "batched_tools.toml";

pub const MAX_PARALLEL: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub enabled: bool,
}

pub fn config_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CONFIG_NAME)
}

pub fn settings(config_dir: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(config_path(config_dir)) else {
        return Settings::default();
    };
    parse_file(&text)
}

fn parse_file(text: &str) -> Settings {
    let mut enabled = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("enabled") && value.trim().eq_ignore_ascii_case("true") {
            enabled = true;
        }
    }
    Settings { enabled }
}

fn session_native(name: &str) -> bool {
    matches!(
        name,
        "todo_write"
            | "ask_user_question"
            | "report_findings"
            | "enter_plan_mode"
            | "exit_plan_mode"
    )
}

/// Whether this call may share a parallel read-only batch (gate readonly, not MCP).
pub fn can_batch_call(name: &str, arguments: &str) -> bool {
    if !is_readonly(name) && name != SHOW_PANEL {
        return false;
    }
    if session_native(name) || crate::mcp::is_mcp_call(name, arguments) {
        return false;
    }
    true
}

/// Length of the maximal consecutive batch-eligible run starting at `start`.
pub fn batch_run_len(calls: &[FunctionCall], start: usize) -> usize {
    let mut n = 0usize;
    for call in calls.iter().skip(start) {
        if can_batch_call(&call.name, &call.arguments) {
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// Run `dispatch` for each call index in `indices` with at most [`MAX_PARALLEL`] workers.
/// `should_stop` is checked before each job starts; stopped jobs get `cancelled_output`.
/// `session` is bound on each worker thread for deferred tool schemas (R4).
pub fn run_parallel<F, G>(
    indices: &[usize],
    session: Option<&str>,
    should_stop: &G,
    dispatch: F,
) -> Vec<ToolOutput>
where
    F: Fn(usize) -> ToolOutput + Sync,
    G: Fn() -> bool + Sync,
{
    if indices.is_empty() {
        return Vec::new();
    }
    let results: Mutex<Vec<Option<ToolOutput>>> = Mutex::new(vec![None; indices.len()]);
    let next = AtomicUsize::new(0);
    let workers = indices.len().min(MAX_PARALLEL);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let slot = next.fetch_add(1, Ordering::SeqCst);
                if slot >= indices.len() {
                    break;
                }
                let out = if should_stop() {
                    cancelled_output()
                } else {
                    if let Some(session) = session {
                        bind_deferred_session(session);
                    }
                    dispatch(indices[slot])
                };
                results.lock().unwrap_or_else(|e| e.into_inner())[slot] = Some(out);
            });
        }
    });
    let collected = {
        let slots = results.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .iter()
            .map(|o| o.as_ref().expect("batch worker left a hole").clone())
            .collect::<Vec<ToolOutput>>()
    };
    collected
}

pub fn cancelled_output() -> ToolOutput {
    ToolOutput::err(crate::gate::user_cancelled("tool"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn call(name: &str, id: &str) -> FunctionCall {
        FunctionCall {
            call_id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
        }
    }

    #[test]
    fn settings_default_off() {
        let dir = std::env::temp_dir().join(format!("gh-batch-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!settings(&dir).enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_enabled_from_toml() {
        let dir = std::env::temp_dir().join(format!("gh-batch-on-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(CONFIG_NAME), "enabled = true\n").unwrap();
        assert!(settings(&dir).enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partition_stops_at_write() {
        let calls = vec![
            call("read_file", "1"),
            call("read_file", "2"),
            call("write", "3"),
            call("read_file", "4"),
        ];
        assert_eq!(batch_run_len(&calls, 0), 2);
        assert_eq!(batch_run_len(&calls, 2), 0);
        assert_eq!(batch_run_len(&calls, 3), 1);
        assert!(!can_batch_call("write", "{}"));
        assert!(can_batch_call("read_file", "{}"));
        assert!(can_batch_call("load_tool_schema", r#"{"name":"write"}"#));
    }

    #[test]
    fn parallel_runs_concurrently_up_to_four() {
        let n = AtomicUsize::new(0);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let barrier = Arc::new(());
        let indices: Vec<usize> = (0..6).collect();
        let outs = run_parallel(&indices, None, &|| false, |_i| {
            n.fetch_add(1, Ordering::SeqCst);
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            let prev_peak = peak.load(Ordering::SeqCst);
            if now > prev_peak {
                peak.store(now, Ordering::SeqCst);
            }
            let _guard = barrier.clone();
            std::thread::sleep(Duration::from_millis(30));
            active.fetch_sub(1, Ordering::SeqCst);
            ToolOutput::ok("ok")
        });
        assert_eq!(outs.len(), 6);
        assert_eq!(n.load(Ordering::SeqCst), 6);
        assert!(peak.load(Ordering::SeqCst) <= MAX_PARALLEL);
        assert!(peak.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn parallel_preserves_order() {
        let indices: Vec<usize> = (0..5).collect();
        let outs = run_parallel(&indices, None, &|| false, |i| {
            ToolOutput::ok(format!("v{i}"))
        });
        assert_eq!(
            outs.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(),
            vec!["v0", "v1", "v2", "v3", "v4"]
        );
    }

    #[test]
    fn parallel_honours_stop_before_start() {
        // The first wave of workers checks the stop flag side by side, so which of
        // jobs 0..MAX_PARALLEL run is up to the scheduler. A worker claims a later job
        // only after its first one ran or was cancelled, so the flag is up by then.
        let started = AtomicUsize::new(0);
        let indices: Vec<usize> = (0..MAX_PARALLEL * 2).collect();
        let outs = run_parallel(
            &indices,
            None,
            &|| started.load(Ordering::SeqCst) >= 1,
            |i| {
                started.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                ToolOutput::ok(format!("ran{i}"))
            },
        );
        assert_eq!(outs.len(), 8);
        let ran = started.load(Ordering::SeqCst);
        assert!((1..=MAX_PARALLEL).contains(&ran), "ran {ran} jobs");
        for (i, o) in outs[..MAX_PARALLEL].iter().enumerate() {
            if !o.failed {
                assert_eq!(o.text, format!("ran{i}"));
            }
        }
        assert_eq!(outs.iter().filter(|o| !o.failed).count(), ran);
        for o in &outs[MAX_PARALLEL..] {
            assert!(o.failed);
            assert!(o.text.contains("cancelled"));
        }
    }

    #[test]
    fn parallel_stop_already_set_runs_nothing() {
        let started = AtomicUsize::new(0);
        let indices: Vec<usize> = (0..4).collect();
        let outs = run_parallel(&indices, None, &|| true, |_| {
            started.fetch_add(1, Ordering::SeqCst);
            ToolOutput::ok("ran")
        });
        assert_eq!(started.load(Ordering::SeqCst), 0);
        assert_eq!(outs.len(), 4);
        for o in &outs {
            assert!(o.failed);
            assert!(o.text.contains("cancelled"));
        }
    }

    #[test]
    fn parallel_load_tool_schema_uses_worker_session_binding() {
        let dir = std::env::temp_dir().join(format!("gh-batch-def-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("deferred_tools.toml"), "enabled = true\n").unwrap();
        let _cfg_guard = crate::perm::ConfigGuard::set(&dir);
        let gate = crate::gate::Gate {
            mode: crate::gate::PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        };
        let cfg = crate::tools::registry::settings(&dir);
        assert!(cfg.enabled);
        crate::tools::registry::begin_turn("chat-a");
        let indices = vec![0usize, 1];
        let outs = run_parallel(&indices, Some("chat-a"), &|| false, |_slot| {
            crate::tools::registry::run_load_tool_schema(
                &serde_json::json!({"name": if _slot == 0 { "web_fetch" } else { "monitor" }}),
            )
        });
        assert_eq!(outs.len(), 2);
        assert!(!outs[0].failed);
        assert!(!outs[1].failed);
        let tools = crate::tools::schemas_for(&gate);
        let names: Vec<_> = tools.iter().filter_map(|s| s["name"].as_str()).collect();
        assert!(names.contains(&"web_fetch"));
        assert!(names.contains(&"monitor"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timing_beats_sequential_for_three_reads() {
        let indices: Vec<usize> = (0..3).collect();
        let delay = Duration::from_millis(25);
        let start = Instant::now();
        let _ = run_parallel(&indices, None, &|| false, |_i| {
            std::thread::sleep(delay);
            ToolOutput::ok("x")
        });
        let parallel_ms = start.elapsed().as_millis();
        let start = Instant::now();
        for _ in 0..3 {
            std::thread::sleep(delay);
        }
        let sequential_ms = start.elapsed().as_millis();
        assert!(parallel_ms < sequential_ms);
    }
}
