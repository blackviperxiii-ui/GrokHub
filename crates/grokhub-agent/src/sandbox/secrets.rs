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
/// fixed lists. The workspace and the folders above it are never hidden (no
/// work gets done): a secret folder that holds the workspace (a helper's
/// tree in `<config>/helper-work`) has everything else in it hidden, at
/// each level down to the workspace.
pub fn hidden(home: Option<&Path>, config_dir: &Path, workspace: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let ws = workspace.canonicalize().ok();
    let mut todo = std::collections::VecDeque::new();
    if let Some(home) = home {
        todo.extend(
            SECRET_DIRS
                .iter()
                .chain(SECRET_FILES)
                .map(|rel| home.join(rel)),
        );
    }
    todo.extend(grokhub_core::cli_history::grok_home());
    todo.push_back(config_dir.to_path_buf());
    // An agent's home sits inside the app's folder: the app's folder is
    // hidden too, except the way down to the agent's workspace (the person's
    // chats and memory and the other agents' homes are beside that way).
    todo.extend(grokhub_core::agent_team::agent_turn(config_dir).map(|(root, _)| root));
    while let Some(p) = todo.pop_front() {
        let Ok(p) = p.canonicalize() else { continue };
        if out.contains(&p) || ws.as_ref() == Some(&p) {
            continue;
        }
        match &ws {
            Some(ws) if ws.starts_with(&p) => {
                // Each entry beside the way down to the workspace.
                let entries = std::fs::read_dir(&p).into_iter().flatten().flatten();
                todo.extend(entries.map(|e| e.path()));
            }
            _ => out.push(p),
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

    /// A helper's tree sits inside the config folder: the folder stays
    /// open on the way down to the tree, and everything beside that way
    /// (the app's secrets, the ledger, other helpers' trees) is hidden.
    #[test]
    fn a_workspace_inside_a_secret_folder_hides_what_sits_beside_it() {
        let home =
            std::env::temp_dir().join(format!("grokhub-secrets-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let config = home.join(".config/GrokHub");
        let tree = config.join("helper-work/chat/k1");
        let other = config.join("helper-work/chat/k2");
        for dir in [&tree, &other, &config.join("routines"), &home.join(".ssh")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for file in [
            "secrets.json",
            "app.json",
            "hub-state.json",
            "consent.jsonl",
        ] {
            std::fs::write(config.join(file), "{}").unwrap();
        }
        std::fs::write(home.join(".ssh/id_rsa"), "x").unwrap();
        std::fs::write(config.join("helper-work/chat/work.json"), "{}").unwrap();
        std::fs::write(tree.join("main.rs"), "fn main() {}").unwrap();
        let real = |rel: &str| home.join(rel).canonicalize().unwrap();

        let mut got = hidden(Some(&home), &config, &tree);
        // The sign-in folder of whoever runs the test lies elsewhere.
        got.retain(|p| p.starts_with(real("")));
        got.sort();
        let mut want = vec![
            real(".ssh"),
            real(".config/GrokHub/app.json"),
            real(".config/GrokHub/consent.jsonl"),
            real(".config/GrokHub/helper-work/chat/k2"),
            real(".config/GrokHub/helper-work/chat/work.json"),
            real(".config/GrokHub/hub-state.json"),
            real(".config/GrokHub/routines"),
            real(".config/GrokHub/secrets.json"),
        ];
        want.sort();
        assert_eq!(got, want);
        let _ = std::fs::remove_dir_all(&home);
    }
}
