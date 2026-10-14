//! Host rails. YOLO skips the prompt, not these.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn contains_path_leaf(cmd: &str, leaf: &str) -> bool {
    let cmd = cmd.replace('\\', "/");
    // `+` splits python concat leftovers (`"/home/j/"+.ssh+"/id_rsa"`) so a
    // rebuilt leaf is still visible after chr() collapse.
    cmd.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '=' | ',' | ';' | '+'))
        .any(|tok| {
            let t = tok.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '(' | ')'));
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
                    && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'.' | b'_' | b'-'))
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

/// Copy one UTF-8 char at `i` onto `out` and advance `i`. Never reinterpret
/// multi-byte codepoints as Latin-1 — that made the echo-collapse fixpoint
/// loop forever on curly-quoted ask labels (`click “Reset zoom”`).
fn push_utf8_char(cmd: &str, i: &mut usize, out: &mut String) {
    let Some(ch) = cmd[*i..].chars().next() else {
        *i = cmd.len();
        return;
    };
    out.push(ch);
    *i += ch.len_utf8();
}

fn collapse_echo_dollar(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let lower = cmd.to_ascii_lowercase();
    let bytes = lower.as_bytes();
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
        push_utf8_char(cmd, &mut i, &mut out);
    }
    out
}

fn collapse_echo_backtick(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let lower = cmd.to_ascii_lowercase();
    let bytes = lower.as_bytes();
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
        push_utf8_char(cmd, &mut i, &mut out);
    }
    out
}

fn deobfuscate(cmd: &str) -> String {
    let mut s = cmd.to_ascii_lowercase().replace('\\', "/");
    s = expand_shell_assignments(&s);
    s = collapse_string_concats(&s);
    s = collapse_echo_subs(&s);
    s = collapse_chr_calls(&s);
    s
}

/// Collapse Python-style `chr(46)` / `chr(0x2e)` / `chr(0o56)` so leaf path checks
/// still fire. Adjacent `chr()` results joined with `+` are glued (`.ssh` rebuild).
fn collapse_chr_calls(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let lower = cmd.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"chr(" {
            let mut j = i + 4;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut base = 10u32;
            if j + 2 <= bytes.len() && bytes[j] == b'0' {
                match bytes[j + 1] {
                    b'x' | b'X' => {
                        base = 16;
                        j += 2;
                    }
                    b'o' | b'O' => {
                        base = 8;
                        j += 2;
                    }
                    _ => {}
                }
            }
            let num_start = j;
            while j < bytes.len()
                && match base {
                    16 => bytes[j].is_ascii_hexdigit(),
                    8 => matches!(bytes[j], b'0'..=b'7'),
                    _ => bytes[j].is_ascii_digit(),
                }
            {
                j += 1;
            }
            let num_end = j;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if num_end > num_start && j < bytes.len() && bytes[j] == b')' {
                if let Ok(n) = u32::from_str_radix(&lower[num_start..num_end], base) {
                    if let Some(ch) = char::from_u32(n) {
                        if ch.is_ascii() {
                            out.push(ch);
                            i = j + 1;
                            // Skip `+` between adjacent chr() so `.`+`s`+`s`+`h` → `.ssh`.
                            let mut k = i;
                            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                                k += 1;
                            }
                            if k < bytes.len() && bytes[k] == b'+' {
                                k += 1;
                                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                                    k += 1;
                                }
                                if k + 4 <= bytes.len() && &bytes[k..k + 4] == b"chr(" {
                                    i = k;
                                }
                            }
                            continue;
                        }
                    }
                }
            }
        }
        push_utf8_char(cmd, &mut i, &mut out);
    }
    out
}

/// `echo … | base64 -d | sh` (and decode|bash) rebuilds a forbidden leaf after rails.
/// Shell sinks are token-matched so `| sha256sum` / `|shuf` / `|shred` stay allowed.
fn base64_pipe_shell(cmd: &str) -> bool {
    let c = cmd.to_ascii_lowercase().replace('\\', "/");
    let has_b64 = c.contains("base64") || c.contains("b64decode");
    if !has_b64 {
        return false;
    }
    pipes_or_chains_to_shell(&c)
}

/// True when `|`, `|&`, `;`, newline, or `&&` is followed by a shell token
/// (`sh` / `bash` / `zsh`), optionally as a path basename (`/bin/sh`), after simple
/// quotes and common wrappers (`env` / `sudo` / `command` / `nice` / `nohup`),
/// skipping wrapper option flags (`-i`, `-n`, `-p`, `--…`). Word-boundary only —
/// never a prefix of `sha256sum`, `shuf`, `shred`, etc.
fn pipes_or_chains_to_shell(cmd: &str) -> bool {
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len()
            && bytes[i] == b'&'
            && bytes[i + 1] == b'&'
            && next_token_is_shell(&cmd[i + 2..])
        {
            return true;
        }
        if i + 1 < bytes.len() && bytes[i] == b'&' && bytes[i + 1] == b'&' {
            i += 2;
            continue;
        }
        // bash `|&` pipes stdout+stderr; skip the `&` so the shell sink is seen.
        if bytes[i] == b'|' {
            let after = if i + 1 < bytes.len() && bytes[i + 1] == b'&' {
                i + 2
            } else {
                i + 1
            };
            if next_token_is_shell(&cmd[after..]) {
                return true;
            }
            i = after;
            continue;
        }
        // `;` and newline are both command separators under `bash -lc`.
        if (bytes[i] == b';' || bytes[i] == b'\n' || bytes[i] == b'\r')
            && next_token_is_shell(&cmd[i + 1..])
        {
            return true;
        }
        i += 1;
    }
    false
}

/// Leading tokens that exec another command — peel them so `| env sh` / `| sudo bash`
/// still count as shell sinks.
const SHELL_WRAPPERS: &[&str] = &["env", "sudo", "command", "nice", "nohup"];

fn next_token_is_shell(rest: &str) -> bool {
    let mut rest = rest.trim_start();
    // Peel a short chain of wrappers (`env`, `sudo`, …) before the sink basename.
    for _ in 0..4 {
        if rest.is_empty() {
            return false;
        }
        let Some((tok, after)) = take_cmd_token(rest) else {
            return false;
        };
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if matches!(base, "sh" | "bash" | "zsh") {
            return true;
        }
        if !SHELL_WRAPPERS.contains(&base) {
            return false;
        }
        // `| env -i sh` / `| sudo -n sh` / `| command -p sh` — skip option flags.
        rest = skip_option_flags(after);
    }
    false
}

/// Drop leading `-…` / `--…` tokens after a wrapper so the sink basename is reached.
fn skip_option_flags(rest: &str) -> &str {
    let mut rest = rest.trim_start();
    for _ in 0..8 {
        let Some((tok, after)) = take_cmd_token(rest) else {
            break;
        };
        if tok.starts_with('-') {
            rest = after.trim_start();
            continue;
        }
        break;
    }
    rest
}

/// Next command token, stripping a matching pair of `'` / `"` when present.
/// Returns `(token, remainder)`.
fn take_cmd_token(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    let bytes = rest.as_bytes();
    if bytes[0] == b'\'' || bytes[0] == b'"' {
        let quote = bytes[0];
        let end = rest.as_bytes()[1..]
            .iter()
            .position(|&b| b == quote)
            .map(|i| i + 1)?;
        let tok = &rest[1..end];
        if tok.is_empty() {
            return None;
        }
        return Some((tok, &rest[end + 1..]));
    }
    let tok_end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '/' | '-' | '_'))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if tok_end == 0 {
        return None;
    }
    Some((&rest[..tok_end], &rest[tok_end..]))
}

pub fn forbidden_reason(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_ascii_lowercase().replace('\\', "/");
    if base64_pipe_shell(&c) {
        return Some("base64|shell rebuild");
    }
    if let Some(why) = forbidden_on(&c) {
        return Some(why);
    }
    let d = deobfuscate(cmd);
    if d != c {
        if base64_pipe_shell(&d) {
            return Some("base64|shell rebuild");
        }
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
        // Curly quotes are multi-byte UTF-8; the echo-collapse pass must not
        // reinterpret them as Latin-1 or the deobfuscate fixpoint never ends
        // (hung ask_cards_that_click_are_classified_by_the_quoted_label in CI).
        assert!(
            forbidden_reason("click \u{201c}Reset zoom\u{201d}").is_none(),
            "curly-quoted ask labels must stay allowed and finish"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(46)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "python chr() must not rebuild .ssh under the leaf check"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | sh"),
            Some("base64|shell rebuild"),
            "base64 decode piped to a shell rebuilds a forbidden leaf"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d |sh"),
            Some("base64|shell rebuild"),
            "tight |sh must still count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | /bin/sh"),
            Some("base64|shell rebuild"),
            "path-qualified | /bin/sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | /usr/bin/bash"),
            Some("base64|shell rebuild"),
            "path-qualified | /usr/bin/bash must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d && zsh"),
            Some("base64|shell rebuild"),
            "&& zsh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d ;zsh"),
            Some("base64|shell rebuild"),
            ";zsh must count as a shell sink"
        );
        assert!(
            forbidden_reason("echo hello | base64").is_none(),
            "encode-only base64 without a shell sink must stay allowed"
        );
        assert!(
            forbidden_reason("echo abc | base64 | sha256sum").is_none(),
            "| sha256sum must not false-positive as | sh"
        );
        assert!(
            forbidden_reason("echo abc | base64 |sha1sum").is_none(),
            "|sha1sum must not false-positive as |sh"
        );
        assert!(
            forbidden_reason("echo abc | base64 | shuf").is_none(),
            "| shuf must not false-positive as | sh"
        );
        assert!(
            forbidden_reason("echo abc | base64 | shred -u").is_none(),
            "| shred must not false-positive as | sh"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | 'sh'"),
            Some("base64|shell rebuild"),
            "single-quoted | 'sh' must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason(r#"echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | "sh""#),
            Some("base64|shell rebuild"),
            "double-quoted | \"sh\" must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env sh"),
            Some("base64|shell rebuild"),
            "| env sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | sudo sh"),
            Some("base64|shell rebuild"),
            "| sudo sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | command sh"),
            Some("base64|shell rebuild"),
            "| command sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d |& sh"),
            Some("base64|shell rebuild"),
            "|& sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | nice /bin/bash"),
            Some("base64|shell rebuild"),
            "| nice /bin/bash must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | nohup zsh"),
            Some("base64|shell rebuild"),
            "| nohup zsh must count as a shell sink"
        );
        assert!(
            forbidden_reason("echo abc | base64 | env sha256sum").is_none(),
            "| env sha256sum must stay allowed"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env -i sh"),
            Some("base64|shell rebuild"),
            "| env -i sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | sudo -n sh"),
            Some("base64|shell rebuild"),
            "| sudo -n sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | command -p sh"),
            Some("base64|shell rebuild"),
            "| command -p sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d\nsh"),
            Some("base64|shell rebuild"),
            "newline before sh must count as a command separator"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0o56)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "python octal chr(0o56) must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason(
                r#"python3 -c 'open("/home/j/"+chr(46)+chr(115)+chr(115)+chr(104)+"/id_rsa").read()'"#
            ),
            Some("forbidden path: ssh keys"),
            "adjacent chr() joined with + must rebuild .ssh"
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
