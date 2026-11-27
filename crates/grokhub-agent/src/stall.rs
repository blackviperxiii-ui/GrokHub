//! The run's runaway guard, in place of a token cap.
//!
//! A run goes on until the task is done, however many tokens that takes. What
//! stops a loop that gets nowhere is this: model steps in a row where no tool
//! call did new work. [`Did::of`] names what each call that ran did. A change,
//! a check or other work always counts. A read or search counts only when it
//! brings back output the run has not seen, so different searches that keep
//! finding nothing are no work after the first. A wait on a task still running
//! neither counts nor breaks the count, when the step did nothing else and the
//! wait waited [`WAIT_FLOOR_MS`] or more or brought back new output; a quicker
//! read that found nothing new is a look. After [`QUIET_WAITS`] such steps in
//! a row with nothing new, they count like any idle step. A call that failed,
//! was refused, or only touched the checklist is no work. After [`REDIRECT_AFTER`] such steps
//! the model gets one note to change approach; after [`STOP_AFTER`] the run
//! stops and says why. The repeat guard in `run.rs` reads the same [`Did`].

use crate::tools::ToolOutput;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// Steps in a row with no new work before the model is told to change approach.
pub const REDIRECT_AFTER: u32 = 6;
/// Steps in a row with no new work before the run stops.
pub const STOP_AFTER: u32 = 12;

/// The note the model gets at [`REDIRECT_AFTER`].
pub const REDIRECT_NOTE: &str = "Redirect: the last several steps did no new work (every call \
failed, was refused, repeated one already made or found nothing new). Stop trying variations of \
the same thing. Say in one line what is blocking you, then take a different route: the app's own \
updater or installer, a built-in tool instead of the shell, or a smaller step. If only the user \
can unblock it, say exactly what you need and stop.";

/// What the chat shows when the run stops at [`STOP_AFTER`].
pub const STOP_NOTE: &str = "I stopped here: my last steps kept failing, repeating or finding \
nothing new. Say continue with a hint (or what changed) and I'll try a different way.";

/// Wait-only steps in a row that brought nothing new before each counts as
/// idle: with two-minute waits, an hour of a task saying nothing.
pub const QUIET_WAITS: u32 = 30;
/// The shortest wait that counts as one; a quicker read is a snapshot.
pub const WAIT_FLOOR_MS: u64 = 1000;
/// The tool that reads background tasks and helpers.
pub const TASK_READ: &str = "get_command_or_subagent_output";

/// A task's status line while it is still running.
const RUNNING: &str = "status: running";

/// What one call that ran did, for the repeat and stall guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Did {
    /// It failed, or it only touched the checklist or the session.
    Nothing,
    /// It waited on a background command or helper that is still running,
    /// or brought back new output from one.
    Waited,
    /// It read: a file, a search, a page, the screen, a task that ended.
    Looked,
    /// It ran something that is neither a read nor a change: a test or build
    /// run, any other tool.
    Worked,
    /// It changed the project or the screen, or started or merged a helper.
    Changed,
}

impl Did {
    /// What call `name` with `arguments` did, given its `output`.
    pub fn of(name: &str, arguments: &str, output: &ToolOutput) -> Self {
        if output.failed || crate::drive::is_session_tool(name) {
            return Self::Nothing;
        }
        match name {
            "write"
            | "search_replace"
            | crate::tools::control::MERGE_WORK
            | "spawn_subagent"
            | "send_subagent_message" => Self::Changed,
            TASK_READ if waited(arguments, &output.text) => Self::Waited,
            "run_terminal_command" => shell(arguments),
            "screenshot" => Self::Looked,
            n if crate::gate::is_desktop(n) => Self::Changed,
            n if crate::gate::is_readonly(n)
                || crate::gate::is_web_lookup(n)
                || crate::gate::is_look(n) =>
            {
                Self::Looked
            }
            _ => Self::Worked,
        }
    }
}

/// A read of tasks, with `arguments` and output `text`, that found one still
/// running. A joined read heads each task's part with its id.
pub(crate) fn still_running(arguments: &str, text: &str) -> bool {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    match crate::tools::control::task_ids(&args).as_slice() {
        [_] => text.starts_with(RUNNING),
        ids => ids.iter().any(|id| {
            let head = format!("{id}:\n{RUNNING}");
            text.starts_with(&head) || text.contains(&format!("\n\n{head}"))
        }),
    }
}

/// A read of tasks in which one is still running, and that waited
/// [`WAIT_FLOOR_MS`] or more or brought back new output.
fn waited(arguments: &str, text: &str) -> bool {
    let timeout_ms = serde_json::from_str::<serde_json::Value>(arguments)
        .unwrap_or_default()
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    still_running(arguments, text) && (timeout_ms >= WAIT_FLOOR_MS || text.contains("\noutput:\n"))
}

/// A shell command that clearly changes files is a change, a read-only one a
/// look, and any other (a test or build run, an interpreter) work.
fn shell(arguments: &str) -> Did {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let command = args
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if crate::file_checkpoint::changes_files(command) {
        Did::Changed
    } else if crate::perm::shell_is_readonly(command) {
        Did::Looked
    } else {
        Did::Worked
    }
}

/// What the run loop does after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Redirect(&'static str),
    Stop,
}

/// Steps since the run last did new work.
#[derive(Debug, Default)]
pub struct Stall {
    idle: u32,
    redirected: bool,
    /// Every look's output so far, hashed.
    seen: HashSet<u64>,
    /// This step did new work.
    worked: bool,
    /// This step's calls that waited on a task still running.
    waits: usize,
    /// One of them brought back something not seen before.
    news: bool,
    /// Wait-only steps in a row that brought nothing new.
    quiet: u32,
}

impl Stall {
    /// One call of the step ran and did `did`, with `output`.
    pub fn note(&mut self, did: Did, output: &ToolOutput) {
        match did {
            Did::Nothing => {}
            Did::Waited => {
                self.waits += 1;
                self.news |= self.seen.insert(hash(output));
            }
            Did::Looked => self.worked |= self.seen.insert(hash(output)),
            Did::Worked | Did::Changed => self.worked = true,
        }
    }

    /// The step's `calls` all ended, refused ones too.
    pub fn after_step(&mut self, calls: usize) -> Verdict {
        let worked = std::mem::take(&mut self.worked);
        let waits = std::mem::take(&mut self.waits);
        let news = std::mem::take(&mut self.news);
        if worked {
            self.idle = 0;
            self.redirected = false;
            self.quiet = 0;
            return Verdict::Go;
        }
        // A step that only waited leaves the count as it was, until such
        // steps have brought nothing new for long.
        if waits > 0 && waits >= calls {
            self.quiet = if news {
                0
            } else {
                self.quiet.saturating_add(1)
            };
            if self.quiet <= QUIET_WAITS {
                return Verdict::Go;
            }
        }
        self.idle = self.idle.saturating_add(1);
        if self.idle >= STOP_AFTER {
            return Verdict::Stop;
        }
        if self.idle >= REDIRECT_AFTER && !self.redirected {
            self.redirected = true;
            return Verdict::Redirect(REDIRECT_NOTE);
        }
        Verdict::Go
    }

    /// The history was compacted: a look at something seen before the
    /// summary brings it back, so it is new again.
    pub fn forget_seen(&mut self) {
        self.seen.clear();
    }
}

/// A look's text and any image: a screenshot's text never changes, its image does.
fn hash(output: &ToolOutput) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    output.text.hash(&mut h);
    output.image_data_url.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One step whose calls did `dids`, each with output `text`.
    fn step(stall: &mut Stall, dids: &[Did], text: &str) -> Verdict {
        for did in dids {
            stall.note(*did, &ToolOutput::ok(text));
        }
        stall.after_step(dids.len())
    }

    #[test]
    fn work_resets_the_count_and_idle_steps_redirect_then_stop() {
        let mut stall = Stall::default();
        // Steady work never trips anything, however long the run.
        for _ in 0..500 {
            assert_eq!(step(&mut stall, &[Did::Worked], "x"), Verdict::Go);
        }
        // Five idle steps: still going.
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[], ""), Verdict::Go);
        }
        // The sixth gets the one redirect, the next five go on quietly.
        assert_eq!(step(&mut stall, &[], ""), Verdict::Redirect(REDIRECT_NOTE));
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[], ""), Verdict::Go);
        }
        // The twelfth idle step stops the run.
        assert_eq!(step(&mut stall, &[], ""), Verdict::Stop);
    }

    #[test]
    fn new_work_after_a_redirect_earns_a_fresh_redirect() {
        let mut stall = Stall::default();
        for _ in 0..5 {
            step(&mut stall, &[Did::Nothing], "");
        }
        assert_eq!(
            step(&mut stall, &[Did::Nothing], ""),
            Verdict::Redirect(REDIRECT_NOTE)
        );
        assert_eq!(step(&mut stall, &[Did::Worked], "x"), Verdict::Go);
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[Did::Nothing], ""), Verdict::Go);
        }
        assert_eq!(
            step(&mut stall, &[Did::Nothing], ""),
            Verdict::Redirect(REDIRECT_NOTE)
        );
    }

    #[test]
    fn a_look_is_work_only_when_it_brings_back_something_new() {
        let mut stall = Stall::default();
        assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        }
        assert_eq!(
            step(&mut stall, &[Did::Looked], "a"),
            Verdict::Redirect(REDIRECT_NOTE)
        );
        assert_eq!(step(&mut stall, &[Did::Looked], "b"), Verdict::Go);
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[Did::Looked], "b"), Verdict::Go);
        }
        assert_eq!(
            step(&mut stall, &[Did::Looked], "b"),
            Verdict::Redirect(REDIRECT_NOTE)
        );
    }

    #[test]
    fn a_new_image_is_a_new_look() {
        let mut stall = Stall::default();
        let shot = |image: &str| ToolOutput {
            text: "geom".into(),
            image_data_url: Some(image.into()),
            failed: false,
            mcp_app: None,
        };
        stall.note(Did::Looked, &shot("data:image/png;base64,QQ=="));
        assert_eq!(stall.after_step(1), Verdict::Go);
        for _ in 0..5 {
            stall.note(Did::Looked, &shot("data:image/png;base64,QQ=="));
            assert_eq!(stall.after_step(1), Verdict::Go);
        }
        stall.note(Did::Looked, &shot("data:image/png;base64,Qg=="));
        assert_eq!(stall.after_step(1), Verdict::Go, "the screen changed");
    }

    #[test]
    fn a_wait_neither_resets_nor_adds_to_the_count() {
        let mut stall = Stall::default();
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[], ""), Verdict::Go);
        }
        for _ in 0..20 {
            assert_eq!(
                step(&mut stall, &[Did::Waited], "status: running"),
                Verdict::Go
            );
        }
        assert_eq!(step(&mut stall, &[], ""), Verdict::Redirect(REDIRECT_NOTE));
    }

    #[test]
    fn waits_that_bring_nothing_new_count_as_idle_after_a_long_quiet() {
        let mut stall = Stall::default();
        let wait = |stall: &mut Stall, text: &str| {
            stall.note(Did::Waited, &ToolOutput::ok(text));
            stall.after_step(1)
        };
        // The first wait brings something new, then QUIET_WAITS bring nothing.
        for _ in 0..=QUIET_WAITS {
            assert_eq!(wait(&mut stall, "status: running"), Verdict::Go);
        }
        // From there each is idle: the sixth redirects, the twelfth stops.
        for _ in 0..5 {
            assert_eq!(wait(&mut stall, "status: running"), Verdict::Go);
        }
        assert_eq!(
            wait(&mut stall, "status: running"),
            Verdict::Redirect(REDIRECT_NOTE)
        );
        // New output starts the quiet over, but not the idle count.
        assert_eq!(
            wait(&mut stall, "status: running\noutput:\nstep 1"),
            Verdict::Go
        );
        for _ in 0..QUIET_WAITS {
            assert_eq!(wait(&mut stall, "status: running"), Verdict::Go);
        }
        for _ in 0..5 {
            assert_eq!(wait(&mut stall, "status: running"), Verdict::Go);
        }
        assert_eq!(wait(&mut stall, "status: running"), Verdict::Stop);
    }

    #[test]
    fn a_wait_beside_calls_that_did_nothing_new_is_idle() {
        let mut stall = Stall::default();
        assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        // A step is left alone only when all it did was wait.
        for _ in 0..5 {
            assert_eq!(
                step(&mut stall, &[Did::Nothing, Did::Waited], "a"),
                Verdict::Go
            );
        }
        assert_eq!(
            step(&mut stall, &[Did::Looked, Did::Waited], "a"),
            Verdict::Redirect(REDIRECT_NOTE)
        );
        for _ in 0..5 {
            assert_eq!(
                step(&mut stall, &[Did::Nothing, Did::Waited], "a"),
                Verdict::Go
            );
        }
        assert_eq!(
            step(&mut stall, &[Did::Nothing, Did::Waited], "a"),
            Verdict::Stop
        );
    }

    #[test]
    fn a_wait_is_one_only_while_a_task_runs_and_it_waited_or_brought_output() {
        let wait = |args: &str, text: &str| {
            Did::of(
                "get_command_or_subagent_output",
                args,
                &ToolOutput::ok(text),
            )
        };
        let both = "s1:\nstatus: running\n\ns2:\nstatus: running";
        let joined = r#"{"task_id":"s1,s2","timeout_ms":5000}"#;
        assert_eq!(wait(joined, both), Did::Waited);
        assert_eq!(
            wait(
                r#"{"task_id":"s1, s2","timeout_ms":5000}"#,
                "s1:\nstatus: exited\nexit_code: 0\n\ns2:\nstatus: running"
            ),
            Did::Waited
        );
        assert_eq!(
            wait(
                joined,
                "s1:\nstatus: exited\nexit_code: 0\n\ns2:\nstatus: killed"
            ),
            Did::Looked
        );
        assert_eq!(
            wait(r#"{"task_id":"s1","timeout_ms":3000}"#, "status: running"),
            Did::Waited
        );
        // A snapshot that waited no time and found nothing new is a look.
        assert_eq!(wait(r#"{"task_id":"s1"}"#, "status: running"), Did::Looked);
        assert_eq!(
            wait(r#"{"task_id":"s1","timeout_ms":0}"#, "status: running"),
            Did::Looked
        );
        assert_eq!(
            wait(r#"{"task_id":"s1","timeout_ms":1}"#, "status: running"),
            Did::Looked
        );
        assert_eq!(wait(r#"{"task_id":"s1,s2"}"#, both), Did::Looked);
        assert_eq!(
            wait(r#"{"task_id":"s1"}"#, "status: running\noutput:\nbuilding"),
            Did::Waited
        );
    }

    #[test]
    fn a_look_after_a_compaction_is_new_again() {
        let mut stall = Stall::default();
        assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        }
        stall.forget_seen();
        // The sixth idle step would redirect; the re-read is new work.
        assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        for _ in 0..5 {
            assert_eq!(step(&mut stall, &[Did::Looked], "a"), Verdict::Go);
        }
        assert_eq!(
            step(&mut stall, &[Did::Looked], "a"),
            Verdict::Redirect(REDIRECT_NOTE)
        );
    }

    #[test]
    fn a_check_or_a_change_is_work_even_with_the_same_output() {
        for did in [Did::Worked, Did::Changed] {
            let mut stall = Stall::default();
            for _ in 0..30 {
                assert_eq!(step(&mut stall, &[did], "exit 0"), Verdict::Go, "{did:?}");
            }
        }
    }

    #[test]
    fn did_of_names_each_kind() {
        let ok = ToolOutput::ok("ok");
        let shell = |cmd: &str| {
            Did::of(
                "run_terminal_command",
                &serde_json::json!({ "command": cmd }).to_string(),
                &ok,
            )
        };
        let write = r#"{"path":"a.rs","content":"x"}"#;
        assert_eq!(Did::of("write", write, &ok), Did::Changed);
        assert_eq!(
            Did::of("write", write, &ToolOutput::err("denied")),
            Did::Nothing
        );
        assert_eq!(Did::of("todo_write", "{}", &ok), Did::Nothing);
        assert_eq!(Did::of("report_findings", "{}", &ok), Did::Nothing);
        assert_eq!(
            Did::of("merge_subagent_work", r#"{"work":"s1"}"#, &ok),
            Did::Changed
        );
        assert_eq!(Did::of("spawn_subagent", "{}", &ok), Did::Changed);
        let wait = r#"{"task_id":"s1"}"#;
        assert_eq!(
            Did::of(
                "get_command_or_subagent_output",
                wait,
                &ToolOutput::ok("status: running\noutput:\nbuilding")
            ),
            Did::Waited
        );
        assert_eq!(
            Did::of(
                "get_command_or_subagent_output",
                wait,
                &ToolOutput::ok("status: exited\nexit_code: 0")
            ),
            Did::Looked
        );
        assert_eq!(
            Did::of("read_file", r#"{"target_file":"a.rs"}"#, &ok),
            Did::Looked
        );
        assert_eq!(Did::of("web_search", r#"{"query":"x"}"#, &ok), Did::Looked);
        assert_eq!(Did::of("screenshot", "{}", &ok), Did::Looked);
        assert_eq!(Did::of("click", "{}", &ok), Did::Changed);
        assert_eq!(shell("sed -i s/a/b/ a.rs"), Did::Changed);
        assert_eq!(shell("echo v1 > a.txt"), Did::Changed);
        assert_eq!(shell("Set-Content a.txt v1"), Did::Changed);
        assert_eq!(shell("git status"), Did::Looked);
        assert_eq!(shell("cargo test"), Did::Worked);
        assert_eq!(shell("cargo test 2>&1 | tail -5"), Did::Worked);
        assert_eq!(shell("python -m pytest"), Did::Worked);
        assert_eq!(
            Did::of("image_generate", r#"{"prompt":"x"}"#, &ok),
            Did::Worked
        );
    }
}
