//! `message_agent`: hand work to one of the person's agent profiles
//! ([`grokhub_core::agent_team`]). The agent runs as a helper pinned to its
//! own home, so its chats, memory, skills, connections and permission rules
//! are its own, and it sees only the message. Isolation model:
//! docs/design/agent-teams/README.md.

use std::path::{Path, PathBuf};

use grokhub_core::agent_tasks::Origin;
use grokhub_core::agent_team::{self, Access, Profile, Sender, TeamFile};
use serde_json::{json, Value};

use crate::gate::{Gate, PermMode};
use crate::perm::{Action, ConfigGuard, Policy};

pub const TOOL: &str = "message_agent";

/// Longest message one agent hands another, in characters.
const MAX_MESSAGE: usize = 8_000;

/// Who is sending: the app's config folder (where the team file is) and,
/// inside an agent's turn, that agent's id.
struct From {
    root: PathBuf,
    me: Option<String>,
}

impl From {
    fn here() -> From {
        let config = crate::perm::config_dir();
        match agent_team::whose_home(&config) {
            Some((root, me)) => From { root, me: Some(me) },
            None => From {
                root: config,
                me: None,
            },
        }
    }

    fn sender(&self) -> Sender<'_> {
        match &self.me {
            Some(me) => Sender::Agent(me),
            None => Sender::Person,
        }
    }

    /// The agents already on this chain: the sender, inside an agent's turn.
    fn chain(&self) -> Vec<String> {
        self.me.iter().cloned().collect()
    }
}

/// The tool, while the sender may message at least one agent. No team file
/// (or no agent it may reach) offers no tool, so the tool list is unchanged.
pub fn schema() -> Option<Value> {
    let from = From::here();
    let file = TeamFile::load(&from.root).ok()?;
    let chain = from.chain();
    let reach: Vec<&Profile> = file
        .agents
        .iter()
        .filter(|p| file.may_message(from.sender(), &p.id, &chain).is_ok())
        .collect();
    if reach.is_empty() {
        return None;
    }
    let mut description = String::from(
        "Hand work to one of the person's agents. It works with its own chats, memory and permissions, sees only your message, and answers once. Agents:",
    );
    for p in &reach {
        description.push_str(&format!("\n- {}: {} ({})", p.id, p.name, p.kind.label()));
        if let Some(line) = p.about.lines().map(str::trim).find(|l| !l.is_empty()) {
            description.push_str(&format!(". {line}"));
        }
    }
    let ids: Vec<&str> = reach.iter().map(|p| p.id.as_str()).collect();
    Some(json!({
        "type": "function",
        "name": TOOL,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": {
                "agent": {"type": "string", "enum": ids},
                "message": {"type": "string"}
            },
            "required": ["agent", "message"],
            "additionalProperties": false
        }
    }))
}

/// One agent's turn, ready to run as a helper.
pub(crate) struct Brief {
    pub profile: Profile,
    /// Its config folder, pinned for the whole turn.
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub gate: Gate,
    pub policy: Policy,
    pub system: String,
    pub message: String,
    /// The app's config folder, where the task log is.
    pub root: PathBuf,
    pub from: Origin,
}

/// Checks that the sender may message the agent and builds its turn.
/// `caller` is the sender's gate and `policy` its rules; neither is ever
/// widened.
pub(crate) fn brief(
    arguments: &str,
    caller: &Gate,
    policy: Option<&Policy>,
) -> Result<Brief, String> {
    let value: Value =
        serde_json::from_str(arguments).map_err(|err| format!("arguments are not JSON: {err}"))?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    let (id, message) = (text("agent"), text("message"));
    if id.is_empty() || message.is_empty() {
        return Err("agent and message are required".into());
    }
    if message.chars().count() > MAX_MESSAGE {
        return Err(format!(
            "a message to an agent is at most {MAX_MESSAGE} characters; send the gist, not the whole file"
        ));
    }
    let from = From::here();
    let file = TeamFile::load(&from.root)?;
    file.may_message(from.sender(), &id, &from.chain())
        .map_err(|refusal| refusal.to_string())?;
    let profile = file
        .agent(&id)
        .cloned()
        .ok_or_else(|| format!("`{id}` isn't an agent"))?;
    let (Some(home), Some(workspace)) = (
        agent_team::home(&from.root, &id),
        agent_team::workspace(&from.root, &id),
    ) else {
        return Err(format!("`{id}` isn't an agent id"));
    };
    std::fs::create_dir_all(&workspace).map_err(|err| format!("{id}'s folder: {err}"))?;
    let gate = agent_gate(caller, &profile);
    let policy = agent_policy(policy, &home, &workspace);
    let system = {
        let _home = ConfigGuard::set(&home);
        let rules = grokhub_core::cabin_rules::cabin_rules_for("", gate.desktop);
        let mut system = crate::prompt::system_prompt(&rules, &workspace);
        if let Some(steps) = playbook_steps(&profile, &message, &from.root) {
            system.push_str("\n\n");
            system.push_str(&steps);
        }
        system.push_str("\n\n");
        system.push_str(&persona(&profile));
        system
    };
    Ok(Brief {
        profile,
        home,
        workspace,
        gate,
        policy,
        system,
        message,
        from: from.me.map_or(Origin::Person, Origin::Agent),
        root: from.root,
    })
}

/// The access a caller's gate stands for.
fn caller_access(gate: &Gate) -> Access {
    match (gate.readonly_session, gate.mode) {
        (true, _) => Access::ReadOnly,
        (false, PermMode::Ask) => Access::Ask,
        (false, PermMode::Auto | PermMode::Always) => Access::Auto,
    }
}

/// The agent's gate: its own access, never wider than the caller's (and
/// never `Always`), and the desktop only when both allow it.
fn agent_gate(caller: &Gate, profile: &Profile) -> Gate {
    let access = profile.access_under(caller_access(caller));
    Gate {
        mode: match access {
            Access::Auto => PermMode::Auto,
            Access::Ask | Access::ReadOnly => PermMode::Ask,
        },
        readonly_session: access == Access::ReadOnly,
        attended: caller.attended,
        desktop: profile.desktop_under(caller.desktop),
    }
}

/// The agent's own rules and grants, after the caller's deny and ask rules:
/// those only tighten, so they hold for the agent too. The caller's allow
/// rules and grants stay the caller's.
fn agent_policy(caller: Option<&Policy>, home: &Path, workspace: &Path) -> Policy {
    let own = {
        let _home = ConfigGuard::set(home);
        Policy::load(workspace)
    };
    let mut rules: Vec<_> = caller
        .map(|p| {
            p.rules
                .iter()
                .filter(|r| r.action != Action::Allow)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    rules.extend(own.rules);
    Policy {
        rules,
        grants: own.grants,
    }
}

/// The steps of the playbook that fits the message, from the app's
/// playbooks and only those the profile lists (when it lists any). The ask
/// is sorted without a model; an unclear one gets no playbook, as in a chat.
fn playbook_steps(profile: &Profile, message: &str, root: &Path) -> Option<String> {
    let brief = crate::brain::classify(
        &crate::brain::BriefInput {
            text: message,
            ..Default::default()
        },
        None,
    );
    if brief.by == "default" {
        return None;
    }
    let (mut books, _) = crate::brain::playbook::load(root);
    if let Some(allowed) = &profile.playbooks {
        books.retain(|book| allowed.contains(&book.playbook.id));
    }
    let os = crate::brain::playbook::host_os();
    crate::brain::playbook::pick(
        &books,
        brief.intent.as_str(),
        &brief.domain,
        os,
        &brief.mood,
    )
    .map(|book| book.for_os(os).prompt_block())
}

fn persona(profile: &Profile) -> String {
    let mut text = format!(
        "You are {}, one of the person's agents ({} agent). You work with your own chats, memory and connections. A message from the person or another agent follows; do what it asks within your permissions and answer with what the sender needs.",
        profile.name.trim(),
        profile.kind.label()
    );
    let about = profile.about.trim();
    if !about.is_empty() {
        text.push_str("\n<about_you>\n");
        text.push_str(about);
        text.push_str("\n</about_you>");
    }
    text
}

#[cfg(test)]
#[path = "agent_message_tests.rs"]
mod tests;
