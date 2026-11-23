//! Pre-run checks for scheduled automations: required binaries, named
//! connectors, and model-route readiness. Fail fast before a claim or a
//! model call so a broken job does not burn tokens.

use crate::automation::Automation;
use crate::automation_check::CheckGate;
use crate::recipe::{bin_on_path, default_bin_extra_dirs};

/// Why a job must not fire yet. Shown on the row and in the ran event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightBlock {
    pub reasons: Vec<String>,
}

impl PreflightBlock {
    /// One line for health and the ran event.
    pub fn line(&self) -> String {
        if self.reasons.is_empty() {
            return "Preflight failed".into();
        }
        format!("Preflight: {}", self.reasons.join("; "))
    }
}

/// Live facts the shell (or a test) supplies for one preflight pass.
#[derive(Debug, Clone)]
pub struct PreflightCtx<'a> {
    /// A model route is ready (signed in / key / local).
    pub route_ready: bool,
    /// Why the route is missing; empty when ready.
    pub route_error: &'a str,
    /// Configured connector names that are usable right now.
    pub ready_connectors: &'a [String],
    /// Configured connector names that exist but are not ready
    /// (disabled, needs sign-in, or last check failed).
    pub blocked_connectors: &'a [(String, String)],
    /// All configured connector names (ready or not), for "mentioned but missing".
    pub configured_connectors: &'a [String],
    /// Optional PATH override for tests. `None` reads the process PATH.
    pub path_env: Option<&'a str>,
    /// Home for sidecar bin dirs; empty skips them.
    pub home: &'a str,
}

impl<'a> PreflightCtx<'a> {
    /// A context that only checks the route.
    pub fn route_only(ready: bool, error: &'a str) -> Self {
        Self {
            route_ready: ready,
            route_error: error,
            ready_connectors: &[],
            blocked_connectors: &[],
            configured_connectors: &[],
            path_env: None,
            home: "",
        }
    }
}

/// Stop reason on a ran event that never claimed a model turn.
pub const PREFLIGHT_STOP: &str = "preflight";

/// Whether this job will ask a model (not script-only).
pub fn needs_model_route(gate: &CheckGate, script_only: bool) -> bool {
    !script_only && !gate.no_agent
}

/// First program token of a check command (`git -C …` → `git`).
pub fn check_binary(cmd: &str) -> Option<&str> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return None;
    }
    // Skip leading ENV=value tokens.
    for tok in cmd.split_whitespace() {
        if tok.contains('=') && !tok.starts_with('-') {
            continue;
        }
        let name = tok.trim_matches(|c| c == '"' || c == '\'');
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return None;
        }
        return Some(name);
    }
    None
}

/// Connector-shaped words in `text` that match a known configured name.
pub fn mentioned_connectors<'a>(text: &str, known: &'a [String]) -> Vec<&'a str> {
    if text.trim().is_empty() || known.is_empty() {
        return Vec::new();
    }
    let padded = format!(
        " {} ",
        text.to_ascii_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut out = Vec::new();
    for name in known {
        let needle = name.trim().to_ascii_lowercase();
        if needle.is_empty() {
            continue;
        }
        let pat = format!(" {needle} ");
        if padded.contains(&pat) && !out.contains(&name.as_str()) {
            out.push(name.as_str());
        }
    }
    out
}

/// Built-in names scanned even when no connector is configured yet, so a
/// job that names Linear/GitHub fails closed before a token spend.
pub const WELL_KNOWN_CONNECTORS: &[&str] = &[
    "linear",
    "github",
    "slack",
    "notion",
    "gmail",
    "google-calendar",
    "discord",
    "jira",
    "asana",
    "todoist",
];

fn well_known_mentions(text: &str) -> Vec<&'static str> {
    let padded = format!(
        " {} ",
        text.to_ascii_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    );
    WELL_KNOWN_CONNECTORS
        .iter()
        .copied()
        .filter(|n| padded.contains(&format!(" {n} ")))
        .collect()
}

fn binary_ready(name: &str, ctx: &PreflightCtx<'_>) -> bool {
    let path = ctx
        .path_env
        .map(str::to_string)
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    let extras = default_bin_extra_dirs(ctx.home);
    let extra_refs: Vec<&str> = extras.iter().map(String::as_str).collect();
    if bin_on_path(name, &path, &extra_refs) {
        return true;
    }
    // Windows installs often omit the .exe in the command string.
    #[cfg(windows)]
    {
        let exe = format!("{name}.exe");
        if bin_on_path(&exe, &path, &extra_refs) {
            return true;
        }
    }
    false
}

/// Validate `job` against live facts. `script_only` skips the model-route
/// check (the run is the check output alone).
pub fn preflight(
    job: &Automation,
    script_only: bool,
    ctx: &PreflightCtx<'_>,
) -> Option<PreflightBlock> {
    let mut reasons = Vec::new();

    if needs_model_route(&job.gate, script_only) && !ctx.route_ready {
        let why = ctx.route_error.trim();
        reasons.push(if why.is_empty() {
            "no model route (sign in or add a key)".into()
        } else {
            why.to_string()
        });
    }

    if let Some(bin) = check_binary(&job.check_command) {
        if !binary_ready(bin, ctx) {
            reasons.push(format!("missing binary `{bin}` on PATH"));
        }
    }

    let hay = format!("{} {}", job.instructions, job.check_command);
    // Configured connectors the job names.
    for name in mentioned_connectors(&hay, ctx.configured_connectors) {
        if ctx.ready_connectors.iter().any(|n| n == name) {
            continue;
        }
        if let Some((_, detail)) = ctx.blocked_connectors.iter().find(|(n, _)| n == name) {
            let detail = detail.trim();
            reasons.push(if detail.is_empty() {
                format!("connector `{name}` is not ready")
            } else {
                format!("connector `{name}` is not ready ({detail})")
            });
        } else {
            reasons.push(format!("connector `{name}` is not configured"));
        }
    }
    // Well-known names with no matching configured entry.
    for name in well_known_mentions(&hay) {
        let known = ctx
            .configured_connectors
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name));
        if known {
            continue;
        }
        if reasons
            .iter()
            .any(|r| r.contains(&format!("connector `{name}`")))
        {
            continue;
        }
        reasons.push(format!("connector `{name}` is not configured"));
    }

    if reasons.is_empty() {
        None
    } else {
        Some(PreflightBlock { reasons })
    }
}

/// Advance the schedule and mark health failed without counting a run.
pub fn mark_automation_preflight_failed(
    mut a: Automation,
    why: &str,
    now_ms: u64,
    clock: crate::organs::LocalClock,
) -> Automation {
    a = crate::mark_automation_failed(a, why);
    a.last_run = Some(now_ms);
    a.next_run = Some(crate::compute_next_run(&a, clock).max(now_ms.saturating_add(60_000)));
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_check::CheckGate;
    use crate::organs::LocalClock;

    fn job(check: &str, instructions: &str, no_agent: bool) -> Automation {
        Automation {
            id: "auto-1".into(),
            name: "test".into(),
            schedule: "daily".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: instructions.into(),
            heartbeat_every_min: 0,
            check_command: check.into(),
            gate: CheckGate {
                no_agent,
                ..CheckGate::default()
            },
            wait: Default::default(),
            enabled: true,
            last_run: None,
            next_run: Some(1),
            run_count: 0,
            health: Default::default(),
            lifecycle: Default::default(),
        }
    }

    #[test]
    fn check_binary_skips_env_and_takes_the_program() {
        assert_eq!(check_binary("git status"), Some("git"));
        assert_eq!(check_binary("PATH=/bin git -C /tmp status"), Some("git"));
        assert_eq!(check_binary(""), None);
        assert_eq!(check_binary("/usr/bin/git status"), None);
    }

    #[test]
    fn missing_route_blocks_a_model_job() {
        let a = job("", "every day at 9 summarize the board", false);
        let block = preflight(
            &a,
            false,
            &PreflightCtx::route_only(false, "Sign in with Grok or add an API key."),
        )
        .expect("blocked");
        assert!(block.line().contains("Sign in with Grok"));
        assert!(preflight(&a, true, &PreflightCtx::route_only(false, "x")).is_none());
        let script = job("", "every day at 9", true);
        assert!(preflight(&script, false, &PreflightCtx::route_only(false, "x")).is_none());
    }

    #[test]
    fn missing_binary_blocks_before_a_run() {
        let a = job("definitely-not-a-real-bin-961 --help", "check disk", true);
        let ctx = PreflightCtx {
            route_ready: true,
            route_error: "",
            ready_connectors: &[],
            blocked_connectors: &[],
            configured_connectors: &[],
            path_env: Some("/usr/bin:/bin"),
            home: "",
        };
        let block = preflight(&a, true, &ctx).expect("blocked");
        assert!(
            block.line().contains("definitely-not-a-real-bin-961"),
            "{}",
            block.line()
        );
    }

    #[test]
    fn missing_or_blocked_connector_fails_closed() {
        let a = job(
            "",
            "every day at 9 sync Linear issues into the board",
            false,
        );
        let configured = vec!["linear".into(), "github".into()];
        let blocked = vec![("linear".into(), "needs sign-in".into())];
        let ctx = PreflightCtx {
            route_ready: true,
            route_error: "",
            ready_connectors: &[],
            blocked_connectors: &blocked,
            configured_connectors: &configured,
            path_env: Some("/usr/bin"),
            home: "",
        };
        let block = preflight(&a, false, &ctx).expect("blocked");
        assert!(block.line().contains("linear"), "{}", block.line());
        assert!(block.line().contains("sign-in"), "{}", block.line());

        let a2 = job("", "every day at 9 pull Notion pages", false);
        let empty = PreflightCtx {
            route_ready: true,
            route_error: "",
            ready_connectors: &[],
            blocked_connectors: &[],
            configured_connectors: &[],
            path_env: Some("/usr/bin"),
            home: "",
        };
        let block2 = preflight(&a2, false, &empty).expect("blocked");
        assert!(block2.line().contains("notion"), "{}", block2.line());
    }

    #[test]
    fn preflight_failed_marks_health_without_a_run_count() {
        let clock = LocalClock {
            now_ms: 1_000,
            weekday: 5,
            hour: 9,
            minute: 0,
        };
        let a = job("", "every day at 9", false);
        let failed = mark_automation_preflight_failed(a, "Preflight: no model route", 1_000, clock);
        assert_eq!(failed.run_count, 0);
        assert!(failed.health.failing());
        assert!(failed.health.error.contains("no model route"));
        assert!(failed.next_run.unwrap() > 1_000);
    }
}
