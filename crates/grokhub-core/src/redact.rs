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

/// True when the app's redactors would change `text`: it has a key, a token
/// or a password in it. Three redactors are asked, and each counts a key
/// only where one can start, so a branch named `fix/desk-sessions-halt` or a
/// file named `task-runner-notes.md` is not taken for a key:
/// - the one the Code tab's terminal uses for typed commands (`sk-…`,
///   `xai-…`, GitHub tokens, `Bearer …`);
/// - the debug bundle's token shapes (AWS, Slack, Hugging Face, GitLab, npm
///   and the like, a password in a URL, a JWT);
/// - the debug bundle's credential assignments: `API_KEY=…`,
///   `--password=…`, an `Authorization` header. This one goes by the key's
///   name, so `TOKEN=abc make` counts too.
pub fn holds_secret(text: &str) -> bool {
    redact_secret_words(text) != text
        || redact_token_shapes(text) != text
        || crate::diagnostics::has_credential_value(text)
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
/// least `min` characters that pass `ok`. Where a long hyphenated file name
/// fits that too, `mixed` asks for what a key has and such a name does not:
/// a digit or a capital in the body.
struct TokenShape {
    prefix: &'static str,
    min: usize,
    ok: fn(char) -> bool,
    mixed: bool,
}

const TOKEN_SHAPES: &[TokenShape] = &[
    TokenShape {
        prefix: "AKIA",
        min: 16,
        ok: is_upper_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "ASIA",
        min: 16,
        ok: is_upper_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "xoxb-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "xoxp-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "xapp-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "sk_live_",
        min: 10,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "rk_live_",
        min: 10,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "sk_test_",
        min: 10,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "rk_test_",
        min: 10,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "AIza",
        min: 30,
        ok: is_url_b64,
        mixed: false,
    },
    TokenShape {
        prefix: "ntn_",
        min: 20,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "secret_",
        min: 20,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "hf_",
        min: 30,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "glpat-",
        min: 20,
        ok: is_url_b64_dot,
        mixed: true,
    },
    TokenShape {
        prefix: "npm_",
        min: 36,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "xoxa-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "xoxr-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "xoxs-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "xoxe-",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    // With rotation on, an access token is `xoxe.xoxp-…`, `xoxe.xoxb-…` or
    // `xoxe.xapp-…`. The `.` keeps the inner prefix from starting a word, so
    // `xoxe.` is the shape and the inner prefix is part of the body.
    TokenShape {
        prefix: "xoxe.",
        min: 10,
        ok: is_slack_char,
        mixed: false,
    },
    TokenShape {
        prefix: "whsec_",
        min: 24,
        ok: is_b64,
        mixed: false,
    },
    TokenShape {
        prefix: "ghr_",
        min: 30,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "gsk_",
        min: 30,
        ok: is_alnum,
        mixed: false,
    },
    TokenShape {
        prefix: "pypi-",
        min: 30,
        ok: is_url_b64,
        mixed: true,
    },
    TokenShape {
        prefix: "GOCSPX-",
        min: 20,
        ok: is_url_b64,
        mixed: false,
    },
    TokenShape {
        prefix: "ya29.",
        min: 30,
        ok: is_url_b64_dot,
        mixed: false,
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

fn is_url_b64_dot(c: char) -> bool {
    is_url_b64(c) || c == '.'
}

/// Standard base64, padding included.
fn is_b64(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// Redact keys that [`redact_secrets`] does not know, for exports that leave
/// the app: AWS access keys, Slack, Stripe, Google, Notion, Hugging Face,
/// GitLab, npm, PyPI, Groq and GitHub refresh tokens, JWTs, passwords in
/// URLs (`scheme://user:pass@host`) and PEM private key blocks. Ordinary
/// words and paths stay.
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
    let TokenShape {
        prefix,
        min,
        ok,
        mixed,
    } = *shape;
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + prefix.len()..];
        // The body is measured only where a key could start. Measured after
        // every prefix inside one long run, it costs the run's length squared.
        let starts_word = !out.chars().next_back().is_some_and(is_word_char);
        let n = after.chars().take_while(|c| starts_word && ok(*c)).count();
        let body = &after[..after.chars().take(n).map(char::len_utf8).sum::<usize>()];
        let name = mixed
            && !body
                .chars()
                .any(|c| c.is_ascii_digit() || c.is_ascii_uppercase());
        if !starts_word || n < min || name {
            out.push_str(prefix);
            rest = after;
            continue;
        }
        out.push_str("[redacted]");
        rest = &after[body.len()..];
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
        // Measured only where a token could start, and no further than its
        // own characters run: looking for the dots after every `eyJ` costs
        // the length of what follows each time.
        let starts_word = !out.chars().next_back().is_some_and(is_word_char);
        let Some(len) = starts_word.then(|| jwt_len(cand)).flatten() else {
            out.push_str("eyJ");
            rest = &cand[3..];
            continue;
        };
        out.push_str("[redacted]");
        rest = &cand[len..];
    }
    out.push_str(rest);
    out
}

/// How long the JWT at the start of `s` is: base64url runs of at least 10, 4
/// and 4 characters, with a dot right after the first and the second.
fn jwt_len(s: &str) -> Option<usize> {
    let run = |s: &str| s.chars().take_while(|c| is_url_b64(*c)).count();
    let head = run(s);
    let after_head = s[head..].strip_prefix('.')?;
    let payload = run(after_head);
    let sig = run(after_head[payload..].strip_prefix('.')?);
    (head >= 10 && payload >= 4 && sig >= 4).then_some(head + payload + sig + 2)
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

/// A whole `-----BEGIN <label>-----` private key block through its matching
/// `-----END <label>-----` line; with no matching END line, everything after
/// BEGIN. Another END line inside the block does not close it.
fn redact_pem_private_keys(input: &str) -> String {
    const BEGIN: &str = "-----BEGIN ";
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(BEGIN) {
        let head = &rest[idx..];
        let label = head[BEGIN.len()..]
            .split('\n')
            .next()
            .and_then(|line| line.split_once("-----"))
            .map(|(label, _)| label)
            .filter(|label| label.contains("PRIVATE KEY"));
        let Some(label) = label else {
            out.push_str(&rest[..idx + BEGIN.len()]);
            rest = &rest[idx + BEGIN.len()..];
            continue;
        };
        out.push_str(&rest[..idx]);
        out.push_str("[redacted private key]");
        let end = format!("-----END {label}-----");
        rest = head.find(&end).map_or("", |at| &head[at + end.len()..]);
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
        assert_eq!(
            redact_token_shapes(
                "a\n-----BEGIN RSA PRIVATE KEY-----\nfirst-half\n-----END NOT-A-KEY-----\n\
                 second-half\n-----END RSA PRIVATE KEY-----\nb"
            ),
            "a\n[redacted private key]\nb",
            "only the matching END line closes the block"
        );
        assert_eq!(
            redact_token_shapes(
                "-----BEGIN EC PRIVATE KEY-----\nhalf\n-----END RSA PRIVATE KEY-----\nmore"
            ),
            "[redacted private key]",
            "a mismatched END keeps redacting to the end"
        );
        let kept = "path /srv/secret_sauce_recipe and MYAKIAFAKEFAKEFAKE0000, \
                    AKIA short, http://example.com:8080/x, user@example.com, \
                    -----BEGIN CERTIFICATE----- eyJnot.a.jwt";
        assert_eq!(redact_token_shapes(kept), kept);
    }

    #[test]
    fn more_bare_token_shapes_are_redacted_and_look_alike_words_stay() {
        // Fakes a secret scanner would read as real are written in parts.
        for token in [
            // Hugging Face
            concat!("hf_", "abcdefghijklmnopqrstuvwxyzABCDEFGH"),
            // GitLab
            concat!("glpat-", "FAKEfake_FAKE-fake00"),
            // GitLab routable token: payload, version, then length and checksum
            concat!("glpat-", "FAKEfake_FAKE-fake00FAKE.01.0a1b2c3d4"),
            // npm
            "npm_FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAKE",
            // Slack: app, refresh, session and rotating tokens
            "xoxa-0000-FAKE-fake",
            "xoxr-0000-FAKE-fake",
            "xoxs-0000-FAKE-fake",
            "xoxe-0000-FAKE-fake",
            // Slack access tokens once rotation is on
            concat!("xoxe.", "xoxp-", "1-FAKEfake0000FAKEfake"),
            concat!("xoxe.", "xoxb-", "1-FAKEfake0000FAKEfake"),
            concat!("xoxe.", "xapp-", "1-FAKEfake0000FAKEfake"),
            // Stripe webhook signing secret
            "whsec_FAKEfakeFAKEfakeFAKEfake",
            // The same prefix on a standard base64 secret
            "whsec_FAKEfakeFAKEfakeFAKE+fake/FAKEfake=",
            // GitHub refresh token
            "ghr_FAKEfakeFAKEfakeFAKEfakeFAKEfa",
            // Groq
            "gsk_FAKEfakeFAKEfakeFAKEfakeFAKEfa",
            // PyPI
            "pypi-FAKEfake_FAKE-fakeFAKEfakeFAKE",
            // Google OAuth client secret and access token
            "GOCSPX-FAKEfake_FAKE-fake00",
            "ya29.FAKEfake_FAKE-fake.FAKEfakeFAKE",
        ] {
            assert_eq!(
                redact_token_shapes(&format!("t {token} x")),
                "t [redacted] x",
                "{token}"
            );
        }
        let kept = "npm_config_cache hf_hub_download /tmp/glpat-notes pypi-mirror \
                    xoxa-short whsec_short my_gsk_FAKEfakeFAKEfakeFAKEfakeFAKEfa \
                    ya29.txt GOCSPX-short ghr_fake";
        assert_eq!(redact_token_shapes(kept), kept);
        // A long hyphenated name has no digit and no capital; a key has.
        let names = "/home/user/.cache/pypi-dependency-confusion-attack-demo/x \
                     see pypi-attestations-publishing-workflow-notes.md \
                     glpat-rotation-procedure-for-ci.md";
        assert_eq!(redact_token_shapes(names), names);
    }

    #[test]
    fn a_prefix_that_does_not_start_a_word_has_no_body_measured() {
        use std::cell::Cell;
        thread_local! {
            static LOOKS: Cell<usize> = const { Cell::new(0) };
        }
        fn counted(c: char) -> bool {
            LOOKS.with(|n| n.set(n.get() + 1));
            is_slack_char(c)
        }
        let shape = TokenShape {
            prefix: "xoxb-",
            min: 10,
            ok: counted,
            mixed: false,
        };
        // The `a` in front joins the whole run into one word.
        let run = format!("a{}", "xoxb-".repeat(1000));
        assert!(redact_prefixed(&run, &shape) == run);
        assert_eq!(LOOKS.with(Cell::get), 0);
        // A key that does start a word is measured once, with the one
        // character that ends it.
        assert_eq!(
            redact_prefixed("t xoxb-0000-FAKE-fake x", &shape),
            "t [redacted] x"
        );
        assert_eq!(LOOKS.with(Cell::get), 15);
    }

    #[test]
    fn a_planted_run_of_token_starts_is_read_in_step_with_its_size() {
        // Each of these once cost the square of its length: minutes here.
        for (what, input) in [
            ("eyJ run", format!("a{}", "eyJ".repeat(80_000))),
            ("eyJ words", " eyJa".repeat(400_000)),
            ("ya29. run", format!("a{}", "ya29.".repeat(48_000))),
        ] {
            let start = std::time::Instant::now();
            assert!(redact_token_shapes(&input) == input, "{what}");
            let took = start.elapsed();
            assert!(took.as_secs() < 5, "{what}: {took:?}");
        }
    }
}
