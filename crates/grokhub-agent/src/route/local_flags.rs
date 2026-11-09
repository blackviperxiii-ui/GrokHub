//! What the installed `llama-server` accepts, read once per binary from its
//! `--help`, so a launch only passes flags this build knows (an unknown flag
//! stops the engine before it loads), and the run flags the planner picks
//! from it ([`RunFlags`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use grokhub_core::local_profile::Backend;

/// The option names in `--help` text: the dash-led names that start each
/// option's line (`-ub, --ubatch-size N` gives `-ub` and `--ubatch-size`).
pub fn parse_options(help: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in help.lines() {
        for token in line.split_whitespace() {
            if !token.starts_with('-') {
                break;
            }
            let name = token.trim_end_matches(',');
            // A name, not a rule of dashes or a negative number.
            if !name
                .trim_start_matches('-')
                .starts_with(|c: char| c.is_ascii_alphabetic())
            {
                break;
            }
            out.insert(name.to_string());
            if !token.ends_with(',') {
                break;
            }
        }
    }
    out
}

type HelpCache = Mutex<BTreeMap<PathBuf, Arc<str>>>;

fn help_cache() -> &'static HelpCache {
    static C: std::sync::OnceLock<HelpCache> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// `binary --help`, stdout and stderr, read once per binary (empty when it
/// won't run).
pub fn help_text(binary: &Path) -> Arc<str> {
    if let Some(t) = help_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(binary).cloned())
    {
        return t;
    }
    let mut cmd = Command::new(binary);
    cmd.arg("--help").stdin(Stdio::null());
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let text: Arc<str> = cmd
        .output()
        .map(|o| {
            let mut t = String::from_utf8_lossy(&o.stdout).into_owned();
            t.push_str(&String::from_utf8_lossy(&o.stderr));
            t
        })
        .unwrap_or_default()
        .into();
    if let Ok(mut c) = help_cache().lock() {
        c.insert(binary.to_path_buf(), text.clone());
    }
    text
}

/// The options `binary` accepts (see [`parse_options`]).
pub fn engine_options(binary: &Path) -> BTreeSet<String> {
    parse_options(&help_text(binary))
}

/// Flags for how the engine shares the machine, each passed only when the
/// build has it. Empty (the engine's defaults) unless the planner set them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFlags {
    /// `--poll`: 0 sleeps between jobs instead of spinning on a core.
    pub poll: Option<u32>,
    /// `--prio`: -1 runs the engine's threads at low priority.
    pub prio: Option<i32>,
    /// `--ubatch-size`: smaller GPU batches, so each submission is short.
    pub ubatch: Option<u32>,
    /// `GGML_VK_MAX_NODES_PER_SUBMIT` in the child's environment: fewer
    /// graph nodes per Vulkan submission, so a game's frames wait less.
    pub vk_nodes_per_submit: Option<u32>,
    /// Start the child below normal priority (nice 10, `BELOW_NORMAL`).
    pub low_priority: bool,
}

/// Ubatch size while the model shares the GPU under Throttle.
pub const SHARED_UBATCH: u32 = 128;
/// Vulkan graph nodes per submission while it shares the GPU.
pub const SHARED_VK_NODES: u32 = 20;

/// The run flags for a model kept up under Throttle: no spinning, low
/// priority, and (on a GPU) short submissions, from what `options` has.
pub fn shared(options: &BTreeSet<String>, backend: Backend, on_gpu: bool) -> RunFlags {
    let has = |f: &str| options.contains(f);
    RunFlags {
        poll: has("--poll").then_some(0),
        prio: has("--prio").then_some(-1),
        ubatch: (on_gpu && has("--ubatch-size")).then_some(SHARED_UBATCH),
        vk_nodes_per_submit: (on_gpu && backend == Backend::Vulkan).then_some(SHARED_VK_NODES),
        low_priority: true,
    }
}

impl RunFlags {
    /// The command-line half (the env var and priority go on the child).
    pub fn to_args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if let Some(p) = self.poll {
            a.extend(["--poll".to_string(), p.to_string()]);
        }
        if let Some(p) = self.prio {
            a.extend(["--prio".to_string(), p.to_string()]);
        }
        if let Some(u) = self.ubatch {
            a.extend(["--ubatch-size".to_string(), u.to_string()]);
        }
        a
    }

    /// Environment the child starts with.
    pub fn env(&self) -> Vec<(&'static str, String)> {
        self.vk_nodes_per_submit
            .map(|n| ("GGML_VK_MAX_NODES_PER_SUBMIT", n.to_string()))
            .into_iter()
            .collect()
    }
}

/// Nice value a low-priority child runs at on Unix.
pub const LOW_NICE: i32 = 10;

/// Start `cmd`'s child below normal priority on Windows (on Unix, see
/// [`lower_priority`] once it runs). Call after `hide_windows_console`,
/// which this keeps.
pub fn below_normal(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    let _ = cmd;
}

/// Lower a running child to [`LOW_NICE`] on Unix (best effort).
pub fn lower_priority(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: setpriority only reads its arguments; a stale pid fails.
        unsafe {
            libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, LOW_NICE);
        }
    }
    let _ = pid;
}

#[cfg(test)]
#[path = "local_flags_tests.rs"]
mod tests;
