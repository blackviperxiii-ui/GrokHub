//! Host rails. YOLO skips the prompt, not these.
//!
//! A command names a guarded path when any way a shell could read it names
//! the path (see [`shell_readings`]): quotes, line continuations and
//! backslash escapes taken out, `$'…'` decoded, brace lists and sequences
//! expanded, repeated slashes, `.` parts and `..` steps folded, and words
//! after a `cd` read inside its folder. A glob counts when it could match
//! the path (see [`glob_names`]).

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

/// Guarded paths a glob may reach, as their last parts. A dot folder is
/// guarded wherever it is; a GrokHub file only in a folder named `GrokHub`,
/// so `*.json` in a project is not one.
const GLOB_GUARDED: &[(&[&str], &str)] = &[
    (&[".ssh"], "forbidden path: ssh keys"),
    (&[".gnupg"], "forbidden path: gnupg"),
    (&[".aws"], "forbidden path: aws credentials"),
    (&[".kube"], "forbidden path: kube config"),
    (&["etc", "shadow"], "forbidden path: /etc/shadow"),
    (&["etc", "sudoers"], "forbidden path: /etc/sudoers"),
    (&["etc", "sudoers.d"], "forbidden path: /etc/sudoers"),
    (&[".grok", "auth.json"], "forbidden path: grok login"),
    (&["grokhub", "app.json"], "forbidden path: app secrets"),
    (&["grokhub", "secrets.json"], "forbidden path: app secrets"),
    (
        &["grokhub", "hub-state.json"],
        "forbidden path: hub pair tokens",
    ),
];

fn glob_forbidden(reading: &str) -> Option<&'static str> {
    GLOB_GUARDED
        .iter()
        .find(|(path, _)| glob_names(reading, path))
        .map(|(_, why)| *why)
}

/// Characters that end a word in a reading: blanks, shell operators, quotes
/// and the joins [`contains_path_leaf`] splits on.
fn ends_word(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ';' | '|' | '&' | '<' | '>' | '(' | ')' | '=' | ',' | '+' | '\'' | '"' | '`'
        )
}

/// The ways a shell could read `cmd`, lowercased: as written with `\` as
/// `/` (a Windows path), with the quotes taken out (`~/."ssh"`), and with
/// line continuations, backslash escapes, `$"…"` and `$'…'` decoded
/// (`~/.s\sh`, `$'\x2essh'`); on Windows also as PowerShell reads it, a
/// backtick escaping the next character (`` .s`sh ``). Each also comes with
/// its brace lists and sequences expanded (`.{ssh,x}`, `.s{s..s}h`) and its
/// paths folded: repeated slashes, `.` parts and `..` steps (`/etc//shadow`),
/// and on Windows the trailing dots it drops (`.ssh.`). When the command
/// changes folder, each also comes read from there ([`after_cd`]).
pub fn shell_readings(cmd: &str) -> Vec<String> {
    readings(cmd, cfg!(windows))
}

/// [`shell_readings`] for Windows (`windows`) or a POSIX shell.
fn readings(cmd: &str, windows: bool) -> Vec<String> {
    let lower = cmd.to_ascii_lowercase();
    let written = lower.replace('\\', "/");
    let unquoted = written.replace(['\'', '"'], "");
    let unescaped = unescape(cmd);
    let mut base = vec![written];
    if windows {
        base.push(unbacktick(&unquoted));
    }
    base.extend([unquoted, unescaped]);
    let mut out: Vec<String> = Vec::new();
    for reading in base {
        let folded = map_words(&reading, |word| {
            expand_braces(word)
                .iter()
                .map(|w| fold_path(w, windows))
                .collect::<Vec<_>>()
                .join(" ")
        });
        for r in [reading, folded] {
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let moved: Vec<String> = out
        .iter()
        .filter_map(|r| after_cd(r, windows, &mut seen))
        .collect();
    out.extend(moved);
    out
}

/// Characters that end a word where a shell splits a command.
fn splits_words(c: char) -> bool {
    c.is_whitespace() || matches!(c, ';' | '|' | '&' | '<' | '>' | '(' | ')' | '`')
}

/// The words after a `cd <folder>` (or `pushd`, `chdir`, `Set-Location`,
/// `sl`) in `reading`, each read inside that folder the way the shell opens
/// it: `cd ~ && cat .*/id_rsa` gives `~/.*/id_rsa`. A word already rooted
/// (`/x`, `~/x`, `$HOME/x`, `C:/x`) is left out, and so is a plain word
/// that can't end a guarded path (no `/`, `.`, glob or brace, and not
/// `shadow` or `sudoers`); in `--opt=value` the value is read. A bare `cd`
/// goes home. Each word comes once, joined by blanks, and only when no
/// earlier reading gave it (`seen`); `None` when no word is left.
fn after_cd(
    reading: &str,
    windows: bool,
    seen: &mut std::collections::HashSet<String>,
) -> Option<String> {
    const CD: &[&str] = &["cd", "pushd", "chdir", "set-location", "sl"];
    let rooted = |w: &str| {
        w.is_empty() || w.starts_with(['/', '~', '$', '%']) || w.as_bytes().get(1) == Some(&b':')
    };
    let mut cwd = Folder::default();
    let mut moved = false;
    let mut want_dir = false;
    let mut out = String::new();
    let mut word = String::new();
    // A trailing blank flushes the last word.
    for c in reading.chars().chain([' ']) {
        if !splits_words(c) {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            let w = std::mem::take(&mut word);
            if want_dir && !w.starts_with('-') {
                cwd.enter(&w, rooted(&w), windows);
                want_dir = false;
            } else if CD.contains(&w.as_str()) {
                want_dir = true;
                moved = true;
            } else if moved {
                let (name, value) = match w.split_once('=') {
                    Some((name, value)) => (format!("{name}="), value),
                    None => (String::new(), w.as_str()),
                };
                if !rooted(value) && may_end_a_guarded_path(value) {
                    let read = format!(
                        "{name}{}",
                        fold_path(&format!("{}/{value}", cwd.tail()), windows)
                    );
                    if seen.insert(read.clone()) {
                        out.push_str(&read);
                        out.push(' ');
                    }
                }
            }
        }
        if want_dir && matches!(c, ';' | '|' | '&' | '(' | ')' | '`' | '\n' | '\r') {
            cwd.enter("~", true, windows);
            want_dir = false;
        }
    }
    out.pop();
    (!out.is_empty()).then_some(out)
}

/// Whether a word read inside a folder could end a guarded path: it has a
/// `/`, `.`, glob or brace in it, or is the last part of one (`shadow`).
fn may_end_a_guarded_path(word: &str) -> bool {
    word.contains(['/', '.', '*', '?', '[', '{'])
        || GLOB_GUARDED
            .iter()
            .any(|(path, _)| path.last() == Some(&word))
}

/// The folder a command `cd`ed to, as path parts (`["", "etc"]` for
/// `/etc`, `["~", ".config"]`), folded as it goes. Only its last parts are
/// ever read ([`Folder::tail`]).
#[derive(Default)]
struct Folder {
    parts: Vec<String>,
}

impl Folder {
    /// `cd dir`: from the root when `rooted`, else from where it is.
    fn enter(&mut self, dir: &str, rooted: bool, windows: bool) {
        if rooted {
            self.parts.clear();
        }
        for (i, part) in dir.split('/').enumerate() {
            let part = opened_part(part, windows);
            match part {
                "" if i == 0 && self.parts.is_empty() => self.parts.push(String::new()),
                "" | "." => {}
                // Like `fold_path`: `..` steps back over a plain name, and
                // the root is its own parent.
                ".." => match self.parts.last() {
                    Some(p) if p.is_empty() => {}
                    Some(p) if p != ".." && !p.starts_with(['~', '$', '%']) => {
                        self.parts.pop();
                    }
                    _ => self.parts.push(part.into()),
                },
                part => self.parts.push(squash(part)),
            }
        }
    }

    /// The last three parts, more than a guarded path that ends in the word
    /// (three parts at most, one of them the word's) or the home folder
    /// before a dot name (two) needs: `/` for the root, `.` for a folder not
    /// known (`cd .`).
    fn tail(&self) -> String {
        let from = self.parts.len().saturating_sub(3);
        match self.parts.as_slice() {
            [] => ".".into(),
            [root] if root.is_empty() => "/".into(),
            parts => parts[from..].join("/"),
        }
    }
}

/// A folder part kept short: no guarded name or home marker is longer than
/// 16 bytes, so a longer plain name reads as `_` (`~_` for a `~user`), and
/// a longer glob as `*` (`.*` for a dot glob), which reaches every name it
/// could.
fn squash(part: &str) -> String {
    if part.len() <= 16 {
        return part.to_string();
    }
    match (part.contains(['*', '?', '[']), part.starts_with('.')) {
        (true, true) => ".*".into(),
        (true, false) => "*".into(),
        _ if part.starts_with('~') => "~_".into(),
        _ => "_".into(),
    }
}

/// A PowerShell reading of `text` (lowercase, quotes out): a backtick
/// escapes the next character (`` .s`sh `` reads `.ssh`), and `` `u{73} ``
/// is that character.
fn unbacktick(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'u') && chars.get(i + 2) == Some(&'{') {
            let close = (i + 3..chars.len().min(i + 12)).find(|&j| chars[j] == '}');
            let ch = close.and_then(|close| {
                let hex: String = chars[i + 3..close].iter().collect();
                u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|ch| (ch.to_ascii_lowercase(), close + 1))
            });
            if let Some((ch, next)) = ch {
                out.push(ch);
                i = next;
                continue;
            }
        }
        out.extend(chars.get(i + 1));
        i += 2;
    }
    out
}

/// `text` with each word replaced by `f(word)`; the blanks and operators
/// between words stay. Quotes, commas and `=` stay inside a word, as the
/// shell glues them (`~/."ssh"`, `.{ssh,x}`, `--file=/etc/x`).
fn map_words(text: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for c in text.chars() {
        if splits_words(c) {
            if !word.is_empty() {
                out.push_str(&f(&word));
                word.clear();
            }
            out.push(c);
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        out.push_str(&f(&word));
    }
    out
}

/// `cmd` lowercased with its quotes out (`$"…"` and `$'…'` too), line
/// continuations dropped and backslash escapes decoded, the way POSIX
/// shells read them, plus `\xHH`, `\uHHHH`, `\UHHHHHHHH` and octal escapes
/// wherever they appear (`$'\x2e'`, and what `printf` or `echo -e` would
/// print). Reading more than a shell would only finds more.
fn unescape(cmd: &str) -> String {
    let chars: Vec<char> = cmd.chars().collect();
    let mut out = String::with_capacity(cmd.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let (ch, next) = escaped(&chars, i + 1);
                out.extend(ch);
                i = next;
            }
            '$' if matches!(chars.get(i + 1), Some('\'' | '"')) => i += 2,
            '\'' | '"' => i += 1,
            c => {
                out.push(c.to_ascii_lowercase());
                i += 1;
            }
        }
    }
    out
}

/// The character a backslash at `chars[i - 1]` stands for, lowercased, and
/// where the text goes on. A backslash before a newline continues the line:
/// the shell drops both.
fn escaped(chars: &[char], i: usize) -> (Option<char>, usize) {
    let Some(&c) = chars.get(i) else {
        return (None, i);
    };
    let digits = |from: usize, radix: u32, max: usize| {
        let end = (from..chars.len().min(from + max))
            .take_while(|&j| chars[j].is_digit(radix))
            .last()
            .map_or(from, |j| j + 1);
        let text: String = chars[from..end].iter().collect();
        u32::from_str_radix(&text, radix)
            .ok()
            .and_then(char::from_u32)
            .filter(char::is_ascii)
            .map(|ch| (Some(ch.to_ascii_lowercase()), end))
    };
    let decoded = match c {
        '\n' => return (None, i + 1),
        'x' => digits(i + 1, 16, 2),
        'u' => digits(i + 1, 16, 4),
        'U' => digits(i + 1, 16, 8),
        '0'..='7' => digits(i, 8, 3),
        _ => None,
    };
    decoded.unwrap_or((Some(c.to_ascii_lowercase()), i + 1))
}

/// A word with its first brace list or sequence expanded, and so on in each
/// result (`.{ssh,x}` gives `.ssh` and `.x`, `.s{r..t}h` gives `.srh`,
/// `.ssh` and `.sth`). At most 64 words, and only for a word of up to
/// 64 KiB; past that, the word as written and with each brace list as `*`
/// ([`star_braces`]), a glob that still matches every expansion.
fn expand_braces(word: &str) -> Vec<String> {
    let too_big = || vec![word.to_string(), star_braces(word)];
    if word.len() > 1 << 16 {
        return too_big();
    }
    let mut done = Vec::new();
    let mut todo = vec![word.to_string()];
    while let Some(w) = todo.pop() {
        let (open, close, items) = if let Some((open, close, commas)) = brace_list(&w) {
            let mut cuts = vec![open];
            cuts.extend(commas);
            cuts.push(close);
            let items: Vec<String> = cuts
                .windows(2)
                .map(|pair| w[pair[0] + 1..pair[1]].to_string())
                .collect();
            (open, close, items)
        } else if let Some(seq) = brace_seq(&w) {
            seq
        } else {
            done.push(w);
            continue;
        };
        if items.is_empty() || done.len() + todo.len() + items.len() > 64 {
            return too_big();
        }
        for item in items {
            todo.push(format!("{}{item}{}", &w[..open], &w[close + 1..]));
        }
    }
    done
}

/// The first brace sequence in `word` (`{a..e}`, `{1..9..2}`, letters or
/// whole numbers): where it opens and closes, and its items. No items when
/// there would be more than 64.
fn brace_seq(word: &str) -> Option<(usize, usize, Vec<String>)> {
    let mut open = None;
    for (j, b) in word.bytes().enumerate() {
        match b {
            b'{' => open = Some(j),
            b'}' => {
                if let Some(items) = open
                    .take()
                    .and_then(|at| seq_items(&word[at + 1..j]).map(|items| (at, items)))
                {
                    return Some((items.0, j, items.1));
                }
            }
            _ => {}
        }
    }
    None
}

/// The items of a sequence body such as `a..e` or `9..1..2`; `None` when it
/// is not one, no items past 64.
fn seq_items(body: &str) -> Option<Vec<String>> {
    let mut parts = body.split("..");
    let (from, to) = (parts.next()?, parts.next()?);
    let step = match parts.next() {
        Some(step) => step.parse::<i64>().ok()?.unsigned_abs().max(1),
        None => 1,
    };
    if parts.next().is_some() {
        return None;
    }
    let letter = |text: &str| match text.as_bytes() {
        [b] if b.is_ascii_alphabetic() => Some(i128::from(*b)),
        _ => None,
    };
    let (a, b, letters) = match (from.parse::<i64>(), to.parse::<i64>()) {
        (Ok(a), Ok(b)) => (i128::from(a), i128::from(b), false),
        _ => (letter(from)?, letter(to)?, true),
    };
    let count = (b - a).unsigned_abs() / u128::from(step) + 1;
    if count > 64 {
        return Some(Vec::new());
    }
    let step = if b < a {
        -i128::from(step)
    } else {
        i128::from(step)
    };
    let items = (0..count as i128)
        .map(|k| a + k * step)
        .map(|v| match u8::try_from(v) {
            Ok(byte) if letters => char::from(byte).to_string(),
            _ => v.to_string(),
        })
        .collect();
    Some(items)
}

/// The first `{…}` in `word` that holds a top-level comma: where it opens,
/// where it closes, and its commas. One pass over the word.
fn brace_list(word: &str) -> Option<(usize, usize, Vec<usize>)> {
    let mut open: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut first: Option<(usize, usize, Vec<usize>)> = None;
    for (j, b) in word.bytes().enumerate() {
        match b {
            b'{' => open.push((j, Vec::new())),
            b',' => {
                if let Some((_, commas)) = open.last_mut() {
                    commas.push(j);
                }
            }
            b'}' => {
                if let Some((at, commas)) = open.pop() {
                    if !commas.is_empty() && first.as_ref().is_none_or(|f| at < f.0) {
                        first = Some((at, j, commas));
                    }
                }
            }
            _ => {}
        }
    }
    first
}

/// `word` with each outermost `{…}` as `*` (`~/.{ssh,x}{1,2}` reads `~/.**`).
fn star_braces(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut depth = 0usize;
    for c in word.chars() {
        match c {
            '{' => {
                if depth == 0 {
                    out.push('*');
                }
                depth += 1;
            }
            '}' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            c => out.push(c),
        }
    }
    out
}

/// A path word without empty or `.` parts, each `..` stepping back over a
/// plain name: `/etc//shadow` and `/etc/x/../shadow` read `/etc/shadow`. On
/// Windows (`windows`) each part also loses the trailing dots it drops when
/// it opens a path (`.ssh.` opens `.ssh`).
fn fold_path(word: &str, windows: bool) -> String {
    if !word.contains('/') {
        return opened_part(word, windows).to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in word.split('/').map(|part| opened_part(part, windows)) {
        match part {
            "" | "." => {}
            ".." if parts
                .last()
                .is_some_and(|p| *p != ".." && !p.starts_with(['~', '$', '%'])) =>
            {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let body = parts.join("/");
    if word.starts_with('/') {
        format!("/{body}")
    } else {
        body
    }
}

/// A path part as the system opens it: on Windows without its trailing
/// dots (`.ssh.` opens `.ssh`), except a part made only of dots.
fn opened_part(part: &str, windows: bool) -> &str {
    match part.trim_end_matches('.') {
        trimmed if windows && !trimmed.is_empty() => trimmed,
        _ => part,
    }
}

/// Whether a word in `reading` (one of [`shell_readings`]) names `path`, a
/// guarded path's last parts in lowercase (`["etc", "shadow"]`), outright or
/// through a glob that could match it. Like the shell, a glob reaches a name
/// that starts with `.` only when it starts with `.` too. One part of the
/// window must be written out or pin its name down ([`pins`]): `/etc/*` and
/// `/e?c/s*` count, `*/*` does not, and neither does `*.jsonl` alone. A dot
/// folder is also reached by any dot glob straight in a home folder
/// (`~/.*`): a bare `.*` elsewhere is mostly a pattern (`grep '.*'`).
pub fn glob_names(reading: &str, path: &[&str]) -> bool {
    if path.is_empty() {
        return false;
    }
    reading.split(ends_word).any(|word| {
        let parts: Vec<&str> = word.split('/').collect();
        (0..parts.len())
            .filter(|&at| at + path.len() <= parts.len())
            .any(|at| {
                let window = &parts[at..at + path.len()];
                let fits = window
                    .iter()
                    .zip(path)
                    .all(|(part, name)| part == name || glob_reaches(part, name));
                fits && ((path[0].starts_with('.') && in_home(&parts[..at]))
                    || window
                        .iter()
                        .zip(path)
                        .any(|(part, name)| part == name || pins(part, name)))
            })
    })
}

/// Whether a glob `part` writes out some of `name`, not only wildcards: past
/// the dot of a dot name (`.ss?`, `.s*`, not `.*`), or before the first dot
/// of any other name (`consent.json?`, `c*.jsonl`, not `*.jsonl` or
/// `[[:alpha:]]*.jsonl`).
fn pins(part: &str, name: &str) -> bool {
    if name.starts_with('.') {
        return specific_dot_glob(part);
    }
    let chars: Vec<char> = part.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '[' => match class_end(&chars, i) {
                Some(end) => i = end,
                None => return true,
            },
            '.' => return false,
            '*' | '?' => {}
            _ => return true,
        }
        i += 1;
    }
    false
}

/// A glob `part` that could match `name`, with the shell's rule for names
/// that start with `.`.
fn glob_reaches(part: &str, name: &str) -> bool {
    part.contains(['*', '?', '['])
        && (!name.starts_with('.') || part.starts_with('.'))
        && glob_match(
            &part.chars().collect::<Vec<_>>(),
            &name.chars().collect::<Vec<_>>(),
        )
}

/// A dot glob with more than wildcards past the dot (`.s*`, `.[s]*`).
fn specific_dot_glob(part: &str) -> bool {
    part.chars().skip(1).any(|c| !matches!(c, '*' | '?'))
}

/// Whether the parts before a name end in a home folder: `~`, `$HOME`,
/// `%USERPROFILE%`, `/root`, or `/home/<name>` and `/Users/<name>`.
fn in_home(before: &[&str]) -> bool {
    before.last().is_some_and(|last| {
        last.starts_with('~')
            || matches!(
                *last,
                "$home" | "${home}" | "%userprofile%" | "$env:userprofile" | "$env:home"
            )
    }) || matches!(before, ["", "root"] | [.., "home" | "users", _])
}

/// Shell glob match: `*`, `?` and `[…]` classes (`!` or `^` negates, `a-z`
/// ranges, `[:alpha:]` and the other named classes). A `[` without its `]`
/// is a plain character.
fn glob_match(pat: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        let step = match pat.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
                continue;
            }
            Some('?') => Some(p + 1),
            Some('[') => match class_end(pat, p) {
                Some(end) => class_has(&pat[p + 1..end], name[n]).then_some(end + 1),
                None => (name[n] == '[').then_some(p + 1),
            },
            Some(&c) => (c == name[n]).then_some(p + 1),
            None => None,
        };
        match (step, star) {
            (Some(next), _) => {
                p = next;
                n += 1;
            }
            (None, Some((sp, sn))) => {
                p = sp + 1;
                n = sn + 1;
                star = Some((sp, sn + 1));
            }
            (None, None) => return false,
        }
    }
    pat[p..].iter().all(|&c| c == '*')
}

/// Where the class that opens at `pat[open]` closes. A `]` right after the
/// `[` (or after `[!`) is part of the class, and so is the `]` of a
/// `[:alpha:]`, `[=a=]` or `[.a.]` inside it.
fn class_end(pat: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    if matches!(pat.get(i), Some('!' | '^')) {
        i += 1;
    }
    if pat.get(i) == Some(&']') {
        i += 1;
    }
    while i < pat.len() {
        match pat[i] {
            ']' => return Some(i),
            '[' => i = inner_end(pat, i).map_or(i + 1, |end| end + 1),
            _ => i += 1,
        }
    }
    None
}

/// Where a `[:name:]`, `[=c=]` or `[.c.]` that opens at `pat[at]` closes.
fn inner_end(pat: &[char], at: usize) -> Option<usize> {
    let kind = *pat.get(at + 1).filter(|c| matches!(c, ':' | '=' | '.'))?;
    (at + 2..pat.len().saturating_sub(1))
        .find(|&j| pat[j] == kind && pat[j + 1] == ']')
        .map(|j| j + 1)
}

/// Whether the named class (`alpha`, `digit`, …) holds `c`. The reading is
/// lowercase, so `upper` reads as letters too; a name the floor doesn't
/// know holds everything.
fn named_class_has(name: &str, c: char) -> bool {
    match name {
        "alpha" | "upper" | "lower" => c.is_ascii_alphabetic(),
        "alnum" => c.is_ascii_alphanumeric(),
        "word" => c.is_ascii_alphanumeric() || c == '_',
        "digit" => c.is_ascii_digit(),
        "xdigit" => c.is_ascii_hexdigit(),
        "punct" => c.is_ascii_punctuation(),
        "space" | "blank" => c.is_whitespace(),
        "cntrl" => c.is_control(),
        "graph" => !c.is_control() && !c.is_whitespace(),
        "print" => !c.is_control(),
        _ => true,
    }
}

/// Whether `c` is in the class body between `[` and `]`.
fn class_has(body: &[char], c: char) -> bool {
    let (negate, body) = match body.first() {
        Some('!' | '^') => (true, &body[1..]),
        _ => (false, body),
    };
    let mut hit = false;
    let mut i = 0;
    while i < body.len() {
        if let Some(end) = (body[i] == '[').then(|| inner_end(body, i)).flatten() {
            let inner: String = body[i + 2..end - 1].iter().collect();
            hit |= match body[i + 1] {
                ':' => named_class_has(&inner, c),
                _ => inner.contains(c),
            };
            i = end + 1;
            continue;
        }
        if body.get(i + 1) == Some(&'-') && i + 2 < body.len() {
            hit |= (body[i]..=body[i + 2]).contains(&c);
            i += 3;
        } else {
            hit |= body[i] == c;
            i += 1;
        }
    }
    hit != negate
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

/// Collapse Python-style `chr(46)` / `chr (46)` / `chr(46.0)` / `chr(0x2e)` /
/// `chr(0o56)` / `chr(0b101110)` so leaf path checks still fire. Adjacent
/// `chr()` results joined with `+` are glued.
fn collapse_chr_calls(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let lower = cmd.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(after_call) = try_collapse_one_chr(bytes, &lower, i) {
            let (ch, next_i) = after_call;
            out.push(ch);
            i = next_i;
            // Skip `+` between adjacent chr() / `"ssh"` so `.`+`"ssh"` → `.ssh`.
            let mut k = i;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'+' {
                k += 1;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if looks_like_chr_call(bytes, k) {
                    i = k;
                } else if let Some((lit, after)) = take_quoted_literal(bytes, k) {
                    out.push_str(lit);
                    i = after;
                }
            }
            continue;
        }
        push_utf8_char(cmd, &mut i, &mut out);
    }
    out
}

/// `"ssh"` / `'ssh'` at `i` → `(literal, index_after_closing_quote)`.
fn take_quoted_literal(bytes: &[u8], i: usize) -> Option<(&str, usize)> {
    if i >= bytes.len() {
        return None;
    }
    let q = bytes[i];
    if q != b'\'' && q != b'"' {
        return None;
    }
    let end = bytes[i + 1..]
        .iter()
        .position(|&b| b == q)
        .map(|p| i + 1 + p)?;
    let lit = std::str::from_utf8(&bytes[i + 1..end]).ok()?;
    Some((lit, end + 1))
}

fn looks_like_chr_call(bytes: &[u8], i: usize) -> bool {
    if i + 3 > bytes.len() || &bytes[i..i + 3] != b"chr" {
        return false;
    }
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
        return false;
    }
    let mut j = i + 3;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    j < bytes.len() && bytes[j] == b'('
}

/// Parse one `chr (…)` / `chr((46))` / `chr(0x_2e)` / `chr(+46)` / `chr(46,)` call.
fn try_collapse_one_chr(bytes: &[u8], _lower: &str, i: usize) -> Option<(char, usize)> {
    if !looks_like_chr_call(bytes, i) {
        return None;
    }
    let mut j = i + 3;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    // `looks_like_chr_call` already verified `(`
    j += 1;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    // One grouping level: `chr((46))`.
    let mut grouped = false;
    if j < bytes.len() && bytes[j] == b'(' {
        grouped = true;
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
    }
    // Unary plus: `chr(+46)`.
    if j < bytes.len() && bytes[j] == b'+' {
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
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
            b'b' | b'B' => {
                base = 2;
                j += 2;
            }
            _ => {}
        }
    }
    // Digits with optional Python underscores, including after the base prefix
    // (`0x_2e`, `4_6`, `0b10_1110`).
    let mut digits = String::new();
    let mut last_was_underscore = false;
    while j < bytes.len() {
        let b = bytes[j];
        let is_digit = match base {
            16 => b.is_ascii_hexdigit(),
            8 => matches!(b, b'0'..=b'7'),
            2 => matches!(b, b'0' | b'1'),
            _ => b.is_ascii_digit(),
        };
        if is_digit {
            digits.push(b.to_ascii_lowercase() as char);
            last_was_underscore = false;
            j += 1;
            continue;
        }
        if b == b'_' {
            // Allow `_` after base prefix before the first digit (`0x_2e`).
            if last_was_underscore {
                return None;
            }
            last_was_underscore = true;
            j += 1;
            continue;
        }
        break;
    }
    if digits.is_empty() || last_was_underscore {
        return None;
    }
    // Integer-valued floats: `46.0`, `46.`, `4_6.0` (decimal only).
    if base == 10 && j < bytes.len() && bytes[j] == b'.' {
        j += 1;
        let frac_start = j;
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'_') {
            if bytes[j] != b'_' && bytes[j] != b'0' {
                return None;
            }
            j += 1;
        }
        if j > frac_start && bytes[j - 1] == b'_' {
            return None;
        }
    }
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    // Trailing comma: `chr(46,)`.
    if j < bytes.len() && bytes[j] == b',' {
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
    }
    if grouped {
        if j >= bytes.len() || bytes[j] != b')' {
            return None;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
    }
    if j >= bytes.len() || bytes[j] != b')' {
        return None;
    }
    let n = u32::from_str_radix(&digits, base).ok()?;
    let ch = char::from_u32(n)?;
    if !ch.is_ascii() {
        return None;
    }
    Some((ch, j + 1))
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
/// (`sh` / `bash` / `zsh` / `dash`), optionally as a path basename (`/bin/sh`),
/// after simple quotes and common wrappers (`env` / `sudo` / `timeout` / …),
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
/// / `| exec sh` / `| timeout 5 sh` still count as shell sinks.
const SHELL_WRAPPERS: &[&str] = &[
    "env", "sudo", "command", "nice", "nohup", "exec", "timeout", "setsid", "busybox", "unshare",
    "stdbuf", "ionice", "xargs",
];

fn is_shell_basename(base: &str) -> bool {
    matches!(base, "sh" | "bash" | "zsh" | "dash" | "ash" | "ksh")
}

/// `$SHELL` / `${SHELL}` (command is lowercased before matching).
fn is_shell_var(rest: &str) -> bool {
    let rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix("${shell}") {
        return after.is_empty()
            || !after
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    }
    if let Some(after) = rest.strip_prefix("$shell") {
        return after.is_empty()
            || !after
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    }
    false
}

fn next_token_is_shell(rest: &str) -> bool {
    let mut rest = rest.trim_start();
    // Peel a short chain of wrappers (`env`, `sudo`, …) before the sink basename.
    for _ in 0..6 {
        if rest.is_empty() {
            return false;
        }
        if is_shell_var(rest) {
            return true;
        }
        let Some((tok, after)) = take_cmd_token(rest) else {
            return false;
        };
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if is_shell_basename(base) {
            return true;
        }
        // `| env -Ssh` lowercases to `-ssh` — shell attached to the flag.
        if let Some(shell) = tok.strip_prefix("-s") {
            if is_shell_basename(shell) {
                return true;
            }
        }
        if !SHELL_WRAPPERS.contains(&base) {
            return false;
        }
        // Skip flags, arity-1 values (`sudo -u root`), and `KEY=VAL` (`env FOO=1 sh`).
        rest = skip_wrapper_args(base, after);
        // `timeout [opts] DURATION COMMAND` — peel the duration so `sh` is visible.
        if base == "timeout" {
            rest = skip_timeout_duration(rest);
        }
    }
    false
}

/// Drop leading wrapper args so the sink basename is reached: boolean flags (`-n`),
/// arity-1 flags + values (`-u root`, `-n 10`), and `NAME=value` assignments.
fn skip_wrapper_args<'a>(wrapper: &str, rest: &'a str) -> &'a str {
    let mut rest = rest.trim_start();
    for _ in 0..12 {
        if let Some(after) = take_env_assignment(rest) {
            rest = after;
            continue;
        }
        let Some((tok, after)) = take_cmd_token(rest) else {
            break;
        };
        if !tok.starts_with('-') {
            break;
        }
        // `| env -Ssh` → token `-ssh` after lowercase; leave for next_token_is_shell.
        if wrapper == "env" {
            if let Some(shell) = tok.strip_prefix("-s") {
                if is_shell_basename(shell) {
                    break;
                }
            }
        }
        rest = after.trim_start();
        // `--user=root` arrives as `--user` + `=root` because `=` ends tokens.
        // `env -S` / `-s` / `--split-string[=…]` must not swallow a following shell.
        // (Command is lowercased, so `-S` is `-s`.)
        if let Some(stripped) = rest.strip_prefix('=') {
            if wrapper == "env" && matches!(tok, "-s" | "--split-string") {
                rest = stripped.trim_start();
                break;
            }
            rest = skip_one_value(stripped);
            continue;
        }
        if wrapper == "env" && matches!(tok, "-s" | "--split-string") {
            // Bare `-s` / `--split-string`; next token is often `sh`.
            break;
        }
        if flag_takes_value(wrapper, tok) {
            rest = skip_one_value(rest);
        }
    }
    rest
}

fn flag_takes_value(wrapper: &str, flag: &str) -> bool {
    match wrapper {
        "sudo" => matches!(
            flag,
            "-u" | "-g"
                | "-C"
                | "-D"
                | "-R"
                | "-T"
                | "--user"
                | "--group"
                | "--close-from"
                | "--chdir"
                | "--chroot"
                | "--other-user"
        ),
        "nice" => matches!(flag, "-n" | "--adjustment"),
        // `-S` / `--split-string` omitted on purpose — their "value" is often `sh`.
        "env" => matches!(flag, "-u" | "-C" | "--unset" | "--chdir"),
        "exec" => matches!(flag, "-a" | "--as"),
        "timeout" => matches!(flag, "-k" | "-s" | "--kill-after" | "--signal"),
        "stdbuf" => matches!(
            flag,
            "-o" | "-e" | "-i" | "--output" | "--error" | "--input"
        ),
        "ionice" => matches!(
            flag,
            "-c" | "-n" | "-p" | "--class" | "--classdata" | "--pid"
        ),
        "xargs" => matches!(
            flag,
            "-n" | "-P"
                | "-s"
                | "-i"
                | "-I"
                | "-E"
                | "-a"
                | "-d"
                | "--max-args"
                | "--max-procs"
                | "--max-chars"
                | "--replace"
                | "--eof"
                | "--arg-file"
                | "--delimiter"
        ),
        // `command -p`, `nohup`, `setsid`, `busybox`, `unshare` flags are boolean here.
        _ => false,
    }
}

fn skip_timeout_duration(rest: &str) -> &str {
    let rest = rest.trim_start();
    let Some((tok, after)) = take_cmd_token(rest) else {
        return rest;
    };
    if looks_like_duration(tok) {
        after.trim_start()
    } else {
        rest
    }
}

fn looks_like_duration(tok: &str) -> bool {
    let mut chars = tok.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    tok.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 's' | 'm' | 'h' | 'd'))
}

/// `NAME=value` (possibly empty value) — used by `env FOO=bar sh`.
fn take_env_assignment(rest: &str) -> Option<&str> {
    let rest = rest.trim_start();
    let (name, after) = take_cmd_token(rest)?;
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return None;
    }
    let after = after.trim_start();
    let after_eq = after.strip_prefix('=')?;
    Some(skip_one_value(after_eq))
}

fn skip_one_value(rest: &str) -> &str {
    let rest = rest.trim_start();
    if rest.is_empty() {
        return rest;
    }
    match take_cmd_token(rest) {
        Some((_, after)) => after.trim_start(),
        None => rest,
    }
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
    forbidden_reason_in(cmd, cfg!(windows))
}

fn forbidden_reason_in(cmd: &str, windows: bool) -> Option<&'static str> {
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
    let mut all = readings(cmd, windows);
    all.extend(readings(&d, windows));
    all.iter()
        .find_map(|r| forbidden_on(r).or_else(|| glob_forbidden(r)))
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
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | sudo -u root sh"),
            Some("base64|shell rebuild"),
            "| sudo -u root sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | nice -n 10 sh"),
            Some("base64|shell rebuild"),
            "| nice -n 10 sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env -u HOME sh"),
            Some("base64|shell rebuild"),
            "| env -u HOME sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | exec sh"),
            Some("base64|shell rebuild"),
            "| exec sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env FOO=bar sh"),
            Some("base64|shell rebuild"),
            "| env FOO=bar sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0b101110)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "python binary chr(0b101110) must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr (46)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr with space before ( must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(46.0)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(46.0) integer-valued float must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | dash"),
            Some("base64|shell rebuild"),
            "| dash must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | /bin/dash"),
            Some("base64|shell rebuild"),
            "| /bin/dash must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | timeout 5 sh"),
            Some("base64|shell rebuild"),
            "| timeout 5 sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | setsid sh"),
            Some("base64|shell rebuild"),
            "| setsid sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | busybox sh"),
            Some("base64|shell rebuild"),
            "| busybox sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env -S sh"),
            Some("base64|shell rebuild"),
            "| env -S sh must not swallow sh as an arity-1 value"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env --split-string sh"),
            Some("base64|shell rebuild"),
            "| env --split-string sh must not swallow sh"
        );
        assert!(
            forbidden_reason("echo abc | base64 | sha256sum").is_none(),
            "| sha256sum must stay allowed after dash/timeout peel"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(4_6)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(4_6) underscore literal must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0b10_1110)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(0b10_1110) must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0x2_e)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(0x2_e) must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(+46)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(+46) unary plus must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(46,)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(46,) trailing comma must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | unshare sh"),
            Some("base64|shell rebuild"),
            "| unshare sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | stdbuf -oL sh"),
            Some("base64|shell rebuild"),
            "| stdbuf -oL sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | ionice sh"),
            Some("base64|shell rebuild"),
            "| ionice sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | ash"),
            Some("base64|shell rebuild"),
            "| ash must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | ksh"),
            Some("base64|shell rebuild"),
            "| ksh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | busybox ash"),
            Some("base64|shell rebuild"),
            "| busybox ash must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0x_2e)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(0x_2e) underscore after base prefix must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0o_56)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(0o_56) underscore after base prefix must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr(0b_101110)ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr(0b_101110) underscore after base prefix must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason("cat ~/chr((46))ssh/id_rsa"),
            Some("forbidden path: ssh keys"),
            "chr((46)) grouped code point must rebuild .ssh"
        );
        assert_eq!(
            forbidden_reason(r#"cat ~/chr(46)+"ssh"/id_rsa"#),
            Some("forbidden path: ssh keys"),
            r#"chr(46)+"ssh" mixed concat must rebuild .ssh"#
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | xargs sh"),
            Some("base64|shell rebuild"),
            "| xargs sh must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | $SHELL"),
            Some("base64|shell rebuild"),
            "| $SHELL must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | ${SHELL}"),
            Some("base64|shell rebuild"),
            "| ${{SHELL}} must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env -Ssh"),
            Some("base64|shell rebuild"),
            "| env -Ssh (attached) must count as a shell sink"
        );
        assert_eq!(
            forbidden_reason("echo Y2F0IH4vLnNzaC9pZF9yc2E= | base64 -d | env -S=sh"),
            Some("base64|shell rebuild"),
            "| env -S=sh must not swallow sh"
        );
        assert!(
            forbidden_reason("echo abc | base64 | shuf").is_none(),
            "| shuf must stay allowed after unshare/ash peel"
        );
        assert!(
            forbidden_reason("echo abc | base64 | sha256sum").is_none(),
            "| sha256sum must stay allowed after xargs/$SHELL peel"
        );
        assert!(
            forbidden_reason("echo abc | base64 | shred").is_none(),
            "| shred must stay allowed after xargs/$SHELL peel"
        );
    }

    /// The floor reads a path the way a shell would: quotes and escapes
    /// out, braces expanded, slashes and `.` parts folded, and a glob that
    /// could match a guarded path counts.
    #[test]
    fn the_floor_reads_paths_the_way_a_shell_does() {
        let ssh = Some("forbidden path: ssh keys");
        for cmd in [
            r#"cat ~/."ssh"/id_rsa"#,
            r#"cat ~/.'ssh'/id_rsa"#,
            r#"cat ~/.s\sh/id_rsa"#,
            r#"cat ~/$'\x2essh'/id_rsa"#,
            r#"cat ~/$'\056ssh'/id_rsa"#,
            r#"cat ~/$(printf '\x2essh')/id_rsa"#,
            "cat ~/.ss?/id_rsa",
            "cat ~/.s[s]h/id_rsa",
            "cat ~/.s*h/id_rsa",
            "cat ~/.*/id_rsa",
            "cp -r /home/user/.* /tmp/x",
            "cd ~ && cd .ss? && cat id_rsa",
            "cat ~/.{ssh,x}/id_rsa",
            "cat ~/.{ssh,a}/x{1,2}{1,2}{1,2}{1,2}{1,2}{1,2}{1,2}/../id_rsa",
            "cat ~/x/../.ssh/id_rsa",
            "cat ~//.ssh//id_rsa",
        ] {
            assert_eq!(forbidden_reason(cmd), ssh, "{cmd}");
        }
        for (cmd, why) in [
            ("cat /etc/shado?", "forbidden path: /etc/shadow"),
            ("cat /e?c/s*", "forbidden path: /etc/shadow"),
            ("cat /etc/*", "forbidden path: /etc/shadow"),
            ("cat /etc//shadow", "forbidden path: /etc/shadow"),
            ("cat /etc/./shadow", "forbidden path: /etc/shadow"),
            ("cat /etc/ssl/../shadow", "forbidden path: /etc/shadow"),
            ("cat /etc/sudoers.?/x", "forbidden path: /etc/sudoers"),
            ("cat ~/.grok//auth.json", "forbidden path: grok login"),
            ("cat ~/.grok/./auth.json", "forbidden path: grok login"),
            ("cat ~/.gro?/auth.json", "forbidden path: grok login"),
            ("cat ~/.grok/*.json", "forbidden path: grok login"),
            (
                "cat ~/.config/GrokHub/secret?.json",
                "forbidden path: app secrets",
            ),
            ("cat ~/.config/GrokHub/*", "forbidden path: app secrets"),
            (
                "cat ~/.config/Grok?ub/secret?.json",
                "forbidden path: app secrets",
            ),
            (
                "cat ~/.config/GrokHub/hub-state.jso?",
                "forbidden path: hub pair tokens",
            ),
            ("ls ~/.gnup[g]", "forbidden path: gnupg"),
            ("cat ~/.aw?/credentials", "forbidden path: aws credentials"),
            ("cat ~/.ku*/config", "forbidden path: kube config"),
        ] {
            assert_eq!(forbidden_reason(cmd), Some(why), "{cmd}");
        }
        // Globs and patterns that can't reach a guarded path still run.
        for cmd in [
            "grep -n '.*' notes.txt",
            "sed 's/.*/x/' notes.txt",
            r"rg 'fn .*\(' src",
            "ls *.json",
            "cat */*.json",
            "wc -l logs/*.jsonl",
            "ls ~/.config/*",
            "ls ~/.config/*/*.json",
            "cat /*/*",
            "cat ~/*/id_rsa",
            "cat ~/.*rc",
            "cat ~/.bashrc",
            "ls src/*/mod.rs",
            "find . -name '*.rs'",
            "cat ./notes//todo.md",
            "echo 'a && b'",
            "git log --format='%h {a,b}'",
            "ls ~/.grok",
            "cat my-app/config/auth.json",
        ] {
            assert_eq!(forbidden_reason(cmd), None, "{cmd}");
        }
    }

    /// The floor reads what a shell reads past bracket classes
    /// (`[[:alpha:]]`), line continuations, `$"…"` and `$'\u…'` strings,
    /// brace sequences (`{s..s}`) and a `cd` earlier in the command, which
    /// a bare glob after it is read inside.
    #[test]
    fn the_floor_reads_classes_continuations_sequences_and_cd() {
        let ssh = Some("forbidden path: ssh keys");
        for cmd in [
            "cat ~/.[[:alpha:]]sh/id_rsa",
            "cat ~/.s[[:lower:]]h/id_rsa",
            "cat ~/.s[[:upper:]]h/id_rsa",
            "cat ~/.[![:digit:]]sh/id_rsa",
            "cat ~/.ss\\\nh/id_rsa",
            "cat ~/.$\"ssh\"/id_rsa",
            "cat ~/$'.ssh'/id_rsa",
            r"cat ~/$'\u002essh'/id_rsa",
            r"cat ~/$'\U0000002essh'/id_rsa",
            r"cat ~/$'.\x53sh'/id_rsa",
            "cat ~/.s{s..s}h/id_rsa",
            "cat ~/.s{a..z}h/id_rsa",
            "cat ~/.s{a..z..2}h/id_rsa",
            "cat ~/.s{t..r}h/id_rsa",
            "cat ~/.s{1..999}h/id_rsa",
            "cd ~ && cat .*/id_rsa",
            "cd && cat .*/id_rsa",
            "cd /home/user; cp -r .* /tmp/x",
            "cd ~/.config && cd .. && cat .*/id_rsa",
            "pushd $HOME && tar czf /tmp/x.tgz .*",
            concat!(
                "cd /home/a-user-name-longer-than-sixteen-bytes",
                " && cat .*/id_rsa"
            ),
        ] {
            assert_eq!(forbidden_reason(cmd), ssh, "{cmd}");
        }
        for (cmd, why) in [
            ("cat /etc/[[:lower:]]hadow", "forbidden path: /etc/shadow"),
            ("cat /etc/s{h..h}adow", "forbidden path: /etc/shadow"),
            ("cd /etc && cat *", "forbidden path: /etc/shadow"),
            (
                "cat ~/.grok/auth.jso[[:alpha:]]",
                "forbidden path: grok login",
            ),
            ("cat ~/.grok/auth.js\\\non", "forbidden path: grok login"),
            (
                "cd ~/.config/GrokHub && sed -i 's/x/y/' *",
                "forbidden path: app secrets",
            ),
            (
                "cd ~/.config/GrokHub/logs && cat ../*.json",
                "forbidden path: app secrets",
            ),
            (
                "cd ~/.config/GrokHub && grep -r x --include=*",
                "forbidden path: app secrets",
            ),
            (
                "cd ~/.config/GrokHub/a/b/c/d && cd ../../../.. && cat *",
                "forbidden path: app secrets",
            ),
            (
                concat!(
                    "cd ~/.config/GrokHub/a-folder-name-longer-than-sixteen",
                    " && cd .. && cat *"
                ),
                "forbidden path: app secrets",
            ),
            ("cd /etc && cat shadow", "forbidden path: /etc/shadow"),
            ("cd ~/.grok && cat auth.json", "forbidden path: grok login"),
        ] {
            assert_eq!(forbidden_reason(cmd), Some(why), "{cmd}");
        }
        for cmd in [
            "grep -n '[[:alpha:]]' notes.txt",
            "grep -E '^[[:space:]]+$' src/main.rs",
            "cat ~/.s{x..z}h/id_rsa",
            "echo {1..5}",
            "cd src && cat *.rs",
            "cd ~ && ls",
            "cd ~/my-app && cat .*rc",
            "cd /home/user/GrokHub && wc -l logs/*.jsonl",
            "cd ~/.config && ls *",
            "cd . && cat *.md",
            "cd ~/.config/GrokHub/a/b/c/d && cd ../.. && cat *",
        ] {
            assert_eq!(forbidden_reason(cmd), None, "{cmd}");
        }
    }

    /// Where the shell is PowerShell, a backtick escapes the next character
    /// (`` .s`sh ``), and Windows drops a trailing dot or blank from a path
    /// part (`.ssh.`), so the floor reads past both there. Elsewhere `.ssh.`
    /// is another folder.
    #[test]
    fn on_windows_the_floor_reads_backticks_and_trailing_dots() {
        let ssh = Some("forbidden path: ssh keys");
        for cmd in [
            r"Get-Content $env:USERPROFILE\.s`sh\id_rsa",
            r"Get-Content $env:USERPROFILE\`.ssh\id_rsa",
            r"Get-Content $env:USERPROFILE\.s`u{73}h\id_rsa",
            r"type %USERPROFILE%\.ssh.\id_rsa",
            r"type C:\Users\User\.ssh.",
        ] {
            assert_eq!(forbidden_reason_in(cmd, true), ssh, "{cmd}");
        }
        assert_eq!(
            forbidden_reason_in(r"type $env:APPDATA\GrokHub\secrets.json.", true),
            Some("forbidden path: app secrets")
        );
        for cmd in [r"type %USERPROFILE%\.ssh.\id_rsa", "cat ~/.ssh./id_rsa"] {
            assert_eq!(forbidden_reason_in(cmd, false), None, "{cmd}");
        }
        assert_eq!(
            forbidden_reason(r"type %USERPROFILE%\.ssh.\id_rsa"),
            if cfg!(windows) { ssh } else { None }
        );
    }

    #[test]
    fn glob_names_matches_a_window_of_parts() {
        let path = ["grokhub", "consent.jsonl"];
        assert!(glob_names(
            "echo x >> ~/.config/grokhub/consent.json?",
            &path
        ));
        assert!(glob_names("echo x >> ~/.config/grokhub/consent.*", &path));
        assert!(glob_names("cat /home/user/.config/g*/consent.jsonl", &path));
        assert!(!glob_names("wc -l logs/*.jsonl", &path));
        assert!(glob_names("cat g*/c*.jsonl", &path));
        assert!(glob_names("cat x >> consent.js?nl", &["consent.jsonl"]));
        assert!(glob_names("cat x >> [c]onsent.*", &["consent.jsonl"]));
        assert!(!glob_names("cat x >> *.jsonl", &["consent.jsonl"]));
        assert!(!glob_names("cat x >> [a-z]*.jsonl", &["consent.jsonl"]));
        assert!(!glob_names("cat */*", &path));
        assert!(!glob_names("echo x", &[]));
        let mut both = expand_braces("~/.{ssh,x}/id_rsa");
        both.sort();
        assert_eq!(both, ["~/.ssh/id_rsa", "~/.x/id_rsa"]);
        let long = format!("~/.{{ssh,b}}/{}", "x".repeat(70_000));
        assert_eq!(
            expand_braces(&long),
            [long.clone(), format!("~/.*/{}", "x".repeat(70_000))]
        );
        assert_eq!(brace_list("{x{a,b}"), Some((2, 6, vec![4])));
        assert_eq!(brace_list("{a,{b,c}}"), Some((0, 8, vec![2])));
        assert_eq!(brace_list("{a}b,c}"), None);
        assert_eq!(
            readings(r#"cat ~/."ssh"//id_rsa"#, false),
            [
                r#"cat ~/."ssh"//id_rsa"#,
                r#"cat ~/."ssh"/id_rsa"#,
                "cat ~/.ssh//id_rsa",
                "cat ~/.ssh/id_rsa",
            ]
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
