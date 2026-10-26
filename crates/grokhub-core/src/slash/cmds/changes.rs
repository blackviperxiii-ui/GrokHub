//! `/skills`, `/connections`, `/automations` change reports and `/workflow`.

use crate::slash::registry::register_slash;
use crate::slash::{Slash, WorkflowVerb};

/// `/skills undo|restore <name>` and `/skills changes`. Anything else stays the catalog.
fn parse_skills_arg(rest: &str) -> Slash {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("").to_ascii_lowercase();
    let name = parts.next().unwrap_or("").trim();
    match head.as_str() {
        "changes" | "history" => Slash::SkillChanges,
        "undo" if name.is_empty() => Slash::SkillChanges,
        "undo" => Slash::SkillUndo(name.to_string()),
        "restore" if !name.is_empty() => Slash::SkillRestore(name.to_string()),
        "restore" => Slash::SkillChanges,
        _ => Slash::GrokSkills,
    }
}

/// `changes`, `history`, `undo`, or nothing after `/connections` or `/automations`.
fn is_changes_arg(rest: &str) -> bool {
    matches!(
        rest.to_ascii_lowercase().as_str(),
        "" | "changes" | "history" | "undo"
    )
}

/// `/workflow pause|resume|stop <target>`, or a launch name when the first word is not a verb.
fn parse_workflow_arg(rest: &str) -> Slash {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("").trim();
    match WorkflowVerb::parse(head) {
        Some(_) if target.is_empty() => Slash::WorkflowUsage,
        Some(verb) => Slash::WorkflowCtl {
            verb,
            target: target.to_string(),
        },
        None => Slash::Workflow(rest.to_string()),
    }
}

register_slash! {
    SKILLS {
        names: ["/skills"],
        effect: Local,
        sample: "/skills undo tidy",
        parse: |rest| Some(parse_skills_arg(rest)),
    }
}

register_slash! {
    CONNECTIONS {
        names: ["/connections"],
        effect: Local,
        sample: "/connections changes",
        parse: |rest| is_changes_arg(rest).then_some(Slash::ConnectionChanges),
    }
}

register_slash! {
    AUTOMATIONS {
        names: ["/automations"],
        effect: Local,
        sample: "/automations changes",
        parse: |rest| is_changes_arg(rest).then_some(Slash::AutomationChanges),
    }
}

register_slash! {
    /// Workflows were a CLI feature; the cabin answers that they are gone.
    WORKFLOW {
        names: ["/workflow"],
        effect: Local,
        sample: "/workflow pause nightly",
        parse: |rest| {
            if rest.is_empty() {
                Some(Slash::GrokWorkflows)
            } else {
                Some(parse_workflow_arg(rest))
            }
        },
    }
}
