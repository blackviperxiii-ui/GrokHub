use crate::recipe::{computer_cmd_line, parse_computer_op};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRisk {
    Safe,
    Moderate,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPlanStep {
    pub cmd: String,
    pub risk: HostRisk,
    pub explain: String,
    pub checked: bool,
}

fn host_words(cmd: &str) -> impl Iterator<Item = &str> {
    cmd.split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '`' | '(' | ')'))
        .filter(|t| !t.is_empty())
}

pub fn host_risk(cmd: &str) -> HostRisk {
    let c = cmd.to_ascii_lowercase();
    if c.contains("--force")
        || host_words(&c).any(|t| t == "rm" || t == "dd")
        || c.contains("mkfs")
        || c.contains(" sudo ")
        || c.starts_with("sudo ")
    {
        HostRisk::Destructive
    } else if c.contains("git push")
        || c.contains('>')
        || c.contains("curl ")
        || c.contains("wget ")
        || c.contains("chmod ")
        || c.contains("systemctl")
    {
        HostRisk::Moderate
    } else {
        HostRisk::Safe
    }
}

pub fn explain_host_risk(cmd: &str, risk: HostRisk) -> String {
    match risk {
        HostRisk::Destructive if cmd.to_ascii_lowercase().contains("force") => {
            "force can rewrite history or destroy remotes".into()
        }
        HostRisk::Destructive => "destructive — can destroy data".into(),
        HostRisk::Moderate => "writes or leaves this box".into(),
        HostRisk::Safe => "read-only".into(),
    }
}

pub fn step_from_cmd(cmd: impl Into<String>) -> HostPlanStep {
    let cmd = cmd.into();
    let risk = host_risk(&cmd);
    let explain = explain_host_risk(&cmd, risk);
    HostPlanStep {
        cmd,
        risk,
        explain,
        checked: true,
    }
}

/// Parse a `HOST_PLAN:` block of numbered steps: `1. ls ~/proj — list files`
pub fn parse_host_plan(text: &str) -> Option<Vec<HostPlanStep>> {
    let mut steps = Vec::new();
    let mut in_plan = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "HOST_PLAN:" || t == "HOST_PLAN" {
            in_plan = true;
            continue;
        }
        if !in_plan {
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if !t.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let rest = t
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')', ':'])
            .trim();
        if rest.is_empty() {
            continue;
        }
        let (cmd, _why) = rest
            .split_once(" — ")
            .or_else(|| rest.split_once(" —"))
            .unwrap_or((rest, ""));
        let cmd = cmd.trim();
        if cmd.is_empty() {
            continue;
        }
        steps.push(step_from_cmd(cmd));
    }
    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

pub fn plan_from_text(text: &str) -> Option<Vec<HostPlanStep>> {
    if let Some(p) = parse_host_plan(text) {
        return Some(p);
    }
    let mut steps = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(cmd) = strip_host_cmd_line(t) {
            steps.push(step_from_cmd(cmd));
            continue;
        }
        if let Some(op) = parse_computer_op(t) {
            steps.push(HostPlanStep {
                cmd: computer_cmd_line(&op),
                risk: HostRisk::Moderate,
                explain: "desktop hands — mouse/keyboard".into(),
                checked: true,
            });
        }
    }
    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

/// `HOST_CMD:` / `HOST_CMD ` only — not `HOST_CMDLINE`.
pub fn strip_host_cmd_line(line: &str) -> Option<&str> {
    let t = line.trim();
    let rest = if let Some(r) = t.strip_prefix("HOST_CMD:") {
        r
    } else {
        let r = t.strip_prefix("HOST_CMD")?;
        if r.is_empty() || r.starts_with(':') || r.starts_with(char::is_whitespace) {
            r
        } else {
            return None;
        }
    };
    let cmd = rest.trim().trim_start_matches(':').trim();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd)
    }
}

/// Keep a YOLO-held plan when HostDone is for a different auto-run batch.
pub fn retain_held_plan(
    pending: Option<Vec<HostPlanStep>>,
    ran: &[String],
) -> Option<Vec<HostPlanStep>> {
    let pending = pending?;
    if pending.is_empty() {
        return None;
    }
    let ran_pending = pending.iter().any(|s| ran.iter().any(|c| c == &s.cmd));
    if ran_pending {
        None
    } else {
        Some(pending)
    }
}

pub fn approved_cmds(steps: &[HostPlanStep]) -> Vec<String> {
    steps
        .iter()
        .filter(|s| s.checked)
        .map(|s| s.cmd.clone())
        .collect()
}

/// YOLO still holds destructive steps when `/approve risky` is on,
/// and holds anything that leaves the bound project.
pub fn yolo_plan_split(
    plan: &[HostPlanStep],
    risky_only: bool,
    project_root: &str,
) -> (Vec<String>, Vec<HostPlanStep>) {
    yolo_plan_split_in(plan, risky_only, project_root, std::env::var("HOME").ok().as_deref())
}

pub fn yolo_plan_split_in(
    plan: &[HostPlanStep],
    risky_only: bool,
    project_root: &str,
    home: Option<&str>,
) -> (Vec<String>, Vec<HostPlanStep>) {
    let mut run = Vec::new();
    let mut hold = Vec::new();
    for s in plan {
        let outside = crate::project::host_cmd_leaves_project_in(&s.cmd, project_root, home);
        if (risky_only && s.risk == HostRisk::Destructive) || outside {
            hold.push(s.clone());
        } else if s.checked {
            run.push(s.cmd.clone());
        }
    }
    (run, hold)
}

pub fn move_step(steps: &mut [HostPlanStep], idx: usize, up: bool) {
    if up {
        if idx == 0 || idx >= steps.len() {
            return;
        }
        steps.swap(idx, idx - 1);
    } else if idx + 1 < steps.len() {
        steps.swap(idx, idx + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_checklist_and_risk() {
        let text = "HOST_PLAN:\n1. ls ~/proj — list files\n2. git status — see dirty tree\n";
        let steps = parse_host_plan(text).unwrap();
        assert_eq!(steps.len(), 2);
        let spaced = parse_host_plan("HOST_PLAN:\n1. ls\n\n2. git status\n").unwrap();
        assert_eq!(spaced.len(), 2, "blank lines must not end the plan");
        assert_eq!(steps[0].cmd, "ls ~/proj");
        assert_eq!(steps[0].risk, HostRisk::Safe);
        assert!(explain_host_risk("git push --force", HostRisk::Destructive).contains("force"));
        assert_eq!(host_risk("git push --force"), HostRisk::Destructive);
        let mut steps = plan_from_text("HOST_CMD: echo a\nHOST_CMD: echo b\n").unwrap();
        assert_eq!(steps.len(), 2);
        steps[1].checked = false;
        assert_eq!(approved_cmds(&steps), vec!["echo a"]);
        move_step(&mut steps, 1, true);
        assert_eq!(steps[0].cmd, "echo b");
        let mixed = plan_from_text(
            "HOST_CMD: echo hi\nCOMPUTER_CMD: click 10 20\nCOMPUTER_CMD: type hello\n",
        )
        .unwrap();
        assert_eq!(mixed.len(), 3);
        assert_eq!(mixed[0].cmd, "echo hi");
        assert_eq!(mixed[1].cmd, "COMPUTER_CMD: click 10 20");
        assert_eq!(mixed[2].cmd, "COMPUTER_CMD: type hello");
        assert_eq!(mixed[1].risk, HostRisk::Moderate);
        let risky = plan_from_text("HOST_PLAN:\n1. ls\n2. rm -rf /tmp/x\n").unwrap();
        let (run, hold) = yolo_plan_split_in(&risky, true, "", None);
        assert_eq!(run, vec!["ls".to_string()]);
        assert_eq!(hold.len(), 1);
        assert_eq!(hold[0].cmd, "rm -rf /tmp/x");
    }

    #[test]
    fn rm_without_flags_is_destructive() {
        assert_eq!(host_risk("rm foo.txt"), HostRisk::Destructive);
        assert_eq!(host_risk("rm\tfoo.txt"), HostRisk::Destructive);
        assert_eq!(host_risk("ls foo.txt"), HostRisk::Safe);
    }

    #[test]
    fn host_cmd_prefix_is_not_a_substring() {
        assert!(plan_from_text("HOST_CMDLINE: backup the repo\n").is_none());
        assert_eq!(
            plan_from_text("HOST_CMD echo ok\n").unwrap()[0].cmd,
            "echo ok"
        );
    }

    #[test]
    fn host_plan_keeps_later_steps_and_real_hyphens() {
        let empty_num = parse_host_plan("HOST_PLAN:\n1. ls\n2.\n3. git status\n").unwrap();
        assert_eq!(
            empty_num.iter().map(|s| s.cmd.as_str()).collect::<Vec<_>>(),
            vec!["ls", "git status"]
        );
        let prose = parse_host_plan("HOST_PLAN:\n1. ls\nThis explains step 1\n2. git status\n").unwrap();
        assert_eq!(
            prose.iter().map(|s| s.cmd.as_str()).collect::<Vec<_>>(),
            vec!["ls", "git status"]
        );
        let hyphen = parse_host_plan("HOST_PLAN:\n1. cp a - b\n").unwrap();
        assert_eq!(hyphen[0].cmd, "cp a - b");
    }

    #[test]
    fn yolo_hold_survives_unrelated_host_done() {
        let hold = vec![step_from_cmd("rm foo.txt")];
        let kept = retain_held_plan(Some(hold.clone()), &["ls".into()]);
        assert_eq!(kept.as_ref().map(|p| p[0].cmd.as_str()), Some("rm foo.txt"));
        assert!(retain_held_plan(Some(hold), &["rm foo.txt".into()]).is_none());
    }
}
