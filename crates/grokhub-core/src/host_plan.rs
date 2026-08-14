use crate::chat::extract_host_cmds;

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

pub fn host_risk(cmd: &str) -> HostRisk {
    let c = cmd.to_ascii_lowercase();
    if c.contains("--force")
        || c.contains("rm -")
        || c.contains("mkfs")
        || c.split_whitespace().next() == Some("dd")
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
            break;
        }
        let rest = t
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')', ':'])
            .trim();
        if rest.is_empty() {
            break;
        }
        let (cmd, _why) = rest
            .split_once(" — ")
            .or_else(|| rest.split_once(" - "))
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
    let cmds = extract_host_cmds(text);
    if cmds.is_empty() {
        None
    } else {
        Some(cmds.into_iter().map(step_from_cmd).collect())
    }
}

pub fn approved_cmds(steps: &[HostPlanStep]) -> Vec<String> {
    steps
        .iter()
        .filter(|s| s.checked)
        .map(|s| s.cmd.clone())
        .collect()
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
    }
}
