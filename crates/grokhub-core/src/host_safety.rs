//! Host rails. YOLO skips the prompt, not these.

pub fn forbidden_reason(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_ascii_lowercase();
    let needles = [
        ("/etc/shadow", "forbidden path: /etc/shadow"),
        ("/etc/sudoers", "forbidden path: /etc/sudoers"),
        ("/.ssh/", "forbidden path: ssh keys"),
        ("/.ssh", "forbidden path: ssh keys"),
        ("~/.ssh", "forbidden path: ssh keys"),
        (".ssh/", "forbidden path: ssh keys"),
        ("/.gnupg", "forbidden path: gnupg"),
        (".gnupg", "forbidden path: gnupg"),
        ("app.json", "forbidden path: app secrets"),
    ];
    for (n, why) in needles {
        if c.contains(n) {
            return Some(why);
        }
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
