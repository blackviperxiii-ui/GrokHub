//! The folders and files a sandboxed command never gets to read: keys,
//! cloud and registry credentials, browser profiles, the sign-in folder and
//! Loadout's config folder. Linux covers them with an empty folder or
//! `/dev/null` in the command's own mount namespace ([`super::netns`]);
//! macOS denies them in the command's Seatbelt profile ([`super::seatbelt`]);
//! Windows keeps them out of the command's process container ([`super::mxc`]).

use std::path::{Path, PathBuf};

/// Folders under the home folder whose contents a sandboxed command never
/// needs and a leak of which is costly. Missing ones are skipped, so each
/// system's folders cost nothing on the others.
pub const SECRET_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".kube",
    ".docker",
    ".password-store",
    ".local/share/keyrings",
    ".mozilla",
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/microsoft-edge",
    ".config/vivaldi",
    ".config/gh",
    // macOS keeps keychain files and browser profiles under Library.
    "Library/Keychains",
    "Library/Cookies",
    "Library/Application Support/Google/Chrome",
    "Library/Application Support/Chromium",
    "Library/Application Support/Firefox",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Microsoft Edge",
    "Library/Application Support/Vivaldi",
    // Windows keeps credentials, browser profiles and CLI sign-ins in AppData.
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Local/Microsoft/Credentials",
    "AppData/Roaming/Microsoft/Protect",
    "AppData/Roaming/Microsoft/Crypto",
    "AppData/Roaming/Microsoft/SystemCertificates/My",
    "AppData/Local/Google/Chrome/User Data",
    "AppData/Local/Chromium/User Data",
    "AppData/Local/Microsoft/Edge/User Data",
    "AppData/Local/BraveSoftware",
    "AppData/Local/Vivaldi/User Data",
    "AppData/Roaming/Mozilla/Firefox",
    "AppData/Roaming/gcloud",
    "AppData/Roaming/GitHub CLI",
    "AppData/Roaming/gnupg",
];

/// Single files that hold tokens. Missing ones are skipped.
pub const SECRET_FILES: &[&str] = &[
    ".netrc",
    ".git-credentials",
    ".cargo/credentials",
    ".cargo/credentials.toml",
    ".pypirc",
];

/// The folders and files to hide, existing ones only, for this home and
/// config folder. The sign-in folder and the config folder are added to the
/// fixed lists.
pub fn hidden(home: Option<&Path>, config_dir: &Path, workspace: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let ws = workspace.canonicalize().ok();
    let mut push = |p: PathBuf| {
        let Ok(p) = p.canonicalize() else { return };
        // Never hide the workspace or anything above it: no work gets done.
        if ws.as_ref().is_some_and(|ws| ws.starts_with(&p)) || out.contains(&p) {
            return;
        }
        out.push(p);
    };
    if let Some(home) = home {
        for rel in SECRET_DIRS.iter().chain(SECRET_FILES) {
            push(home.join(rel));
        }
    }
    if let Some(sign_in) = grokhub_core::cli_history::grok_home() {
        push(sign_in);
    }
    push(config_dir.to_path_buf());
    // An agent's workspace is inside its home, inside the app's folder, so
    // neither folder can be hidden whole. Hide what's next to the way down
    // instead: the app's own files (the person's chats and memory), the
    // other agents' homes, and the agent's own files outside its workspace.
    if let Some((root, me)) = grokhub_core::agent_team::agent_turn(config_dir) {
        let entries = |dir: &Path| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect::<Vec<_>>()
        };
        let mine = grokhub_core::agent_team::home(&root, &me.id);
        for path in entries(&root) {
            if path.file_name() != Some(std::ffi::OsStr::new(grokhub_core::agent_team::HOMES_DIR)) {
                push(path);
                continue;
            }
            for other in entries(&path) {
                if Some(&other) == mine.as_ref() {
                    entries(&other).into_iter().for_each(&mut push);
                } else {
                    push(other);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_hidden_but_never_the_workspace_or_a_folder_above_it() {
        let home = std::env::temp_dir().join(format!("loadout-secrets-{}", std::process::id()));
        let ssh = home.join(".ssh");
        let keychains = home.join("Library/Keychains");
        let config = home.join(".config/loadout");
        for dir in [&ssh, &keychains, &config] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(home.join(".netrc"), "machine x").unwrap();
        let ws = home.join("src/app");
        std::fs::create_dir_all(&ws).unwrap();

        let got = hidden(Some(&home), &config, &ws);
        for want in [&ssh, &keychains, &config, &home.join(".netrc")] {
            let want = want.canonicalize().unwrap();
            assert!(
                got.contains(&want),
                "{} missing from {got:?}",
                want.display()
            );
        }
        // Missing ones are skipped.
        assert!(!got.iter().any(|p| p.ends_with(".aws")), "{got:?}");

        // A workspace inside a secret folder keeps it visible.
        let inside = ssh.join("work");
        std::fs::create_dir_all(&inside).unwrap();
        let got = hidden(Some(&home), &config, &inside);
        assert!(!got.contains(&ssh.canonicalize().unwrap()), "{got:?}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_agents_shell_sees_only_its_own_workspace_of_the_app_folder() {
        use grokhub_core::agent_team::{Kind, Profile, TeamFile, VERSION};
        let app = std::env::temp_dir().join(format!("loadout-agent-hide-{}", std::process::id()));
        let kid = app.join("agents/kid");
        let ws = kid.join("work");
        for dir in [
            &app.join("sessions"),
            &app.join("memory"),
            &app.join("agents/planner"),
            &kid.join("memory"),
            &ws,
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        TeamFile {
            version: VERSION,
            agents: vec![
                Profile::new(Kind::Family, "kid", "Kid"),
                Profile::new(Kind::Role, "planner", "Planner"),
            ],
            teams: vec![],
        }
        .save(&app)
        .unwrap();
        let canon = |p: &Path| p.canonicalize().unwrap();
        let got = hidden(None, &kid, &ws);
        for want in [
            app.join("sessions"),
            app.join("memory"),
            app.join(grokhub_core::agent_team::TEAM_FILE),
            app.join("agents/planner"),
            kid.join("memory"),
        ] {
            assert!(
                got.contains(&canon(&want)),
                "{} missing from {got:?}",
                want.display()
            );
        }
        // The way down to the workspace, and the workspace, stay.
        for keep in [&app, &app.join("agents"), &kid, &ws] {
            assert!(
                !got.contains(&canon(keep)),
                "{} hidden: {got:?}",
                keep.display()
            );
        }
        // The person's own shell hides the whole app folder, as before.
        let elsewhere = app
            .parent()
            .unwrap()
            .join(format!("loadout-ws-{}", std::process::id()));
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(hidden(None, &app, &elsewhere).last(), Some(&canon(&app)));
        let _ = std::fs::remove_dir_all(&app);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }
}
