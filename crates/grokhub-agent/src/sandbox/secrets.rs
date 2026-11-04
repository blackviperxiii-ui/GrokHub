//! The folders and files a sandboxed command never gets to read: keys,
//! cloud and registry credentials, browser profiles, the sign-in folder and
//! Loadout's config folder. Linux covers them with an empty folder or
//! `/dev/null` in the command's own mount namespace ([`super::netns`]);
//! macOS denies them in the command's Seatbelt profile ([`super::seatbelt`]).

use std::path::{Path, PathBuf};

/// Folders under the home folder whose contents a sandboxed command never
/// needs and a leak of which is costly. Missing ones are skipped, so the
/// macOS folders cost nothing on Linux and the other way round.
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
}
