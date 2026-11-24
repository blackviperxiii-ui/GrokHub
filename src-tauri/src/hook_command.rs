//! What Connectors → Hooks shows of a hook's command line or http URL.
//!
//! The page is where you read a project's hooks before trusting its folder,
//! so it must show what a hook runs while hiding the keys in it. The rule
//! that keeps both: a hidden span holds only plain value characters
//! (letters, digits and `-_.~+/=:@%,`; a URL also `?#[]!*`). Whitespace,
//! quotes, backslashes, shell operators, `$` and anything non-ASCII always
//! stay on the page, so no chained, substituted, redirected or backgrounded
//! command can hide behind a `[redacted]`, however the line is quoted. The
//! one wider span is a flag's own quoted value made only of value
//! characters and spaces (`--password "correct horse"`): with no operator
//! or newline in it, it can only hold arguments, never a command.
//!
//! A `--flag`'s value is hidden only when the flag is named for its secret
//! (its last part is a secret word: `--password`, `--auth-token`) and its
//! first part doesn't make it a switch: one that asks for the secret or
//! turns one on (`--ask-password`, `--always-auth`, `--exit-code`) takes
//! no value, and nor does one with a secret word inside its name
//! (`--keep-session-cookies`, `--password-stdin`), so the word after it,
//! often the file a hook writes or the download it runs, stays on the
//! page. A word after a secret flag that is shaped like a flag (`--verbose`,
//! `-o`, `-O/home/user/.bashrc`) is the next flag; any other word is the
//! value, even one that starts with `-`, as a base64url token can.

const HIDDEN: &str = "[redacted]";

/// A name (a flag, `NAME=value`, a `Name: value` header) holds a secret when
/// one of its `_`/`-` parts is one of these...
const SECRET_PARTS: &[&str] = &[
    "key",
    "apikey",
    "pass",
    "passwd",
    "password",
    "passphrase",
    "pw",
    "pwd",
    "token",
    "secret",
    "auth",
    "authorization",
    "bearer",
    "cookie",
    "session",
    "sessionid",
    "sig",
    "signature",
    "cred",
    "creds",
    "credential",
    "credentials",
    "code",
];

/// ...or ends in one of these (`APIKEY`, `accesstoken`).
const SECRET_ENDS: &[&str] = &["key", "token", "secret", "password", "passwd"];

/// Programs that read a bare `host.tld/path` word as a URL.
const URL_PROGRAMS: &[&str] = &["curl", "wget", "http", "https", "xh"];

/// Programs whose `-p` is a password.
const PASSWORD_P: &[&str] = &["mysql", "mysqldump", "mariadb", "mariadb-dump", "sshpass"];

/// Schemes an `Authorization:` value starts with; the credential follows.
const AUTH_SCHEMES: &[&str] = &["bearer", "basic", "token", "digest"];

fn quote_char(c: char) -> bool {
    c == '\'' || c == '"'
}

/// A character a hidden value may hold.
fn value_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '_' | '.' | '~' | '+' | '/' | '=' | ':' | '@' | '%' | ','
        )
}

/// A character a hidden URL may hold. `&` is not one: outside quotes it
/// runs a command in the background, and a query's later `&name=value`
/// pairs get the name rule instead.
fn url_char(c: char) -> bool {
    value_char(c) || matches!(c, '?' | '#' | '[' | ']' | '!' | '*')
}

fn run_len(text: &str, keep: fn(char) -> bool) -> usize {
    text.find(|c: char| !keep(c)).unwrap_or(text.len())
}

/// One run of value characters at the start of `text`, hidden:
/// `(shown, bytes used)`.
fn hide_run(text: &str) -> (String, usize) {
    match run_len(text, value_char) {
        0 => (String::new(), 0),
        len => (HIDDEN.to_string(), len),
    }
}

/// A flag's value at the start of `text`: a quoted value made only of value
/// characters and spaces is hidden whole, quotes kept; anything else hides
/// its first run of value characters (inside its quote, if it opens one).
fn hide_argument(text: &str) -> (String, usize) {
    if let Some(q) = text.chars().next().filter(|c| quote_char(*c)) {
        let body = &text[1..];
        let len = body
            .find(|c: char| !(value_char(c) || c == ' ' || c == '\t'))
            .unwrap_or(body.len());
        if len > 0 && body[len..].starts_with(q) {
            return (format!("{q}{HIDDEN}{q}"), len + 2);
        }
        let (shown, used) = hide_run(body);
        return (format!("{q}{shown}"), 1 + used);
    }
    hide_run(text)
}

/// `user:password` at the start of `text`: the user stays, the password is
/// hidden like a flag's value. `None` when there is no colon right after
/// the user (`python3 -u script.py`).
fn hide_after_colon(text: &str) -> Option<String> {
    let open = text.chars().next().filter(|c| quote_char(*c));
    let start = usize::from(open.is_some());
    let user = text[start..]
        .find(|c: char| !value_char(c) || c == ':')
        .map_or(text.len(), |i| start + i);
    if !text[user..].starts_with(':') {
        return None;
    }
    let pass = user + 1;
    let (shown, used) = match open {
        Some(q) => {
            let len = text[pass..]
                .find(|c: char| !(value_char(c) || c == ' ' || c == '\t'))
                .unwrap_or(text.len() - pass);
            if len > 0 && text[pass + len..].starts_with(q) {
                (format!("{HIDDEN}{q}"), len + 1)
            } else {
                hide_run(&text[pass..])
            }
        }
        None => hide_run(&text[pass..]),
    };
    Some(format!("{}{shown}{}", &text[..pass], &text[pass + used..]))
}

fn secret_part(part: &str) -> bool {
    SECRET_PARTS.contains(&part) || SECRET_ENDS.iter().any(|end| part.ends_with(end))
}

pub(crate) fn secret_name(name: &str) -> bool {
    let name = name.trim_start_matches('-').to_ascii_lowercase();
    name.split(['_', '-']).any(secret_part)
}

/// First parts of a switch's name that say it takes no value: it asks for
/// the secret, says when one is sent, or turns one on, off or out
/// (`--ask-password`, `--always-auth`, `--exit-code`, `--show-token`).
const SWITCH_LEADS: &[&str] = &[
    "allow", "always", "ask", "disable", "enable", "exit", "forget", "junk", "keep", "never", "no",
    "print", "prompt", "remember", "require", "save", "show", "skip", "store", "with", "without",
];

/// A `--flag` named for the secret it takes: its last part is a secret word
/// (`--api-key`, `--oauth2-bearer`), not just one of its parts
/// (`--keep-session-cookies`, `--cookie-jar`, `--password-stdin`), and its
/// first part isn't one that makes it a switch (`--ask-password`).
fn secret_flag_name(name: &str) -> bool {
    let name = name.trim_start_matches('-').to_ascii_lowercase();
    let first = name.split(['_', '-']).next().unwrap_or_default();
    let last = name.rsplit(['_', '-']).next().unwrap_or_default();
    let switch = name.contains(['_', '-']) && SWITCH_LEADS.contains(&first);
    secret_part(last) && !switch
}

/// A word after a secret flag that is the next flag, not the secret:
/// `--name`, `-x`, or `-x` with a path joined to it (`-O/home/user/.bashrc`,
/// `-o../out`). A secret can start with `-` (base64url), so any other word
/// that does, like `-AbC123`, is still the value.
fn next_flag(word: &str) -> bool {
    let mut chars = word.chars();
    if chars.next() != Some('-') {
        return false;
    }
    match chars.next() {
        Some('-') => chars.next().is_none_or(|c| c.is_ascii_alphabetic()),
        Some(c) if c.is_ascii_alphabetic() => chars
            .next()
            .is_none_or(|c| matches!(c, '/' | '.' | '~' | '\\')),
        _ => false,
    }
}

/// The words of a command line as byte ranges: runs of non-space text, with
/// quoted spans kept inside one word. A backslash outside single quotes
/// escapes the next character, and a `#` that starts a word comments out
/// the rest of its line.
fn words(command: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut chars = command.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' => {
                chars.next();
            }
            Some(_) => {}
            None if c == '\\' => {
                start.get_or_insert(i);
                chars.next();
            }
            None if c == '#' && start.is_none() => {
                while chars.next_if(|&(_, c)| c != '\n').is_some() {}
            }
            None if quote_char(c) => {
                quote = Some(c);
                start.get_or_insert(i);
            }
            None if c.is_whitespace() => {
                if let Some(s) = start.take() {
                    out.push((s, i));
                }
            }
            None => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(s) = start {
        out.push((s, command.len()));
    }
    out
}

fn unquoted(word: &str) -> String {
    word.chars().filter(|c| !quote_char(*c)).collect()
}

/// The program a command line runs: its first word that isn't `KEY=value`.
fn program(command: &str, words: &[(usize, usize)]) -> String {
    words
        .iter()
        .map(|&(s, e)| unquoted(&command[s..e]))
        .find(|w| !w.contains('='))
        .map(|w| w.rsplit(['/', '\\']).next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// How a flag carries a secret.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Secret {
    Value,
    /// `user:password`; a value without a colon isn't a secret.
    AfterColon,
}

fn secret_flag(name: &str, program: &str) -> Option<Secret> {
    match name {
        "-u" | "--user" | "--proxy-user" => Some(Secret::AfterColon),
        "-p" if PASSWORD_P.contains(&program) => Some(Secret::Value),
        "-a" if program == "redis-cli" => Some(Secret::Value),
        _ if name.starts_with("--") && secret_flag_name(name) => Some(Secret::Value),
        _ => None,
    }
}

/// A flag's value word (or the rest of `--flag=` / `-p`) with its secret
/// hidden and anything after the value kept.
fn hide_flag_value(secret: Secret, text: &str) -> Option<String> {
    match secret {
        Secret::Value => {
            let (shown, used) = hide_argument(text);
            Some(format!("{shown}{}", &text[used..]))
        }
        Secret::AfterColon => hide_after_colon(text),
    }
}

/// A word with its flag value hidden (`--token=…`, `-pS3cret`), and whether
/// the next word is a flag value.
fn flag_word(word: &str, program: &str) -> (String, Option<Secret>) {
    let unchanged = (word.to_string(), None);
    if !word.starts_with('-') {
        let flag = unquoted(word);
        return match secret_flag(&flag, program) {
            Some(secret) if flag.starts_with('-') => (word.to_string(), Some(secret)),
            _ => unchanged,
        };
    }
    let split = match word.find('=') {
        Some(eq) => Some((&word[..eq], &word[eq + 1..], "=")),
        None if !word.starts_with("--") && word.len() > 2 && word.is_char_boundary(2) => {
            Some((&word[..2], &word[2..], ""))
        }
        None => None,
    };
    if let Some((name, value, sep)) = split {
        if let Some(secret) = secret_flag(name, program) {
            return match hide_flag_value(secret, value) {
                Some(shown) => (format!("{name}{sep}{shown}"), None),
                None => unchanged,
            };
        }
    }
    match secret_flag(word, program) {
        Some(secret) => (word.to_string(), Some(secret)),
        None => unchanged,
    }
}

/// Pass 1: the values of password and token flags.
fn hide_flags(command: &str) -> String {
    let words = words(command);
    let program = program(command, &words);
    let mut out = String::new();
    let mut last = 0;
    let mut pending: Option<Secret> = None;
    for (s, e) in words {
        out.push_str(&command[last..s]);
        let word = &command[s..e];
        let shown = match pending.take() {
            // `--token -o out.txt`: `-o` is the next flag, not the value.
            Some(secret) if !next_flag(word) => {
                hide_flag_value(secret, word).unwrap_or_else(|| word.to_string())
            }
            _ => {
                let (shown, next) = flag_word(word, &program);
                pending = next;
                shown
            }
        };
        out.push_str(&shown);
        last = e;
    }
    out.push_str(&command[last..]);
    out
}

/// A URL cut to its scheme and host: a webhook's secret is often its path
/// or query.
fn short_url(url: &str) -> String {
    let rest_at = url.find("://").map_or(0, |i| i + 3);
    let (scheme, rest) = url.split_at(rest_at);
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(host_end);
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => format!("{HIDDEN}@{host}"),
        None => authority.to_string(),
    };
    let tail = if tail.is_empty() || tail == "/" {
        tail.to_string()
    } else {
        format!("/{HIDDEN}")
    };
    format!("{scheme}{host}{tail}")
}

/// `ntfy.sh/topic`: a host with a dot, then a path, with no scheme.
fn bare_url(word: &str) -> bool {
    let Some((host, path)) = word.split_once('/') else {
        return false;
    };
    let host = host.split(':').next().unwrap_or("");
    !path.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// A `NAME` at the start of `text` followed by `=` or `:` (not `://`):
/// `(name, separator length)`.
fn named(text: &str) -> Option<(&str, usize)> {
    let first = text.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(text.len());
    let rest = &text[end..];
    if rest.starts_with('=') || (rest.starts_with(':') && !rest.starts_with("://")) {
        Some((&text[..end], 1))
    } else {
        None
    }
}

/// Pass 2: URLs down to their host, and the value of each `NAME=value` and
/// `Name: value` whose name holds a secret, anywhere in the line, inside
/// quotes too (`bash -c '…'`). Every hidden span is a run of value (or URL)
/// characters, so this needs no idea of where quotes open and close.
fn hide_named_and_urls(command: &str, program: &str) -> String {
    let lower = command.to_ascii_lowercase();
    let mut out = String::new();
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < command.len() {
        let rest = &command[i..];
        let boundary =
            prev.is_none_or(|p| !(p.is_ascii_alphanumeric() || matches!(p, '_' | '-' | '.')));
        if boundary && (lower[i..].starts_with("http://") || lower[i..].starts_with("https://")) {
            let len = run_len(rest, url_char);
            out.push_str(&short_url(&rest[..len]));
            prev = rest[..len].chars().last();
            i += len;
            continue;
        }
        if boundary {
            if let Some((name, sep)) = named(rest).filter(|(name, _)| secret_name(name)) {
                let mut at = name.len() + sep;
                if rest[name.len()..].starts_with(':') {
                    at += rest[at..].len() - rest[at..].trim_start_matches([' ', '\t']).len();
                    // `Authorization: Bearer <token>`: the token is the value.
                    let word = run_len(&rest[at..], |c| c.is_ascii_alphabetic());
                    let scheme = rest[at..at + word].to_ascii_lowercase();
                    if AUTH_SCHEMES.contains(&scheme.as_str()) && rest[at + word..].starts_with(' ')
                    {
                        at += word + 1;
                    }
                }
                // `KEY="value"`: the quote stays, the value inside is hidden.
                if rest[at..].starts_with(quote_char) {
                    at += 1;
                }
                out.push_str(&rest[..at]);
                let (shown, used) = hide_run(&rest[at..]);
                out.push_str(&shown);
                prev = rest[..at + used].chars().last();
                i += at + used;
                continue;
            }
            let word_start = prev.is_none_or(|p| p.is_whitespace() || quote_char(p));
            if word_start && URL_PROGRAMS.contains(&program) {
                let len = run_len(rest, url_char);
                if bare_url(&rest[..len]) {
                    out.push_str(&short_url(&rest[..len]));
                    prev = rest[..len].chars().last();
                    i += len;
                    continue;
                }
            }
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}

/// Pass 3: every shape `redact_for_share` knows (key prefixes, token
/// shapes), one run of value characters at a time, so it can't reach past
/// anything else.
fn redact_values(command: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in command.chars() {
        if value_char(c) {
            run.push(c);
        } else {
            out.push_str(&grokhub_core::redact_for_share(&run));
            run.clear();
            out.push(c);
        }
    }
    out.push_str(&grokhub_core::redact_for_share(&run));
    out
}

/// What the page shows of a hook's command line or http URL.
pub fn shown(command: &str) -> String {
    let program = program(command, &words(command));
    let flags = hide_flags(command);
    redact_values(&hide_named_and_urls(&flags, &program))
}

#[cfg(test)]
#[path = "hook_command_tests.rs"]
mod tests;
