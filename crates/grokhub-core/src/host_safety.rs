//! Host rails. YOLO skips the prompt, not these.

fn contains_path_leaf(cmd: &str, leaf: &str) -> bool {
    cmd.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '=' | ',' | ';'))
        .any(|tok| {
            let t = tok.trim_matches(|ch: char| matches!(ch, '"' | '\''));
            t == leaf
                || t.ends_with(&format!("/{leaf}"))
                || t.starts_with(&format!("{leaf}/"))
                || t.contains(&format!("/{leaf}/"))
                || t == format!("~/{leaf}")
                || t.starts_with(&format!("~/{leaf}/"))
        })
}

pub fn forbidden_reason(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_ascii_lowercase();
    if c.contains("/etc/shadow") {
        return Some("forbidden path: /etc/shadow");
    }
    if c.contains("/etc/sudoers") {
        return Some("forbidden path: /etc/sudoers");
    }
    if contains_path_leaf(&c, ".ssh") {
        return Some("forbidden path: ssh keys");
    }
    if contains_path_leaf(&c, ".gnupg") {
        return Some("forbidden path: gnupg");
    }
    if contains_path_leaf(&c, "app.json") {
        return Some("forbidden path: app secrets");
    }
    None
}

pub fn recall_hits(query: &str, corpus: &[(&str, &str)]) -> Vec<String> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut out = vec![];
    for (name, body) in corpus {
        for (i, line) in body.lines().enumerate() {
            if line.to_ascii_lowercase().contains(&q) {
                out.push(format!("{name}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    out.truncate(20);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_shadow_and_ssh() {
        assert!(forbidden_reason("cat /etc/shadow").is_some());
        assert!(forbidden_reason("cat ~/.ssh/id_ed25519").is_some());
        assert!(forbidden_reason("cat .ssh/id_ed25519").is_some());
        assert!(forbidden_reason("ls /home/j/.ssh").is_some());
        assert!(forbidden_reason("cd /home/j/.ssh").is_some());
        assert!(forbidden_reason("cat .gnupg/pubring.kbx").is_some());
        assert!(forbidden_reason("ls /tmp").is_none());
    }

    #[test]
    fn yolo_does_not_lift_forbidden() {
        // caller must still check forbidden_reason when yolo is true
        assert!(forbidden_reason("rm ~/.ssh/id_rsa").is_some());
        assert!(forbidden_reason("cat /etc/sudoers").is_some());
        assert!(forbidden_reason("ls ~/.gnupg").is_some());
        assert!(forbidden_reason("cat ~/.config/GrokHub/app.json").is_some());
        assert!(
            forbidden_reason("CAT /ETC/SHADOW").is_some(),
            "path rails are case-insensitive"
        );
        assert!(forbidden_reason("cat /etc/passwd").is_none());
        assert!(
            forbidden_reason("cat my.gnupg_backup/file").is_none(),
            "unrelated names that contain .gnupg must not trip the rail"
        );
    }

    #[test]
    fn recall_substring() {
        let hits = recall_hits(
            "nvim",
            &[("USER.md", "editor: nvim\nshell: zsh"), ("MEMORY.md", "no match")],
        );
        assert_eq!(hits, vec!["USER.md:1: editor: nvim"]);
    }
}
