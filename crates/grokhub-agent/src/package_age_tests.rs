use super::*;

const NOV_5: u64 = 1_793_836_800_000; // 2026-11-05T00:00:00Z

#[test]
fn seven_days_by_default_and_the_file_can_change_or_stop_it() {
    assert_eq!(parse_days(""), 7);
    assert_eq!(parse_days("days = 3"), 3);
    assert_eq!(parse_days("days = 0"), 0);
    assert_eq!(parse_days("days = 400"), 90);
    let dir = crate::harness::test_dir("package-age");
    assert_eq!(days(&dir), DEFAULT_DAYS);
    std::fs::write(dir.join(CONFIG), "days = 14\n").unwrap();
    assert_eq!(days(&dir), 14);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn each_tool_gets_its_own_unit() {
    let env: std::collections::BTreeMap<String, String> = env_for(7, NOV_5).into_iter().collect();
    assert_eq!(env["NPM_CONFIG_MIN_RELEASE_AGE"], "7");
    assert_eq!(env["PIPX_COOLDOWN"], "7");
    assert_eq!(env["POETRY_SOLVER_MIN_RELEASE_AGE"], "7");
    assert!(
        env["UV_EXCLUDE_NEWER"].starts_with("2026-10-29T00:00:00"),
        "{}",
        env["UV_EXCLUDE_NEWER"]
    );
    assert_eq!(env["PIP_UPLOADED_PRIOR_TO"], env["UV_EXCLUDE_NEWER"]);
    assert!(env_for(0, NOV_5).is_empty());
}

#[test]
fn a_plain_install_beside_a_lockfile_gets_the_locked_form() {
    let dir = crate::harness::test_dir("package-age-lock");
    assert_eq!(lockfile_hint("npm install", &dir), None);
    std::fs::write(dir.join("package-lock.json"), "{}").unwrap();
    let hint = lockfile_hint("npm install", &dir).unwrap();
    assert!(hint.contains("`npm ci`"), "{hint}");
    assert!(lockfile_hint("npm i", &dir).is_some());
    // Adding a package is a different ask.
    assert_eq!(lockfile_hint("npm install left-pad", &dir), None);
    assert_eq!(lockfile_hint("npm ci", &dir), None);
    std::fs::write(dir.join("uv.lock"), "").unwrap();
    assert!(lockfile_hint("uv sync", &dir).unwrap().contains("--locked"));
    let _ = std::fs::remove_dir_all(dir);
}
