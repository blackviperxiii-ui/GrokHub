use super::*;
use std::fs;
use std::path::PathBuf;

/// A scratch config, home and git project, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "grokhub-project-hooks-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ws").join(".git")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        Self(root)
    }
    fn cfg(&self) -> PathBuf {
        self.0.join("cfg")
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn ws(&self) -> PathBuf {
        self.0.join("ws")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn hook_file(dir: &Path, name: &str, matcher: &str, command: &str) {
    fs::create_dir_all(dir).unwrap();
    let body = serde_json::json!({
        "hooks": {"PreToolUse": [{"matcher": matcher, "hooks": [{"type": "command", "command": command}]}]}
    });
    fs::write(dir.join(name), body.to_string()).unwrap();
}

/// (origin, command, matcher, runs), sorted by command.
fn summary(page: &Page) -> Vec<(&'static str, String, String, bool)> {
    let mut rows: Vec<_> = page
        .rows
        .iter()
        .map(|r| (r.origin, r.command.clone(), r.matcher.clone(), r.runs))
        .collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    rows
}

fn trusted_file(cfg: &Path) -> Vec<String> {
    serde_json::from_str(&fs::read_to_string(cfg.join("trusted_folders.json")).unwrap()).unwrap()
}

#[test]
fn project_hooks_are_listed_but_do_not_run_until_the_folder_is_trusted() {
    let s = Scratch::new("trust");
    let (cfg, home, ws) = (s.cfg(), s.home(), s.ws());
    hook_file(&home.join(".grok").join("hooks"), "u.json", "", "echo user");
    hook_file(
        &ws.join(".grok").join("hooks"),
        "p.json",
        "Bash",
        "echo project",
    );

    let page = load(&cfg, &ws, Some(&home));
    assert_eq!(page.folder, ws.display().to_string());
    assert!(!page.trusted);
    assert_eq!(page.project_hooks, 1);
    assert_eq!(page.status, "");
    assert_eq!(
        summary(&page),
        vec![
            (
                "project",
                "echo project".to_string(),
                "Bash".to_string(),
                false
            ),
            ("user", "echo user".to_string(), String::new(), true),
        ]
    );
    let project = page.rows.iter().find(|r| r.origin == "project").unwrap();
    assert_eq!(project.event, "PreToolUse");
    assert_eq!(project.kind, "command");
    assert!(project.path.ends_with("p.json"), "{}", project.path);
    assert_eq!(project.note, WAITS_FOR_TRUST);
    let mine = page.rows.iter().find(|r| r.origin == "user").unwrap();
    assert_eq!(mine.note, "");

    let page = set_trust(&cfg, &ws, Some(&home), true).unwrap();
    assert_eq!(
        page.status,
        format!(
            "Trusted {}. Its hooks run from your next message.",
            ws.display()
        )
    );
    assert!(page.trusted);
    assert_eq!(
        summary(&page),
        vec![
            (
                "project",
                "echo project".to_string(),
                "Bash".to_string(),
                true
            ),
            ("user", "echo user".to_string(), String::new(), true),
        ]
    );
    let key = fs::canonicalize(&ws).unwrap().display().to_string();
    assert_eq!(trusted_file(&cfg), vec![key]);
    // The chat's own check reads the same file.
    {
        let _config = ConfigGuard::set(&cfg);
        assert!(grokhub_agent::folder_trusted(&ws));
    }

    let page = set_trust(&cfg, &ws, Some(&home), false).unwrap();
    assert_eq!(
        page.status,
        format!(
            "Stopped trusting {}. Its hooks don't run from your next message.",
            ws.display()
        )
    );
    assert!(!page.trusted);
    assert!(
        !page
            .rows
            .iter()
            .find(|r| r.origin == "project")
            .unwrap()
            .runs
    );
    assert_eq!(trusted_file(&cfg), Vec::<String>::new());
    // A fresh load agrees.
    assert!(!load(&cfg, &ws, Some(&home)).trusted);
}

#[test]
fn the_filesystem_root_is_refused_in_plain_words() {
    let s = Scratch::new("root");
    let (cfg, home) = (s.cfg(), s.home());
    assert_eq!(
        set_trust(&cfg, Path::new("/"), Some(&home), true).unwrap_err(),
        HOME_REFUSED
    );
    assert!(!cfg.join("trusted_folders.json").exists());
}

#[test]
fn secrets_in_hook_commands_are_hidden_and_script_names_stay_readable() {
    let s = Scratch::new("secret");
    let (cfg, home, ws) = (s.cfg(), s.home(), s.ws());
    let dir = ws.join(".grok").join("hooks");
    hook_file(
        &dir,
        "a.json",
        "",
        "curl -H 'Authorization: Bearer sk-abcdefghijklmnopqrstuv' https://example.com",
    );
    hook_file(
        &dir,
        "b.json",
        "",
        "API_KEY=abcdefghijklmnop123456 ./notify.sh",
    );
    hook_file(&dir, "c.json", "", "./scripts/task-runner-lint.sh");
    hook_file(&dir, "d.json", "", "~/.grok/hooks/desk-guard-check.sh");
    let page = load(&cfg, &ws, Some(&home));
    let wire = serde_json::to_string(&page).unwrap();
    assert!(!wire.contains("sk-abcdefghijklmnopqrstuv"), "{wire}");
    assert!(!wire.contains("abcdefghijklmnop123456"), "{wire}");
    let commands: Vec<&str> = page.rows.iter().map(|r| r.command.as_str()).collect();
    assert_eq!(commands.len(), 4);
    assert!(commands[0].starts_with("curl -H"), "{}", commands[0]);
    assert!(
        commands[0].ends_with("https://example.com"),
        "{}",
        commands[0]
    );
    assert!(commands[1].starts_with("API_KEY="), "{}", commands[1]);
    assert!(commands[1].ends_with(" ./notify.sh"), "{}", commands[1]);
    assert_eq!(commands[2], "./scripts/task-runner-lint.sh");
    assert_eq!(commands[3], "~/.grok/hooks/desk-guard-check.sh");
}

#[test]
fn an_http_hook_shows_its_host_on_the_page() {
    let s = Scratch::new("http");
    let (cfg, home, ws) = (s.cfg(), s.home(), s.ws());
    let dir = home.join(".grok").join("hooks");
    fs::create_dir_all(&dir).unwrap();
    let body = serde_json::json!({
        "hooks": {"SessionEnd": [{"hooks": [{"type": "http", "url": "https://hooks.chat.example/services/T01/B02/abcdEFGHijklMNOP"}]}]}
    });
    fs::write(dir.join("h.json"), body.to_string()).unwrap();
    let page = load(&cfg, &ws, Some(&home));
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].kind, "http");
    assert_eq!(page.rows[0].command, "https://hooks.chat.example/[redacted]");
    // The runners only run command hooks.
    assert!(!page.rows[0].runs);
    assert_eq!(page.rows[0].note, HTTP_NOT_RUN);
}

#[test]
fn a_hook_for_an_event_nothing_fires_says_so() {
    let s = Scratch::new("never");
    let (cfg, home, ws) = (s.cfg(), s.home(), s.ws());
    let dir = home.join(".grok").join("hooks");
    fs::create_dir_all(&dir).unwrap();
    let body = serde_json::json!({
        "hooks": {
            "SessionEnd": [{"hooks": [{"type": "command", "command": "./save.sh"}]}],
            "PreCompact": [{"matcher": "manual", "hooks": [{"type": "command", "command": "echo manual"}]}]
        }
    });
    fs::write(dir.join("h.json"), body.to_string()).unwrap();
    let mut notes: Vec<(String, bool, String)> = load(&cfg, &ws, Some(&home))
        .rows
        .into_iter()
        .map(|r| (r.command, r.runs, r.note))
        .collect();
    notes.sort();
    assert_eq!(
        notes,
        vec![
            (
                "./save.sh".to_string(),
                false,
                "Never runs: SessionEnd isn't fired here yet".to_string()
            ),
            ("echo manual".to_string(), false, NEVER_MATCHES.to_string()),
        ]
    );
}

#[test]
fn a_plugin_hook_the_project_repeats_shows_as_the_plugins_and_running() {
    let s = Scratch::new("plugin");
    let (cfg, home, ws) = (s.cfg(), s.home(), s.ws());
    hook_file(&ws.join(".claude"), "settings.json", "Bash", "echo guard");
    let plugin = s.0.join("plugin");
    hook_file(&plugin, "hooks.json", "Bash", "echo guard");
    let plugin_file = plugin.join("hooks.json");
    let _config = ConfigGuard::set(&cfg);
    let rows = |page: Page| -> Vec<(&'static str, String, bool)> {
        page.rows
            .into_iter()
            .map(|r| (r.origin, r.path, r.runs))
            .collect()
    };
    let untrusted = page(
        &ws,
        Some(&home),
        String::new(),
        std::slice::from_ref(&plugin_file),
    );
    assert_eq!(untrusted.project_hooks, 0);
    assert_eq!(
        rows(untrusted),
        vec![("plugin", plugin_file.display().to_string(), true)]
    );
    grokhub_agent::set_folder_trust(&ws, true).unwrap();
    let trusted = page(
        &ws,
        Some(&home),
        String::new(),
        std::slice::from_ref(&plugin_file),
    );
    assert_eq!(trusted.project_hooks, 1);
    assert_eq!(
        rows(trusted),
        vec![(
            "project",
            ws.join(".claude")
                .join("settings.json")
                .display()
                .to_string(),
            true
        )]
    );
}

#[test]
fn a_folder_with_only_your_own_hooks_has_no_project_hooks() {
    let s = Scratch::new("user-only");
    let (cfg, home, ws) = (s.cfg(), s.home(), s.ws());
    hook_file(
        &home.join(".grok").join("hooks"),
        "u.json",
        "Read",
        "echo user",
    );
    let page = load(&cfg, &ws, Some(&home));
    assert_eq!(page.project_hooks, 0);
    assert_eq!(
        summary(&page),
        vec![("user", "echo user".to_string(), "Read".to_string(), true)]
    );
}
