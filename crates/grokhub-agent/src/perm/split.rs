// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Hand-written command splitter.
//! Tree-sitter-bash is a C build this tree cannot prove on Windows MSVC, so anything
//! that is not a plain sequence of words (subshells, backticks, `$(...)`, heredocs,
//! and file redirects we cannot classify) prompts instead of auto-allowing.
//! Splits on `&&`, `||`, `;`, `|`, and newlines. Peels `timeout`, `nice`, `ionice`,
//! `chrt`, `stdbuf`, `env`, and leading `NAME=value` assignments.

use crate::perm::risk::{self, EnvRisk};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seg {
    pub command: String,
    pub words: Vec<String>,
    pub pipe_in: bool,
    pub readonly: bool,
    pub eligible: bool,
    pub dangerous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// `None` when the script is not a plain word sequence. Allow and read-only auto do not apply.
    pub segments: Option<Vec<Seg>>,
    /// Peeled commands we could still see, including an `env -S` payload.
    pub probes: Vec<String>,
    pub dangerous: bool,
}

struct RawSeg {
    words: Vec<String>,
    pipe_in: bool,
}

enum Strip {
    Inner {
        words: Vec<String>,
        env_risk: EnvRisk,
    },
    Opaque {
        payload: Option<String>,
    },
}

struct Peel {
    words: Vec<String>,
    eligible: bool,
    dangerous_env: bool,
    opaque: bool,
    payload: Option<String>,
}

pub fn analyze(script: &str) -> Facts {
    analyze_depth(script, 0)
}

/// The peeled command when the script is one plain segment. `None` when it does not peel cleanly.
pub fn peeled_primary(script: &str) -> Option<String> {
    let facts = analyze(script);
    let segs = facts.segments?;
    if segs.len() == 1 {
        Some(segs[0].command.clone())
    } else {
        None
    }
}

fn analyze_depth(script: &str, depth: usize) -> Facts {
    if depth > 4 {
        return Facts {
            segments: None,
            probes: Vec::new(),
            dangerous: true,
        };
    }
    let Some(raw) = split_words(script) else {
        return Facts {
            segments: None,
            probes: Vec::new(),
            dangerous: true,
        };
    };
    let mut segs = Vec::new();
    let mut probes = Vec::new();
    let mut dangerous = false;
    for raw_seg in raw {
        let (words, assign_risk) = strip_assignments(&raw_seg.words);
        if words.is_empty() {
            return Facts {
                segments: None,
                probes,
                dangerous: true,
            };
        }
        let peel = peel_wrappers(&words);
        if assign_risk == EnvRisk::Injection {
            dangerous = true;
        }
        dangerous |= peel.dangerous_env;
        if peel.opaque {
            if let Some(payload) = peel.payload {
                let inner = analyze_depth(&payload, depth + 1);
                probes.extend(inner.probes);
            }
            return Facts {
                segments: None,
                probes,
                dangerous: true,
            };
        }
        let eligible = peel.eligible && assign_risk == EnvRisk::Safe;
        let seg_danger = risk::is_dangerous(&peel.words)
            || peel.dangerous_env
            || assign_risk == EnvRisk::Injection;
        dangerous |= seg_danger;
        let command = peel.words.join(" ");
        if !command.is_empty() {
            probes.push(command.clone());
        }
        let readonly = eligible && !seg_danger && risk::is_readonly(&peel.words);
        segs.push(Seg {
            command,
            words: peel.words,
            pipe_in: raw_seg.pipe_in,
            readonly,
            eligible,
            dangerous: seg_danger,
        });
    }
    if segs.is_empty() {
        return Facts {
            segments: None,
            probes,
            dangerous: true,
        };
    }
    if pipeline_to_shell(&segs) {
        dangerous = true;
    }
    Facts {
        segments: Some(segs),
        probes,
        dangerous,
    }
}

fn pipeline_to_shell(segs: &[Seg]) -> bool {
    let mut group: Vec<&Seg> = Vec::new();
    let mut hit = false;
    for seg in segs {
        if !seg.pipe_in && !group.is_empty() {
            hit |= group_is_curl_sh(&group);
            group.clear();
        }
        group.push(seg);
    }
    hit || group_is_curl_sh(&group)
}

fn group_is_curl_sh(group: &[&Seg]) -> bool {
    group.len() >= 2
        && group.iter().any(|seg| risk::is_downloader(&seg.words))
        && group.iter().any(|seg| risk::is_shell_consumer(&seg.words))
}

fn strip_assignments(words: &[String]) -> (Vec<String>, EnvRisk) {
    let mut i = 0usize;
    let mut risk = EnvRisk::Safe;
    while let Some(name) = words.get(i).and_then(|word| assignment_name(word)) {
        risk = worse(risk, risk::env_key_risk(name));
        i += 1;
    }
    (words.get(i..).unwrap_or_default().to_vec(), risk)
}

fn worse(a: EnvRisk, b: EnvRisk) -> EnvRisk {
    match (a, b) {
        (EnvRisk::Injection, _) | (_, EnvRisk::Injection) => EnvRisk::Injection,
        (EnvRisk::Unvetted, _) | (_, EnvRisk::Unvetted) => EnvRisk::Unvetted,
        _ => EnvRisk::Safe,
    }
}

fn assignment_name(word: &str) -> Option<&str> {
    let (name, _) = word.split_once('=')?;
    let mut bytes = name.bytes();
    let first = bytes.next()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    if name.len() > 128 {
        return None;
    }
    bytes
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        .then_some(name)
}

fn peel_wrappers(words: &[String]) -> Peel {
    let mut current = words.to_vec();
    let mut eligible = true;
    let mut dangerous_env = false;
    for _ in 0..8 {
        if wrapper_kind(&current).is_none() {
            return Peel {
                words: current,
                eligible,
                dangerous_env,
                opaque: false,
                payload: None,
            };
        }
        match strip_one(&current) {
            Strip::Inner { words, env_risk } => {
                if env_risk == EnvRisk::Injection {
                    dangerous_env = true;
                }
                if env_risk != EnvRisk::Safe {
                    eligible = false;
                }
                current = words;
            }
            Strip::Opaque { payload } => {
                return Peel {
                    words: current,
                    eligible: false,
                    dangerous_env,
                    opaque: true,
                    payload,
                };
            }
        }
    }
    Peel {
        words: current,
        eligible: false,
        dangerous_env,
        opaque: true,
        payload: None,
    }
}

fn wrapper_kind(words: &[String]) -> Option<&'static str> {
    let base = risk::program_name(words.first()?);
    match base.as_str() {
        "timeout" => Some("timeout"),
        "nice" => Some("nice"),
        "ionice" => Some("ionice"),
        "chrt" => Some("chrt"),
        "stdbuf" => Some("stdbuf"),
        "env" => Some("env"),
        _ => None,
    }
}

fn strip_one(words: &[String]) -> Strip {
    let Some(kind) = wrapper_kind(words) else {
        return Strip::Opaque { payload: None };
    };
    let mut i = 1usize;
    match kind {
        "timeout" => {
            while words.get(i).is_some_and(|tok| tok.starts_with('-')) {
                if matches!(
                    words.get(i).map(String::as_str),
                    Some("-k" | "-s" | "--kill-after" | "--signal")
                ) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            i += 1;
        }
        "nice" => {
            while words.get(i).is_some_and(|tok| tok.starts_with('-')) {
                if matches!(
                    words.get(i).map(String::as_str),
                    Some("-n" | "--adjustment")
                ) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
        "ionice" => {
            while words.get(i).is_some_and(|tok| tok.starts_with('-')) {
                if matches!(
                    words.get(i).map(String::as_str),
                    Some(
                        "-c" | "-n"
                            | "-p"
                            | "-P"
                            | "-u"
                            | "--class"
                            | "--classdata"
                            | "--pid"
                            | "--pgid"
                            | "--uid"
                    )
                ) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
        "chrt" => {
            while words.get(i).is_some_and(|tok| tok.starts_with('-')) {
                i += 1;
            }
            i += 1;
        }
        "stdbuf" => {
            while words.get(i).is_some_and(|tok| tok.starts_with('-')) {
                if matches!(words.get(i).map(String::as_str), Some("-i" | "-o" | "-e")) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
        "env" => return strip_env(words),
        _ => return Strip::Opaque { payload: None },
    }
    match words.get(i..) {
        Some(inner) if !inner.is_empty() => Strip::Inner {
            words: inner.to_vec(),
            env_risk: EnvRisk::Safe,
        },
        _ => Strip::Opaque { payload: None },
    }
}

fn strip_env(words: &[String]) -> Strip {
    let mut i = 1usize;
    let mut risk = EnvRisk::Safe;
    while let Some(tok) = words.get(i).map(String::as_str) {
        if tok == "--" {
            i += 1;
            break;
        }
        if tok == "-" {
            i += 1;
            continue;
        }
        if !tok.starts_with('-') {
            if let Some(name) = assignment_name(tok) {
                risk = worse(risk, risk::env_key_risk(name));
                i += 1;
                continue;
            }
            break;
        }
        if tok == "--split-string" {
            let payload = words.get(i + 1).cloned();
            return Strip::Opaque { payload };
        }
        if let Some(payload) = tok.strip_prefix("--split-string=") {
            let payload = if payload.is_empty() {
                None
            } else {
                Some(payload.to_string())
            };
            return Strip::Opaque { payload };
        }
        if tok == "-S" {
            let payload = words.get(i + 1).cloned();
            return Strip::Opaque { payload };
        }
        if let Some(payload) = tok.strip_prefix("-S") {
            if !payload.is_empty() && !payload.starts_with('-') {
                return Strip::Opaque {
                    payload: Some(payload.to_string()),
                };
            }
        }
        if tok.starts_with("--") {
            if matches!(
                tok,
                "--ignore-environment" | "--null" | "--debug" | "--version" | "--help"
            ) {
                i += 1;
                continue;
            }
            if tok == "--chdir" || tok == "--unset" || tok == "--path" || tok == "--argv0" {
                if words.get(i + 1).is_none() {
                    return Strip::Opaque { payload: None };
                }
                i += 2;
                continue;
            }
            if tok.starts_with("--chdir=")
                || tok.starts_with("--unset=")
                || tok.starts_with("--path=")
                || tok.starts_with("--argv0=")
            {
                i += 1;
                continue;
            }
            return Strip::Opaque { payload: None };
        }
        let body = tok.trim_start_matches('-');
        if body.is_empty() {
            return Strip::Opaque { payload: None };
        }
        let mut idx = 0usize;
        let chars: Vec<char> = body.chars().collect();
        let mut consumed = false;
        while idx < chars.len() {
            match chars[idx] {
                'i' | 'v' | '0' => idx += 1,
                'S' => {
                    let glued: String = chars.get(idx + 1..).unwrap_or(&[]).iter().collect();
                    let payload = if glued.is_empty() {
                        words.get(i + 1).cloned()
                    } else {
                        Some(glued)
                    };
                    return Strip::Opaque { payload };
                }
                'u' | 'C' | 'P' | 'a' => {
                    let glued = idx + 1 < chars.len();
                    if glued {
                        i += 1;
                    } else if words.get(i + 1).is_none() {
                        return Strip::Opaque { payload: None };
                    } else {
                        i += 2;
                    }
                    consumed = true;
                    break;
                }
                _ => return Strip::Opaque { payload: None },
            }
        }
        if !consumed {
            i += 1;
        }
    }
    match words.get(i..) {
        Some(inner) if !inner.is_empty() => Strip::Inner {
            words: inner.to_vec(),
            env_risk: risk,
        },
        _ => Strip::Opaque { payload: None },
    }
}

fn split_words(script: &str) -> Option<Vec<RawSeg>> {
    let chars: Vec<char> = script.chars().collect();
    let mut i = 0usize;
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut segs: Vec<RawSeg> = Vec::new();
    let mut pipe_in = false;

    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
                i += 1;
                continue;
            }
            if q == '"' && c == '\\' {
                let next = *chars.get(i + 1)?;
                if next == '$' || next == '`' {
                    return None;
                }
                if matches!(next, '"' | '\\' | '\n') {
                    if next != '\n' {
                        cur.push(next);
                    }
                } else {
                    cur.push('\\');
                    cur.push(next);
                }
                i += 2;
                continue;
            }
            if q == '"' && (c == '$' || c == '`') {
                return None;
            }
            cur.push(c);
            i += 1;
            continue;
        }
        if c == '\\' {
            match chars.get(i + 1).copied() {
                Some('\n') => i += 2,
                Some(next) => {
                    cur.push(next);
                    i += 2;
                }
                None => return None,
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if matches!(c, '`' | '$' | '(' | ')' | '{' | '}') {
            return None;
        }
        if c == '#' && cur.is_empty() {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '\n' || c == '\r' || c == ';' {
            flush_word(&mut cur, &mut words);
            end_seg(&mut segs, &mut words, pipe_in, true)?;
            pipe_in = false;
            i += 1;
            continue;
        }
        if c == '&' {
            if chars.get(i + 1) == Some(&'&') {
                flush_word(&mut cur, &mut words);
                end_seg(&mut segs, &mut words, pipe_in, false)?;
                pipe_in = false;
                i += 2;
                continue;
            }
            if chars.get(i + 1) == Some(&'>') {
                let start = if chars.get(i + 2) == Some(&'>') {
                    i + 3
                } else {
                    i + 2
                };
                let (target, next) = read_word(&chars, start)?;
                if !safe_sink(&target) {
                    return None;
                }
                i = next;
                continue;
            }
            return None;
        }
        if c == '|' {
            if chars.get(i + 1) == Some(&'|') {
                flush_word(&mut cur, &mut words);
                end_seg(&mut segs, &mut words, pipe_in, false)?;
                pipe_in = false;
                i += 2;
                continue;
            }
            if chars.get(i + 1) == Some(&'&') {
                return None;
            }
            flush_word(&mut cur, &mut words);
            end_seg(&mut segs, &mut words, pipe_in, false)?;
            pipe_in = true;
            i += 1;
            continue;
        }
        if c == '>' || c == '<' {
            i = consume_redirect(&chars, i, &mut cur)?;
            continue;
        }
        if c.is_whitespace() {
            flush_word(&mut cur, &mut words);
            i += 1;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    if quote.is_some() {
        return None;
    }
    flush_word(&mut cur, &mut words);
    if !words.is_empty() {
        segs.push(RawSeg { words, pipe_in });
    }
    if segs.is_empty() {
        None
    } else {
        Some(segs)
    }
}

fn flush_word(cur: &mut String, words: &mut Vec<String>) {
    if !cur.is_empty() {
        words.push(std::mem::take(cur));
    }
}

fn end_seg(
    segs: &mut Vec<RawSeg>,
    words: &mut Vec<String>,
    pipe_in: bool,
    soft: bool,
) -> Option<()> {
    if words.is_empty() {
        if segs.is_empty() && soft {
            return Some(());
        }
        return None;
    }
    segs.push(RawSeg {
        words: std::mem::take(words),
        pipe_in,
    });
    Some(())
}

fn consume_redirect(chars: &[char], i: usize, cur: &mut String) -> Option<usize> {
    let fd = !cur.is_empty() && cur.chars().all(|c| c.is_ascii_digit());
    if chars.get(i) == Some(&'>') {
        let mut j = i + 1;
        if chars.get(j) == Some(&'>') {
            j += 1;
        }
        if chars.get(j) == Some(&'&') {
            j += 1;
            if chars
                .get(j)
                .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                && (fd || cur.is_empty())
            {
                cur.clear();
                return Some(j + 1);
            }
            return None;
        }
        let (target, next) = read_word(chars, skip_ws(chars, j))?;
        if safe_sink(&target) && (fd || cur.is_empty()) {
            cur.clear();
            return Some(next);
        }
        return None;
    }
    let mut j = i + 1;
    if chars.get(j) == Some(&'<') {
        return None;
    }
    if chars.get(j) == Some(&'&') {
        j += 1;
        if chars
            .get(j)
            .is_some_and(|c| c.is_ascii_digit() || *c == '-')
            && (fd || cur.is_empty())
        {
            cur.clear();
            return Some(j + 1);
        }
        return None;
    }
    let (target, next) = read_word(chars, skip_ws(chars, j))?;
    if safe_sink(&target) && (fd || cur.is_empty()) {
        cur.clear();
        return Some(next);
    }
    None
}

fn read_word(chars: &[char], mut i: usize) -> Option<(String, usize)> {
    i = skip_ws(chars, i);
    if i >= chars.len() {
        return None;
    }
    let mut out = String::new();
    let mut quote = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
                i += 1;
                continue;
            }
            if c == '$' || c == '`' {
                return None;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c.is_whitespace() || matches!(c, '&' | '|' | ';' | '\n' | '<' | '>') {
            break;
        }
        if c == '\\' {
            i += 1;
            out.push(*chars.get(i)?);
            i += 1;
            continue;
        }
        if matches!(c, '`' | '$' | '(' | ')') {
            return None;
        }
        out.push(c);
        i += 1;
    }
    if quote.is_some() || out.is_empty() {
        None
    } else {
        Some((out, i))
    }
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while chars
        .get(i)
        .is_some_and(|c| c.is_whitespace() && *c != '\n')
    {
        i += 1;
    }
    i
}

fn safe_sink(target: &str) -> bool {
    matches!(target, "/dev/null" | "/dev/stdout" | "/dev/stderr")
}
