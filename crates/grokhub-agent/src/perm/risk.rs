// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Read-only commands auto-run. Dangerous commands always prompt.
//! Ambient git config is not scanned; exec-risk flags still fail closed.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvRisk {
    Safe,
    Unvetted,
    Injection,
}

const SAFE_ENV_KEYS: &[&str] = &[
    "CARGO_TERM_COLOR",
    "CARGO_TERM_PROGRESS_WHEN",
    "RUST_LOG",
    "RUST_LOG_STYLE",
    "RUST_BACKTRACE",
    "RUST_TEST_THREADS",
    "RUST_MIN_STACK",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "FORCE_COLOR",
    "COLORTERM",
];

const INJECTION_ENV_KEYS: &[&str] = &[
    "LD_PRELOAD",
    "LD_AUDIT",
    "BASH_ENV",
    "ENV",
    "IFS",
    "PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_PROXY_COMMAND",
    "PROMPT_COMMAND",
];

const INJECTION_ENV_PREFIXES: &[&str] = &["LD_", "DYLD_", "GIT_CONFIG"];

const SAFE_GIT: &[&str] = &[
    "status",
    "branch",
    "log",
    "diff",
    "ls-files",
    "show",
    "rev-parse",
    "blame",
    "grep",
    "describe",
    "merge-base",
    "check-ignore",
    "check-attr",
    "cat-file",
    "ls-tree",
    "show-ref",
    "for-each-ref",
    "rev-list",
    "name-rev",
    "count-objects",
    "shortlog",
];

const GIT_UNSAFE_OPTIONS: &[&str] = &[
    "--filters",
    "--textconv",
    "--output",
    "--ext-diff",
    "--open-files-in-pager",
];

const READONLY_HEADS: &[&str] = &[
    "ls", "cat", "pwd", "date", "whoami", "hostname", "uptime", "ps", "head", "tail", "wc", "uniq",
    "tr", "cut", "grep",
];

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish"];
const DOWNLOADERS: &[&str] = &["curl", "wget", "fetch"];

pub fn env_key_risk(key: &str) -> EnvRisk {
    if SAFE_ENV_KEYS.contains(&key) {
        EnvRisk::Safe
    } else if INJECTION_ENV_KEYS.contains(&key)
        || INJECTION_ENV_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
    {
        EnvRisk::Injection
    } else {
        EnvRisk::Unvetted
    }
}

pub fn program_name(token: &str) -> String {
    let base = token.rsplit(['/', '\\']).next().unwrap_or(token);
    let lower = base.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

pub fn is_dangerous(words: &[String]) -> bool {
    let Some(head_tok) = words.first() else {
        return false;
    };
    let head = program_name(head_tok);
    if matches!(
        head.as_str(),
        "rm" | "chmod"
            | "chown"
            | "chgrp"
            | "chattr"
            | "pkill"
            | "kill"
            | "killall"
            | "sudo"
            | "doas"
            | "su"
            | "dd"
    ) || head == "mkfs"
        || head.starts_with("mkfs.")
    {
        return true;
    }
    if SHELLS.contains(&head.as_str()) && words.iter().skip(1).any(|word| shell_dash_c(word)) {
        return true;
    }
    if head == "git"
        && (git_has_exec_risk(words) || git_has_push(words) || git_unsafe_query_option(words))
    {
        return true;
    }
    if head == "sort" && sort_compress(words) {
        return true;
    }
    if head == "rg" && rg_pre(words) {
        return true;
    }
    false
}

pub fn is_readonly(words: &[String]) -> bool {
    let Some(head_tok) = words.first() else {
        return false;
    };
    if is_dangerous(words) {
        return false;
    }
    let head = program_name(head_tok);
    if READONLY_HEADS.contains(&head.as_str()) {
        return true;
    }
    if head == "sort" {
        return !sort_compress(words);
    }
    if head == "rg" {
        return !rg_pre(words);
    }
    if head_tok == "git" {
        return git_readonly(words);
    }
    if head == "kubectl" {
        return matches!(
            words.get(1).map(String::as_str),
            Some("get" | "logs" | "describe")
        );
    }
    false
}

pub fn is_downloader(words: &[String]) -> bool {
    words
        .first()
        .is_some_and(|head| DOWNLOADERS.contains(&program_name(head).as_str()))
}

pub fn is_shell_consumer(words: &[String]) -> bool {
    let Some(head) = words.first() else {
        return false;
    };
    if SHELLS.contains(&program_name(head).as_str()) {
        return true;
    }
    words
        .get(1)
        .is_some_and(|next| SHELLS.contains(&program_name(next).as_str()))
        && matches!(program_name(head).as_str(), "sudo" | "doas" | "su")
}

fn shell_dash_c(word: &str) -> bool {
    if word == "--command" || word.starts_with("--command=") {
        return true;
    }
    let Some(rest) = word.strip_prefix('-') else {
        return false;
    };
    if rest.starts_with('-') {
        return false;
    }
    rest.contains('c')
}

fn git_has_push(words: &[String]) -> bool {
    words.iter().skip(1).any(|word| word == "push")
}

fn git_readonly(words: &[String]) -> bool {
    if words.first().map(String::as_str) != Some("git") {
        return false;
    }
    if git_has_exec_risk(words) || git_unsafe_query_option(words) {
        return false;
    }
    let Some(idx) = git_verb_index(words) else {
        return false;
    };
    words
        .get(idx)
        .is_some_and(|verb| SAFE_GIT.contains(&verb.as_str()))
}

fn git_unsafe_query_option(words: &[String]) -> bool {
    if words.iter().skip(1).any(|word| {
        let flag = word.split('=').next().unwrap_or(word);
        flag.len() > 2 && GIT_UNSAFE_OPTIONS.iter().any(|full| full.starts_with(flag))
    }) {
        return true;
    }
    matches!(git_verb_index(words), Some(idx) if words.get(idx).map(String::as_str) == Some("grep"))
        && words.iter().skip(1).any(|word| word.starts_with("-O"))
}

fn git_verb_index(words: &[String]) -> Option<usize> {
    let mut i = 1usize;
    loop {
        let tok = words.get(i).map(String::as_str)?;
        if tok == "-" || tok == "--" {
            return None;
        }
        if !tok.starts_with('-') {
            return Some(i);
        }
        if tok == "-C" {
            i += 2;
            continue;
        }
        if tok.starts_with("-C") && !tok.starts_with("--") && tok.len() > 2 {
            i += 1;
            continue;
        }
        if tok == "--no-pager" || tok == "-P" {
            i += 1;
            continue;
        }
        return None;
    }
}

fn long_prefix(flag: &str, full: &str, min_len: usize) -> bool {
    flag.starts_with("--")
        && flag.len() >= min_len
        && full.starts_with(flag)
        && flag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn git_has_exec_risk(words: &[String]) -> bool {
    let mut i = 1usize;
    while i < words.len() {
        let Some(tok) = words.get(i).map(String::as_str) else {
            break;
        };
        if tok == "--" || !tok.starts_with('-') || tok == "-" {
            return false;
        }
        if tok == "-c"
            || (tok.starts_with("-c") && tok.len() > 2 && !tok.starts_with("--"))
            || tok == "--config-env"
            || tok.starts_with("--config-env=")
            || long_prefix(
                tok.split_once('=').map(|(f, _)| f).unwrap_or(tok),
                "--config-env",
                4,
            )
            || tok == "--git-dir"
            || tok.starts_with("--git-dir=")
            || tok == "--work-tree"
            || tok.starts_with("--work-tree=")
            || long_prefix(
                tok.split_once('=').map(|(f, _)| f).unwrap_or(tok),
                "--git-dir",
                4,
            )
            || long_prefix(
                tok.split_once('=').map(|(f, _)| f).unwrap_or(tok),
                "--work-tree",
                4,
            )
        {
            return true;
        }
        if tok.starts_with("-C") && !tok.starts_with("--") && tok.len() > 2 {
            i += 1;
            continue;
        }
        if !tok.contains('=') && git_option_takes_value(tok) && words.get(i + 1).is_some() {
            i += 1;
        }
        i += 1;
    }
    false
}

fn git_option_takes_value(tok: &str) -> bool {
    matches!(
        tok,
        "-C" | "-c"
            | "--git-dir"
            | "--work-tree"
            | "--namespace"
            | "--super-prefix"
            | "--exec-path"
            | "--list-cmds"
            | "--attr-source"
            | "--config-env"
    ) || long_prefix(tok, "--config-env", 4)
        || long_prefix(tok, "--git-dir", 4)
        || long_prefix(tok, "--work-tree", 4)
        || long_prefix(tok, "--namespace", 7)
        || long_prefix(tok, "--super-prefix", 8)
        || long_prefix(tok, "--exec-path", 7)
        || long_prefix(tok, "--list-cmds", 7)
        || long_prefix(tok, "--attr-source", 8)
}

fn sort_compress(words: &[String]) -> bool {
    for word in words.iter().skip(1) {
        if word == "--" {
            break;
        }
        if word == "--compress-program" || word.starts_with("--compress-program=") {
            return true;
        }
        let flag = word
            .split_once('=')
            .map(|(f, _)| f)
            .unwrap_or(word.as_str());
        if long_prefix(flag, "--compress-program", 4) {
            return true;
        }
    }
    false
}

fn rg_pre(words: &[String]) -> bool {
    words.iter().skip(1).any(|word| {
        word == "--pre"
            || word.starts_with("--pre=")
            || word == "--pre-glob"
            || word.starts_with("--pre-glob=")
    })
}
