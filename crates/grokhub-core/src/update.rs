use std::path::{Path, PathBuf};

/// Overlay update only. Never wipes `~/.config/GrokHub`.
pub fn is_grokhub_source(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && dir.join("scripts/install.sh").is_file()
        && dir.join("crates/grokhub-app").is_dir()
}

pub fn walk_up_source(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if is_grokhub_source(&cur) {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

pub fn discover_source(hints: &[PathBuf]) -> Option<PathBuf> {
    for h in hints {
        let p = h.as_path();
        if p.as_os_str().is_empty() {
            continue;
        }
        if is_grokhub_source(p) {
            return Some(p.to_path_buf());
        }
        if let Some(found) = walk_up_source(p) {
            return Some(found);
        }
    }
    None
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn update_cmds(source: &Path) -> Result<Vec<String>, String> {
    if !is_grokhub_source(source) {
        return Err("not a GrokHub source tree — set Settings → source or GROKHUB_SRC".into());
    }
    let src = sh_quote(&source.display().to_string());
    Ok(vec![
        format!("git -C {src} pull --ff-only"),
        format!("{src}/scripts/install.sh --user"),
    ])
}

pub fn update_wipes_config(cmds: &[String]) -> bool {
    cmds.iter().any(|c| {
        let l = c.to_ascii_lowercase();
        l.contains(".config/grokhub") && (l.contains("rm ") || l.contains("rm\t") || l.contains("rm -"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn source_and_overlay_plan() {
        let root = std::env::temp_dir().join(format!("grokhub-src-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        assert!(is_grokhub_source(&root));
        assert!(!is_grokhub_source(&std::env::temp_dir()));
        assert_eq!(walk_up_source(&root.join("crates/grokhub-app")), Some(root.clone()));
        assert_eq!(discover_source(&[root.join("crates")]), Some(root.clone()));
        let cmds = update_cmds(&root).unwrap();
        assert!(cmds[0].contains("git -C"));
        assert!(cmds[0].contains("pull --ff-only"));
        assert!(cmds[1].contains("/scripts/install.sh"));
        assert!(cmds[1].ends_with("--user"));
        assert!(cmds[0].contains(&format!("'{}'", root.display())));
        assert!(!update_wipes_config(&cmds));
        assert!(update_wipes_config(&[
            "rm -rf ~/.config/GrokHub".into()
        ]));
        assert!(update_cmds(Path::new("/tmp/not-grokhub")).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
