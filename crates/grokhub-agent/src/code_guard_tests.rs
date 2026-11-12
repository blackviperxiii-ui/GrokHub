use super::*;
use std::path::PathBuf;

/// Refuses a push to main, lets a beta push run, asks for any other push,
/// and leaves every other call alone.
fn pushes() -> Guard {
    Arc::new(|name: &str, arguments: &str, _: &Path| {
        if name != "run_terminal_command" || !arguments.contains("git push") {
            return None;
        }
        Some(if arguments.contains("main") {
            Decision::Refuse("main is the main or default branch.".into())
        } else if arguments.contains("code/fix") {
            Decision::Run
        } else {
            Decision::Ask
        })
    })
}

fn shell(command: &str) -> String {
    serde_json::json!({ "command": command }).to_string()
}

fn ws() -> PathBuf {
    std::env::temp_dir()
}

#[test]
fn no_guard_leaves_the_gate_as_it_was() {
    let args = shell("git push origin main");
    for base in [Decision::Run, Decision::Ask] {
        assert_eq!(
            apply(
                "cg-none",
                "run_terminal_command",
                &args,
                &ws(),
                None,
                base.clone()
            ),
            base
        );
    }
    assert!(!is_set("cg-none"));
}

#[test]
fn a_refusal_wins_even_over_always() {
    set("cg-refuse", Some(pushes()));
    let args = shell("git push origin main");
    assert_eq!(
        apply(
            "cg-refuse",
            "run_terminal_command",
            &args,
            &ws(),
            None,
            Decision::Run
        ),
        Decision::Refuse("main is the main or default branch.".into())
    );
    set("cg-refuse", None);
    assert_eq!(
        apply(
            "cg-refuse",
            "run_terminal_command",
            &args,
            &ws(),
            None,
            Decision::Run
        ),
        Decision::Run
    );
}

#[test]
fn an_allowed_push_runs_without_a_card_and_others_ask() {
    set("cg-run", Some(pushes()));
    let ok = shell("git push origin code/fix");
    assert_eq!(
        apply(
            "cg-run",
            "run_terminal_command",
            &ok,
            &ws(),
            None,
            Decision::Ask
        ),
        Decision::Run
    );
    let other = shell("git push origin code/other");
    assert_eq!(
        apply(
            "cg-run",
            "run_terminal_command",
            &other,
            &ws(),
            None,
            Decision::Run
        ),
        Decision::Ask
    );
    // Not a push: the gate's answer stands.
    let test = shell("cargo test");
    assert_eq!(
        apply(
            "cg-run",
            "run_terminal_command",
            &test,
            &ws(),
            None,
            Decision::Ask
        ),
        Decision::Ask
    );
    set("cg-run", None);
}

#[test]
fn the_guard_never_loosens_a_refusal() {
    set("cg-plan", Some(pushes()));
    let ok = shell("git push origin code/fix");
    assert_eq!(
        apply(
            "cg-plan",
            "run_terminal_command",
            &ok,
            &ws(),
            None,
            Decision::Refuse("read-only in this phase".into())
        ),
        Decision::Refuse("read-only in this phase".into())
    );
    set("cg-plan", None);
}

#[test]
fn a_helper_uses_its_parent_chats_guard() {
    set("cg-parent", Some(pushes()));
    assert!(is_set("cg-parent/helper-1"));
    let args = shell("git push --force origin main");
    assert_eq!(
        apply(
            "cg-parent/helper-1/inner",
            "run_terminal_command",
            &args,
            &ws(),
            None,
            Decision::Run
        ),
        Decision::Refuse("main is the main or default branch.".into())
    );
    assert!(!is_set("cg-parentless"));
    set("cg-parent", None);
    assert!(!is_set("cg-parent/helper-1"));
}
