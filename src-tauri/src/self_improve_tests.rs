use std::path::PathBuf;

use grokhub_agent::harness::{self as hx, VersionStatus};
use grokhub_core::run_metrics::DomainRow;

use super::*;

const DAY: u64 = 86_400_000;
const NOW: u64 = 1_800_000_000_000;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gh-self-improve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn version(id: &str, version: &str, status: VersionStatus, at: u64) -> hx::Version {
    hx::Version {
        version: version.into(),
        kind: "skill".into(),
        id: id.into(),
        label: format!("Use ctrl+s in {id}"),
        diff: "+2 -1 lines".into(),
        class: format!("skill:{id}"),
        status,
        started_at: at,
        at,
        ..Default::default()
    }
}

fn write_versions(dir: &Path, versions: &[hx::Version]) {
    let path = hx::versions_path(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines: Vec<String> = versions
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    std::fs::write(path, lines.join("\n")).unwrap();
}

#[test]
fn history_reads_newest_first_and_offers_revert_only_on_the_newest_kept() {
    let mut kept_old = version(
        "save-page",
        "skill:save-page@aaaaaaaaaaaa",
        VersionStatus::Kept,
        NOW - 3 * DAY,
    );
    kept_old.note = "Kept: Use ctrl+s in save-page, 18% fewer steps over 12 runs".into();
    let rows = history_rows(
        &[
            kept_old,
            version(
                "save-page",
                "skill:save-page@bbbbbbbbbbbb",
                VersionStatus::Canary,
                NOW - DAY,
            ),
            version(
                "deploy",
                "skill:deploy@cccccccccccc",
                VersionStatus::Kept,
                NOW - 2 * DAY,
            ),
        ],
        NOW,
    );
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        vec![
            "Kept: Use ctrl+s in deploy",
            "Trying: Use ctrl+s in save-page",
            "Kept: Use ctrl+s in save-page",
        ]
    );
    // The deploy change is the newest on its skill; save-page changed again since.
    assert_eq!(rows[0].revert.as_deref(), Some("skill:deploy@cccccccccccc"));
    assert_eq!(rows[1].revert, None);
    assert_eq!(rows[2].revert, None);
    assert!(rows[2].hint.starts_with(
        "Kept: Use ctrl+s in save-page, 18% fewer steps over 12 runs · +2 -1 lines · "
    ));
}

#[test]
fn history_shows_at_most_eight_rows() {
    let many: Vec<hx::Version> = (0..12)
        .map(|i| {
            version(
                &format!("s{i}"),
                &format!("skill:s{i}@{i:012}"),
                VersionStatus::Ended,
                NOW - i,
            )
        })
        .collect();
    assert_eq!(history_rows(&many, NOW).len(), HISTORY_ROWS);
    assert_eq!(
        history_rows(&many, NOW)[0].title,
        "Ended: Use ctrl+s in s11"
    );
}

#[test]
fn the_view_reads_the_cabins_files() {
    let dir = temp("view");
    write_versions(
        &dir,
        &[version(
            "deploy",
            "skill:deploy@cccccccccccc",
            VersionStatus::RolledBack,
            NOW - DAY,
        )],
    );
    std::fs::write(
        dir.join(SELF_REVIEW_FILE),
        r#"{"last_day":"2026-11-08","pending":[],
            "quiet_log":[{"day":"2026-11-08","source":"draft:deploy","title":"Make a skill: deploy","body":"You did this 4 times in the last two weeks"}],
            "saved_ideas":[{"title":"Older idea","text":"a","at":1},{"title":"Retry the build","text":"b","at":2}]}"#,
    )
    .unwrap();
    let v = view(&dir, NOW);
    assert!(!v.quiet);
    assert_eq!(
        v.quiet_note, None,
        "the log shows only while quiet mode is on"
    );
    assert_eq!(v.history[0].title, "Rolled back: Use ctrl+s in deploy");
    assert_eq!(v.history[0].revert, None);
    assert_eq!(
        v.ideas.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
        vec!["Retry the build", "Older idea"]
    );
    assert_eq!(v.domains, None);

    grokhub_core::app_json::patch(&dir, &[(QUIET_KEY, true.into())]).unwrap();
    let v = view(&dir, NOW);
    assert!(v.quiet);
    assert_eq!(
        v.quiet_note.as_deref(),
        Some("Logged suggestions, newest first:\n2026-11-08 · Make a skill: deploy: You did this 4 times in the last two weeks")
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("app.json")).unwrap()).unwrap();
    assert_eq!(json["selfImproveQuiet"], serde_json::json!(true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quiet_with_nothing_logged_says_when_the_review_runs() {
    assert_eq!(
        quiet_log_note(&[]),
        "Nothing logged yet. The self-review runs on Sunday at dream time."
    );
}

#[test]
fn revert_refuses_what_is_not_kept_and_what_is_not_there() {
    let dir = temp("revert");
    write_versions(
        &dir,
        &[version(
            "deploy",
            "skill:deploy@cccccccccccc",
            VersionStatus::Canary,
            NOW,
        )],
    );
    assert_eq!(
        revert(&dir, "skill:deploy@cccccccccccc", NOW + 1),
        Err("Only a kept change can be reverted; this one is canary".into())
    );
    assert_eq!(
        revert(&dir, "skill:nope@000000000000", NOW + 1),
        Err("That change is not in the history".into())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_issue_leaves_out_the_home_folder_and_emails() {
    let dir = temp("issue");
    std::fs::write(
        dir.join(SELF_REVIEW_FILE),
        r#"{"saved_ideas":[{"title":"Retry the build","text":"Logs in /home/sam/proj, mail sam@example.com","at":2}]}"#,
    )
    .unwrap();
    let text = issue(&dir, "Retry the build", Some("/home/sam")).unwrap();
    assert!(text.starts_with("## Retry the build\n\n"), "{text}");
    assert!(text.contains("Logs in ~/proj"), "{text}");
    assert!(text.contains("[email]"), "{text}");
    assert!(!text.contains("sam@example.com"), "{text}");
    assert_eq!(
        issue(&dir, "Gone", None),
        Err("That idea isn't saved any more.".into())
    );
    assert_eq!(
        issue_copied(),
        format!(
            "Issue copied. Paste it on GitHub yourself; {} sent nothing.",
            grokhub_core::brand::product_name()
        )
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn areas_read_as_the_cabins_lines() {
    let rows = vec![DomainRow {
        domain: "system".into(),
        runs: 4,
        worked: 3,
        steps: 26,
        verified: 2,
    }];
    assert_eq!(
        domain_block(&rows).as_deref(),
        Some(
            "Runs by area (runs on file):\nsystem · 4 runs · 75% worked · 6.5 steps · 50% checked"
        )
    );
    assert_eq!(domain_block(&[]), None);
}
