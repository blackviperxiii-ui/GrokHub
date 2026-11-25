//! Host rails. YOLO skips the prompt, not these.
//!
//! # What the floor promises
//!
//! [`forbidden_reason`] refuses a shell command when any word in it, read
//! the way the shell reads it, could name a guarded path: the key folders
//! (`.ssh`, `.gnupg`, `.aws`, `.kube`), the Grok sign-in file, the app's
//! secrets and pair tokens, `/etc/shadow` and `/etc/sudoers`.
//! [`names_consent`] does the same for the consent ledger.
//!
//! A word is read the way the shell reads it:
//!
//! - Quotes, backslash escapes, `$'…'` and `$"…"` strings and line
//!   continuations come out. On Windows, PowerShell's backtick escapes and
//!   cmd's `^` do too.
//! - Braces and `${x:-word}` give each of their words, `~` and the known
//!   folder variables (`$HOME`, `$env:APPDATA`, `%USERPROFILE%`) give their
//!   folders, and a variable the command sets gives its value.
//! - An expansion the floor can't know (`$x`, `${x}`, `$@`, `$(…)`,
//!   backticks) counts as any text within one path part: maybe empty, maybe
//!   starting with a dot, maybe splitting the word.
//! - Text that reads as a command (`bash -c '…'`, `eval "…"`, the inside of
//!   `$(…)`, a here-document) is read as a command too.
//! - A word is read from the folder the shell opens it in: the workspace, or
//!   wherever a `cd` or `pushd` in the command moves to. When the floor
//!   can't follow a move, it reads the word from the workspace, each folder
//!   above it, the home folder and `/`.
//! - A glob counts when, with bash's rules (no dotglob: `*`, `?` and `[…]`
//!   don't match a leading dot), it matches a guarded path's written-out
//!   form, so a guarded file that doesn't exist yet counts. It also counts
//!   when, read on the real filesystem, it matches a path that resolves
//!   (links followed) to a guarded path or inside one.
//! - Names compare without case everywhere. On Linux that refuses a little
//!   more than it must. On Windows a name also drops an NTFS stream suffix
//!   (`::$DATA`) and trailing dots and blanks, and `SSH~1` counts as the
//!   8.3 short name of `.ssh`.
//!
//! Ordinary commands still run: a word that names a folder that *holds* a
//! guarded path (`ls ~`, `cd ~/.config`) is not refused.
//!
//! # Its limit
//!
//! Text is all the floor sees. A program that builds a path when it runs is
//! outside it: `python3 -c` joining strings, a script file, `eval` of a
//! variable, a variable or command substitution whose value is a path, and
//! a tool that walks folders itself (`find ~`, `grep -r`, `tar`, `cp -r`).
//! With the shell sandbox on, its secrets masks (`grokhub-agent`
//! `sandbox::secrets`) hide those paths from every command, however it
//! names them. With the sandbox off, the floor is the only rail.
//!
//! The filesystem reads are bounded (time, folders, entries, paths). Past
//! the bound, a word counts only when it matches a guarded path as written.
//!
//! The literal checks still see a guarded name anywhere in the text, inside
//! code strings and after common rebuilds (`chr(46)`, `"a"+"b"`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// A guarded name written out anywhere in `c` (lowercase, `/` for `\`).
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

// ---------------------------------------------------------------------------
// Where a command runs
// ---------------------------------------------------------------------------

/// Where a command runs, and whose files the floor guards.
#[derive(Clone, Debug)]
pub struct Place {
    /// The folder the command starts in.
    pub cwd: PathBuf,
    /// The user's home folder.
    pub home: Option<PathBuf>,
    /// The app's config folders: the one in use and the default one.
    pub configs: Vec<PathBuf>,
    /// The Grok sign-in folder.
    pub grok_home: Option<PathBuf>,
    /// The shell is PowerShell or cmd, else a POSIX shell.
    pub windows: bool,
    /// Read the filesystem to match globs and follow links.
    pub fs: bool,
}

thread_local! {
    static WORKSPACE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Runs `f` with the floor reading commands as run in `workspace` (the
/// shell tool's folder), not in the process's own folder.
pub fn in_workspace<R>(workspace: &Path, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            WORKSPACE.with(|w| *w.borrow_mut() = prev);
        }
    }
    let prev = WORKSPACE.with(|w| w.replace(Some(workspace.to_path_buf())));
    let _restore = Restore(prev);
    f()
}

impl Place {
    /// A command run in `cwd` by this user, on this system.
    pub fn at(cwd: &Path) -> Self {
        let cwd = if cwd.is_absolute() {
            cwd.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(cwd)
        };
        let mut configs = vec![crate::paths::config_dir()];
        let default = crate::paths::default_config_dir();
        if !configs.contains(&default) {
            configs.push(default);
        }
        Self {
            cwd,
            home: crate::paths::user_home(),
            configs,
            grok_home: crate::cli_history::grok_home(),
            windows: cfg!(windows),
            fs: true,
        }
    }

    /// The workspace [`in_workspace`] names, else the process's folder.
    pub fn current() -> Self {
        let cwd = WORKSPACE
            .with(|w| w.borrow().clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self::at(&cwd)
    }
}

// ---------------------------------------------------------------------------
// Guarded paths
// ---------------------------------------------------------------------------

/// What the floor guards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guarded {
    SshKeys,
    Gnupg,
    Aws,
    Kube,
    Shadow,
    Sudoers,
    GrokLogin,
    AppSecrets,
    HubTokens,
    /// The consent ledger and the routine consent store.
    ConsentLedger,
}

impl Guarded {
    fn reason(self) -> &'static str {
        match self {
            Self::SshKeys => "forbidden path: ssh keys",
            Self::Gnupg => "forbidden path: gnupg",
            Self::Aws => "forbidden path: aws credentials",
            Self::Kube => "forbidden path: kube config",
            Self::Shadow => "forbidden path: /etc/shadow",
            Self::Sudoers => "forbidden path: /etc/sudoers",
            Self::GrokLogin => "forbidden path: grok login",
            Self::AppSecrets => "forbidden path: app secrets",
            Self::HubTokens => "forbidden path: hub pair tokens",
            Self::ConsentLedger => "consent ledger",
        }
    }
}

/// What [`forbidden_reason`] refuses.
const FLOOR: &[Guarded] = &[
    Guarded::SshKeys,
    Guarded::Gnupg,
    Guarded::Aws,
    Guarded::Kube,
    Guarded::Shadow,
    Guarded::Sudoers,
    Guarded::GrokLogin,
    Guarded::AppSecrets,
    Guarded::HubTokens,
];

/// A word whose braces give more words than the floor reads, where one of
/// them holds a `/`: it could be any path.
const BRACES_TOO_BIG: &str = "forbidden path: brace expansion too large to check";
/// A Windows path with more parts of only dots and blanks (`...`) than the
/// floor reads each way.
const DOTS_TOO_MANY: &str = "forbidden path: too many dot-only path parts to check";

/// Where a path starts.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Root {
    /// `/`.
    Posix,
    /// A Windows drive, lowercase.
    Drive(char),
    /// Some drive the floor can't tell.
    AnyDrive,
    /// A path the floor can't place (relative, or empty).
    Elsewhere,
}

/// A part of a guarded path's written-out form.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GPart {
    /// This name, lowercase.
    Name(Vec<char>),
    /// Some user's name (`/home/<name>`): any name that doesn't start with
    /// a dot.
    AnyName,
}

/// One written-out form of a guarded path.
#[derive(Clone, Debug)]
struct GuardForm {
    root: Root,
    parts: Vec<GPart>,
    what: Guarded,
}

/// Each guarded path under a home folder.
const HOME_GUARDS: &[(&[&str], Guarded)] = &[
    (&[".ssh"], Guarded::SshKeys),
    (&[".gnupg"], Guarded::Gnupg),
    (&[".aws"], Guarded::Aws),
    (&[".kube"], Guarded::Kube),
    (&[".grok", "auth.json"], Guarded::GrokLogin),
];

/// Each guarded file in the app's config folder.
const CONFIG_GUARDS: &[(&[&str], Guarded)] = &[
    (&["app.json"], Guarded::AppSecrets),
    (&["secrets.json"], Guarded::AppSecrets),
    (&["hub-state.json"], Guarded::HubTokens),
    (&["consent.jsonl"], Guarded::ConsentLedger),
    (&["routines", "consent.json"], Guarded::ConsentLedger),
];

/// Where the app's config folder sits under a home folder: Linux and macOS,
/// and Windows (`%APPDATA%`).
const CONFIG_UNDER_HOME: &[&[&str]] = &[&[".config"], &["appdata", "roaming"]];

fn name(text: &str) -> GPart {
    GPart::Name(text.to_lowercase().chars().collect())
}

/// A real path's root and lowercase parts: `C:\x` on drive C, `\\?\C:\x`
/// too, `/x` from the root.
fn path_parts(path: &Path) -> Option<(Root, Vec<String>)> {
    let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let text = text
        .strip_prefix("//?/")
        .or_else(|| text.strip_prefix("//./"))
        .unwrap_or(&text);
    let (root, rest) = match text.as_bytes() {
        [d, b':', ..] if d.is_ascii_alphabetic() => (Root::Drive(char::from(*d)), &text[2..]),
        [b'/', ..] => (Root::Posix, text),
        _ => return None,
    };
    let parts = rest
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .map(str::to_string)
        .collect();
    Some((root, parts))
}

/// A path and, when a link leads elsewhere, the path it resolves to.
fn both_spellings(path: &Path) -> Vec<(Root, Vec<GPart>)> {
    let mut out = Vec::new();
    let real = path.canonicalize().ok().filter(|c| c != path);
    for p in std::iter::once(path.to_path_buf()).chain(real) {
        if let Some((root, parts)) = path_parts(&p) {
            out.push((root, parts.iter().map(|p| name(p)).collect()));
        }
    }
    out
}

/// The written-out forms of `kinds`: under this user's home folder, under
/// any user's home folder (`/root`, `/home/<name>`, `/Users/<name>`,
/// `C:\Users\<name>`), in the app's config folders, the sign-in folder,
/// and in `/etc`.
fn guard_forms(place: &Place, kinds: &[Guarded]) -> Vec<GuardForm> {
    let named = |parts: &[&str]| parts.iter().map(|p| name(p)).collect::<Vec<_>>();
    let under = |first: &[&str]| {
        let mut parts = named(first);
        parts.push(GPart::AnyName);
        parts
    };
    let mut homes: Vec<(Root, Vec<GPart>)> = vec![
        (Root::Posix, named(&["root"])),
        (Root::Posix, under(&["home"])),
        (Root::Posix, under(&["users"])),
        (Root::Posix, under(&["var", "home"])),
        (Root::AnyDrive, under(&["users"])),
        (Root::AnyDrive, under(&["documents and settings"])),
    ];
    if place.windows {
        // Bash on Windows: `/c/Users/<name>` (Git Bash, MSYS2),
        // `/mnt/c/Users/<name>` (WSL), `/cygdrive/c/Users/<name>`.
        for first in [&[][..], &["mnt"], &["cygdrive"]] {
            let mut parts = named(first);
            parts.extend([GPart::AnyName, name("users"), GPart::AnyName]);
            homes.push((Root::Posix, parts));
        }
    }
    if let Some(home) = &place.home {
        homes.extend(both_spellings(home));
    }
    let mut out = Vec::new();
    let mut push = |root: &Root, base: &[GPart], tail: &[&str], what: Guarded| {
        if kinds.contains(&what) {
            let mut parts = base.to_vec();
            parts.extend(tail.iter().map(|t| name(t)));
            out.push(GuardForm {
                root: root.clone(),
                parts,
                what,
            });
        }
    };
    for (root, base) in &homes {
        for (tail, what) in HOME_GUARDS {
            push(root, base, tail, *what);
        }
        for dir in CONFIG_UNDER_HOME {
            let mut dir: Vec<&str> = dir.to_vec();
            dir.push(crate::brand::INTERNAL_ID);
            let mut base = base.clone();
            base.extend(named(&dir));
            for (tail, what) in CONFIG_GUARDS {
                push(root, &base, tail, *what);
            }
        }
    }
    for config in &place.configs {
        for (root, base) in both_spellings(config) {
            for (tail, what) in CONFIG_GUARDS {
                push(&root, &base, tail, *what);
            }
        }
    }
    if let Some(grok) = &place.grok_home {
        for (root, base) in both_spellings(grok) {
            push(&root, &base, &["auth.json"], Guarded::GrokLogin);
        }
    }
    push(&Root::Posix, &[], &["etc", "shadow"], Guarded::Shadow);
    push(&Root::Posix, &[], &["etc", "sudoers"], Guarded::Sudoers);
    push(&Root::Posix, &[], &["etc", "sudoers.d"], Guarded::Sudoers);
    out
}

// ---------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------

/// One character of a word, as the shell reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Atom {
    /// A character taken as written: quoted, escaped or plain.
    Lit(char),
    /// A path separator: `/`, and `\` on Windows.
    Sep,
    /// An unquoted `*`.
    Star,
    /// An unquoted `?`.
    One,
    /// An unquoted `[…]`: its body between the brackets.
    Class(Vec<char>),
    /// An expansion the floor can't know: any text within one path part,
    /// maybe empty, maybe starting with a dot. Unquoted (`split`), its
    /// value may also hold blanks that split the word.
    Any { split: bool },
}

/// A word before its choices are made.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    A(Atom),
    /// One of these (`{a,b}`, `${x:-word}`).
    Choice(Vec<Vec<Piece>>),
}

/// A word of a command.
#[derive(Clone, Debug, Default)]
struct Word {
    pieces: Vec<Piece>,
    /// The text as written.
    raw: String,
    /// Some of it was quoted or escaped.
    quoted: bool,
    /// Where the first unquoted `=` falls in `pieces` (`--file=x`, `x=y`).
    eq: Option<usize>,
}

impl Word {
    /// The word's text when it is all plain characters (`cd`, `-P`).
    fn plain(&self) -> Option<String> {
        self.pieces
            .iter()
            .map(|p| match p {
                Piece::A(Atom::Lit(c)) => Some(*c),
                Piece::A(Atom::Sep) => Some('/'),
                _ => None,
            })
            .collect()
    }
}

enum Tok {
    Word(Word),
    /// An operator or a line end.
    Op,
}

/// How many spellings the floor reads a word as before it reads its next
/// choice as any text ([`alternatives`]).
const MAX_ALTERNATIVES: usize = 256;
/// How deep the floor reads commands inside commands.
const MAX_DEPTH: usize = 4;

/// Reads one command text into words, the way bash (or, on Windows,
/// PowerShell and cmd) splits and unquotes it.
struct Lexer<'p> {
    c: Vec<char>,
    i: usize,
    place: &'p Place,
    /// Read as PowerShell or cmd, not as a POSIX shell.
    ps: bool,
    /// Values the command gives variables (`x=~/.ssh`).
    vars: HashMap<String, Vec<Vec<Piece>>>,
    /// Command texts inside this one: `$(…)`, backticks, here-documents.
    nested: Vec<String>,
    /// Here-documents waiting for the line to end: delimiter and `<<-`.
    heredocs: Vec<(String, bool)>,
    /// A word names a folder the floor can't follow (`~-`, `$OLDPWD`).
    lost: bool,
}

impl<'p> Lexer<'p> {
    fn new(text: &str, place: &'p Place, ps: bool, vars: HashMap<String, Vec<Vec<Piece>>>) -> Self {
        Self {
            c: text.chars().collect(),
            i: 0,
            place,
            ps,
            vars,
            nested: Vec::new(),
            heredocs: Vec::new(),
            lost: false,
        }
    }

    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    /// Read as PowerShell or cmd, not as a POSIX shell.
    fn ps(&self) -> bool {
        self.ps
    }

    /// Characters that end a word outside quotes.
    fn ends_word(&self, ch: char) -> bool {
        ch.is_whitespace()
            || matches!(ch, ';' | '&' | '|' | '(' | ')' | '<' | '>')
            || (self.ps() && matches!(ch, '{' | '}' | ','))
    }

    fn tokens(&mut self) -> Vec<Tok> {
        let mut toks = Vec::new();
        while let Some(ch) = self.at(0) {
            match ch {
                '\\' if !self.ps() && self.at(1) == Some('\n') => self.i += 2,
                '`' if self.ps() && self.at(1) == Some('\n') => self.i += 2,
                '\n' => {
                    self.i += 1;
                    toks.push(Tok::Op);
                    self.read_heredocs();
                }
                c if c.is_whitespace() => self.i += 1,
                '#' => {
                    while self.at(0).is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                '<' if self.ps() && self.at(1) == Some('#') => {
                    // A PowerShell block comment.
                    let close = (self.i + 2..self.c.len().saturating_sub(1))
                        .find(|&j| self.c[j] == '#' && self.c[j + 1] == '>');
                    self.i = close.map_or(self.c.len(), |j| j + 2);
                }
                '<' if !self.ps() && self.at(1) == Some('<') && self.at(2) != Some('<') => {
                    self.heredoc_start();
                    toks.push(Tok::Op);
                }
                c if self.ends_word(c) => {
                    self.i += 1;
                    toks.push(Tok::Op);
                }
                _ => toks.push(Tok::Word(self.word())),
            }
        }
        self.read_heredocs();
        toks
    }

    /// `<<WORD` or `<<-WORD`: the body starts on the next line.
    fn heredoc_start(&mut self) {
        self.i += 2;
        let strip = self.at(0) == Some('-');
        if strip {
            self.i += 1;
        }
        while self.at(0).is_some_and(|c| c == ' ' || c == '\t') {
            self.i += 1;
        }
        if self.at(0).is_none_or(|c| self.ends_word(c)) {
            return;
        }
        let w = self.word();
        self.heredocs.push((decoded(&w.pieces), strip));
    }

    /// The bodies of the here-documents the line just ended opened. Each is
    /// read as a command: it may be a script a shell runs (`bash <<EOF`).
    fn read_heredocs(&mut self) {
        for (delim, strip) in std::mem::take(&mut self.heredocs) {
            let mut body = String::new();
            while self.i < self.c.len() {
                let end = (self.i..self.c.len())
                    .find(|&j| self.c[j] == '\n')
                    .unwrap_or(self.c.len());
                let line: String = self.c[self.i..end].iter().collect();
                self.i = (end + 1).min(self.c.len());
                let cmp = if strip {
                    line.trim_start_matches('\t')
                } else {
                    &line
                };
                if cmp == delim {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            self.nested.push(body);
        }
    }

    /// One word, from here to the next blank or operator.
    fn word(&mut self) -> Word {
        let start = self.i;
        let mut w = Word::default();
        w.pieces = self.pieces(false, &mut w).0;
        w.raw = self.c[start..self.i].iter().collect();
        if let Some(eq) = w.eq {
            self.assign(&w, eq);
        }
        w
    }

    /// `name=value`: a later `$name` reads as `value` too.
    fn assign(&mut self, w: &Word, eq: usize) {
        let name: Option<String> = w.pieces[..eq]
            .iter()
            .map(|p| match p {
                Piece::A(Atom::Lit(c)) if c.is_ascii_alphanumeric() || *c == '_' => Some(*c),
                _ => None,
            })
            .collect();
        let Some(name) =
            name.filter(|n| !n.is_empty() && !n.starts_with(|c: char| c.is_ascii_digit()))
        else {
            return;
        };
        let value = w.pieces[eq + 1..].to_vec();
        self.vars.entry(name).or_default().push(value);
    }

    /// Pieces up to the end of the word or, inside braces (`in_brace`), up
    /// to a `,` or `}` at this level, which is returned.
    fn pieces(&mut self, in_brace: bool, w: &mut Word) -> (Vec<Piece>, Option<char>) {
        let mut out: Vec<Piece> = Vec::new();
        let start = self.i;
        while let Some(ch) = self.at(0) {
            if self.ends_word(ch) {
                return (out, None);
            }
            if in_brace && matches!(ch, ',' | '}') {
                return (out, Some(ch));
            }
            let tilde_ok =
                self.i == start || matches!(self.c.get(self.i.wrapping_sub(1)), Some('=' | ':'));
            if self.ps() {
                self.ps_char(ch, tilde_ok, &mut out, w);
            } else {
                self.sh_char(ch, tilde_ok, &mut out, w);
            }
        }
        (out, None)
    }

    fn lit(&self, out: &mut Vec<Piece>, ch: char) {
        out.push(Piece::A(
            if ch == '/' || (self.place.windows && ch == '\\') {
                Atom::Sep
            } else {
                Atom::Lit(ch)
            },
        ));
    }

    fn lits(&self, out: &mut Vec<Piece>, text: &str) {
        for ch in text.chars() {
            self.lit(out, ch);
        }
    }

    /// One unquoted character (or what it starts) of a POSIX word.
    fn sh_char(&mut self, ch: char, tilde_ok: bool, out: &mut Vec<Piece>, w: &mut Word) {
        match ch {
            '\\' => {
                w.quoted = true;
                match self.at(1) {
                    Some('\n') => {}
                    Some(next) => self.lit(out, next),
                    None => self.lit(out, '\\'),
                }
                self.i = (self.i + 2).min(self.c.len());
            }
            '\'' => match (self.i + 1..self.c.len()).find(|&j| self.c[j] == '\'') {
                Some(close) => {
                    w.quoted = true;
                    for j in self.i + 1..close {
                        self.lit(out, self.c[j]);
                    }
                    self.i = close + 1;
                }
                // An open quote: the shell refuses the line; read on as if
                // the quote weren't there.
                None => {
                    self.lit(out, '\'');
                    self.i += 1;
                }
            },
            '"' => self.double_quoted(out, w),
            '$' if self.at(1) == Some('\'') => {
                w.quoted = true;
                self.ansi_c(out);
            }
            '$' if self.at(1) == Some('"') => {
                self.i += 1;
                self.double_quoted(out, w);
            }
            '$' => self.dollar(false, out),
            '`' => self.backtick(false, out),
            '*' | '?' | '+' | '@' | '!' if self.at(1) == Some('(') => {
                // An extglob group: it may match any name.
                let close = matching(&self.c, self.i + 1, '(', ')');
                out.push(Piece::A(Atom::Any { split: false }));
                self.i = (close + 1).min(self.c.len());
            }
            '*' => {
                out.push(Piece::A(Atom::Star));
                self.i += 1;
            }
            '?' => {
                out.push(Piece::A(Atom::One));
                self.i += 1;
            }
            '[' => self.class(out),
            '{' => self.brace(out, w),
            '~' if tilde_ok => self.tilde(out),
            c => {
                if c == '=' && w.eq.is_none() {
                    w.eq = Some(out.len());
                }
                self.lit(out, c);
                self.i += 1;
            }
        }
    }

    /// One character (or what it starts) of a PowerShell or cmd word.
    /// Quotes only group: the cmdlets expand `*`, `?` and `[…]` inside
    /// them too.
    fn ps_char(&mut self, ch: char, tilde_ok: bool, out: &mut Vec<Piece>, w: &mut Word) {
        match ch {
            '`' => {
                w.quoted = true;
                self.i += 1;
                self.ps_escape(out);
            }
            '^' => {
                // cmd's escape.
                w.quoted = true;
                if let Some(next) = self.at(1) {
                    self.lit(out, next);
                }
                self.i = (self.i + 2).min(self.c.len());
            }
            '\'' => {
                let mut j = self.i + 1;
                let mut body = Vec::new();
                let mut closed = false;
                while j < self.c.len() {
                    if self.c[j] == '\'' {
                        if self.c.get(j + 1) == Some(&'\'') {
                            body.push('\'');
                            j += 2;
                            continue;
                        }
                        closed = true;
                        break;
                    }
                    body.push(self.c[j]);
                    j += 1;
                }
                if closed {
                    w.quoted = true;
                    for c in body {
                        self.ps_glob_or_lit(c, out);
                    }
                    self.i = j + 1;
                } else {
                    self.lit(out, '\'');
                    self.i += 1;
                }
            }
            '"' => self.ps_double_quoted(out, w),
            '$' => self.ps_dollar(false, out),
            '%' => self.cmd_percent(out),
            '[' => self.class(out),
            '*' | '?' => {
                self.ps_glob_or_lit(ch, out);
                self.i += 1;
            }
            '~' if tilde_ok => self.tilde(out),
            c => {
                let flag_value = c == ':' && self.c.get(w_start(&self.c, self.i)) == Some(&'-');
                if (c == '=' || flag_value) && w.eq.is_none() {
                    w.eq = Some(out.len());
                }
                self.lit(out, c);
                self.i += 1;
            }
        }
    }

    fn ps_glob_or_lit(&self, c: char, out: &mut Vec<Piece>) {
        match c {
            '*' => out.push(Piece::A(Atom::Star)),
            '?' => out.push(Piece::A(Atom::One)),
            c => self.lit(out, c),
        }
    }

    /// The character a PowerShell backtick just before `self.i` escapes.
    fn ps_escape(&mut self, out: &mut Vec<Piece>) {
        let Some(next) = self.at(0) else {
            return;
        };
        if next == 'u' && self.at(1) == Some('{') {
            let close = (self.i + 2..self.c.len().min(self.i + 12)).find(|&j| self.c[j] == '}');
            if let Some(close) = close {
                let hex: String = self.c[self.i + 2..close].iter().collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(Piece::A(Atom::Lit(ch)));
                    self.i = close + 1;
                    return;
                }
            }
        }
        self.i += 1;
        match next {
            // A line continuation.
            '\n' => {}
            'n' => out.push(Piece::A(Atom::Lit('\n'))),
            't' => out.push(Piece::A(Atom::Lit('\t'))),
            '0' => out.push(Piece::A(Atom::Lit('\0'))),
            c => out.push(Piece::A(Atom::Lit(c))),
        }
    }

    /// `"…"` in PowerShell: backtick escapes, `""`, and `$` expansions.
    fn ps_double_quoted(&mut self, out: &mut Vec<Piece>, w: &mut Word) {
        let Some(close) = ps_quote_end(&self.c, self.i + 1) else {
            self.lit(out, '"');
            self.i += 1;
            return;
        };
        w.quoted = true;
        self.i += 1;
        while self.i < close {
            match self.c[self.i] {
                '`' => {
                    self.i += 1;
                    self.ps_escape(out);
                }
                '"' => {
                    // `""` inside is one quote.
                    self.lit(out, '"');
                    self.i += 2;
                }
                '$' => self.ps_dollar(true, out),
                '%' => self.cmd_percent(out),
                c => {
                    self.ps_glob_or_lit(c, out);
                    self.i += 1;
                }
            }
        }
        self.i = close + 1;
    }

    /// `$…` in PowerShell: `$env:NAME`, `${env:NAME}`, `$HOME`, `$PWD`,
    /// `$(…)`, or a variable the floor can't know.
    fn ps_dollar(&mut self, quoted: bool, out: &mut Vec<Piece>) {
        match self.at(1) {
            Some('(') => {
                let close = matching(&self.c, self.i + 1, '(', ')');
                let inner: String = self.c[self.i + 2..close.min(self.c.len())].iter().collect();
                self.nested.push(inner);
                out.push(Piece::A(Atom::Any { split: !quoted }));
                self.i = (close + 1).min(self.c.len());
            }
            Some('{') => {
                let close = (self.i + 2..self.c.len())
                    .find(|&j| self.c[j] == '}')
                    .unwrap_or(self.c.len());
                let name: String = self.c[self.i + 2..close].iter().collect();
                self.i = (close + 1).min(self.c.len());
                self.var(&name, quoted, out);
            }
            Some(c) if c.is_alphanumeric() || c == '_' => {
                let mut j = self.i + 1;
                while let Some(&c) = self.c.get(j) {
                    let scope = c == ':' && self.c.get(j + 1).is_some_and(|n| n.is_alphanumeric());
                    if !(c.is_alphanumeric() || c == '_' || scope) {
                        break;
                    }
                    j += 1;
                }
                let name: String = self.c[self.i + 1..j].iter().collect();
                self.i = j;
                self.var(&name, quoted, out);
            }
            _ => {
                self.lit(out, '$');
                self.i += 1;
            }
        }
    }

    /// `%NAME%` (cmd).
    fn cmd_percent(&mut self, out: &mut Vec<Piece>) {
        let close =
            (self.i + 1..self.c.len()).find(|&j| self.c[j] == '%' || self.c[j].is_whitespace());
        match close {
            Some(close) if self.c[close] == '%' && close > self.i + 1 => {
                let name: String = self.c[self.i + 1..close].iter().collect();
                self.i = close + 1;
                self.var(&name, true, out);
            }
            _ => {
                self.lit(out, '%');
                self.i += 1;
            }
        }
    }

    /// `"…"` in a POSIX shell: `\` escapes `$`, `` ` ``, `"`, `\` and a
    /// newline; `$` and backticks expand; globs are plain.
    fn double_quoted(&mut self, out: &mut Vec<Piece>, w: &mut Word) {
        let Some(close) = sh_quote_end(&self.c, self.i + 1) else {
            self.lit(out, '"');
            self.i += 1;
            return;
        };
        w.quoted = true;
        self.i += 1;
        while self.i < close {
            match self.c[self.i] {
                '\\' => match self.c.get(self.i + 1) {
                    Some('\n') => self.i += 2,
                    Some(&next @ ('$' | '`' | '"' | '\\')) => {
                        self.lit(out, next);
                        self.i += 2;
                    }
                    _ => {
                        self.lit(out, '\\');
                        self.i += 1;
                    }
                },
                '$' => self.dollar(true, out),
                '`' => self.backtick(true, out),
                c => {
                    self.lit(out, c);
                    self.i += 1;
                }
            }
        }
        self.i = close + 1;
    }

    /// `$'…'`: backslash escapes decoded.
    fn ansi_c(&mut self, out: &mut Vec<Piece>) {
        let mut j = self.i + 2;
        while j < self.c.len() {
            match self.c[j] {
                '\\' => {
                    let (ch, next) = escaped(&self.c, j + 1);
                    if let Some(ch) = ch {
                        self.lit(out, ch);
                    }
                    j = next;
                }
                '\'' => break,
                c => {
                    self.lit(out, c);
                    j += 1;
                }
            }
        }
        self.i = (j + 1).min(self.c.len());
    }

    /// `$…` in a POSIX shell.
    fn dollar(&mut self, quoted: bool, out: &mut Vec<Piece>) {
        let any = Piece::A(Atom::Any { split: !quoted });
        match self.at(1) {
            Some('(') if self.at(2) == Some('(') => {
                // Arithmetic: a number.
                let close = matching(&self.c, self.i + 1, '(', ')');
                out.push(Piece::A(Atom::Any { split: false }));
                self.i = (close + 1).min(self.c.len());
            }
            Some('(') => {
                let close = matching(&self.c, self.i + 1, '(', ')');
                let inner: String = self.c[self.i + 2..close.min(self.c.len())].iter().collect();
                self.nested.push(inner);
                out.push(any);
                self.i = (close + 1).min(self.c.len());
            }
            Some('{') => {
                let close = matching(&self.c, self.i + 1, '{', '}');
                let body: String = self.c[self.i + 2..close.min(self.c.len())].iter().collect();
                self.i = (close + 1).min(self.c.len());
                self.parameter(&body, quoted, out);
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut j = self.i + 1;
                while self
                    .c
                    .get(j)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    j += 1;
                }
                let name: String = self.c[self.i + 1..j].iter().collect();
                self.i = j;
                self.var(&name, quoted, out);
            }
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                out.push(any);
                self.i += 2;
            }
            _ => {
                self.lit(out, '$');
                self.i += 1;
            }
        }
    }

    /// `${…}`: a name, maybe with a word used when it is unset
    /// (`${x:-word}`) or set (`${x:+word}`). Any other form is unknown.
    fn parameter(&mut self, body: &str, quoted: bool, out: &mut Vec<Piece>) {
        let name_len = body
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(body.len());
        let (name, rest) = body.split_at(name_len);
        if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
            out.push(Piece::A(Atom::Any { split: !quoted }));
            return;
        }
        let mut value = Vec::new();
        self.var(name, quoted, &mut value);
        if rest.is_empty() {
            out.extend(value);
            return;
        }
        let op = [":-", ":=", ":+", ":?", "-", "=", "+", "?"]
            .into_iter()
            .find(|op| rest.starts_with(op));
        let Some(op) = op else {
            out.push(Piece::A(Atom::Any { split: !quoted }));
            return;
        };
        let mut sub = Lexer::new(&rest[op.len()..], self.place, self.ps, self.vars.clone());
        let mut sw = Word::default();
        let mut word = Vec::new();
        while let Some(ch) = sub.at(0) {
            if sub.ends_word(ch) {
                sub.lit(&mut word, ch);
                sub.i += 1;
            } else {
                word.append(&mut sub.pieces(false, &mut sw).0);
            }
        }
        self.nested.append(&mut sub.nested);
        self.lost |= sub.lost;
        match op.trim_start_matches(':') {
            "-" | "=" => out.push(Piece::Choice(vec![value, word])),
            "+" => out.push(Piece::Choice(vec![Vec::new(), word])),
            _ => out.extend(value),
        }
    }

    /// What `$name` reads as: the folder the floor knows it for, or any
    /// text, and each value the command gave it.
    fn var(&mut self, name: &str, quoted: bool, out: &mut Vec<Piece>) {
        let mut first = Vec::new();
        match self.known_var(name) {
            Some(dir) => self.lits(&mut first, &dir),
            None => first.push(Piece::A(Atom::Any { split: !quoted })),
        }
        let Some(values) = self.vars.get(name) else {
            out.extend(first);
            return;
        };
        let mut choices = vec![first];
        choices.extend(values.iter().map(|v| {
            v.iter()
                .map(|p| match p {
                    // Unquoted, a value's `*` and `?` glob.
                    Piece::A(Atom::Lit('*')) if !quoted => Piece::A(Atom::Star),
                    Piece::A(Atom::Lit('?')) if !quoted => Piece::A(Atom::One),
                    p => p.clone(),
                })
                .collect()
        }));
        out.push(Piece::Choice(choices));
    }

    /// The folder a variable names, when the floor knows it.
    fn known_var(&mut self, name: &str) -> Option<String> {
        let upper = name.to_ascii_uppercase();
        let name = upper.strip_prefix("ENV:").unwrap_or(&upper);
        let home = self
            .place
            .home
            .as_ref()
            .map(|h| h.to_string_lossy().into_owned());
        let env = |key: &str| std::env::var(key).ok().filter(|v| !v.trim().is_empty());
        let under_home = |rest: &str| home.as_ref().map(|h| format!("{h}/{rest}"));
        match name {
            "HOME" => home.clone(),
            "PWD" => Some(".".into()),
            "OLDPWD" => {
                self.lost = true;
                Some(".".into())
            }
            "USERPROFILE" => env("USERPROFILE").or_else(|| home.clone()),
            "APPDATA" => env("APPDATA").or_else(|| under_home("AppData/Roaming")),
            "LOCALAPPDATA" => env("LOCALAPPDATA").or_else(|| under_home("AppData/Local")),
            "XDG_CONFIG_HOME" => env("XDG_CONFIG_HOME").or_else(|| under_home(".config")),
            "GROKHUB_CONFIG" | "XDG_DATA_HOME" | "TMPDIR" | "TEMP" | "TMP" => env(name),
            _ => None,
        }
    }

    /// `` `…` ``: a command whose output the floor can't know.
    fn backtick(&mut self, quoted: bool, out: &mut Vec<Piece>) {
        let mut j = self.i + 1;
        let mut inner = String::new();
        while j < self.c.len() && self.c[j] != '`' {
            if self.c[j] == '\\' && matches!(self.c.get(j + 1), Some('`' | '$' | '\\')) {
                j += 1;
            }
            inner.push(self.c[j]);
            j += 1;
        }
        self.nested.push(inner);
        out.push(Piece::A(Atom::Any { split: !quoted }));
        self.i = (j + 1).min(self.c.len());
    }

    /// `[…]`: a class when it closes within the word, else a plain `[`.
    fn class(&mut self, out: &mut Vec<Piece>) {
        let end = class_end(&self.c, self.i).filter(|&end| {
            self.c[self.i + 1..end]
                .iter()
                .all(|&c| c != '/' && !c.is_whitespace() && !matches!(c, '\'' | '"'))
        });
        match end {
            Some(end) => {
                out.push(Piece::A(Atom::Class(self.c[self.i + 1..end].to_vec())));
                self.i = end + 1;
            }
            None => {
                self.lit(out, '[');
                self.i += 1;
            }
        }
    }

    /// `{a,b}` or `{x..y}`: each word it gives; anything else is plain.
    fn brace(&mut self, out: &mut Vec<Piece>, w: &mut Word) {
        let (start, quoted, eq, nested) = (self.i, w.quoted, w.eq, self.nested.len());
        let restore = |lx: &mut Self, w: &mut Word, out: &mut Vec<Piece>| {
            lx.i = start + 1;
            w.quoted = quoted;
            w.eq = eq;
            lx.nested.truncate(nested);
            lx.lit(out, '{');
        };
        self.i += 1;
        let mut items: Vec<Vec<Piece>> = Vec::new();
        loop {
            let (item, stop) = self.pieces(true, w);
            items.push(item);
            match stop {
                Some(',') => self.i += 1,
                Some(_) => {
                    self.i += 1;
                    break;
                }
                None => return restore(self, w, out),
            }
        }
        w.eq = eq;
        if items.len() >= 2 {
            out.push(Piece::Choice(items));
            return;
        }
        let body: String = self.c[start + 1..self.i - 1].iter().collect();
        match seq_items(&body) {
            Some(seq) if !seq.is_empty() => out.push(Piece::Choice(
                seq.into_iter()
                    .map(|item| item.chars().map(|c| Piece::A(Atom::Lit(c))).collect())
                    .collect(),
            )),
            // Past 64 items: any text there.
            Some(_) => out.push(Piece::A(Atom::Any { split: false })),
            None => restore(self, w, out),
        }
    }

    /// `~`, `~/…`, `~+`, `~-` or `~name` at the start of a word.
    fn tilde(&mut self, out: &mut Vec<Piece>) {
        let mut j = self.i + 1;
        while let Some(&c) = self.c.get(j) {
            let sep = c == '/' || (self.place.windows && c == '\\');
            if sep || self.ends_word(c) || matches!(c, ',' | '}' | ':') {
                break;
            }
            j += 1;
        }
        let user: String = self.c[self.i + 1..j].iter().collect();
        if !user
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        {
            self.lit(out, '~');
            self.i += 1;
            return;
        }
        self.i = j;
        let home = self.place.home.clone();
        match user.as_str() {
            "" => match home {
                Some(h) => self.lits(out, &h.to_string_lossy()),
                None => out.push(Piece::A(Atom::Any { split: false })),
            },
            "+" => self.lit(out, '.'),
            "-" => {
                self.lost = true;
                self.lit(out, '.');
            }
            user => {
                let mut homes = vec![
                    format!("/home/{user}"),
                    format!("/Users/{user}"),
                    format!("/var/home/{user}"),
                ];
                if user == "root" {
                    homes.push("/root".into());
                }
                let named = |h: &PathBuf| {
                    h.file_name()
                        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(user))
                };
                if let Some(h) = home.filter(named) {
                    homes.push(h.to_string_lossy().into_owned());
                }
                let homes = homes
                    .iter()
                    .map(|h| {
                        let mut p = Vec::new();
                        self.lits(&mut p, h);
                        p
                    })
                    .collect();
                out.push(Piece::Choice(homes));
            }
        }
    }
}

/// Where the word that holds `c[at]` starts.
fn w_start(c: &[char], at: usize) -> usize {
    (0..at)
        .rev()
        .find(|&j| c[j].is_whitespace())
        .map_or(0, |j| j + 1)
}

/// Where the POSIX `"…"` whose body starts at `from` closes.
fn sh_quote_end(c: &[char], from: usize) -> Option<usize> {
    let mut j = from;
    while j < c.len() {
        match c[j] {
            '\\' => j += 2,
            '"' => return Some(j),
            '$' if c.get(j + 1) == Some(&'(') => j = matching(c, j + 1, '(', ')') + 1,
            '`' => {
                j += 1;
                while j < c.len() && c[j] != '`' {
                    j += if c[j] == '\\' { 2 } else { 1 };
                }
                j += 1;
            }
            _ => j += 1,
        }
    }
    None
}

/// Where the PowerShell `"…"` whose body starts at `from` closes.
fn ps_quote_end(c: &[char], from: usize) -> Option<usize> {
    let mut j = from;
    while j < c.len() {
        match c[j] {
            '`' => j += 2,
            '"' if c.get(j + 1) == Some(&'"') => j += 2,
            '"' => return Some(j),
            _ => j += 1,
        }
    }
    None
}

/// Where the bracket that opens at `c[open]` closes, quotes and nesting
/// skipped; the end of the text when it doesn't.
fn matching(c: &[char], open: usize, left: char, right: char) -> usize {
    let mut depth = 0usize;
    let mut j = open;
    while j < c.len() {
        match c[j] {
            '\\' => j += 1,
            '\'' if left == '(' => {
                j = (j + 1..c.len()).find(|&k| c[k] == '\'').unwrap_or(c.len());
            }
            '"' => j = sh_quote_end(c, j + 1).unwrap_or(c.len()),
            ch if ch == left => depth += 1,
            ch if ch == right => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
        j += 1;
    }
    c.len()
}

/// The character a backslash at `chars[i - 1]` stands for in `$'…'`, and
/// where the text goes on.
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
            .map(|ch| (Some(ch), end))
    };
    let decoded = match c {
        '\n' => return (None, i + 1),
        'x' => digits(i + 1, 16, 2),
        'u' => digits(i + 1, 16, 4),
        'U' => digits(i + 1, 16, 8),
        '0'..='7' => digits(i, 8, 3),
        'n' => Some((Some('\n'), i + 1)),
        't' => Some((Some('\t'), i + 1)),
        _ => None,
    };
    decoded.unwrap_or((Some(c), i + 1))
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

/// The text a word's pieces spell, the first of each choice taken: a
/// here-document's delimiter.
fn decoded(pieces: &[Piece]) -> String {
    let mut out = String::new();
    for p in pieces {
        match p {
            Piece::A(a) => push_atom_text(&mut out, a),
            Piece::Choice(items) => {
                if let Some(first) = items.first() {
                    out.push_str(&decoded(first));
                }
            }
        }
    }
    out
}

/// An atom's text; an expansion the floor can't know reads as nothing.
fn push_atom_text(out: &mut String, a: &Atom) {
    match a {
        Atom::Lit(c) => out.push(*c),
        Atom::Sep => out.push('/'),
        Atom::Star => out.push('*'),
        Atom::One => out.push('?'),
        Atom::Class(body) => {
            out.push('[');
            out.extend(body);
            out.push(']');
        }
        Atom::Any { .. } => {}
    }
}

/// A word's text with one layer of quoting taken off, the way a program it
/// is handed (`bash -c`, `eval`, `pwsh -Command`) reads it.
fn unquote_once(raw: &str, ps: bool) -> String {
    let c: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\\' if !ps => {
                if let Some(&next) = c.get(i + 1) {
                    if next != '\n' {
                        out.push(next);
                    }
                }
                i += 2;
            }
            '`' if ps => {
                out.extend(c.get(i + 1));
                i += 2;
            }
            '\'' => {
                let close = (i + 1..c.len()).find(|&j| c[j] == '\'').unwrap_or(c.len());
                out.extend(&c[i + 1..close]);
                i = close + 1;
            }
            '$' if !ps && c.get(i + 1) == Some(&'\'') => {
                let mut j = i + 2;
                while j < c.len() && c[j] != '\'' {
                    if c[j] == '\\' {
                        let (ch, next) = escaped(&c, j + 1);
                        out.extend(ch);
                        j = next;
                    } else {
                        out.push(c[j]);
                        j += 1;
                    }
                }
                i = j + 1;
            }
            '"' => {
                let close = if ps {
                    ps_quote_end(&c, i + 1)
                } else {
                    sh_quote_end(&c, i + 1)
                }
                .unwrap_or(c.len());
                let mut j = i + 1;
                while j < close {
                    let escapes = !ps && c[j] == '\\';
                    if escapes && matches!(c.get(j + 1), Some('$' | '`' | '"' | '\\' | '\n')) {
                        if c[j + 1] != '\n' {
                            out.push(c[j + 1]);
                        }
                        j += 2;
                    } else {
                        out.push(c[j]);
                        j += 1;
                    }
                }
                i = close + 1;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// Whether `text` reads as a command rather than a name.
fn reads_as_command(text: &str) -> bool {
    text.contains(|c: char| {
        c.is_whitespace() || matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')' | '`')
    }) || text.contains("$(")
}

/// A word's spellings once its choices are made, at most
/// [`MAX_ALTERNATIVES`]. Past that, each choice left reads as any text in
/// one part, and the second value says whether such a choice held a `/`.
fn alternatives(pieces: &[Piece]) -> (Vec<Vec<Atom>>, bool) {
    let mut done: Vec<Vec<Atom>> = vec![Vec::new()];
    let mut deep = false;
    for p in pieces {
        match p {
            Piece::A(a) => {
                for d in &mut done {
                    d.push(a.clone());
                }
            }
            Piece::Choice(items) => {
                let mut subs: Vec<Vec<Atom>> = Vec::new();
                let mut over = false;
                for item in items {
                    let (alts, d) = alternatives(item);
                    deep |= d;
                    subs.extend(alts);
                    if subs.len() * done.len() > MAX_ALTERNATIVES {
                        over = true;
                        break;
                    }
                }
                if over {
                    deep |= items.iter().any(|item| holds_sep(item));
                    for d in &mut done {
                        d.push(Atom::Any { split: false });
                    }
                    continue;
                }
                let mut next = Vec::with_capacity(done.len() * subs.len());
                for d in &done {
                    for s in &subs {
                        let mut both = d.clone();
                        both.extend(s.iter().cloned());
                        next.push(both);
                    }
                }
                done = next;
            }
        }
    }
    (done, deep)
}

fn holds_sep(pieces: &[Piece]) -> bool {
    pieces.iter().any(|p| match p {
        Piece::A(a) => *a == Atom::Sep,
        Piece::Choice(items) => items.iter().any(|i| holds_sep(i)),
    })
}

/// The words an unquoted expansion's value could split `atoms` into: the
/// whole, and each run that starts or ends where such an expansion is.
fn spans(atoms: &[Atom]) -> Vec<&[Atom]> {
    let cuts: Vec<usize> = atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a, Atom::Any { split: true }))
        .map(|(i, _)| i)
        .take(8)
        .collect();
    let mut out = vec![atoms];
    let starts: Vec<usize> = std::iter::once(0)
        .chain(cuts.iter().map(|c| c + 1))
        .collect();
    let ends: Vec<usize> = cuts
        .iter()
        .copied()
        .chain(std::iter::once(atoms.len()))
        .collect();
    for &s in &starts {
        for &e in &ends {
            if s < e && (s, e) != (0, atoms.len()) {
                out.push(&atoms[s..e]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading a command
// ---------------------------------------------------------------------------

/// Every word of a command, commands inside it included, and where it moves.
#[derive(Default)]
struct Reading {
    words: Vec<Word>,
    /// Each `cd` (or `pushd`, `Set-Location`) in order: its folder word, or
    /// `None` for home.
    cds: Vec<Option<Word>>,
    /// A move the floor can't follow (`cd -`, `popd`, `~-`).
    lost: bool,
    /// The command loops or defines a function, so a `cd` may run more
    /// than once, or before words that come earlier.
    loops: bool,
}

const CD_WORDS: &[&str] = &[
    "cd",
    "chdir",
    "pushd",
    "set-location",
    "sl",
    "push-location",
];
const LOST_WORDS: &[&str] = &["popd", "pop-location"];
const LOOP_WORDS: &[&str] = &[
    "for",
    "while",
    "until",
    "select",
    "function",
    "foreach",
    "foreach-object",
    "%",
    "do",
];

fn read_command(cmd: &str, place: &Place, ps: bool) -> Reading {
    let mut reading = Reading::default();
    let mut todo = vec![(cmd.to_string(), 0usize, HashMap::new())];
    let mut budget = cmd.len().saturating_mul(MAX_DEPTH);
    while let Some((text, depth, vars)) = todo.pop() {
        budget = budget.saturating_sub(text.len());
        if text.contains("()") {
            reading.loops = true;
        }
        let mut lexer = Lexer::new(&text, place, ps, vars);
        let toks = lexer.tokens();
        note_moves(&toks, ps, &mut reading);
        for tok in toks {
            let Tok::Word(w) = tok else { continue };
            if w.quoted {
                let inner = unquote_once(&w.raw, ps);
                if inner != w.raw && reads_as_command(&inner) {
                    lexer.nested.push(inner);
                }
            }
            reading.words.push(w);
        }
        reading.lost |= lexer.lost;
        if depth + 1 < MAX_DEPTH {
            for inner in std::mem::take(&mut lexer.nested) {
                if inner.len() <= budget {
                    todo.push((inner, depth + 1, lexer.vars.clone()));
                }
            }
        }
    }
    reading
}

/// The moves in `toks`, and whether they loop.
fn note_moves(toks: &[Tok], ps: bool, reading: &mut Reading) {
    let mut i = 0;
    while i < toks.len() {
        let Tok::Word(w) = &toks[i] else {
            i += 1;
            continue;
        };
        i += 1;
        let head = w
            .plain()
            .map(|p| p.to_ascii_lowercase())
            .unwrap_or_default();
        reading.loops |= LOOP_WORDS.contains(&head.as_str());
        if LOST_WORDS.contains(&head.as_str()) {
            reading.lost = true;
            continue;
        }
        if !CD_WORDS.contains(&head.as_str()) {
            continue;
        }
        let mut target = None;
        while let Some(Tok::Word(arg)) = toks.get(i) {
            i += 1;
            let text = arg.plain().unwrap_or_default().to_ascii_lowercase();
            let flag = (text.len() > 1 && text.starts_with('-') && !text.starts_with("--"))
                || (ps && text == "/d");
            if text == "--" || flag {
                continue;
            }
            target = Some(arg.clone());
            break;
        }
        let text = target.as_ref().and_then(Word::plain).unwrap_or_default();
        let swaps = head == "pushd" && (target.is_none() || text.starts_with(['+', '-']));
        if text == "-" || swaps {
            reading.lost = true;
        } else {
            reading.cds.push(target);
        }
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// A path part: a pattern for one name, or (a share's root) any number of
/// folders that are not inside a guarded path.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Pat(Vec<Atom>),
    Deep,
}

/// A path a word could name, from its root.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PathPat {
    root: Root,
    parts: Vec<Part>,
}

enum Dots {
    Here,
    Up,
    /// On Windows, a part of only dots and blanks (`...`, `. .`): the floor
    /// reads it each way.
    Odd,
    Name,
}

fn dots_kind(part: &Part, windows: bool) -> Dots {
    let Some(text) = part_text(part) else {
        return Dots::Name;
    };
    match text.as_str() {
        "" | "." => Dots::Here,
        ".." => Dots::Up,
        t if windows && t.chars().all(|c| c == '.' || c == ' ') => match t.trim_end_matches(' ') {
            "." => Dots::Here,
            ".." => Dots::Up,
            _ => Dots::Odd,
        },
        _ => Dots::Name,
    }
}

impl PathPat {
    fn of(path: &Path) -> Self {
        match path_parts(path) {
            Some((root, parts)) => Self {
                root,
                parts: parts
                    .into_iter()
                    .map(|p| Part::Pat(p.chars().map(Atom::Lit).collect()))
                    .collect(),
            },
            None => Self {
                root: Root::Elsewhere,
                parts: Vec::new(),
            },
        }
    }

    fn join(&self, more: &[Part]) -> Self {
        let mut parts = self.parts.clone();
        parts.extend(more.iter().cloned());
        Self {
            root: self.root.clone(),
            parts,
        }
    }

    /// The paths this one reads as without `.` parts and with each `..`
    /// stepping back; on Windows a part of only dots and blanks each way;
    /// macOS's data volume and `/private` as the paths they stand for.
    /// `None` past three such Windows parts.
    fn folded(&self, windows: bool) -> Option<Vec<Self>> {
        let mut all: Vec<Vec<Part>> = vec![Vec::new()];
        let mut odd = 0;
        for part in &self.parts {
            match dots_kind(part, windows) {
                Dots::Here => {}
                Dots::Up => all.iter_mut().for_each(|p| {
                    p.pop();
                }),
                Dots::Name => all.iter_mut().for_each(|p| p.push(part.clone())),
                Dots::Odd => {
                    odd += 1;
                    if odd > 3 {
                        return None;
                    }
                    let mut next = Vec::with_capacity(all.len() * 3);
                    for p in all {
                        let mut up = p.clone();
                        up.pop();
                        let mut kept = p.clone();
                        kept.push(part.clone());
                        next.extend([p, up, kept]);
                    }
                    all = next;
                }
            }
        }
        Some(
            all.into_iter()
                .map(|mut parts| {
                    if self.root == Root::Posix {
                        unfirm(&mut parts);
                    }
                    Self {
                        root: self.root.clone(),
                        parts,
                    }
                })
                .collect(),
        )
    }
}

/// macOS: `/System/Volumes/Data/x` is `/x`, and `/private/etc` is `/etc`.
fn unfirm(parts: &mut Vec<Part>) {
    let lower: Vec<Option<String>> = parts
        .iter()
        .take(3)
        .map(|p| part_text(p).map(|t| t.to_lowercase()))
        .collect();
    let is = |i: usize, want: &[&str]| {
        lower
            .get(i)
            .and_then(|p| p.as_deref())
            .is_some_and(|p| want.contains(&p))
    };
    if is(0, &["system"]) && is(1, &["volumes"]) && is(2, &["data"]) {
        parts.drain(..3);
    } else if is(0, &["private"]) && is(1, &["etc", "var", "tmp"]) {
        parts.remove(0);
    }
}

/// A part's text when it is all plain characters.
fn part_text(part: &Part) -> Option<String> {
    match part {
        Part::Pat(atoms) => lit_text(atoms),
        Part::Deep => None,
    }
}

fn lit_text(atoms: &[Atom]) -> Option<String> {
    atoms
        .iter()
        .map(|a| match a {
            Atom::Lit(c) => Some(*c),
            _ => None,
        })
        .collect()
}

/// `?`, `.` or `??` before a device path (`\\?\`, `\\.\`, `\??\`), however
/// the `?` was read.
fn device_mark(part: &[Atom]) -> bool {
    let marks = part.iter().all(|a| matches!(a, Atom::One | Atom::Lit('?')));
    part == [Atom::Lit('.')] || (!part.is_empty() && part.len() <= 2 && marks)
}

/// The drive a part such as `C:` names (`None` for a drive the floor can't
/// tell: `[c]:`, `$d:`), and the rest of the part after the colon.
fn drive_of(part: &[Atom]) -> Option<(Option<char>, &[Atom])> {
    match part {
        [Atom::Lit(d), Atom::Lit(':'), rest @ ..] if d.is_ascii_alphabetic() => {
            Some((Some(d.to_ascii_lowercase()), rest))
        }
        [Atom::One | Atom::Class(_) | Atom::Any { .. }, Atom::Lit(':'), rest @ ..] => {
            Some((None, rest))
        }
        _ => None,
    }
}

fn split_parts(atoms: &[Atom]) -> Vec<Vec<Atom>> {
    atoms
        .split(|a| *a == Atom::Sep)
        .map(<[Atom]>::to_vec)
        .collect()
}

fn to_parts(parts: &[Vec<Atom>]) -> Vec<Part> {
    parts.iter().cloned().map(Part::Pat).collect()
}

/// The paths a word's spelling could name, from each folder it may be
/// read in (`cwds`).
fn resolve(atoms: &[Atom], cwds: &[PathPat], windows: bool) -> Vec<PathPat> {
    if atoms.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if windows {
        // A provider-qualified path (`FileSystem::C:\x`) names what follows.
        let at = (1..atoms.len())
            .rev()
            .find(|&i| atoms[i] == Atom::Lit(':') && atoms[i - 1] == Atom::Lit(':'));
        if let Some(at) = at.filter(|&at| at + 1 < atoms.len()) {
            out.extend(resolve(&atoms[at + 1..], cwds, windows));
        }
    }
    let parts = split_parts(atoms);
    if parts[0].is_empty() {
        if windows && parts.len() > 2 && parts[1].is_empty() {
            // `\\server\share\…`, `\\?\…`, `\\.\…`.
            out.push(unc(&parts[2..]));
            return out;
        }
        if windows && parts.len() > 2 && device_mark(&parts[1]) {
            // `\??\C:\…`.
            out.push(unc(&parts[1..]));
            return out;
        }
        let rest = to_parts(&parts[1..]);
        if !windows {
            out.push(PathPat {
                root: Root::Posix,
                parts: rest,
            });
            return out;
        }
        // Root-relative: the drive the command is on.
        let mut roots = vec![Root::AnyDrive];
        for cwd in cwds {
            if !roots.contains(&cwd.root) {
                roots.push(cwd.root.clone());
            }
        }
        for root in roots {
            out.push(PathPat {
                root,
                parts: rest.clone(),
            });
        }
        return out;
    }
    if windows {
        if let Some((drive, tail)) = drive_of(&parts[0]) {
            let root = drive.map_or(Root::AnyDrive, Root::Drive);
            let mut rest = Vec::new();
            if !tail.is_empty() {
                rest.push(Part::Pat(tail.to_vec()));
            }
            rest.extend(to_parts(&parts[1..]));
            if tail.is_empty() && parts.len() > 1 {
                out.push(PathPat { root, parts: rest });
                return out;
            }
            // `C:x` is `x` in the folder the command is in on C: one it
            // starts in or moves to, or the drive's root.
            out.extend(
                cwds.iter()
                    .filter(|cwd| root_matches(&cwd.root, &root))
                    .map(|cwd| cwd.join(&rest)),
            );
            out.push(PathPat { root, parts: rest });
            return out;
        }
    }
    let rest = to_parts(&parts);
    out.extend(cwds.iter().map(|cwd| cwd.join(&rest)));
    out
}

/// A `\\…` path after its two slashes: a device path (`\\?\C:\x`,
/// `\\.\C:\x`, `\??\C:\x`, `\\?\UNC\server\share\x`), an admin share
/// (`\\host\c$\x` is `C:\x`), or another share, which may hold any folder.
fn unc(parts: &[Vec<Atom>]) -> PathPat {
    let anywhere = |rest: &[Vec<Atom>]| {
        let mut parts = vec![Part::Deep];
        parts.extend(to_parts(rest));
        PathPat {
            root: Root::AnyDrive,
            parts,
        }
    };
    let text = |i: usize| {
        parts
            .get(i)
            .and_then(|p| lit_text(p))
            .map(|t| t.to_ascii_lowercase())
    };
    if parts.first().is_some_and(|p| device_mark(p)) {
        let drive = parts.get(1).and_then(|p| drive_of(p));
        if let Some((drive, [])) = drive {
            return PathPat {
                root: drive.map_or(Root::AnyDrive, Root::Drive),
                parts: to_parts(&parts[2..]),
            };
        }
        if text(1).as_deref() == Some("unc") {
            return unc(&parts[2..]);
        }
        // `Volume{…}`, `GLOBALROOT\Device\…`: some drive, some folder.
        return anywhere(parts.get(2..).unwrap_or_default());
    }
    match text(1).unwrap_or_default().as_bytes() {
        [d, b'$'] if d.is_ascii_alphabetic() => PathPat {
            root: Root::Drive(char::from(*d)),
            parts: to_parts(&parts[2..]),
        },
        _ => anywhere(parts.get(2..).unwrap_or_default()),
    }
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

fn eq_fold(a: char, b: char) -> bool {
    a == b
        || if a.is_ascii() && b.is_ascii() {
            a.eq_ignore_ascii_case(&b)
        } else {
            a.to_lowercase().eq(b.to_lowercase())
        }
}

/// A name as Windows opens it: without an NTFS stream (`x::$DATA` opens
/// `x`) and without trailing dots and blanks.
fn win_name(name: &str) -> &str {
    let name = name.split(':').next().unwrap_or(name);
    name.trim_end_matches(['.', ' '])
}

/// A one-part pattern as Windows opens what it names ([`win_name`]).
fn win_pattern(pat: &[Atom]) -> &[Atom] {
    let mut end = pat
        .iter()
        .position(|a| *a == Atom::Lit(':'))
        .unwrap_or(pat.len());
    while end > 0 && matches!(pat[end - 1], Atom::Lit('.' | ' ')) {
        end -= 1;
    }
    &pat[..end]
}

/// Whether the one-part pattern `pat` matches `name` (lowercase), without
/// case. A POSIX glob follows bash: a leading dot only by a leading dot, or
/// by an expansion's text. On Windows any name, with a stream suffix and
/// trailing dots and blanks dropped, and 8.3 short names.
fn name_matches(pat: &[Atom], name: &[char], windows: bool) -> bool {
    if windows {
        let pat = win_pattern(pat);
        return atoms_match(pat, name) || short_name_matches(pat, name);
    }
    let dot_ok = matches!(pat.first(), Some(Atom::Lit('.') | Atom::Any { .. }));
    if name.first() == Some(&'.') && !dot_ok {
        return false;
    }
    atoms_match(pat, name)
}

fn part_matches(pat: &[Atom], name: &str, windows: bool) -> bool {
    let name: Vec<char> = name.to_lowercase().chars().collect();
    name_matches(pat, &name, windows)
}

/// `ADMINI~1` or `SSH~1` as the 8.3 short name of `Administrator` or
/// `.ssh`: the part before `~` starts the long name with its dots and
/// blanks taken out, or is its first two letters and a hash.
fn short_name_matches(pat: &[Atom], name: &[char]) -> bool {
    let Some(text) = lit_text(pat).map(|t| t.to_lowercase()) else {
        return false;
    };
    let Some((base, rest)) = text.split_once('~') else {
        return false;
    };
    let digits = rest.split('.').next().unwrap_or("");
    let numbered = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    if base.is_empty() || base.len() > 6 || !numbered {
        return false;
    }
    let long: String = name.iter().collect();
    let long = long.trim_start_matches('.');
    let stem: String = long
        .rsplit_once('.')
        .map_or(long, |(s, _)| s)
        .chars()
        .filter(|c| !matches!(c, '.' | ' '))
        .collect();
    let hashed = base.len() == 6
        && base.is_char_boundary(2)
        && stem.starts_with(&base[..2])
        && base[2..].bytes().all(|b| b.is_ascii_hexdigit());
    stem.starts_with(base) || hashed
}

/// Glob match of atoms against a name's characters, without case.
fn atoms_match(pat: &[Atom], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        let step = match pat.get(p) {
            Some(Atom::Star | Atom::Any { .. }) => {
                star = Some((p, n));
                p += 1;
                continue;
            }
            Some(Atom::One) => true,
            Some(Atom::Class(body)) => class_matches(body, name[n]),
            Some(Atom::Lit(c)) => eq_fold(*c, name[n]),
            Some(Atom::Sep) | None => false,
        };
        if step {
            p += 1;
            n += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pat[p..]
        .iter()
        .all(|a| matches!(a, Atom::Star | Atom::Any { .. }))
}

/// Where a bracket class that opens at `pat[open]` closes. A `]` right
/// after `[` (or after `[!`) is part of the class, and so is the `]` of a
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

/// Whether the named class (`alpha`, `digit`, …) holds `c`; a name the
/// floor doesn't know holds everything.
fn named_class_has(name: &str, c: char) -> bool {
    match name {
        "alpha" => c.is_alphabetic(),
        "upper" => c.is_uppercase(),
        "lower" => c.is_lowercase(),
        "alnum" => c.is_alphanumeric(),
        "word" => c.is_alphanumeric() || c == '_',
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

/// Whether the class body (between `[` and `]`, without its `!`) holds `c`.
fn class_holds(body: &[char], c: char) -> bool {
    let mut i = 0;
    while i < body.len() {
        if let Some(end) = (body[i] == '[').then(|| inner_end(body, i)).flatten() {
            let inner: String = body[i + 2..end - 1].iter().collect();
            let hit = match body[i + 1] {
                ':' => named_class_has(&inner, c),
                _ => inner.contains(c),
            };
            if hit {
                return true;
            }
            i = end + 1;
        } else if body.get(i + 1) == Some(&'-') && i + 2 < body.len() {
            if (body[i]..=body[i + 2]).contains(&c) {
                return true;
            }
            i += 3;
        } else {
            if body[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// Whether the class matches `c` in some case.
fn class_matches(body: &[char], c: char) -> bool {
    let (negate, body) = match body.first() {
        Some('!' | '^') => (true, &body[1..]),
        _ => (false, body),
    };
    let mut cases = c.to_lowercase().chain(c.to_uppercase()).chain([c]);
    if negate {
        cases.any(|c| !class_holds(body, c))
    } else {
        cases.any(|c| class_holds(body, c))
    }
}

fn gpart_matches(pat: &[Atom], g: &GPart, windows: bool) -> bool {
    match g {
        GPart::Name(name) => name_matches(pat, name, windows),
        GPart::AnyName => !matches!(pat.first(), Some(Atom::Lit('.'))),
    }
}

/// Whether a path from root `p` could be on the guarded path's root `g`.
/// `/etc/shadow` read on Windows is `\etc\shadow` on some drive, and the
/// form of another user's Windows home names no drive.
fn root_matches(p: &Root, g: &Root) -> bool {
    match (p, g) {
        (Root::Elsewhere, _) | (_, Root::Elsewhere) => false,
        (Root::Drive(a), Root::Drive(b)) => a == b,
        _ => true,
    }
}

/// Whether the path pattern could name the guarded path, or a path inside
/// it, as written.
fn reaches(p: &PathPat, g: &GuardForm, windows: bool) -> bool {
    let at = u64::from(root_matches(&p.root, &g.root));
    walk(at, &p.parts, g, windows) & done(g) != 0
}

/// Where each guarded path's form stands after a folder's parts (0 when
/// the folder is off its root or off the path), for a word read in that
/// folder to go on from. `None` when the folder's spelling could fold
/// together with what follows it (a POSIX folder of under three parts:
/// macOS's `/private/etc`, `/System/Volumes/Data`): such a word goes
/// through [`resolve`].
fn starts_in(cwd: &PathPat, forms: &[GuardForm], windows: bool) -> Option<Vec<u64>> {
    let spellings = cwd.folded(windows)?;
    if spellings
        .iter()
        .any(|f| f.root == Root::Posix && f.parts.len() < 3)
    {
        return None;
    }
    let start = |g: &GuardForm| {
        spellings.iter().fold(0, |at, f| {
            at | walk(
                u64::from(root_matches(&f.root, &g.root)),
                &f.parts,
                g,
                windows,
            )
        })
    };
    Some(forms.iter().map(start).collect())
}

/// A span's parts as written and folded, when it is a path relative to the
/// folder it is read in that never steps above that folder. Anything else
/// (an absolute path, a drive, a provider path, `..` above the start, a
/// Windows dot-only part) goes through [`resolve`].
fn relative(span: &[Atom], windows: bool) -> Option<(Vec<Part>, Vec<Part>)> {
    let parts = split_parts(span);
    if parts[0].is_empty() || (windows && drive_of(&parts[0]).is_some()) {
        return None;
    }
    let provider = span
        .windows(2)
        .any(|w| w == [Atom::Lit(':'), Atom::Lit(':')]);
    if windows && provider {
        return None;
    }
    let raw = to_parts(&parts);
    let mut folded = Vec::new();
    for part in &raw {
        match dots_kind(part, windows) {
            Dots::Here => {}
            Dots::Up => {
                folded.pop()?;
            }
            Dots::Odd => return None,
            Dots::Name => folded.push(part.clone()),
        }
    }
    Some((raw, folded))
}

/// The bit that says a pattern has matched all of the guarded path.
fn done(g: &GuardForm) -> u64 {
    1 << g.parts.len()
}

/// Where a pattern stands against the guarded path after `parts`, from
/// `at`. Bit j: the pattern so far matches the guarded path's first j
/// parts. Once it matches all of them it stays there: what follows is
/// inside the guarded path.
fn walk(mut at: u64, parts: &[Part], g: &GuardForm, windows: bool) -> u64 {
    let m = g.parts.len();
    let done = done(g);
    let all = (done << 1) - 1;
    for part in parts {
        if at & done != 0 || at == 0 {
            return at;
        }
        at = match part {
            // Any number of parts: every state from the first one reached,
            // short of the whole guarded path.
            Part::Deep => all & !((at & at.wrapping_neg()) - 1) & !done,
            Part::Pat(atoms) => (0..m)
                .filter(|&j| at >> j & 1 == 1 && gpart_matches(atoms, &g.parts[j], windows))
                .fold(0, |next, j| next | 1 << (j + 1)),
        };
    }
    at
}

// ---------------------------------------------------------------------------
// The filesystem
// ---------------------------------------------------------------------------

/// What the floor may spend reading the filesystem for one command. Past
/// it, a word counts only when it matches a guarded path as written.
struct Budget {
    until: Instant,
    dirs: u32,
    entries: u32,
    paths: u32,
}

/// The most paths one glob part may match before the floor stops reading it.
const MAX_MATCHES: usize = 512;

impl Budget {
    fn new() -> Self {
        Self {
            until: Instant::now() + Duration::from_millis(20),
            dirs: 64,
            entries: 8192,
            paths: 256,
        }
    }

    fn take(n: &mut u32) -> bool {
        let ok = *n > 0;
        *n = n.saturating_sub(1);
        ok
    }

    fn spent(&self) -> bool {
        self.paths == 0 || Instant::now() > self.until
    }
}

/// Paths as the system resolves them, links followed as far as each path
/// exists, kept so that each folder costs one look.
#[derive(Default)]
struct RealPaths(HashMap<PathBuf, PathBuf>);

impl RealPaths {
    fn of(&mut self, path: &Path) -> PathBuf {
        if let Some(real) = self.0.get(path) {
            return real.clone();
        }
        let real = match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                let joined = self.of(parent).join(name);
                // Windows resolves short names and junctions only this way.
                let link = std::fs::symlink_metadata(&joined)
                    .is_ok_and(|m| cfg!(windows) || m.file_type().is_symlink());
                if link {
                    joined.canonicalize().unwrap_or(joined)
                } else {
                    joined
                }
            }
            _ => path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        };
        self.0.insert(path.to_path_buf(), real.clone());
        real
    }
}

/// Whether `path` is `guard` or inside it, without case.
fn inside(path: &Path, guard: &Path) -> bool {
    let fold = |c: std::path::Component| c.as_os_str().to_string_lossy().to_lowercase();
    let mut p = path.components();
    guard
        .components()
        .all(|g| p.next().is_some_and(|c| fold(c) == fold(g)))
}

/// The system path a root stands for, when there is one here.
fn root_path(root: &Root) -> Option<PathBuf> {
    match root {
        Root::Posix => Some(PathBuf::from("/")),
        Root::Drive(d) if cfg!(windows) => Some(PathBuf::from(format!("{d}:\\"))),
        _ => None,
    }
}

/// The guarded paths on this system, resolved.
fn real_guards(forms: &[GuardForm], real: &mut RealPaths) -> Vec<(PathBuf, Guarded)> {
    let mut out: Vec<(PathBuf, Guarded)> = Vec::new();
    for g in forms {
        let Some(mut path) = root_path(&g.root) else {
            continue;
        };
        let mut named = true;
        for part in &g.parts {
            match part {
                GPart::Name(n) => path.push(n.iter().collect::<String>()),
                GPart::AnyName => named = false,
            }
        }
        if named {
            let path = real.of(&path);
            if !out.iter().any(|(p, _)| *p == path) {
                out.push((path, g.what));
            }
        }
    }
    out
}

/// A guarded path among the real paths the pattern matches: globs read
/// from the filesystem with bash's rules, links followed.
fn fs_hit(
    p: &PathPat,
    guards: &[(PathBuf, Guarded)],
    real: &mut RealPaths,
    budget: &mut Budget,
    windows: bool,
) -> Option<Guarded> {
    let mut frontier = vec![root_path(&p.root)?];
    for part in &p.parts {
        let Part::Pat(atoms) = part else {
            return None;
        };
        let mut next = Vec::new();
        if let Some(name) = lit_text(atoms) {
            next.extend(frontier.iter().map(|f| f.join(&name)));
        } else {
            for dir in &frontier {
                if budget.spent() || !Budget::take(&mut budget.dirs) {
                    return None;
                }
                let Ok(entries) = std::fs::read_dir(dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if !Budget::take(&mut budget.entries) || next.len() >= MAX_MATCHES {
                        return None;
                    }
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if part_matches(atoms, &name, windows) {
                        next.push(dir.join(&*name));
                    }
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            return None;
        }
    }
    for path in frontier {
        if budget.spent() || !Budget::take(&mut budget.paths) {
            return None;
        }
        let path = real.of(&path);
        if let Some((_, what)) = guards.iter().find(|(g, _)| inside(&path, g)) {
            return Some(*what);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The floor
// ---------------------------------------------------------------------------

/// The folders the command may open its words in: where it starts, and
/// each folder a `cd` in it may move to. A move the floor can't follow
/// adds the start, each folder above it, the home folder and the root.
fn folders(reading: &Reading, place: &Place) -> Vec<PathPat> {
    let start = PathPat::of(&place.cwd);
    let mut out = vec![start.clone()];
    if let Ok(real) = place.cwd.canonicalize() {
        let real = PathPat::of(&real);
        if real != start {
            out.push(real);
        }
    }
    let home = place.home.as_deref().map(PathPat::of);
    let lost = || {
        let mut set: Vec<PathPat> = (0..=start.parts.len())
            .map(|n| PathPat {
                root: start.root.clone(),
                parts: start.parts[..n].to_vec(),
            })
            .collect();
        set.extend(home.clone());
        set
    };
    let add = |out: &mut Vec<PathPat>, paths: Vec<PathPat>| {
        for p in paths {
            for p in p.folded(place.windows).unwrap_or_default() {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    };
    let cdpath = std::env::var_os("CDPATH").is_some_and(|v| !v.is_empty());
    let mut follows = !reading.lost;
    for target in &reading.cds {
        let Some(target) = target else {
            add(&mut out, home.iter().cloned().collect());
            continue;
        };
        let (alts, deep) = alternatives(&target.pieces);
        follows &= !deep;
        let cd_path = cdpath || target.raw.to_ascii_lowercase().contains("cdpath");
        let mut found = Vec::new();
        for alt in &alts {
            let relative = alt.first() != Some(&Atom::Sep);
            let unknown = alt.iter().any(|a| matches!(a, Atom::Any { .. }));
            if unknown || (relative && (reading.loops || cd_path)) {
                follows = false;
            }
            found.extend(resolve(alt, &out, place.windows));
        }
        add(&mut out, found);
        if out.len() > 64 {
            out.truncate(64);
            follows = false;
        }
    }
    if !follows {
        add(&mut out, lost());
    }
    out
}

/// What one floor check has read and spent so far.
struct Floor<'p> {
    place: &'p Place,
    forms: Vec<GuardForm>,
    /// The guarded paths on this system, resolved once a word needs them.
    guards: Option<Vec<(PathBuf, Guarded)>>,
    real: RealPaths,
    budget: Budget,
    /// Each word's decoded text, for the literal checks.
    texts: Vec<String>,
    /// For each folder a word may be read in, once a word needs it: where
    /// each guarded path's form stands after the folder's own parts
    /// ([`starts_in`]).
    starts: Vec<Option<Option<Vec<u64>>>>,
}

impl Floor<'_> {
    /// The reason a guarded path the word could name gives.
    fn word(&mut self, word: &Word, cwds: &[PathPat]) -> Option<&'static str> {
        let windows = self.place.windows;
        let mut readings = vec![&word.pieces[..]];
        if let Some(eq) = word.eq {
            // `--file=x` names `x` too.
            readings.push(&word.pieces[eq + 1..]);
        }
        for pieces in readings {
            let (alts, deep) = alternatives(pieces);
            if deep {
                return Some(BRACES_TOO_BIG);
            }
            for alt in &alts {
                let mut text = String::new();
                alt.iter().for_each(|a| push_atom_text(&mut text, a));
                self.texts.push(text);
                for span in spans(alt) {
                    // A word of only expansions has a value the floor can't
                    // see: it is outside, like a command's output.
                    if span.iter().all(|a| matches!(a, Atom::Any { .. })) {
                        continue;
                    }
                    if let Some((raw, folded)) = relative(span, windows) {
                        if let Some(why) = self.relative(&raw, &folded, cwds) {
                            return Some(why);
                        }
                        continue;
                    }
                    for path in resolve(span, cwds, windows) {
                        if let Some(why) = self.path(&path) {
                            return Some(why);
                        }
                    }
                }
            }
        }
        None
    }

    /// The reason a guarded path the path pattern could name gives: as
    /// written, or on the filesystem.
    fn path(&mut self, path: &PathPat) -> Option<&'static str> {
        let windows = self.place.windows;
        let Some(folded) = path.folded(windows) else {
            return Some(DOTS_TOO_MANY);
        };
        let hit = folded
            .iter()
            .find_map(|f| self.forms.iter().find(|g| reaches(f, g, windows)));
        if let Some(g) = hit {
            return Some(g.what.reason());
        }
        self.on_disk(path)
    }

    /// [`Self::path`] for a relative path that stays at or below the folder
    /// it is read in: each form goes on from where that folder left it.
    fn relative(
        &mut self,
        raw: &[Part],
        folded: &[Part],
        cwds: &[PathPat],
    ) -> Option<&'static str> {
        let windows = self.place.windows;
        for (k, cwd) in cwds.iter().enumerate() {
            if self.starts.len() <= k {
                self.starts.resize(k + 1, None);
            }
            let forms = &self.forms;
            let starts = self.starts[k].get_or_insert_with(|| starts_in(cwd, forms, windows));
            let Some(starts) = starts else {
                if let Some(why) = self.path(&cwd.join(raw)) {
                    return Some(why);
                }
                continue;
            };
            let hit = forms
                .iter()
                .zip(starts.iter())
                .find(|(g, at)| **at != 0 && walk(**at, folded, g, windows) & done(g) != 0);
            if let Some((g, _)) = hit {
                return Some(g.what.reason());
            }
            if let Some(why) = self.on_disk(&cwd.join(raw)) {
                return Some(why);
            }
        }
        None
    }

    /// The reason a guarded path the path names on the filesystem gives.
    fn on_disk(&mut self, path: &PathPat) -> Option<&'static str> {
        if !self.place.fs || self.budget.spent() {
            return None;
        }
        let (forms, real) = (&self.forms, &mut self.real);
        let guards = self.guards.get_or_insert_with(|| real_guards(forms, real));
        let windows = self.place.windows;
        fs_hit(path, guards, real, &mut self.budget, windows).map(Guarded::reason)
    }
}

/// The reason the first word of `cmd` that could name a guarded path of
/// `kinds` gives, and the decoded text of each word for the literal checks.
/// On Windows the command is read both as bash reads it (the shell tool
/// runs bash there too) and as PowerShell and cmd read it.
fn floor_hit(cmd: &str, place: &Place, kinds: &[Guarded]) -> (Option<&'static str>, Vec<String>) {
    let mut floor = Floor {
        place,
        forms: guard_forms(place, kinds),
        guards: None,
        real: RealPaths::default(),
        budget: Budget::new(),
        texts: Vec::new(),
        starts: Vec::new(),
    };
    let dialects: &[bool] = if place.windows {
        &[false, true]
    } else {
        &[false]
    };
    for &ps in dialects {
        let reading = read_command(cmd, place, ps);
        let cwds = folders(&reading, place);
        floor.starts.clear();
        for word in &reading.words {
            if let Some(why) = floor.word(word, &cwds) {
                return (Some(why), floor.texts);
            }
        }
    }
    (None, floor.texts)
}

/// A word's text for the literal checks: lowercase, `\` as `/`, and on
/// Windows each part as the system opens it.
fn literal_text(text: &str, windows: bool) -> String {
    let lower = text.to_lowercase().replace('\\', "/");
    if !windows {
        return lower;
    }
    lower.split('/').map(win_name).collect::<Vec<_>>().join("/")
}

/// Whether `cmd` names the consent ledger or a routine consent store:
/// written out anywhere (`consent.jsonl`, `routines/…/consent.json`), or in
/// a word that, read the way the shell reads it, could name one.
pub fn names_consent(cmd: &str) -> bool {
    consent_at(cmd, &Place::current())
}

fn consent_at(cmd: &str, place: &Place) -> bool {
    let (hit, texts) = floor_hit(cmd, place, &[Guarded::ConsentLedger]);
    if hit.is_some() {
        return true;
    }
    let mut all = cmd.to_lowercase();
    for t in &texts {
        all.push(' ');
        all.push_str(&literal_text(t, place.windows));
    }
    all.contains("consent.jsonl") || (all.contains("consent.json") && all.contains("routines"))
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

/// Why the floor refuses `cmd`, read as run in the workspace
/// ([`in_workspace`]) or the process's folder. See the module doc for what
/// it promises and where that stops.
pub fn forbidden_reason(cmd: &str) -> Option<&'static str> {
    forbidden_reason_at(cmd, &Place::current())
}

fn forbidden_reason_at(cmd: &str, place: &Place) -> Option<&'static str> {
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
    let (hit, texts) = floor_hit(cmd, place, FLOOR);
    hit.or_else(|| {
        texts
            .iter()
            .find_map(|t| forbidden_on(&literal_text(t, place.windows)))
    })
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
            // These globs expand to `~/.config/GrokHub/secrets.json` and
            // `/etc/shadow`.
            ("ls ~/.config/*/*.json", "forbidden path: app secrets"),
            ("cat /*/*", "forbidden path: /etc/shadow"),
            ("ls ~/.*", "forbidden path: ssh keys"),
            ("du -sh ~/.*", "forbidden path: ssh keys"),
            ("ls /etc/*", "forbidden path: /etc/shadow"),
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

    /// The floor read as PowerShell and cmd too (`windows`) or only as a
    /// POSIX shell, here. It reads the filesystem only with this system's
    /// own path rules.
    fn forbidden_reason_in(cmd: &str, windows: bool) -> Option<&'static str> {
        let place = Place {
            windows,
            fs: windows == cfg!(windows),
            ..Place::current()
        };
        forbidden_reason_at(cmd, &place)
    }

    /// A POSIX place in `cwd` for a user whose home is `/home/user`, with no
    /// filesystem reads: the floor reads each word as written.
    fn fake(cwd: &str) -> Place {
        Place {
            cwd: cwd.into(),
            home: Some("/home/user".into()),
            configs: vec!["/home/user/.config/GrokHub".into()],
            grok_home: Some("/home/user/.grok".into()),
            windows: false,
            fs: false,
        }
    }

    /// [`fake`] on Windows, for a user whose home is `C:\Users\User`.
    fn fake_windows(cwd: &str) -> Place {
        Place {
            cwd: cwd.into(),
            home: Some(r"C:\Users\User".into()),
            configs: vec![r"C:\Users\User\AppData\Roaming\GrokHub".into()],
            grok_home: Some(r"C:\Users\User\.grok".into()),
            windows: true,
            fs: false,
        }
    }

    /// The consent ledger counts from where a glob could reach it, and not
    /// from anywhere else.
    #[test]
    fn consent_globs_count_from_where_they_reach_the_ledger() {
        let project = fake("/home/user/my-app");
        let config = fake("/home/user/.config");
        let app = fake("/home/user/.config/GrokHub");
        for (cmd, place, want) in [
            ("echo x >> ~/.config/grokhub/consent.json?", &project, true),
            ("echo x >> ~/.config/grokhub/consent.*", &project, true),
            ("cat /home/user/.config/g*/c*.jsonl", &project, true),
            ("cat ~/.config/*/*.jsonl", &project, true),
            ("cat ~/.config/*/routines/*", &project, true),
            (
                "echo '{}' >> ~/.config/[G][r][o][k][H][u][b]/[c][o][n][s][e][n][t].jsonl",
                &project,
                true,
            ),
            ("echo '{}' | tee -a ~/.config/*/*.jsonl", &project, true),
            ("cat g*/c*.jsonl", &config, true),
            ("cat */*", &config, true),
            ("cat x >> consent.js?nl", &app, true),
            ("cat x >> [c]onsent.*", &app, true),
            ("cat x >> *.jsonl", &app, true),
            ("cp x.json routines/*.json", &app, true),
            ("cat g*/c*.jsonl", &project, false),
            ("cat */*", &project, false),
            ("cat x >> consent.js?nl", &project, false),
            ("cat x >> *.jsonl", &project, false),
            ("cat x >> [a-b]*.jsonl", &app, false),
            ("wc -l logs/*.jsonl", &app, false),
            ("ls src/consent.*", &project, false),
            ("echo x", &app, false),
        ] {
            assert_eq!(consent_at(cmd, place), want, "{cmd} in {:?}", place.cwd);
        }
    }

    /// Braces give each of their words, however many there are; past what
    /// the floor reads, a word that may still name any path is refused.
    #[test]
    fn braces_give_each_word_and_a_huge_one_is_refused() {
        let project = fake("/home/user/my-app");
        let ssh = Some("forbidden path: ssh keys");
        assert_eq!(forbidden_reason_at("cat ~/.{x,ssh}/id_rsa", &project), ssh);
        assert_eq!(
            forbidden_reason_at("cat ~/.{x{a,s}sh,b}/id_rsa", &project),
            None
        );
        assert_eq!(
            forbidden_reason_at("cat ~/.{x,s{a,s}h}/id_rsa", &project),
            ssh
        );
        let long = format!("cat ~/.{{ssh,b}}/{}", "x".repeat(70_000));
        assert_eq!(forbidden_reason_at(&long, &project), ssh);
        let many = format!("cat {}", "{a,b}".repeat(9));
        assert_eq!(forbidden_reason_at(&many, &project), None);
        let paths = format!("cat {}", "{a/,b/}".repeat(9));
        assert_eq!(forbidden_reason_at(&paths, &project), Some(BRACES_TOO_BIG));
    }

    /// The ways verify found to name a guarded path past the old readings:
    /// dot globs from above, classes for every letter, globs of a set
    /// length, unknown expansions that may be empty, `${x:-word}`, commands
    /// inside commands, and here-documents a shell runs.
    #[test]
    fn the_floor_reads_globs_and_expansions_the_way_bash_does() {
        let project = fake("/home/user/my-app");
        let ssh = "forbidden path: ssh keys";
        let secrets = "forbidden path: app secrets";
        for (cmd, why) in [
            ("cat ../.???/id_rsa", ssh),
            ("cat /*/*/.???/id_rsa", ssh),
            ("cat ~/../*/.???/id_rsa", ssh),
            ("cd .. && cat .???/id_rsa", ssh),
            ("tar czf /tmp/x.tgz ../.*", ssh),
            ("cat ../.*/id_rsa", ssh),
            ("cat ~/.s${x}sh/id_rsa", ssh),
            ("cat ~/.s$()sh/id_rsa", ssh),
            ("cat ~/.s$@sh/id_rsa", ssh),
            ("cat ~/.s`true`sh/id_rsa", ssh),
            ("cat ~/${x:-.ssh}/id_rsa", ssh),
            // `$x` may be `.ssh`.
            ("cat ~/$x/id_rsa", ssh),
            ("x=~/.s; cat ${x}sh/id_rsa", ssh),
            ("bash -c 'cd ~ && cat .s?h/id_rsa'", ssh),
            ("bash <<'EOF'\ncat ~/.s?h/id_rsa\nEOF", ssh),
            ("cd $HOME; cat .s?h/id_rsa", ssh),
            ("cd ~/x/y && cd ../.. && cat .s?h/id_rsa", ssh),
            ("cd \"$DIR\" && cat .s?h/id_rsa", ssh),
            ("cd ~ && cd - && cat .s?h/id_rsa", ssh),
            ("cat ~-/.s?h/id_rsa", ssh),
            ("cat ~user/.s?h/id_rsa", ssh),
            ("cat ~root/.s?h/id_rsa", ssh),
            ("cat /System/Volumes/Data/home/user/.s?h/id_rsa", ssh),
            ("cat ~/${x:+.gnupg}/pubring.kbx", "forbidden path: gnupg"),
            (
                r#"sh -c "cat \$HOME/.aw?/credentials""#,
                "forbidden path: aws credentials",
            ),
            (
                "eval \"cat ~/.k[u]be/config\"",
                "forbidden path: kube config",
            ),
            ("echo $(cat ~/.k?be/config)", "forbidden path: kube config"),
            (
                "cat /[e][t][c]/[s][h][a][d][o][w]",
                "forbidden path: /etc/shadow",
            ),
            ("cat /private/etc/shado?", "forbidden path: /etc/shadow"),
            ("cat /etc/???????", "forbidden path: /etc/sudoers"),
            ("cat ~/.config/[G][r]okHub/secrets.json", secrets),
            (
                "cat ~/.config/[G][r][o][k][H][u][b]/[s][e][c][r][e][t][s].json",
                secrets,
            ),
            ("cat ~/.config/*/*.json", secrets),
            ("cat ~/.config/???????/???????.json", secrets),
            ("cd ~/.config/GrokHub && cat ????????????", secrets),
            ("cat ~/.gr?k/*", "forbidden path: grok login"),
        ] {
            assert_eq!(forbidden_reason_at(cmd, &project), Some(why), "{cmd}");
        }
        // Expansions whose value the floor can't see, and names that only
        // look close, still run.
        for cmd in [
            "cat $x",
            "cat $x/notes.txt",
            "echo $HOME",
            "cat $(cat list.txt)",
            "cat ~/.sshx/id_rsa",
            "cat ~/.s?/id_rsa",
            "cat ~/*/id_rsa",
            "ls ~/.config",
            "cd ~/.config && ls",
        ] {
            assert_eq!(forbidden_reason_at(cmd, &project), None, "{cmd}");
        }
    }

    /// On Windows a name opens without its NTFS stream and its trailing
    /// dots and blanks, and as its 8.3 short name; a path opens from a drive
    /// root, an admin share, a device path or a provider path.
    #[test]
    fn on_windows_the_floor_reads_streams_short_names_shares_and_devices() {
        let project = fake_windows(r"C:\Users\User\my-app");
        let ssh = "forbidden path: ssh keys";
        let secrets = "forbidden path: app secrets";
        for (cmd, why) in [
            (r"type $env:APPDATA\GrokHub\secrets.json::$DATA", secrets),
            (r"type $env:APPDATA\GrokHub\app.json:$DATA", secrets),
            (r"Get-Content $env:APPDATA\GrokHub\app.json::$DATA", secrets),
            (r"type %APPDATA%\GrokHub\secrets.json::$DATA", secrets),
            (r"type ..\.ssh:$I30:$INDEX_ALLOCATION\id_rsa", ssh),
            (r#"type "..\.ss?. \id_rsa""#, ssh),
            (r"type ..\SSH~1\id_rsa", ssh),
            (r"type ..\.s?h\..\.s*h\id_rsa", ssh),
            (r"type ..\...\.s?h\id_rsa", ssh),
            (r"type \Users\User\.s?h\id_rsa", ssh),
            (r"type \Users\*\.s?h\id_rsa", ssh),
            (r"type \\localhost\c$\Users\User\.s?h\id_rsa", ssh),
            (r"type \\127.0.0.1\C$\Users\User\.s?h\id_rsa", ssh),
            (r"type \\?\C:\Users\User\.s?h\id_rsa", ssh),
            (r"type \??\C:\Users\User\.s?h\id_rsa", ssh),
            (r"type \\?\UNC\localhost\c$\Users\User\.s?h\id_rsa", ssh),
            (r"type \\server\share\.s?h\id_rsa", ssh),
            (r"type FileSystem::C:\Users\User\.s?h\id_rsa", ssh),
            (r"type C:\Users\ADMINI~1\.s?h\id_rsa", ssh),
            (r"type D:\Users\x\.s?h\id_rsa", ssh),
            (r"type [c]:\Users\User\.s?h\id_rsa", ssh),
            (r"type /c/Users/User/.s?h/id_rsa", ssh),
            (r"type ~\.s`sh\id_rsa", ssh),
            (r"Get-Content -Path:..\.s?h\id_rsa", ssh),
            (r"type %USERPROFILE%\.ss?\id_rsa", ssh),
            (r"type ${env:USERPROFILE}\.ss?\id_rsa", ssh),
            (r"type \etc\shado?", "forbidden path: /etc/shadow"),
        ] {
            assert_eq!(forbidden_reason_at(cmd, &project), Some(why), "{cmd}");
        }
        for cmd in [
            r"type .\notes.txt",
            r"Get-ChildItem -Path . -Filter *.json",
            r"type \\server\share\report.docx",
            r"type ..\.sshx\id_rsa",
            r"dir C:\Users\User",
        ] {
            assert_eq!(forbidden_reason_at(cmd, &project), None, "{cmd}");
        }
    }

    /// A made-up home on disk: keys, the app's secrets and the consent
    /// ledger, and a project in it. Returns the folder to delete after.
    fn tree(tag: &str) -> (PathBuf, Place) {
        let root = std::env::temp_dir().join(format!("grokhub-floor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let ws = home.join("my-app");
        for dir in [
            home.join(".ssh"),
            home.join(".config").join("GrokHub"),
            ws.join("src"),
            ws.join("target").join("debug"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for file in [
            home.join(".ssh").join("id_rsa"),
            home.join(".config").join("GrokHub").join("secrets.json"),
            home.join(".config").join("GrokHub").join("consent.jsonl"),
            ws.join("src").join("main.ts"),
            ws.join("lib.rs"),
            ws.join("notes.json"),
            ws.join("target").join("debug").join("app"),
        ] {
            std::fs::write(file, "x").unwrap();
        }
        let place = Place {
            cwd: ws,
            home: Some(home.clone()),
            configs: vec![home.join(".config").join("GrokHub")],
            grok_home: Some(home.join(".grok")),
            windows: cfg!(windows),
            fs: true,
        };
        (root, place)
    }

    /// Everyday commands in a project pass the floor, with keys and app
    /// secrets on disk one folder up.
    #[test]
    fn everyday_commands_pass_the_floor() {
        let (root, place) = tree("everyday");
        for cmd in [
            "ls *.rs",
            "cargo test",
            "git status",
            "find . -name '*.json'",
            "cat src/*.ts",
            "rm -rf target/*",
            "cd .. && ls",
            "du -sh *",
            "grep -r foo .",
            "echo $HOME",
        ] {
            assert_eq!(forbidden_reason_at(cmd, &place), None, "{cmd}");
            assert!(!consent_at(cmd, &place), "{cmd}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Read on the filesystem, a glob or a link that leads to a guarded
    /// path counts, however the word spells it.
    #[cfg(unix)]
    #[test]
    fn links_and_globs_on_disk_count_where_they_lead() {
        let (root, place) = tree("links");
        let home = place.home.clone().unwrap();
        std::os::unix::fs::symlink(home.join(".ssh"), place.cwd.join("keys")).unwrap();
        std::os::unix::fs::symlink(&home, place.cwd.join("up")).unwrap();
        let app = home.join(".config").join("GrokHub");
        std::os::unix::fs::symlink(&app, place.cwd.join("conf")).unwrap();
        let ssh = Some("forbidden path: ssh keys");
        for cmd in [
            "cat keys/id_rsa",
            "ls keys",
            "cat k*/id_rsa",
            "cat ke?s/*",
            "cat up/.s*h/id_rsa",
            "cd up && cat .s*h/*",
        ] {
            assert_eq!(forbidden_reason_at(cmd, &place), ssh, "{cmd}");
        }
        assert_eq!(
            forbidden_reason_at("cat conf/*", &place),
            Some("forbidden path: app secrets")
        );
        assert!(consent_at("tail co?f/*.jsonl", &place));
        for cmd in ["cat up/my-app/lib.rs", "ls up", "cat missing/x", "cat conf"] {
            assert_eq!(forbidden_reason_at(cmd, &place), None, "{cmd}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Past what the floor reads of the filesystem, a word still counts
    /// when it names a guarded path as written.
    #[test]
    fn past_the_filesystem_bound_written_paths_still_count() {
        let (root, place) = tree("bound");
        let globs = "ls */x* ".repeat(400);
        let cmd = format!("{globs}; cat ~/.s?h/id_rsa");
        let started = Instant::now();
        assert_eq!(
            forbidden_reason_at(&cmd, &place),
            Some("forbidden path: ssh keys")
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The floor reads a relative word from the workspace it is told, not
    /// only from the process's folder.
    /// A relative word counts from each spelling of the folder it is read
    /// in, however many parts that folder has.
    #[test]
    fn relative_words_count_from_each_spelling_of_their_folder() {
        let ssh = Some("forbidden path: ssh keys");
        let shadow = Some("forbidden path: /etc/shadow");
        let secrets = Some("forbidden path: app secrets");
        // Globs, so that only the word model can tell.
        for (cwd, cmd, want) in [
            ("/home/user/.config", "cat GrokHub/secre?s.json", secrets),
            ("/home/user/.config", "cat ./x/../G*/secre?s.json", secrets),
            ("/home/user/my-app", "cat ../.s?h/id_rsa", ssh),
            ("/home/user/my-app", "cat src/../../.s?h/id_rsa", ssh),
            ("/home/user/my-app", "cat .s?h/id_rsa", None),
            ("/home/user/my-app", "cat src/../.s?h/id_rsa", None),
            ("/private", "cat etc/shad?w", shadow),
            ("/private/etc", "cat shad?w", shadow),
            ("/System/Volumes/Data/Users/user", "cat .s?h/id_rsa", ssh),
            ("/System/Volumes/Data/Users/user/.ssh", "cat id_rsa", ssh),
            (
                "/System/Volumes/Data/home/user/.config",
                "cat */secre?s.json",
                secrets,
            ),
            ("/System", "cat Volumes/Data/Users/user/.s?h/id_rsa", ssh),
            (
                "/System/Volumes/Data/Users/user/my-app",
                "cat .s?h/id_rsa",
                None,
            ),
        ] {
            assert_eq!(forbidden_reason_at(cmd, &fake(cwd)), want, "{cmd} in {cwd}");
        }
    }

    #[test]
    fn the_floor_reads_words_from_the_workspace() {
        assert_eq!(forbidden_reason("cat shad?w"), None);
        assert_eq!(
            in_workspace(Path::new("/etc"), || forbidden_reason("cat shad?w")),
            Some("forbidden path: /etc/shadow")
        );
        assert_eq!(forbidden_reason("cat shad?w"), None);
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
