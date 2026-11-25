use super::*;
use crate::perm::parse_rule;
use grokhub_core::agent_team::{Kind, Team, VERSION};

fn scratch(tag: &str) -> PathBuf {
    let dir = crate::harness::test_dir(&format!("agent-message-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A lead on the "home" team with a family member and a customer helper,
/// plus a researcher the lead lists by name.
fn household(config: &Path) {
    let mut lead = Profile::new(Kind::Role, "planner", "Planner");
    lead.team = "home".into();
    lead.talks_to = vec!["researcher".into()];
    let mut kid = Profile::new(Kind::Family, "kid", "Kid");
    kid.team = "home".into();
    let mut desk = Profile::new(Kind::Customer, "front-desk", "Front desk");
    desk.team = "home".into();
    let mut researcher = Profile::new(Kind::Role, "researcher", "Researcher");
    researcher.about = "\nFinds sources and says how sure it is.\nCites every claim.".into();
    TeamFile {
        version: VERSION,
        agents: vec![lead, kid, desk, researcher],
        teams: vec![Team {
            id: "home".into(),
            name: "Home".into(),
            lead: "planner".into(),
        }],
    }
    .save(config)
    .unwrap();
}

fn offered(config: &Path) -> Option<(Vec<String>, String)> {
    let _pin = ConfigGuard::set(config);
    let tool = schema()?;
    let ids = tool["parameters"]["properties"]["agent"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect();
    Some((ids, tool["description"].as_str().unwrap().to_string()))
}

fn home_of(config: &Path, id: &str) -> PathBuf {
    agent_team::home(config, id).unwrap()
}

fn gate(mode: PermMode, readonly: bool, desktop: bool) -> Gate {
    Gate {
        mode,
        readonly_session: readonly,
        attended: true,
        desktop,
    }
}

#[test]
fn no_team_offers_no_tool() {
    let config = scratch("none");
    assert_eq!(offered(&config), None);
}

#[test]
fn the_person_is_offered_every_agent_and_each_agent_only_whom_it_may_message() {
    let config = scratch("offer");
    household(&config);
    let (ids, description) = offered(&config).unwrap();
    assert_eq!(ids, ["planner", "kid", "front-desk", "researcher"]);
    assert!(
        description.ends_with(
            "Agents:\n- planner: Planner (role)\n- kid: Kid (family)\n- front-desk: Front desk (customer)\n- researcher: Researcher (role). Finds sources and says how sure it is."
        ),
        "{description}"
    );
    let (ids, _) = offered(&home_of(&config, "planner")).unwrap();
    assert_eq!(ids, ["kid", "front-desk", "researcher"]);
    let (ids, _) = offered(&home_of(&config, "kid")).unwrap();
    assert_eq!(ids, ["planner"]);
    // A customer agent only answers, and the researcher lists nobody.
    assert_eq!(offered(&home_of(&config, "front-desk")), None);
    assert_eq!(offered(&home_of(&config, "researcher")), None);
}

#[test]
fn a_message_the_team_doesnt_allow_is_refused_with_why() {
    let config = scratch("refused");
    household(&config);
    let ask = |from: &Path, args: &str| {
        let _pin = ConfigGuard::set(from);
        brief(args, &gate(PermMode::Auto, false, false), None).map(|b| b.profile.id)
    };
    let kid = home_of(&config, "kid");
    assert_eq!(
        ask(&kid, r#"{"agent":"front-desk","message":"hi"}"#),
        Err("`kid` can't message `front-desk`: add `front-desk` to its talks_to, or put both on a team with one of them as lead.".into())
    );
    assert_eq!(
        ask(&config, r#"{"agent":"ghost","message":"hi"}"#),
        Err("There's no agent `ghost`.".into())
    );
    assert_eq!(
        ask(&config, r#"{"agent":"kid","message":"  "}"#),
        Err("agent and message are required".into())
    );
    let long = json!({"agent": "kid", "message": "x".repeat(8_001)}).to_string();
    assert_eq!(
        ask(&config, &long),
        Err(
            "a message to an agent is at most 8000 characters; send the gist, not the whole file"
                .into()
        )
    );
    assert_eq!(
        ask(
            &kid,
            r#"{"agent":"planner","message":"done with homework"}"#
        ),
        Ok("planner".into())
    );
}

#[test]
fn an_agents_gate_is_never_wider_than_its_callers() {
    let mut planner = Profile::new(Kind::Role, "planner", "Planner");
    // Its kind's default is Ask, whatever the caller allows.
    assert_eq!(
        agent_gate(&gate(PermMode::Always, false, true), &planner),
        gate(PermMode::Ask, false, false)
    );
    planner.access = Access::Auto;
    planner.desktop = true;
    // Auto at most: an agent never runs with Always.
    assert_eq!(
        agent_gate(&gate(PermMode::Always, false, true), &planner),
        gate(PermMode::Auto, false, true)
    );
    assert_eq!(
        agent_gate(&gate(PermMode::Ask, false, false), &planner),
        gate(PermMode::Ask, false, false)
    );
    assert_eq!(
        agent_gate(&gate(PermMode::Auto, true, true), &planner),
        gate(PermMode::Ask, true, true)
    );
    let mut desk = Profile::new(Kind::Customer, "front-desk", "Front desk");
    desk.access = Access::Auto;
    desk.desktop = true;
    assert_eq!(
        agent_gate(&gate(PermMode::Always, false, true), &desk),
        gate(PermMode::Ask, true, false)
    );
}

#[test]
fn an_agent_keeps_the_callers_deny_and_ask_rules_but_not_its_allows_or_grants() {
    let config = scratch("policy");
    let deny = parse_rule("WebFetch(domain:example.com)", Action::Deny).unwrap();
    let ask = parse_rule("Bash(git push:*)", Action::Ask).unwrap();
    let allow = parse_rule("Bash(ls:*)", Action::Allow).unwrap();
    let caller = Policy {
        rules: vec![deny.clone(), allow, ask.clone()],
        grants: vec!["Bash(cargo test:*)".into()],
    };
    let home = home_of(&config, "kid");
    let own = parse_rule("Read(./notes/**)", Action::Allow).unwrap();
    {
        let _pin = ConfigGuard::set(&home);
        crate::perm::save_rules(&home, std::slice::from_ref(&own)).unwrap();
    }
    let policy = agent_policy(Some(&caller), &home, &home.join("work"));
    assert_eq!(policy.rules, vec![deny, ask, own]);
    assert_eq!(policy.grants, Vec::<String>::new());
}

#[test]
fn an_agent_gets_its_own_folders_and_system_text_with_only_the_message() {
    let config = scratch("brief");
    household(&config);
    let _pin = ConfigGuard::set(&config);
    let brief = brief(
        r#"{"agent":"researcher","message":"  Which router has the best range?  "}"#,
        &gate(PermMode::Auto, false, false),
        None,
    )
    .unwrap();
    assert_eq!(brief.home, config.join("agents").join("researcher"));
    assert_eq!(brief.workspace, brief.home.join("work"));
    assert!(brief.workspace.is_dir());
    assert_eq!(brief.message, "Which router has the best range?");
    assert_eq!(brief.gate, gate(PermMode::Ask, false, false));
    assert!(
        brief.system.ends_with(
            "You are Researcher, one of the person's agents (role agent). You work with your own chats, memory and connections. A message from the person or another agent follows; do what it asks within your permissions and answer with what the sender needs.\n<about_you>\nFinds sources and says how sure it is.\nCites every claim.\n</about_you>"
        ),
        "{}",
        brief.system
    );
}
