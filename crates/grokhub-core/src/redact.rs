const PATTERNS: &[(&str, &str)] = &[
    ("sk-", "[redacted]"),
    ("xai-", "[redacted]"),
    ("ghp_", "[redacted]"),
    ("github_pat_", "[redacted]"),
    ("gho_", "[redacted]"),
    ("ghu_", "[redacted]"),
    ("ghs_", "[redacted]"),
    ("Bearer ", "Bearer [redacted]"),
    ("bearer ", "bearer [redacted]"),
];

/// Drop values the user just typed into a secret elicit. Short strings stay,
/// so a one-letter field cannot blank the transcript.
pub fn redact_held_secrets(input: &str, secrets: &[String]) -> String {
    let mut s = input.to_string();
    for secret in secrets {
        let t = secret.trim();
        if t.chars().count() < 4 {
            continue;
        }
        if s.contains(t) {
            s = s.replace(t, "[redacted]");
        }
    }
    s
}

pub fn redact_secrets(input: &str) -> String {
    redact_patterns(input, false)
}

/// [`redact_secrets`] for text that names files and windows: a key prefix
/// counts only at the start of a word, so `/tmp/grokhub-desk-notes` and
/// `task-runner` stay readable on a card.
pub fn redact_secret_words(input: &str) -> String {
    redact_patterns(input, true)
}

fn redact_patterns(input: &str, word_start: bool) -> String {
    let mut s = input.to_string();
    for (needle, _) in PATTERNS {
        let mut out = String::new();
        let mut rest = s.as_str();
        loop {
            let Some(idx) = rest.find(needle) else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..idx]);
            let after = &rest[idx + needle.len()..];
            if word_start
                && out
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                out.push_str(needle);
                rest = after;
                continue;
            }
            let n = after
                .chars()
                .take_while(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(*c, '-' | '_' | '.' | '~' | '+' | '/' | '=')
                })
                .count();
            if n < 12 {
                out.push_str(needle);
                rest = after;
                continue;
            }
            out.push_str("[redacted]");
            let bytes: usize = after.chars().take(n).map(|c| c.len_utf8()).sum();
            rest = &after[bytes..];
        }
        s = out;
    }
    s
}

/// A bare token with no `KEY=` in front: a prefix that starts a word, then at
/// least `min` characters that pass `ok`.
struct TokenShape {
    prefix: &'static str,
    min: usize,
    ok: fn(char) -> bool,
}

const TOKEN_SHAPES: &[TokenShape] = &[
    TokenShape {
        prefix: "AKIA",
        min: 16,
        ok: is_upper_alnum,
    },
    TokenShape {
        prefix: "ASIA",
        min: 16,
        ok: is_upper_alnum,
    },
    TokenShape {
        prefix: "xoxb-",
        min: 10,
        ok: is_slack_char,
    },
    TokenShape {
        prefix: "xoxp-",
        min: 10,
        ok: is_slack_char,
    },
    TokenShape {
        prefix: "xapp-",
        min: 10,
        ok: is_slack_char,
    },
    TokenShape {
        prefix: "sk_live_",
        min: 10,
        ok: is_alnum,
    },
    TokenShape {
        prefix: "rk_live_",
        min: 10,
        ok: is_alnum,
    },
    TokenShape {
        prefix: "sk_test_",
        min: 10,
        ok: is_alnum,
    },
    TokenShape {
        prefix: "rk_test_",
        min: 10,
        ok: is_alnum,
    },
    TokenShape {
        prefix: "AIza",
        min: 30,
        ok: is_url_b64,
    },
    TokenShape {
        prefix: "ntn_",
        min: 20,
        ok: is_alnum,
    },
    TokenShape {
        prefix: "secret_",
        min: 20,
        ok: is_alnum,
    },
];

fn is_upper_alnum(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_slack_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn is_url_b64(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// Redact keys that [`redact_secrets`] does not know, for exports that leave
/// the app: AWS access keys, Slack, Stripe, Google API and Notion tokens,
/// JWTs, passwords in URLs (`scheme://user:pass@host`) and PEM private key
/// blocks. Ordinary words and paths stay.
pub fn redact_token_shapes(input: &str) -> String {
    let s = redact_pem_private_keys(input);
    let s = redact_url_passwords(&s);
    let s = redact_jwts(&s);
    let mut s = s;
    for shape in TOKEN_SHAPES {
        s = redact_prefixed(&s, shape);
    }
    s
}

fn redact_prefixed(input: &str, shape: &TokenShape) -> String {
    let TokenShape { prefix, min, ok } = *shape;
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + prefix.len()..];
        let n = after.chars().take_while(|c| ok(*c)).count();
        let starts_word = !out.chars().next_back().is_some_and(is_word_char);
        if !starts_word || n < min {
            out.push_str(prefix);
            rest = after;
            continue;
        }
        out.push_str("[redacted]");
        rest = &after[after.chars().take(n).map(char::len_utf8).sum::<usize>()..];
    }
    out.push_str(rest);
    out
}

/// `eyJ…` header, payload and signature, each base64url, joined by dots.
fn redact_jwts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("eyJ") {
        out.push_str(&rest[..idx]);
        let cand = &rest[idx..];
        let parts: Vec<usize> = cand
            .splitn(4, '.')
            .take(3)
            .map(|p| p.chars().take_while(|c| is_url_b64(*c)).count())
            .collect();
        let whole = parts.len() == 3
            && parts[0] >= 10
            && parts[1] >= 4
            && parts[2] >= 4
            && cand.split('.').take(2).all(|p| p.chars().all(is_url_b64));
        let starts_word = !out.chars().next_back().is_some_and(is_word_char);
        if !(whole && starts_word) {
            out.push_str("eyJ");
            rest = &cand[3..];
            continue;
        }
        out.push_str("[redacted]");
        rest = &cand[parts[0] + parts[1] + parts[2] + 2..];
    }
    out.push_str(rest);
    out
}

/// `scheme://user:pass@host` keeps the user and host and drops the password.
fn redact_url_passwords(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("://") {
        out.push_str(&rest[..idx + 3]);
        let after = &rest[idx + 3..];
        let auth_end = after
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\'' | '<' | '>')
            })
            .unwrap_or(after.len());
        let authority = &after[..auth_end];
        if let Some(at) = authority.rfind('@') {
            if let Some(colon) = authority[..at].find(':') {
                if colon + 1 < at && &authority[colon + 1..at] != "[redacted]" {
                    out.push_str(&authority[..colon + 1]);
                    out.push_str("[redacted]");
                    rest = &after[at..];
                    continue;
                }
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// A whole `-----BEGIN … PRIVATE KEY-----` block through its END line; with
/// no END line, everything after BEGIN.
fn redact_pem_private_keys(input: &str) -> String {
    const BEGIN: &str = "-----BEGIN ";
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(BEGIN) {
        let head = &rest[idx..];
        let line_end = head.find('\n').unwrap_or(head.len());
        if !head[..line_end].contains("PRIVATE KEY-----") {
            out.push_str(&rest[..idx + BEGIN.len()]);
            rest = &rest[idx + BEGIN.len()..];
            continue;
        }
        out.push_str(&rest[..idx]);
        out.push_str("[redacted private key]");
        rest = match head.find("-----END ") {
            Some(end) => {
                let tail = &head[end + "-----END ".len()..];
                match tail.find("-----") {
                    Some(close) => &tail[close + 5..],
                    None => "",
                }
            }
            None => "",
        };
    }
    out.push_str(rest);
    out
}

pub fn is_plain_text(s: &str) -> bool {
    redact_secrets(s) == s
}

/// `/forget pi` must not eat "principle". A topic matches on word boundaries.
pub fn mentions_topic(line: &str, topic: &str) -> bool {
    let needle = topic.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    let hay = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = hay[from..].find(&needle) {
        let start = from + i;
        let end = start + needle.len();
        let before_ok = hay[..start]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        let after_ok = hay[end..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        from = (start + 1).min(hay.len());
        if from >= hay.len() {
            break;
        }
    }
    false
}

pub fn forget_topic(markdown: &str, topic: &str) -> String {
    let t = topic.trim().to_ascii_lowercase();
    if t.is_empty() {
        return markdown.to_string();
    }
    let mut out = String::new();
    for line in markdown.lines() {
        if mentions_topic(line, &t) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_secret_never_lands_in_the_transcript() {
        let value = "ghp_super-secret-token-value".to_string();
        let echoed = format!("CONNECTOR_RESULT (facts only):\nused {value} then ok");
        let scrubbed = redact_held_secrets(&echoed, std::slice::from_ref(&value));
        assert!(!scrubbed.contains(&value));
        assert!(scrubbed.contains("[redacted]"));
        assert!(scrubbed.contains("CONNECTOR_RESULT"));
        let short = redact_held_secrets("the cat sat", &["the".into()]);
        assert_eq!(short, "the cat sat");
    }

    #[test]
    fn redacts_sk() {
        let out = redact_secrets("token sk-abcdefghijklmnopqrstuv");
        assert!(!out.contains("sk-abcdefghijklmnopqrstuv"));
        let kept = forget_topic("editor: nvim\nwifi printer in den\n", "wifi");
        assert!(kept.contains("nvim"));
        assert!(!kept.contains("wifi"));
    }

    #[test]
    fn forget_takes_the_topic_not_every_word_that_contains_it() {
        let notes =
            "pi is the raspberry pi in the den\nprinciple: keep it small\npicture day is friday\n";
        let out = forget_topic(notes, "pi");
        assert!(!out.contains("raspberry"), "the topic line goes: {out}");
        assert!(
            out.contains("principle") && out.contains("picture"),
            "a substring of a longer word is not the topic: {out}"
        );
        assert!(mentions_topic(
            "Wi-Fi password is taped to the router",
            "wi-fi"
        ));
        assert!(mentions_topic("the wifi.", "wifi"));
        assert!(mentions_topic("wifi-extender in the den", "extender in"));
        assert!(!mentions_topic("wifi-extender in the den", "tender"));
        assert!(!mentions_topic("nothing here", "pi"));
        assert!(!mentions_topic("anything", "  "));
        assert_eq!(
            forget_topic("keep me\n", "   "),
            "keep me\n",
            "an empty topic must not rewrite the file"
        );
    }

    #[test]
    fn redacts_xai_github_and_bearer() {
        let xai = redact_secrets("key xai-abcdefghijklmnopqrstuv");
        assert!(!xai.contains("xai-abcdefghijklmnopqrstuv"), "{xai}");
        let ghp = redact_secrets("export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuv");
        assert!(!ghp.contains("ghp_abcdefghijklmnopqrstuv"), "{ghp}");
        let pat = redact_secrets("token github_pat_11ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        assert!(!pat.contains("github_pat_"), "{pat}");
        let bearer = redact_secrets("Authorization: Bearer abcdefghijklmnop");
        assert!(!bearer.contains("abcdefghijklmnop"), "{bearer}");
        assert_eq!(
            redact_secrets("authorization: bearer abcdefghijklmnop"),
            "authorization: [redacted]"
        );
        assert_eq!(
            redact_secret_words(
                "rm /tmp/grokhub-desk-delete-deny-0/a.txt; echo sk-abcdefghijklmnopqrstuv"
            ),
            "rm /tmp/grokhub-desk-delete-deny-0/a.txt; echo [redacted]"
        );
        assert_eq!(
            redact_secret_words("KEY=sk-abcdefghijklmnopqrstuv \"ghp_abcdefghijklmnopqrstuvwx\""),
            "KEY=[redacted] \"[redacted]\""
        );
        assert!(
            is_plain_text("sk-short"),
            "short tokens stay visible so ordinary words are not eaten"
        );
        assert!(is_plain_text("xai-tiny"));
        let two = redact_secrets("sk-abcdefghijklmnopqrstuv and sk-zyxwvutsrqponmlkjih");
        assert!(!two.contains("sk-"), "{two}");
        assert_eq!(two.matches("[redacted]").count(), 2);
        let mixed = redact_secrets("sk-short sk-abcdefghijklmnopqrstuv");
        assert!(
            !mixed.contains("sk-abcdefghijklmnopqrstuv"),
            "a short sk- prefix must not hide the real key: {mixed}"
        );
        assert!(mixed.contains("sk-short"), "{mixed}");
    }

    #[test]
    fn bare_token_shapes_are_redacted_and_words_stay() {
        assert_eq!(
            redact_token_shapes(
                "aws AKIAFAKEFAKEFAKE0000 slack xoxb-0000-FAKE-fake-token \
                 stripe sk_live_FAKEFAKE0000 google AIzaFAKEFAKEFAKEFAKEFAKEFAKEFAKEFA \
                 notion ntn_FAKEFAKEFAKEFAKEFAKE secret_FAKEFAKEFAKEFAKEFAKE"
            ),
            "aws [redacted] slack [redacted] \
                 stripe [redacted] google [redacted] \
                 notion [redacted] [redacted]"
        );
        assert_eq!(
            redact_token_shapes("jwt eyJhbGciOiJub25lIn0.eyJzdWIiOiJ0ZXN0In0.ZmFrZXNpZw."),
            "jwt [redacted]."
        );
        assert_eq!(
            redact_token_shapes("git https://app:hunter2fake@example.com/repo.git ok"),
            "git https://app:[redacted]@example.com/repo.git ok"
        );
        assert_eq!(
            redact_token_shapes(
                "a\n-----BEGIN RSA PRIVATE KEY-----\nnot-a-real-key\n-----END RSA PRIVATE KEY-----\nb"
            ),
            "a\n[redacted private key]\nb"
        );
        assert_eq!(
            redact_token_shapes("-----BEGIN PRIVATE KEY-----\ncut off"),
            "[redacted private key]"
        );
        let kept = "path /srv/secret_sauce_recipe and MYAKIAFAKEFAKEFAKE0000, \
                    AKIA short, http://example.com:8080/x, user@example.com, \
                    -----BEGIN CERTIFICATE----- eyJnot.a.jwt";
        assert_eq!(redact_token_shapes(kept), kept);
    }
}
