use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("grokhub-agent-team-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A household: a lead on the "home" team with a family member, a researcher
/// the lead lists by name, and a customer helper on the team.
fn household() -> TeamFile {
    let mut lead = Profile::new(Kind::Role, "planner", "Planner");
    lead.team = "home".into();
    lead.talks_to = vec!["researcher".into()];
    let mut kid = Profile::new(Kind::Family, "kid", "Kid");
    kid.team = "home".into();
    let mut desk = Profile::new(Kind::Customer, "front-desk", "Front desk");
    desk.team = "home".into();
    let researcher = Profile::new(Kind::Role, "researcher", "Researcher");
    TeamFile {
        version: VERSION,
        agents: vec![lead, kid, desk, researcher],
        teams: vec![Team {
            id: "home".into(),
            name: "Home".into(),
            lead: "planner".into(),
        }],
    }
}

#[test]
fn each_kind_starts_with_its_own_access() {
    assert_eq!(Profile::new(Kind::Role, "a", "A").access, Access::Ask);
    assert_eq!(Profile::new(Kind::Family, "a", "A").access, Access::Ask);
    assert_eq!(Profile::new(Kind::Staff, "a", "A").access, Access::Ask);
    assert_eq!(
        Profile::new(Kind::Customer, "a", "A").access,
        Access::ReadOnly
    );
    assert!(!Profile::new(Kind::Role, "a", "A").desktop);
}

#[test]
fn a_turn_never_runs_wider_than_its_caller_or_a_customer_past_read_only() {
    let mut auto = Profile::new(Kind::Staff, "s", "S");
    auto.access = Access::Auto;
    assert_eq!(auto.access_under(Access::Auto), Access::Auto);
    assert_eq!(auto.access_under(Access::Ask), Access::Ask);
    assert_eq!(auto.access_under(Access::ReadOnly), Access::ReadOnly);

    // A hand-edited customer file still runs read only, and without the desktop.
    let mut customer = Profile::new(Kind::Customer, "c", "C");
    customer.access = Access::Auto;
    customer.desktop = true;
    assert_eq!(customer.access_under(Access::Auto), Access::ReadOnly);
    assert!(!customer.desktop_under(true));

    auto.desktop = true;
    assert!(auto.desktop_under(true));
    assert!(!auto.desktop_under(false));
}

#[test]
fn a_home_is_under_the_agents_folder_and_only_for_a_real_id() {
    let config = Path::new("/home/user/.config/app");
    assert_eq!(
        home(config, "front-desk"),
        Some(PathBuf::from("/home/user/.config/app/agents/front-desk"))
    );
    assert_eq!(
        workspace(config, "kid"),
        Some(PathBuf::from("/home/user/.config/app/agents/kid/work"))
    );
    for bad in ["", "../x", "a/b", "Kid", "-a", "a-", "a.b", &"x".repeat(41)] {
        assert_eq!(home(config, bad), None, "{bad}");
    }
}

#[test]
fn the_person_may_message_any_agent_and_an_unknown_one_is_named() {
    let file = household();
    for id in ["planner", "kid", "front-desk", "researcher"] {
        assert_eq!(file.may_message(Sender::Person, id, &[]), Ok(()), "{id}");
    }
    assert_eq!(
        file.may_message(Sender::Person, "ghost", &[]),
        Err(Refusal::UnknownAgent("ghost".into()))
    );
}

#[test]
fn agents_message_their_list_and_their_lead_but_not_sideways() {
    let file = household();
    // Listed by name.
    assert_eq!(
        file.may_message(Sender::Agent("planner"), "researcher", &[]),
        Ok(())
    );
    // Lead and members, both ways.
    assert_eq!(
        file.may_message(Sender::Agent("planner"), "kid", &[]),
        Ok(())
    );
    assert_eq!(
        file.may_message(Sender::Agent("kid"), "planner", &[]),
        Ok(())
    );
    assert_eq!(
        file.may_message(Sender::Agent("planner"), "front-desk", &[]),
        Ok(())
    );
    // Two members, neither the lead: no.
    assert_eq!(
        file.may_message(Sender::Agent("kid"), "front-desk", &[]),
        Err(Refusal::NotAllowed {
            from: "kid".into(),
            to: "front-desk".into()
        })
    );
    // Not on the team and not listed: no.
    assert_eq!(
        file.may_message(Sender::Agent("researcher"), "planner", &[]),
        Err(Refusal::NotAllowed {
            from: "researcher".into(),
            to: "planner".into()
        })
    );
}

#[test]
fn a_customer_agent_only_answers() {
    let file = household();
    assert_eq!(
        file.may_message(Sender::Agent("front-desk"), "planner", &[]),
        Err(Refusal::CustomerStarts("front-desk".into()))
    );
    assert_eq!(
        Refusal::CustomerStarts("front-desk".into()).to_string(),
        "`front-desk` is a customer agent: it answers, but can't message other agents."
    );
}

#[test]
fn a_chain_stops_at_three_agents_and_never_asks_one_twice() {
    let file = household();
    let one = vec!["planner".to_string()];
    assert_eq!(
        file.may_message(Sender::Agent("planner"), "kid", &one),
        Ok(())
    );
    let back = vec!["planner".to_string(), "kid".to_string()];
    assert_eq!(
        file.may_message(Sender::Agent("kid"), "planner", &back),
        Err(Refusal::Loop("planner".into()))
    );
    assert_eq!(
        file.may_message(Sender::Agent("planner"), "planner", &[]),
        Err(Refusal::Loop("planner".into()))
    );
    let full = vec!["a".to_string(), "b".to_string(), "planner".to_string()];
    assert_eq!(
        file.may_message(Sender::Agent("planner"), "kid", &full),
        Err(Refusal::TooManyHops)
    );
    assert_eq!(
        Refusal::TooManyHops.to_string(),
        "This chain of messages already reached 3 agents. Answer with what you have."
    );
}

#[test]
fn the_household_checks_and_round_trips_through_its_file() {
    let config = scratch("round-trip");
    assert_eq!(TeamFile::load(&config), Ok(TeamFile::default()));
    let file = household();
    assert_eq!(file.check(), Ok(()));
    file.save(&config).unwrap();
    assert_eq!(TeamFile::load(&config), Ok(file));
    let text = std::fs::read_to_string(config.join(TEAM_FILE)).unwrap();
    assert!(text.contains("\"kind\": \"customer\""), "{text}");
    assert!(text.contains("\"access\": \"read_only\""), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(config.join(TEAM_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn a_file_that_doesnt_parse_is_an_error_and_is_left_alone() {
    let config = scratch("garbled");
    std::fs::write(config.join(TEAM_FILE), "{not json").unwrap();
    let err = TeamFile::load(&config).unwrap_err();
    assert!(
        err.ends_with("doesn't read as an agent team: key must be a string at line 1 column 2"),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(config.join(TEAM_FILE)).unwrap(),
        "{not json"
    );
}

#[test]
fn a_broken_file_is_refused_before_it_is_written() {
    let config = scratch("refused");
    let mut file = household();
    file.agents[2].access = Access::Auto;
    assert_eq!(
        file.save(&config),
        Err("`front-desk` is a customer agent, so its access stays read only.".into())
    );
    assert!(!config.join(TEAM_FILE).exists());
}

/// One edit that breaks one rule.
type Edit = Box<dyn Fn(&mut TeamFile)>;

#[test]
fn every_rule_names_what_broke() {
    let cases: Vec<(Edit, &str)> = vec![
        (
            Box::new(|f| f.version = 2),
            "This agent team file is version 2; this app reads version 1.",
        ),
        (
            Box::new(|f| f.agents[1].id = "Kid".into()),
            "Agent id `Kid` must be lowercase letters, digits and dashes.",
        ),
        (
            Box::new(|f| f.agents[1].id = "planner".into()),
            "Two agents use the id `planner`.",
        ),
        (
            Box::new(|f| f.agents[1].name = "  ".into()),
            "`kid` needs a name.",
        ),
        (
            Box::new(|f| f.agents[1].team = "work".into()),
            "`kid` is on team `work`, which doesn't exist.",
        ),
        (
            Box::new(|f| f.agents[2].desktop = true),
            "`front-desk` is a customer agent, so it can't use the desktop.",
        ),
        (
            Box::new(|f| f.agents[2].talks_to = vec!["planner".into()]),
            "`front-desk` is a customer agent: it answers, but can't message other agents.",
        ),
        (
            Box::new(|f| f.agents[0].talks_to.push("planner".into())),
            "`planner` lists itself in talks_to.",
        ),
        (
            Box::new(|f| f.agents[0].talks_to.push("ghost".into())),
            "`planner` talks to `ghost`, which isn't an agent.",
        ),
        (
            Box::new(|f| f.agents[0].talks_to.push("researcher".into())),
            "`planner` lists `researcher` twice in talks_to.",
        ),
        (
            Box::new(|f| f.agents[0].tools = vec!["rm -rf".into()]),
            "`planner` lists a tool name `rm -rf` the app can't have.",
        ),
        (
            Box::new(|f| f.agents[0].playbooks = Some(vec!["Fix Wifi".into()])),
            "`planner` lists a playbook id `Fix Wifi` the app can't have.",
        ),
        (
            Box::new(|f| f.agents[0].about = "x".repeat(4001)),
            "`planner`'s about is longer than 4000 characters.",
        ),
        (
            Box::new(|f| f.teams[0].lead = "researcher".into()),
            "Team `home`'s lead `researcher` must be on that team.",
        ),
        (
            Box::new(|f| f.teams[0].lead = "ghost".into()),
            "Team `home`'s lead `ghost` isn't an agent.",
        ),
        (
            Box::new(|f| {
                f.teams.push(Team {
                    id: "home".into(),
                    name: "Again".into(),
                    lead: "planner".into(),
                })
            }),
            "Two teams use the id `home`.",
        ),
    ];
    for (edit, want) in cases {
        let mut file = household();
        edit(&mut file);
        assert_eq!(file.check(), Err(want.to_string()));
    }
}

#[test]
fn a_model_tier_must_be_one_model_profiles_uses() {
    let mut file = household();
    file.agents[3].model_tier = "fast".into();
    assert_eq!(file.check(), Ok(()));
    file.agents[3].model_tier = "turbo".into();
    let err = file.check().unwrap_err();
    assert!(
        err.starts_with("`researcher` asks for model tier `turbo`; the tiers are "),
        "{err}"
    );
    assert!(
        err.contains("fast") && err.contains("frontier") && err.contains("free"),
        "{err}"
    );
}
