//! Host rails. YOLO skips the prompt, not these.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn contains_path_leaf(cmd: &str, leaf: &str) -> bool {
    let cmd = cmd.replace('\\', "/");
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

const FORBIDDEN_LEAVES: &[(&str, &str)] = &[
    (".ssh", "forbidden path: ssh keys"),
    (".gnupg", "forbidden path: gnupg"),
    (".aws", "forbidden path: aws credentials"),
    (".kube", "forbidden path: kube config"),
    ("app.json", "forbidden path: app secrets"),
    ("secrets.json", "forbidden path: app secrets"),
    ("hub-state.json", "forbidden path: hub pair tokens"),
];

fn forbidden_on(c: &str) -> Option<&'static str> {
    if c.contains("/etc/shadow") {
        return Some("forbidden path: /etc/shadow");
    }
    if c.contains("/etc/sudoers") {
        return Some("forbidden path: /etc/sudoers");
    }
    if c.contains(".grok/auth.json") {
        return Some("forbidden path: grok login");
    }
    for (leaf, why) in FORBIDDEN_LEAVES {
        if contains_path_leaf(c, leaf) {
            return Some(*why);
        }
    }
    None
}

/// Expand simple `name=value` assignments into `$name` / `${name}` so
/// `s=ssh; cat ~/.$s/id_rsa` still trips the `.ssh` leaf.
fn expand_shell_assignments(cmd: &str) -> String {
    let mut assigns: Vec<(String, String)> = Vec::new();
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'=' {
                let name = &cmd[start..i];
                i += 1;
                let quote = bytes.get(i).copied().filter(|b| *b == b'"' || *b == b'\'');
                let val_start = if quote.is_some() { i + 1 } else { i };
                let mut j = val_start;
                if let Some(q) = quote {
                    while j < bytes.len() && bytes[j] != q {
                        j += 1;
                    }
                    let value = cmd[val_start..j].to_string();
                    if !name.is_empty()
                        && value
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
                    {
                        assigns.push((name.to_string(), value));
                    }
                    i = if j < bytes.len() { j + 1 } else { j };
                    continue;
                }
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric()
                        || matches!(bytes[j], b'.' | b'_' | b'-'))
                {
                    j += 1;
                }
                if j > val_start {
                    assigns.push((name.to_string(), cmd[val_start..j].to_string()));
                }
                i = j;
                continue;
            }
        }
        i = start + 1;
    }
    assigns.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
    let mut out = cmd.to_string();
    for (name, value) in assigns {
        out = out.replace(&format!("${{{name}}}"), &value);
        // Replace `$name` only when the next char is not an identifier char.
        let needle = format!("${name}");
        let mut rebuilt = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(at) = rest.find(&needle) {
            rebuilt.push_str(&rest[..at]);
            let after = &rest[at + needle.len()..];
            let next_id = after
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            if next_id {
                rebuilt.push_str(&needle);
            } else {
                rebuilt.push_str(&value);
            }
            rest = after;
        }
        rebuilt.push_str(rest);
        out = rebuilt;
    }
    out
}

/// Join adjacent quoted fragments glued with `+` (Python / JS style).
fn collapse_string_concats(cmd: &str) -> String {
    let mut s = cmd.to_string();
    loop {
        let before = s.clone();
        s = collapse_one_quote_concat(&s, '"');
        s = collapse_one_quote_concat(&s, '\'');
        if s == before {
            break;
        }
    }
    s
}

fn collapse_one_quote_concat(cmd: &str, q: char) -> String {
    let mut out = String::with_capacity(cmd.len());
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == q {
            let mut piece = String::new();
            i += 1;
            while i < chars.len() && chars[i] != q {
                piece.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // closing quote
            }
            // Absorb further `+ "…"` / `+ '…'` with the same quote.
            loop {
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j >= chars.len() || chars[j] != '+' {
                    break;
                }
                j += 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j >= chars.len() || chars[j] != q {
                    break;
                }
                j += 1;
                while j < chars.len() && chars[j] != q {
                    piece.push(chars[j]);
                    j += 1;
                }
                if j >= chars.len() {
                    break;
                }
                j += 1; // closing
                i = j;
            }
            out.push(q);
            out.push_str(&piece);
            out.push(q);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Collapse `$(echo word)` and `` `echo word` `` so split leaves rejoin.
fn collapse_echo_subs(cmd: &str) -> String {
    let mut s = cmd.to_string();
    loop {
        let before = s.clone();
        s = collapse_echo_dollar(&s);
        s = collapse_echo_backtick(&s);
        if s == before {
            break;
        }
    }
    s
}

fn collapse_echo_dollar(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let lower = cmd.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let orig = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j + 4 <= bytes.len() && &bytes[j..j + 4] == b"echo" {
                j += 4;
                if j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let word_start = j;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || matches!(bytes[j], b'.' | b'_' | b'-'))
                    {
                        j += 1;
                    }
                    let word_end = j;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if word_end > word_start && j < bytes.len() && bytes[j] == b')' {
                        out.push_str(&cmd[word_start..word_end]);
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(orig[i] as char);
        i += 1;
    }
    out
}

fn collapse_echo_backtick(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let lower = cmd.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let orig = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j + 4 <= bytes.len() && &bytes[j..j + 4] == b"echo" {
                j += 4;
                if j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let word_start = j;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || matches!(bytes[j], b'.' | b'_' | b'-'))
                    {
                        j += 1;
                    }
                    let word_end = j;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if word_end > word_start && j < bytes.len() && bytes[j] == b'`' {
                        out.push_str(&cmd[word_start..word_end]);
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(orig[i] as char);
        i += 1;
    }
    out
}

fn deobfuscate(cmd: &str) -> String {
    let mut s = cmd.to_ascii_lowercase().replace('\\', "/");
    s = expand_shell_assignments(&s);
    s = collapse_string_concats(&s);
    s = collapse_echo_subs(&s);
    s
}

pub fn forbidden_reason(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_ascii_lowercase().replace('\\', "/");
    if let Some(why) = forbidden_on(&c) {
        return Some(why);
    }
    let d = deobfuscate(cmd);
    if d != c {
        if let Some(why) = forbidden_on(&d) {
            return Some(why);
        }
    }
    None
}

/// Each host job gets its own cancel flag so a later `/sh` cannot un-halt the previous one.
pub fn mint_host_halt() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
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
        assert!(forbidden_reason("cat ~/.config/GrokHub/secrets.json").is_some());
        assert!(forbidden_reason("cat $HOME/.config/GrokHub/secrets.json").is_some());
        assert!(forbidden_reason(r"type %APPDATA%\GrokHub\secrets.json").is_some());
        assert!(forbidden_reason(r"Get-Content $env:APPDATA\GrokHub\secrets.json").is_some());
        assert!(forbidden_reason(r"type ~\.grok\auth.json").is_some());
        assert!(forbidden_reason("cat ~/.grok/auth.json").is_some());
        assert!(forbidden_reason("cat ~/.config/GrokHub/hub-state.json").is_some());
        assert!(forbidden_reason("cat ~/.aws/credentials").is_some());
        assert!(forbidden_reason("cat ~/.kube/config").is_some());
        assert!(
            forbidden_reason("CAT /ETC/SHADOW").is_some(),
            "path rails are case-insensitive"
        );
        assert!(forbidden_reason("cat /etc/passwd").is_none());
        assert!(
            forbidden_reason("cp -a '/home/j/.config/GrokHub/rewind/rw1/.' '/home/j/proj'")
                .is_none(),
            "cabin rewind copies must still run"
        );
        assert!(
            forbidden_reason("cat my.gnupg_backup/file").is_none(),
            "unrelated names that contain .gnupg must not trip the rail"
        );
    }

    #[test]
    fn blocks_common_obfuscations_that_rebuild_forbidden_leaves() {
        assert_eq!(
            forbidden_reason("s=ssh; cat ~/.$s/id_rsa"),
            Some("forbidden path: ssh keys")
        );
        assert_eq!(
            forbidden_reason(r#"python3 -c 'open("/home/j/.s"+"sh/id_rsa").read()'"#),
            Some("forbidden path: ssh keys")
        );
        assert_eq!(
            forbidden_reason("cat $(echo $HOME)/.s$(echo sh)/id_rsa"),
            Some("forbidden path: ssh keys")
        );
        assert_eq!(
            forbidden_reason("g=gnupg; ls ~/.$g"),
            Some("forbidden path: gnupg")
        );
        assert!(
            forbidden_reason("echo hello; ls /tmp").is_none(),
            "plain commands must stay allowed"
        );
        assert!(
            forbidden_reason("name=ssh; echo $name").is_none(),
            "mentioning ssh as a value without a path leaf must not trip"
        );
    }

    #[test]
    fn new_host_job_does_not_unhalt_the_previous() {
        use std::sync::atomic::Ordering;
        let prev = mint_host_halt();
        prev.store(true, Ordering::SeqCst);
        let next = mint_host_halt();
        assert!(
            prev.load(Ordering::SeqCst),
            "minting a new flag must leave the halted job cancelled"
        );
        assert!(
            !next.load(Ordering::SeqCst),
            "the new host job must start unhalted"
        );
        assert!(!std::sync::Arc::ptr_eq(&prev, &next));
    }

    #[test]
    fn recall_substring() {
        let hits = recall_hits(
            "nvim",
            &[
                ("USER.md", "editor: nvim\nshell: zsh"),
                ("MEMORY.md", "no match"),
            ],
        );
        assert_eq!(hits, vec!["USER.md:1: editor: nvim"]);
    }
}
