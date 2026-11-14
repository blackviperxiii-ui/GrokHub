use super::*;

fn shell(command: &str) -> String {
    serde_json::json!({ "command": command }).to_string()
}

#[test]
fn the_skills_block_carries_every_bundled_skill_in_full() {
    // follow-through writes its closeout script under the config dir.
    let cfg = std::env::temp_dir().join(format!("code-agent-cfg-{}", std::process::id()));
    let _config = grokhub_agent::perm::ConfigGuard::set(&cfg);
    let block = skills_block();
    assert!(block.starts_with("## Coding skills (always on in the Code tab)\n\n"));
    for name in [
        "qa-walkthrough",
        "av-qa-walkthrough",
        "create-verification-skill",
        "maintain-verification-skill",
        "follow-through",
    ] {
        assert!(block.contains(&format!("\n### {name}\n")), "{name}");
    }
    assert_eq!(
        grokhub_agent::BUILTIN_ALWAYS_ON.len(),
        5,
        "a new always-on skill joins the block"
    );
    // The bodies, not their front matter.
    assert!(!block.contains("\n---\n"), "{block}");
    assert!(block.contains(PUSH_NOTE));
    // The other bundled skills are one line each, after the full ones.
    let (full, more) = block
        .split_once("\n### More built-in skills\n")
        .expect("index");
    let elevate = more
        .lines()
        .find_map(|line| line.strip_prefix("- elevate: "))
        .expect("elevate is listed");
    assert!(elevate.ends_with("/elevate"), "{elevate}");
    assert!(!full.contains("### elevate"), "{full}");
    assert!(!more.contains("\n- follow-through:"), "{more}");
    assert!(!more.contains("setup-vstack"), "{more}");
    // Skills upstream marks user-only stay out of the list the model picks from.
    assert!(!more.contains("\n- swarm:"), "{more}");
    assert!(!more.contains("\n- unslop:"), "{more}");
    let listed = grokhub_agent::BUILTIN_SKILL_BODIES
        .iter()
        .filter(|(n, t)| !grokhub_agent::BUILTIN_ALWAYS_ON.contains(n) && !user_only(t))
        .count();
    assert_eq!(more.matches("\n- ").count(), listed);
    assert!(listed > 20, "{listed}");
    // The always-on follow-through names a real folder for its script.
    let script = cfg.join("builtin-skills/follow-through/scripts/closeout.py");
    assert!(script.is_file(), "{}", script.display());
    assert!(!full.contains("<this skill>"), "{full}");
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn the_user_only_flag_and_description_come_from_the_front_matter() {
    assert!(user_only(
        "---\nname: x\ndisable-model-invocation: true\n---\nBody\n"
    ));
    assert!(!user_only(
        "---\nname: x\n---\ndisable-model-invocation: true\n"
    ));
    assert_eq!(
        description("---\nname: x\ndescription: Does x.\n---\ndescription: body\n"),
        "Does x."
    );
}

#[test]
fn front_matter_is_dropped_and_a_plain_body_kept() {
    assert_eq!(
        body("---\nname: x\ndescription: y\n---\n\n# X\nDo it.\n"),
        "# X\nDo it."
    );
    assert_eq!(body("# Plain\n"), "# Plain");
}

#[test]
fn only_shell_tools_with_a_push_reach_the_guard() {
    let dir = std::env::temp_dir().join(format!("code-agent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    assert_eq!(
        check(&dir, "write", &shell("git push --force origin main"), &dir),
        None
    );
    assert_eq!(
        check(&dir, "run_terminal_command", &shell("cargo test"), &dir),
        None
    );
    assert_eq!(
        check(
            &dir,
            "run_terminal_command",
            &shell("git push --force origin main"),
            &dir
        ),
        Some(Decision::Refuse(grokhub_code::chat_push::FORCE.into()))
    );
    assert_eq!(
        check(&dir, "monitor", &shell("git push -f origin code/x"), &dir),
        Some(Decision::Refuse(
            "Force-pushing is never allowed, in any mode. Push without it, or make a new branch."
                .into()
        ))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_code_session_gets_the_guard_and_a_plain_chat_does_not() {
    let dir = std::env::temp_dir().join(format!("code-agent-bind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    bind(&dir, "code-agent-bind", true);
    assert!(code_guard::is_set("code-agent-bind"));
    let args = shell("git push --force origin main");
    assert_eq!(
        code_guard::apply(
            "code-agent-bind",
            "run_terminal_command",
            &args,
            &dir,
            None,
            Decision::Run
        ),
        Decision::Refuse(grokhub_code::chat_push::FORCE.into())
    );
    bind(&dir, "code-agent-bind", false);
    assert!(!code_guard::is_set("code-agent-bind"));
    assert_eq!(
        code_guard::apply(
            "code-agent-bind",
            "run_terminal_command",
            &args,
            &dir,
            None,
            Decision::Run
        ),
        Decision::Run
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Needs the app crate's prompt builder (not in the scratch check).
#[test]
fn only_a_code_session_prompt_carries_the_skills() {
    // follow-through writes its closeout script under the config dir.
    let cfg = std::env::temp_dir().join(format!("code-agent-prompt-cfg-{}", std::process::id()));
    let _config = grokhub_agent::perm::ConfigGuard::set(&cfg);
    let dir = std::env::temp_dir();
    bind(&dir, "code-agent-prompt", true);
    let coding = crate::agent::system_prompt_for(&dir, false, "", "code-agent-prompt", false);
    assert!(coding.contains(SKILLS_HEAD));
    assert!(coding.contains("\n### follow-through\n"));
    bind(&dir, "code-agent-prompt", false);
    let plain = crate::agent::system_prompt_for(&dir, false, "", "code-agent-prompt", false);
    assert!(!plain.contains(SKILLS_HEAD));
    let _ = std::fs::remove_dir_all(&cfg);
}
