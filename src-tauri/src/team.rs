//! The Team page: the person's agents and teams (`agent-team.json`) and the
//! log of what each agent was asked. Every edit loads the file, changes it
//! and saves it, and the save checks every rule first.

use std::path::Path;

use grokhub_core::agent_team::{self, Profile, Team, TeamFile};
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub agents: Vec<Profile>,
    pub teams: Vec<Team>,
    /// The first rule the saved file breaks, when someone edited it by hand.
    pub problem: Option<String>,
    /// The model tiers a profile can name.
    pub tiers: Vec<String>,
}

pub fn load(config: &Path) -> Result<Page, String> {
    let file = TeamFile::load(config)?;
    Ok(Page {
        problem: file.check().err(),
        tiers: agent_team::known_tiers().into_iter().collect(),
        agents: file.agents,
        teams: file.teams,
    })
}

fn edit(
    config: &Path,
    change: impl FnOnce(&mut TeamFile) -> Result<(), String>,
) -> Result<Page, String> {
    let mut file = TeamFile::load(config)?;
    change(&mut file)?;
    file.save(config)?;
    load(config)
}

/// Adds the agent (`add`) or replaces the one with its id.
pub fn save_agent(config: &Path, agent: Profile, add: bool) -> Result<Page, String> {
    edit(config, |file| file.put_agent(agent, add))
}

/// Removes the agent and every mention of it. Its folder stays.
pub fn remove_agent(config: &Path, id: &str) -> Result<Page, String> {
    edit(config, |file| {
        file.remove_agent(id);
        Ok(())
    })
}

pub fn save_team(config: &Path, team: Team, add: bool) -> Result<Page, String> {
    edit(config, |file| file.put_team(team, add))
}

pub fn remove_team(config: &Path, id: &str) -> Result<Page, String> {
    edit(config, |file| {
        file.remove_team(id);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::agent_team::Kind;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("grokhub-team-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_edit_saves_and_a_broken_one_leaves_the_file_as_it_was() {
        let dir = scratch("edit");
        let page = load(&dir).unwrap();
        assert_eq!(
            (page.agents.len(), page.teams.len(), page.problem),
            (0, 0, None)
        );

        let mut writer = Profile::new(Kind::Role, "writer", "Writer");
        writer.talks_to = vec!["editor".into()];
        assert_eq!(
            save_agent(&dir, writer.clone(), true),
            Err("`writer` talks to `editor`, which isn't an agent.".into())
        );
        assert!(!TeamFile::path(&dir).exists());

        writer.talks_to.clear();
        save_agent(&dir, writer, true).unwrap();
        save_agent(&dir, Profile::new(Kind::Role, "editor", "Editor"), true).unwrap();
        let team = Team {
            id: "desk".into(),
            name: "Desk".into(),
            lead: "editor".into(),
        };
        let page = save_team(&dir, team, true).unwrap();
        assert_eq!(page.teams[0].lead, "editor");
        assert_eq!(page.agents[1].team, "desk");

        let page = remove_agent(&dir, "editor").unwrap();
        assert_eq!(
            page.agents
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["writer"]
        );
        assert_eq!(page.teams, Vec::new());
        assert_eq!(TeamFile::load(&dir).unwrap().agents.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hand_edit_that_breaks_a_rule_shows_as_the_problem() {
        let dir = scratch("problem");
        std::fs::write(
            TeamFile::path(&dir),
            r#"{"version":1,"agents":[{"id":"a","name":"A","kind":"role","access":"ask","team":"gone"}]}"#,
        )
        .unwrap();
        let page = load(&dir).unwrap();
        assert_eq!(
            page.problem.as_deref(),
            Some("`a` is on team `gone`, which doesn't exist.")
        );
        assert_eq!(page.agents[0].id, "a");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
