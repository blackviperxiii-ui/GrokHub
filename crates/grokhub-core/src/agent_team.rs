//! Agent teams: profiles that each run as their own agent, in their own home
//! folder, and the rules for which agent may message which.
//! See `docs/design/agent-teams/README.md`.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Profiles and teams, in the app's config folder.
pub const TEAM_FILE: &str = "agent-team.json";
/// Each agent's own config folder sits under `{config}/agents/<id>/`.
pub const HOMES_DIR: &str = "agents";
pub const VERSION: u32 = 1;
/// A chain of messages reaches at most this many agents.
pub const MAX_HOPS: usize = 3;
pub const MAX_AGENTS: usize = 50;
const MAX_ID: usize = 40;
const MAX_NAME: usize = 60;
const MAX_ABOUT: usize = 4000;
const MAX_TOOL: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A job, such as researcher or reviewer.
    Role,
    Family,
    Staff,
    /// Answers people outside the household or business. Read only, never
    /// starts a message.
    Customer,
}

impl Kind {
    pub fn default_access(self) -> Access {
        match self {
            Kind::Customer => Access::ReadOnly,
            Kind::Role | Kind::Family | Kind::Staff => Access::Ask,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Role => "role",
            Kind::Family => "family",
            Kind::Staff => "staff",
            Kind::Customer => "customer",
        }
    }
}

/// What an agent may do with nobody there to answer, narrowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// No writes and no shell.
    ReadOnly,
    /// Anything that would need a person is refused.
    Ask,
    /// The app's judge decides, inside the hard floor and the sandbox.
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// Who it is and how it helps; its persona in every turn.
    #[serde(default)]
    pub about: String,
    /// Its team's id, or empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub team: String,
    pub access: Access,
    #[serde(default)]
    pub desktop: bool,
    /// Tool names it may use. Empty keeps its access level's usual set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Playbook ids it may use. `None` keeps the usual pick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playbooks: Option<Vec<String>>,
    /// A `model_profiles.toml` tier the router picks within, or empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model_tier: String,
    /// Agents it may start a message to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub talks_to: Vec<String>,
    /// Offer its agent card to paired peers.
    #[serde(default)]
    pub share: bool,
}

impl Profile {
    /// A profile with its kind's defaults.
    pub fn new(kind: Kind, id: &str, name: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            about: String::new(),
            team: String::new(),
            access: kind.default_access(),
            desktop: false,
            tools: Vec::new(),
            playbooks: None,
            model_tier: String::new(),
            talks_to: Vec::new(),
            share: false,
        }
    }

    /// The access a turn runs with: never wider than the caller's, and read
    /// only for a customer whatever the file says.
    pub fn access_under(&self, caller: Access) -> Access {
        let own = match self.kind {
            Kind::Customer => Access::ReadOnly,
            _ => self.access,
        };
        own.min(caller)
    }

    /// Desktop control for a turn: only when both this agent and the caller have it.
    pub fn desktop_under(&self, caller: bool) -> bool {
        self.kind != Kind::Customer && self.desktop && caller
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    /// The id of the agent that leads it; that agent's `team` is this team.
    pub lead: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamFile {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub agents: Vec<Profile>,
    #[serde(default)]
    pub teams: Vec<Team>,
}

fn version() -> u32 {
    VERSION
}

impl Default for TeamFile {
    fn default() -> Self {
        TeamFile {
            version: VERSION,
            agents: Vec::new(),
            teams: Vec::new(),
        }
    }
}

/// Who is sending a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender<'a> {
    /// The person, in the main chat or the app.
    Person,
    Agent(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    UnknownAgent(String),
    /// A customer agent only answers.
    CustomerStarts(String),
    NotAllowed {
        from: String,
        to: String,
    },
    TooManyHops,
    /// The agent is already working on this chain.
    Loop(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::UnknownAgent(id) => write!(f, "There's no agent `{id}`."),
            Refusal::CustomerStarts(id) => {
                write!(f, "`{id}` is a customer agent: it answers, but can't message other agents.")
            }
            Refusal::NotAllowed { from, to } => write!(
                f,
                "`{from}` can't message `{to}`: add `{to}` to its talks_to, or put both on a team with one of them as lead."
            ),
            Refusal::TooManyHops => write!(
                f,
                "This chain of messages already reached {MAX_HOPS} agents. Answer with what you have."
            ),
            Refusal::Loop(id) => write!(f, "`{id}` is already working on this chain."),
        }
    }
}

/// `{config}/agents/<id>`: the agent's own config folder. `None` for an id
/// that isn't a valid agent id, so no id can name a path outside it.
pub fn home(config: &Path, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| config.join(HOMES_DIR).join(id))
}

/// The app's config folder and the agent's id when `config` is an agent's
/// home (`{root}/agents/<id>`), else `None`. A turn pinned to a home sends
/// as that agent.
pub fn whose_home(config: &Path) -> Option<(PathBuf, String)> {
    let id = config.file_name()?.to_str()?;
    let agents = config.parent()?;
    if !valid_id(id) || agents.file_name()? != HOMES_DIR {
        return None;
    }
    Some((agents.parent()?.to_path_buf(), id.to_string()))
}

/// Inside an agent's turn: the app's config folder and the agent's profile,
/// when `config` is the home of an agent the team file lists. A folder that
/// only looks like a home (`…/agents/<id>` with no such agent) is not one.
pub fn agent_turn(config: &Path) -> Option<(PathBuf, Profile)> {
    let (root, id) = whose_home(config)?;
    let profile = TeamFile::load(&root).ok()?.agent(&id)?.clone();
    Some((root, profile))
}

/// `{home}/work`: the folder its tools work in.
pub fn workspace(config: &Path, id: &str) -> Option<PathBuf> {
    home(config, id).map(|h| h.join("work"))
}

/// Lowercase letters, digits and inner dashes, 1 to 40 long.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_tool(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_TOOL
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl TeamFile {
    pub fn path(config: &Path) -> PathBuf {
        config.join(TEAM_FILE)
    }

    /// A missing file is an empty team. A file that doesn't parse is an error,
    /// so a save never writes over it.
    pub fn load(config: &Path) -> Result<TeamFile, String> {
        let path = Self::path(config);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("{} doesn't read as an agent team: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TeamFile::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Checked first, then written readable by this user only.
    pub fn save(&self, config: &Path) -> Result<(), String> {
        self.check()?;
        let path = Self::path(config);
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(config).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        crate::state::write_private_synced(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })?;
        #[cfg(windows)]
        crate::win_acl::restrict_user_only(&path);
        Ok(())
    }

    pub fn agent(&self, id: &str) -> Option<&Profile> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn team(&self, id: &str) -> Option<&Team> {
        self.teams.iter().find(|t| t.id == id)
    }

    /// Every rule a saved file keeps. The first broken one is the error.
    pub fn check(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err(format!(
                "This agent team file is version {}; this app reads version {VERSION}.",
                self.version
            ));
        }
        if self.agents.len() > MAX_AGENTS {
            return Err(format!("A team file holds at most {MAX_AGENTS} agents."));
        }
        let mut team_ids = BTreeSet::new();
        for team in &self.teams {
            if !valid_id(&team.id) {
                return Err(format!(
                    "Team id `{}` must be lowercase letters, digits and dashes.",
                    team.id
                ));
            }
            if !team_ids.insert(team.id.as_str()) {
                return Err(format!("Two teams use the id `{}`.", team.id));
            }
            check_name(&team.name, &team.id)?;
        }
        let mut ids = BTreeSet::new();
        for agent in &self.agents {
            if !valid_id(&agent.id) {
                return Err(format!(
                    "Agent id `{}` must be lowercase letters, digits and dashes.",
                    agent.id
                ));
            }
            if !ids.insert(agent.id.as_str()) {
                return Err(format!("Two agents use the id `{}`.", agent.id));
            }
        }
        let tiers = known_tiers();
        for agent in &self.agents {
            check_agent(agent, &ids, &team_ids, &tiers)?;
        }
        for team in &self.teams {
            match self.agent(&team.lead) {
                Some(lead) if lead.team == team.id => {}
                Some(_) => {
                    return Err(format!(
                        "Team `{}`'s lead `{}` must be on that team.",
                        team.id, team.lead
                    ))
                }
                None => {
                    return Err(format!(
                        "Team `{}`'s lead `{}` isn't an agent.",
                        team.id, team.lead
                    ))
                }
            }
        }
        Ok(())
    }

    /// May `from` send a message to `to`? `chain` names the agents already
    /// working on this chain, first asked first; a person's own message has an
    /// empty chain.
    pub fn may_message(&self, from: Sender<'_>, to: &str, chain: &[String]) -> Result<(), Refusal> {
        let target = self
            .agent(to)
            .ok_or_else(|| Refusal::UnknownAgent(to.to_string()))?;
        if let Sender::Agent(id) = from {
            let sender = self
                .agent(id)
                .ok_or_else(|| Refusal::UnknownAgent(id.to_string()))?;
            if sender.kind == Kind::Customer {
                return Err(Refusal::CustomerStarts(id.to_string()));
            }
            if id == to {
                return Err(Refusal::Loop(to.to_string()));
            }
            let listed = sender.talks_to.iter().any(|t| t == to);
            let teammates = !sender.team.is_empty()
                && sender.team == target.team
                && self
                    .team(&sender.team)
                    .is_some_and(|t| t.lead == id || t.lead == to);
            if !listed && !teammates {
                return Err(Refusal::NotAllowed {
                    from: id.to_string(),
                    to: to.to_string(),
                });
            }
        }
        if chain.iter().any(|c| c == to) {
            return Err(Refusal::Loop(to.to_string()));
        }
        if chain.len() >= MAX_HOPS {
            return Err(Refusal::TooManyHops);
        }
        Ok(())
    }
}

fn check_name(name: &str, id: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("`{id}` needs a name."));
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!(
            "`{id}`'s name is longer than {MAX_NAME} characters."
        ));
    }
    Ok(())
}

fn check_agent(
    agent: &Profile,
    ids: &BTreeSet<&str>,
    teams: &BTreeSet<&str>,
    tiers: &BTreeSet<String>,
) -> Result<(), String> {
    let id = agent.id.as_str();
    check_name(&agent.name, id)?;
    if agent.about.chars().count() > MAX_ABOUT {
        return Err(format!(
            "`{id}`'s about is longer than {MAX_ABOUT} characters."
        ));
    }
    if !agent.team.is_empty() && !teams.contains(agent.team.as_str()) {
        return Err(format!(
            "`{id}` is on team `{}`, which doesn't exist.",
            agent.team
        ));
    }
    if agent.kind == Kind::Customer {
        if agent.access != Access::ReadOnly {
            return Err(format!(
                "`{id}` is a customer agent, so its access stays read only."
            ));
        }
        if agent.desktop {
            return Err(format!(
                "`{id}` is a customer agent, so it can't use the desktop."
            ));
        }
        if !agent.talks_to.is_empty() {
            return Err(format!(
                "`{id}` is a customer agent: it answers, but can't message other agents."
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for to in &agent.talks_to {
        if to == id {
            return Err(format!("`{id}` lists itself in talks_to."));
        }
        if !ids.contains(to.as_str()) {
            return Err(format!("`{id}` talks to `{to}`, which isn't an agent."));
        }
        if !seen.insert(to.as_str()) {
            return Err(format!("`{id}` lists `{to}` twice in talks_to."));
        }
    }
    if let Some(bad) = agent.tools.iter().find(|t| !valid_tool(t)) {
        return Err(format!(
            "`{id}` lists a tool name `{bad}` the app can't have."
        ));
    }
    if let Some(bad) = agent.playbooks.iter().flatten().find(|p| !valid_tool(p)) {
        return Err(format!(
            "`{id}` lists a playbook id `{bad}` the app can't have."
        ));
    }
    if !agent.model_tier.is_empty() && !tiers.contains(&agent.model_tier) {
        let known: Vec<&str> = tiers.iter().map(String::as_str).collect();
        return Err(format!(
            "`{id}` asks for model tier `{}`; the tiers are {}.",
            agent.model_tier,
            known.join(", ")
        ));
    }
    Ok(())
}

/// The tiers `model_profiles.toml` (and any override) uses.
fn known_tiers() -> BTreeSet<String> {
    crate::model_registry::builtin::all()
        .into_iter()
        .filter_map(|p| p.tier)
        .collect()
}

#[cfg(test)]
#[path = "agent_team_tests.rs"]
mod tests;
