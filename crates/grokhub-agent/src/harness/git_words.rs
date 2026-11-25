//! How a shell command runs git, read the way the shell reads it, and
//! whether a git command in it can push.
//!
//! The rule is closed: a command in which the shell could run git counts as
//! one that may push unless every git in it is read in full as a command
//! that can't. [`read_shell`] reads the simple commands the shell runs
//! ([`Run`]): quotes, backslashes (`g\it`), `$'…'` and `$"…"` strings,
//! comments, `;` `&&` `||` `|` `&` and newlines, here-documents and
//! here-strings, the bodies of `$(…)`, backquotes, `<(…)` and `>(…)`, a
//! script given to a shell (`sh -c`, `bash -lc`) or to `eval`, and the
//! wrappers whose options it knows (`env`, `sudo`, `nice`, `timeout`,
//! `stdbuf`, `flock`, `setsid`, `nohup`, `xargs`, …). On Windows it reads
//! PowerShell's quoting instead (a backtick escapes, a backslash is a path
//! separator). Anything else in which the shell could run git is
//! [`ShellRead::hidden`]: a command name built when it runs (`G=git; $G`,
//! `${x}it`, a glob), a git word or an expansion that may be git given to a
//! program the reader doesn't know (it may run it: `find -exec`, `watch`,
//! `ssh`, `alias`), text that holds
//! git fed to such a program (`echo … | sh`, a here-document) or written to
//! a file (`> x.sh`, `ln -s /usr/bin/git g`), and a command it can't read.
//!
//! [`git_may_push`] then says a git command can't push only when its verb is
//! one of git's own built-in commands that can't push, it has no alias and
//! nothing on the line changes `PATH`, `GIT_EXEC_PATH`, an editor or any
//! other variable but a few plain ones, there is no `-c`, `--config-env` or
//! `git config` write (but for a few keys that never name a program, such
//! as `user.name`), no transport that runs a program (`ext::`, `fd::`,
//! `--upload-pack`) and no option that names one (`grep -O`, `rebase -x`).
//! Words in quotes are data: `git commit -m "git push"` is a commit.
//!
//! An expansion is read as one that may give git when the command holds
//! text that may be git (a git word anywhere, git's own output, a
//! builder's such as `tr` or `base64 -d`), when the shell builds it from
//! parts (`${x}it`, `$a$b`, `${x/a/b}`, a glob), or when the command sets
//! `IFS`.
//!
//! What text can't show is outside this reading: a program that builds a
//! git command when it runs (`python -c`, a compiled tool, `make`, `cut`
//! on other text), what a script file or a download holds, a program's
//! output (`$(cat f)`, PowerShell's `$_` in a pipeline), the values of
//! variables the command doesn't set (the app's own environment, which an
//! agent's command can't change: each runs in a new shell), and the
//! repository's own hooks and config. The push gates in `grokhub-code`
//! hold for those.

use std::sync::OnceLock;
use std::time::Duration;

/// Stands in a word for an expansion the reader can't see: `$NAME`,
/// `${…}`, `$(…)`, backquotes, `$((…))`, a process substitution.
pub const HIDDEN: char = '\u{FFFC}';

/// How deep scripts inside scripts are read (`bash -c "eval '…'"`).
const MAX_DEPTH: u8 = 8;

// ---- words, as the shell reads them ----

/// One shell word after quote removal.
#[derive(Debug, Clone, Default)]
struct Word {
    /// Its text; each expansion is one [`HIDDEN`].
    text: String,
    /// The names of its plain `$NAME` and `${NAME}` expansions.
    vars: Vec<String>,
    /// It has an expansion other than a plain variable: `$(…)`, `${X:-…}`.
    complex: bool,
    /// The shell computes part of it from text: `${X:-…}`, `${X/a/b}`.
    ops: bool,
    /// An expansion outside double quotes: the shell may split the word.
    split: bool,
    /// An unquoted glob or brace list.
    glob: bool,
    /// Part of it was quoted or escaped.
    quoted: bool,
    /// Commands it runs first: `$(…)`, backquotes, `<(…)`.
    subs: Vec<String>,
    /// Commands that read the output of the command it is in: `>(…)`.
    feeds: Vec<String>,
}

enum Tok {
    Word(Word),
    /// `;`, `&`, `&&`, `||`, a newline.
    Sep,
    /// `|`, `|&`.
    Pipe,
    /// `(`, `)`, and `{`, `}` in PowerShell.
    Paren,
    /// Output to this file.
    Writes(Word),
    /// Input from a file (`false`) or from text in the command (`true`: a
    /// here-document, a here-string, `< <(…)`).
    Reads(Word, bool),
}

struct Lexer {
    c: Vec<char>,
    i: usize,
    ps: bool,
    toks: Vec<Tok>,
    /// Here-documents whose bodies start after the next newline: the token
    /// to fill, the end line, `<<-`, and whether the body expands.
    pending: Vec<(usize, String, bool, bool)>,
}

/// The tokens of `text`, or `None` when it isn't a command the reader can
/// follow (an open quote, an unclosed `$(`).
fn lex(text: &str, ps: bool) -> Option<Vec<Tok>> {
    // PowerShell also opens and closes a string at a curly quote.
    if ps && text.contains(|c: char| ('\u{2018}'..='\u{201e}').contains(&c)) {
        return None;
    }
    Lexer {
        c: text.chars().collect(),
        i: 0,
        ps,
        toks: Vec::new(),
        pending: Vec::new(),
    }
    .run()
}

/// What `` `x `` stands for in PowerShell.
fn ps_escape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' => '\u{1b}',
        'f' => '\u{c}',
        'v' => '\u{b}',
        c => c,
    }
}

impl Lexer {
    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn esc(&self) -> char {
        if self.ps {
            '`'
        } else {
            '\\'
        }
    }

    fn run(mut self) -> Option<Vec<Tok>> {
        loop {
            while matches!(self.at(0), Some(' ' | '\t')) || (self.ps && self.at(0) == Some('\r')) {
                self.i += 1;
            }
            let Some(ch) = self.at(0) else { break };
            if ch == self.esc() && self.at(1) == Some('\n') {
                self.i += 2;
                continue;
            }
            match ch {
                '\n' => {
                    self.i += 1;
                    self.toks.push(Tok::Sep);
                    self.heredocs()?;
                }
                '#' => {
                    while self.at(0).is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                ';' => {
                    while matches!(self.at(0), Some(';' | '&')) {
                        self.i += 1;
                    }
                    self.toks.push(Tok::Sep);
                }
                '&' if self.at(1) == Some('&') => {
                    self.i += 2;
                    self.toks.push(Tok::Sep);
                }
                '&' if self.at(1) == Some('>') && !self.ps => {
                    self.i += 2;
                    if self.at(0) == Some('>') {
                        self.i += 1;
                    }
                    self.out_to()?;
                }
                '&' if self.ps => {
                    // PowerShell's call operator: `& git push`.
                    self.i += 1;
                    self.toks.push(Tok::Word(Word {
                        text: "&".into(),
                        ..Word::default()
                    }));
                }
                '&' => {
                    self.i += 1;
                    self.toks.push(Tok::Sep);
                }
                '|' if self.at(1) == Some('|') => {
                    self.i += 2;
                    self.toks.push(Tok::Sep);
                }
                '|' => {
                    self.i += if self.at(1) == Some('&') { 2 } else { 1 };
                    self.toks.push(Tok::Pipe);
                }
                '(' | ')' => {
                    self.i += 1;
                    self.toks.push(Tok::Paren);
                }
                '{' | '}' if self.ps => {
                    self.i += 1;
                    self.toks.push(Tok::Paren);
                }
                '<' | '>' if self.at(1) != Some('(') => self.redirect()?,
                c if c.is_ascii_digit() && self.fd_number() => self.redirect()?,
                _ => {
                    let w = self.word()?;
                    self.toks.push(Tok::Word(w));
                }
            }
        }
        if !self.pending.is_empty() {
            self.heredocs()?;
        }
        Some(self.toks)
    }

    /// Steps past a descriptor number right before `<` or `>` (`2>&1`).
    fn fd_number(&mut self) -> bool {
        let mut k = 0;
        while self.at(k).is_some_and(|c| c.is_ascii_digit()) {
            k += 1;
        }
        if matches!(self.at(k), Some('<' | '>')) && self.at(k + 1) != Some('(') {
            self.i += k;
            true
        } else {
            false
        }
    }

    fn skip_blanks(&mut self) {
        while matches!(self.at(0), Some(' ' | '\t')) {
            self.i += 1;
        }
    }

    /// The word a redirection names, which must be there.
    fn target(&mut self) -> Option<Word> {
        self.skip_blanks();
        let w = self.word()?;
        (!w.text.is_empty() || w.quoted).then_some(w)
    }

    /// After `>`, `>>`, `&>`: where the output goes.
    fn out_to(&mut self) -> Option<()> {
        let t = self.target()?;
        let sink = matches!(
            t.text.to_ascii_lowercase().as_str(),
            "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/tty" | "nul"
        );
        if !sink {
            self.toks.push(Tok::Writes(t));
        } else if !t.subs.is_empty() {
            self.toks.push(Tok::Reads(t, false));
        }
        Some(())
    }

    fn redirect(&mut self) -> Option<()> {
        let ch = self.at(0)?;
        if ch == '>' {
            self.i += 1;
            if matches!(self.at(0), Some('>' | '|')) {
                self.i += 1;
            }
            if self.at(0) == Some('&') {
                self.i += 1;
                let t = self.target()?;
                if t.text == "-" || t.text.chars().all(|c| c.is_ascii_digit()) {
                    return Some(());
                }
                self.toks.push(Tok::Writes(t));
                return Some(());
            }
            return self.out_to();
        }
        // `<`
        self.i += 1;
        match self.at(0) {
            Some('<') if self.at(1) == Some('<') => {
                self.i += 2;
                let t = self.target()?;
                self.toks.push(Tok::Reads(t, true));
            }
            Some('<') => {
                self.i += 1;
                let strip = self.at(0) == Some('-');
                if strip {
                    self.i += 1;
                }
                let end = self.target()?;
                self.pending
                    .push((self.toks.len(), end.text.clone(), strip, !end.quoted));
                self.toks.push(Tok::Reads(Word::default(), true));
            }
            Some('&') => {
                self.i += 1;
                self.target()?;
            }
            Some('>') => {
                self.i += 1;
                let t = self.target()?;
                self.toks.push(Tok::Writes(t));
            }
            _ => {
                let t = self.target()?;
                let text = t.text.contains(HIDDEN);
                self.toks.push(Tok::Reads(t, text));
            }
        }
        Some(())
    }

    /// The bodies of the here-documents opened on the line just read.
    fn heredocs(&mut self) -> Option<()> {
        for (tok, end, strip, expand) in std::mem::take(&mut self.pending) {
            let mut body = String::new();
            while self.i < self.c.len() {
                let start = self.i;
                while self.at(0).is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
                let line: String = self.c[start..self.i].iter().collect();
                if self.at(0) == Some('\n') {
                    self.i += 1;
                }
                let line = if strip {
                    line.trim_start_matches('\t').to_string()
                } else {
                    line
                };
                if line == end {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            let word = if expand {
                let mut inner = Lexer {
                    c: body.chars().collect(),
                    i: 0,
                    ps: self.ps,
                    toks: Vec::new(),
                    pending: Vec::new(),
                };
                let mut w = Word::default();
                inner.quoted_text(&mut w, None)?;
                w
            } else {
                Word {
                    text: body,
                    quoted: true,
                    ..Word::default()
                }
            };
            self.toks[tok] = Tok::Reads(word, true);
        }
        Some(())
    }

    /// One word, up to an unquoted blank or operator.
    fn word(&mut self) -> Option<Word> {
        let mut w = Word::default();
        let mut brace = 0usize;
        let mut list = false;
        while let Some(ch) = self.at(0) {
            if ch == self.esc() {
                match self.at(1) {
                    Some('\n') => self.i += 2,
                    Some(n) => {
                        w.text.push(if self.ps { ps_escape(n) } else { n });
                        w.quoted = true;
                        self.i += 2;
                    }
                    None => {
                        w.text.push(ch);
                        self.i += 1;
                    }
                }
                continue;
            }
            match ch {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')' => break,
                '\r' | '{' | '}' if self.ps => break,
                '<' | '>' => {
                    // `<(…)`, `>(…)` at the start of a word.
                    if self.at(1) != Some('(') || !w.text.is_empty() || w.quoted {
                        break;
                    }
                    self.i += 2;
                    let inner = self.until_paren()?;
                    if ch == '>' {
                        w.feeds.push(inner);
                    } else {
                        w.subs.push(inner);
                    }
                    w.text.push(HIDDEN);
                    w.complex = true;
                }
                '\'' => {
                    self.i += 1;
                    self.single(&mut w)?;
                }
                '"' => {
                    self.i += 1;
                    self.quoted_text(&mut w, Some('"'))?;
                }
                '$' => self.dollar(&mut w, false)?,
                '@' if self.ps && self.at(1) == Some('(') => {
                    self.i += 2;
                    let inner = self.until_paren()?;
                    w.subs.push(inner);
                    w.text.push(HIDDEN);
                    w.complex = true;
                    w.split = true;
                }
                '`' if !self.ps => {
                    self.i += 1;
                    let inner = self.backquote(false)?;
                    w.subs.push(inner);
                    w.text.push(HIDDEN);
                    w.complex = true;
                    w.split = true;
                }
                '*' | '?' | '[' => {
                    w.glob = true;
                    w.text.push(ch);
                    self.i += 1;
                }
                '{' => {
                    brace += 1;
                    w.text.push(ch);
                    self.i += 1;
                }
                ',' if brace > 0 => {
                    list = true;
                    w.text.push(ch);
                    self.i += 1;
                }
                '.' if brace > 0 && self.at(1) == Some('.') => {
                    list = true;
                    w.text.push_str("..");
                    self.i += 2;
                }
                '}' => {
                    if brace > 0 {
                        brace -= 1;
                        w.glob |= list && !self.ps;
                    }
                    w.text.push(ch);
                    self.i += 1;
                }
                _ => {
                    w.text.push(ch);
                    self.i += 1;
                }
            }
        }
        Some(w)
    }

    /// After an opening `'`.
    fn single(&mut self, w: &mut Word) -> Option<()> {
        w.quoted = true;
        loop {
            let ch = self.at(0)?;
            self.i += 1;
            if ch == '\'' {
                if self.ps && self.at(0) == Some('\'') {
                    w.text.push('\'');
                    self.i += 1;
                    continue;
                }
                return Some(());
            }
            w.text.push(ch);
        }
    }

    /// The inside of double quotes, after the opening one, up to `end`; or
    /// a here-document's body when `end` is `None`.
    fn quoted_text(&mut self, w: &mut Word, end: Option<char>) -> Option<()> {
        w.quoted = true;
        loop {
            let Some(ch) = self.at(0) else {
                return end.is_none().then_some(());
            };
            if Some(ch) == end {
                if self.ps && self.at(1) == Some('"') {
                    w.text.push('"');
                    self.i += 2;
                    continue;
                }
                self.i += 1;
                return Some(());
            }
            if ch == self.esc() {
                let n = self.at(1)?;
                self.i += 2;
                if self.ps {
                    w.text.push(ps_escape(n));
                } else if n == '\n' {
                } else if matches!(n, '$' | '`' | '\\') || (Some(n) == end) {
                    w.text.push(n);
                } else {
                    w.text.push('\\');
                    w.text.push(n);
                }
                continue;
            }
            match ch {
                '$' => self.dollar(w, true)?,
                '`' if !self.ps => {
                    self.i += 1;
                    let inner = self.backquote(end.is_some())?;
                    w.subs.push(inner);
                    w.text.push(HIDDEN);
                    w.complex = true;
                }
                _ => {
                    w.text.push(ch);
                    self.i += 1;
                }
            }
        }
    }

    /// A `$` and what follows it.
    fn dollar(&mut self, w: &mut Word, quoted: bool) -> Option<()> {
        let next = self.at(1);
        if self.ps {
            match next {
                Some('(') => {
                    self.i += 2;
                    let inner = self.until_paren()?;
                    w.subs.push(inner);
                    w.complex = true;
                }
                Some('{') => {
                    self.i += 2;
                    let inner = self.until_brace()?;
                    w.vars.push(inner);
                }
                Some(c) if c.is_alphanumeric() || c == '_' || c == '?' => {
                    self.i += 1;
                    let mut name = String::new();
                    while let Some(c) = self
                        .at(0)
                        .filter(|c| c.is_alphanumeric() || "_:?".contains(*c))
                    {
                        name.push(c);
                        self.i += 1;
                    }
                    w.vars.push(name);
                }
                _ => {
                    w.text.push('$');
                    self.i += 1;
                    return Some(());
                }
            }
            w.text.push(HIDDEN);
            w.split |= !quoted;
            return Some(());
        }
        match next {
            Some('\'') if !quoted => {
                self.i += 2;
                w.quoted = true;
                let mut raw = String::new();
                loop {
                    let ch = self.at(0)?;
                    self.i += 1;
                    if ch == '\'' {
                        break;
                    }
                    raw.push(ch);
                    if ch == '\\' {
                        raw.push(self.at(0)?);
                        self.i += 1;
                    }
                }
                w.text.push_str(&decode_escapes(&raw, true));
                return Some(());
            }
            Some('"') if !quoted => {
                self.i += 2;
                return self.quoted_text(w, Some('"'));
            }
            Some('(') if self.at(2) == Some('(') => {
                self.i += 3;
                let inner = self.until_paren()?;
                // `$(( … ))`: only its substitutions run.
                let mut sub = Lexer {
                    c: inner.chars().collect(),
                    i: 0,
                    ps: false,
                    toks: Vec::new(),
                    pending: Vec::new(),
                };
                while sub.at(0).is_some() {
                    match sub.at(0) {
                        Some('$') => {
                            let mut part = Word::default();
                            sub.dollar(&mut part, true)?;
                            w.subs.extend(part.subs);
                        }
                        Some('`') => {
                            sub.i += 1;
                            w.subs.push(sub.backquote(false)?);
                        }
                        _ => sub.i += 1,
                    }
                }
                if self.at(0) == Some(')') {
                    self.i += 1;
                }
                w.complex = true;
            }
            Some('(') => {
                self.i += 2;
                let inner = self.until_paren()?;
                w.subs.push(inner);
                w.complex = true;
            }
            Some('{') => {
                self.i += 2;
                let inner = self.until_brace()?;
                if is_name(&inner)
                    || matches!(inner.as_str(), "@" | "*" | "#" | "?" | "$" | "!" | "-")
                {
                    w.vars.push(inner);
                } else {
                    w.complex = true;
                    w.ops = true;
                    // `${X:-$(…)}`: what runs inside it.
                    let mut part = Word::default();
                    let mut sub = Lexer {
                        c: inner.chars().collect(),
                        i: 0,
                        ps: false,
                        toks: Vec::new(),
                        pending: Vec::new(),
                    };
                    sub.quoted_text(&mut part, None)?;
                    w.subs.extend(part.subs);
                }
            }
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {
                self.i += 1;
                let mut name = String::new();
                while let Some(c) = self
                    .at(0)
                    .filter(|c| *c == '_' || c.is_ascii_alphanumeric())
                {
                    name.push(c);
                    self.i += 1;
                }
                w.vars.push(name);
                w.text.push(HIDDEN);
                w.split |= !quoted;
                return Some(());
            }
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                self.i += 2;
                w.vars.push(c.to_string());
            }
            _ => {
                w.text.push('$');
                self.i += 1;
                return Some(());
            }
        }
        w.text.push(HIDDEN);
        w.split |= !quoted;
        Some(())
    }

    /// The text up to the `)` that closes the one just read, which is
    /// stepped over.
    fn until_paren(&mut self) -> Option<String> {
        self.until_close('(', ')')
    }

    fn until_brace(&mut self) -> Option<String> {
        self.until_close('{', '}')
    }

    fn until_close(&mut self, open: char, close: char) -> Option<String> {
        let start = self.i;
        let mut depth = 1usize;
        let mut quote: Option<char> = None;
        while let Some(ch) = self.at(0) {
            self.i += 1;
            if let Some(q) = quote {
                if ch == q {
                    quote = None;
                } else if ch == self.esc() && q != '\'' {
                    self.i += 1;
                }
                continue;
            }
            if ch == self.esc() {
                self.i += 1;
            } else if ch == '\'' || ch == '"' || (ch == '`' && !self.ps) {
                quote = Some(ch);
            } else if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(self.c[start..self.i - 1].iter().collect());
                }
            }
        }
        None
    }

    /// After an opening backquote: the command up to the closing one, its
    /// backslashes read as the shell reads them there.
    fn backquote(&mut self, in_dq: bool) -> Option<String> {
        let mut out = String::new();
        loop {
            let ch = self.at(0)?;
            self.i += 1;
            match ch {
                '`' => return Some(out),
                '\\' => {
                    let n = self.at(0)?;
                    self.i += 1;
                    if matches!(n, '`' | '\\' | '$') || (in_dq && n == '"') {
                        out.push(n);
                    } else {
                        out.push('\\');
                        out.push(n);
                    }
                }
                c => out.push(c),
            }
        }
    }
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// `text` with backslash escapes decoded: `\x67`, `\147`, `g`, and
/// with `ansi` the rest of `$'…'` (`\n`, `\e`, `\cX`, `\'`).
fn decode_escapes(text: &str, ansi: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let number = |from: usize, max: usize, radix: u32| -> (Option<char>, usize) {
        let mut j = from;
        while j < chars.len() && j - from < max && chars[j].is_digit(radix) {
            j += 1;
        }
        let digits: String = chars[from..j].iter().collect();
        let ch = u32::from_str_radix(&digits, radix)
            .ok()
            .and_then(char::from_u32);
        (ch.filter(|_| j > from), j)
    };
    while i < chars.len() {
        if chars[i] != '\\' || i + 1 == chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let n = chars[i + 1];
        let (ch, next) = match n {
            'x' => number(i + 2, 2, 16),
            'u' => number(i + 2, 4, 16),
            'U' => number(i + 2, 8, 16),
            '0'..='7' => number(i + 1, if n == '0' { 4 } else { 3 }, 8),
            'n' if ansi => (Some('\n'), i + 2),
            't' if ansi => (Some('\t'), i + 2),
            'r' if ansi => (Some('\r'), i + 2),
            'a' if ansi => (Some('\u{7}'), i + 2),
            'b' if ansi => (Some('\u{8}'), i + 2),
            'e' | 'E' if ansi => (Some('\u{1b}'), i + 2),
            'f' if ansi => (Some('\u{c}'), i + 2),
            'v' if ansi => (Some('\u{b}'), i + 2),
            'c' if ansi && i + 2 < chars.len() => (
                char::from_u32(chars[i + 2].to_ascii_uppercase() as u32 ^ 0x40),
                i + 3,
            ),
            c if ansi => (Some(c), i + 2),
            _ => (None, i + 1),
        };
        match ch {
            Some(c) => {
                out.push(c);
                i = next;
            }
            None => {
                out.push('\\');
                i += 1;
            }
        }
    }
    out
}

// ---- the commands the shell runs ----

/// One simple command the shell runs, as read: past `NAME=value` words,
/// keywords and the wrappers the reader knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// `NAME=value` it runs with: its own, a wrapper's (`env NAME=value`),
    /// and those of a shell it runs in (`NAME=value sh -c '…'`).
    pub env: Vec<String>,
    /// Its words, program first, as the shell passes them; an expansion the
    /// reader can't see is [`HIDDEN`].
    pub words: Vec<String>,
    /// Behind a wrapper, in a script given to a shell or `eval`, or in a
    /// substitution.
    pub nested: bool,
    /// More words come on its stdin when it runs (`xargs`).
    pub more_args: bool,
}

/// What the shell runs for one command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellRead {
    /// Every simple command, in the order the shell runs them, a
    /// substitution's before the command it is in.
    pub runs: Vec<Run>,
    /// Why some part of the command could run git that the reader doesn't
    /// read, if one does.
    pub hidden: Option<&'static str>,
    /// `NAME=value` set for the commands after it (`PATH=/x;`, `export
    /// EDITOR=sh`).
    pub sets: Vec<String>,
}

pub const BUILT: &str = "a command name built when it runs";
pub const GIVEN: &str = "a git word given to a program that may run it";
pub const FED: &str = "text that may hold git, read by a program that may run it";
pub const WRITTEN: &str = "text that holds git, written to a file";
pub const UNREAD: &str = "a command the reader can't follow";
pub const KEPT: &str = "text that holds git, kept in a variable a later command may run";
const DEEP: &str = "scripts nested too deep to read";

/// How `command` runs git, read as the shell that runs it reads it: sh on
/// Linux and macOS, PowerShell on Windows.
pub fn read_shell(command: &str) -> ShellRead {
    read_shell_as(command, cfg!(windows))
}

fn read_shell_as(command: &str, ps: bool) -> ShellRead {
    let mut reader = Reader {
        out: ShellRead::default(),
        raw: command.to_string(),
        ps,
        git_text: false,
        git_ran: false,
        built: false,
        wrote: false,
        shell_built: Vec::new(),
        ifs: false,
    };
    reader.list(
        command,
        &Ctx {
            env: Vec::new(),
            nested: false,
            depth: 0,
            more_args: false,
        },
    );
    reader.out
}

#[derive(Clone)]
struct Ctx {
    env: Vec<String>,
    nested: bool,
    depth: u8,
    more_args: bool,
}

#[derive(Default)]
struct Cmd {
    words: Vec<Word>,
    writes: Vec<Word>,
    reads: Vec<(Word, bool)>,
    pipe_in: bool,
}

struct Reader {
    out: ShellRead,
    raw: String,
    ps: bool,
    /// A word that could be git, or one the reader can't see, was data
    /// earlier in the command.
    git_text: bool,
    /// git ran earlier in the command: its output can be any text.
    git_ran: bool,
    /// A program that builds its output when it runs (`tr`, `printf %s`)
    /// ran earlier in the command.
    built: bool,
    /// Text built when the command runs (git's output, `base64 -d`'s) was
    /// written to a file earlier in the command.
    wrote: bool,
    /// Variables the command sets to a value the shell builds from other
    /// values (`b=${a}it`, `for x in $a$b`), which may be git.
    shell_built: Vec<String>,
    /// The command sets `IFS`, so an unquoted expansion may split anywhere.
    ifs: bool,
}

/// Variables that are the system's own and never set by the command: an
/// expansion of one in a folder part (`$HOME/.cargo/bin/cargo`) doesn't
/// hide the program's name.
const SYSTEM_VARS: &[&str] = &[
    "HOME",
    "PWD",
    "OLDPWD",
    "TMPDIR",
    "USER",
    "LOGNAME",
    "env:HOME",
    "env:USERPROFILE",
    "env:TEMP",
    "env:TMP",
    "env:LOCALAPPDATA",
    "env:APPDATA",
];

/// Programs that never run a word or a line they are given: what they read
/// is data. Conditions on a few are in [`Reader::data_program`].
const DATA_PROGRAMS: &[&str] = &[
    "echo",
    "printf",
    "cat",
    "head",
    "tail",
    "wc",
    "uniq",
    "cut",
    "tr",
    "nl",
    "fold",
    "fmt",
    "column",
    "comm",
    "join",
    "paste",
    "expand",
    "unexpand",
    "rev",
    "tac",
    "od",
    "xxd",
    "hexdump",
    "base64",
    "base32",
    "md5sum",
    "sha1sum",
    "sha224sum",
    "sha256sum",
    "sha384sum",
    "sha512sum",
    "b2sum",
    "cksum",
    "stat",
    "file",
    "du",
    "df",
    "ls",
    "dir",
    "tree",
    "pwd",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "which",
    "whereis",
    "type",
    "whoami",
    "id",
    "date",
    "uname",
    "hostname",
    "true",
    "false",
    "test",
    "[",
    "cd",
    "pushd",
    "popd",
    "mkdir",
    "rmdir",
    "touch",
    "cp",
    "mv",
    "ln",
    "install",
    "rm",
    "chmod",
    "diff",
    "cmp",
    "grep",
    "egrep",
    "fgrep",
    "jq",
    "sleep",
    "wait",
    "tee",
    "yes",
    "seq",
    "printenv",
    "export",
    "declare",
    "typeset",
    "local",
    "readonly",
    "unset",
    "shift",
    "exit",
    "return",
    "set",
    "read",
    "umask",
    "sort",
    "find",
    "sed",
    "rg",
    "fd",
    "nproc",
    "ps",
    "pgrep",
    "kill",
    ":",
    "get-content",
    "get-childitem",
    "select-string",
    "write-output",
    "write-host",
    "set-location",
    "get-location",
    "test-path",
    "new-item",
    "remove-item",
    "copy-item",
    "move-item",
    "measure-object",
    "sort-object",
    "select-object",
    "format-table",
    "format-list",
    "out-string",
    "get-date",
    "gc",
    "gci",
    "sls",
    "del",
    "copy",
    "move",
    "ni",
    "ri",
];

/// Of [`DATA_PROGRAMS`], those that write the files their words name.
const FILE_WRITERS: &[&str] = &[
    "tee",
    "cp",
    "mv",
    "ln",
    "install",
    "copy-item",
    "move-item",
    "new-item",
    "copy",
    "move",
    "ni",
];

/// Of [`DATA_PROGRAMS`], those whose output is built when they run, so it
/// can be any text.
const BUILDERS: &[&str] = &[
    "tr", "rev", "base64", "base32", "xxd", "od", "sed", "printf",
];

const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "ash", "mksh", "yash"];

impl Reader {
    fn hide(&mut self, why: &'static str) {
        self.out.hidden.get_or_insert(why);
    }

    fn list(&mut self, text: &str, ctx: &Ctx) {
        if ctx.depth > MAX_DEPTH {
            self.hide(DEEP);
            return;
        }
        let Some(toks) = lex(text, self.ps) else {
            if could_hold_git(text) {
                self.hide(UNREAD);
            }
            return;
        };
        let mut cmd = Cmd::default();
        let mut piped = false;
        for tok in toks {
            match tok {
                Tok::Word(w) => {
                    if cmd.words.is_empty() {
                        cmd.pipe_in = piped;
                    }
                    cmd.words.push(w);
                }
                Tok::Writes(w) => cmd.writes.push(w),
                Tok::Reads(w, text) => {
                    if cmd.words.is_empty() {
                        cmd.pipe_in = piped;
                    }
                    cmd.reads.push((w, text));
                }
                Tok::Sep => {
                    self.finish(std::mem::take(&mut cmd), ctx);
                    piped = false;
                }
                Tok::Pipe => {
                    self.finish(std::mem::take(&mut cmd), ctx);
                    piped = true;
                }
                Tok::Paren => {
                    if !cmd.words.is_empty() {
                        self.finish(std::mem::take(&mut cmd), ctx);
                        piped = false;
                    }
                }
            }
        }
        self.finish(cmd, ctx);
    }

    fn finish(&mut self, cmd: Cmd, ctx: &Ctx) {
        if cmd.words.is_empty() && cmd.reads.is_empty() && cmd.writes.is_empty() {
            return;
        }
        let inner = Ctx {
            env: ctx.env.clone(),
            nested: true,
            depth: ctx.depth + 1,
            more_args: false,
        };
        let words: Vec<&Word> = cmd
            .words
            .iter()
            .chain(&cmd.writes)
            .chain(cmd.reads.iter().map(|(w, _)| w))
            .collect();
        for w in &words {
            for sub in &w.subs {
                self.list(sub, &inner);
            }
        }
        let feeds: Vec<String> = words.iter().flat_map(|w| w.feeds.clone()).collect();
        self.simple(&cmd, 0, ctx, false);
        // `>(…)` reads what the command writes.
        for feed in feeds {
            self.fed_list(&feed, &inner);
        }
    }

    /// A list that reads the output of the command before it.
    fn fed_list(&mut self, text: &str, ctx: &Ctx) {
        let runs_before = self.out.runs.len();
        let ran = self.git_ran;
        self.git_ran = true;
        let mut fed = ctx.clone();
        fed.more_args = false;
        // Read as if piped: its first command reads the output.
        let piped = format!("true |{text}");
        self.list(&piped, &fed);
        self.git_ran = ran || self.git_ran;
        // The `true` added above is not one of the command's own.
        if self.out.runs.len() > runs_before && self.out.runs[runs_before].words == ["true"] {
            self.out.runs.remove(runs_before);
        }
    }

    /// The words of `cmd` from `from` on, as one command; `wrapped` when
    /// a wrapper before it runs it (so no keyword or `NAME=value` is read).
    fn simple(&mut self, cmd: &Cmd, from: usize, ctx: &Ctx, wrapped: bool) {
        let words = &cmd.words;
        let mut i = from;
        let mut env = ctx.env.clone();
        if !wrapped {
            loop {
                let start = i;
                let mut own = Vec::new();
                while let Some(w) = words.get(i).filter(|w| assignment_name(&w.text).is_some()) {
                    own.push(w.text.clone());
                    self.note(w);
                    self.assign(w);
                    i += 1;
                }
                if i == words.len() {
                    // `NAME=value` on its own sets it for what comes after.
                    for w in &words[start..] {
                        self.keep(w);
                    }
                    return;
                }
                env.extend(own);
                let w = &words[i];
                if !w.quoted
                    && matches!(
                        w.text.as_str(),
                        "if" | "then"
                            | "elif"
                            | "else"
                            | "fi"
                            | "do"
                            | "done"
                            | "while"
                            | "until"
                            | "!"
                            | "{"
                            | "}"
                            | "esac"
                            | "coproc"
                    )
                {
                    i += 1;
                    continue;
                }
                break;
            }
            let w = &words[i];
            // `for x in …`, `case … in`, `[[ … ]]`, `function f`: data.
            if !w.quoted
                && matches!(
                    w.text.as_str(),
                    "for" | "select" | "case" | "[[" | "function" | "in"
                )
            {
                if matches!(w.text.as_str(), "for" | "select") {
                    self.loop_var(&words[i + 1..]);
                }
                for w in &words[i + 1..] {
                    self.note(w);
                }
                return;
            }
            // PowerShell's `$name = …`: a variable, set to what follows.
            if self.ps && words.len() > i + 1 && is_ps_assignment(w, &words[i + 1]) {
                self.ps_assign(w, &words[i + 2..]);
                return;
            }
        }
        let Some(head) = words.get(i) else { return };
        let args = &words[i + 1..];
        let run = Run {
            env: env.clone(),
            words: words[i..].iter().map(|w| w.text.clone()).collect(),
            nested: ctx.nested || wrapped,
            more_args: ctx.more_args,
        };
        let leaf = leaf_of(&head.text);
        let built =
            (head.text.contains(HIDDEN) && !self.exempt(head) && self.may_expand_to_git(head))
                || (head.glob && glob_may_be_git(leaf));
        if built {
            self.hide(BUILT);
            return;
        }
        if is_git_name(leaf) {
            self.out.runs.push(run);
            self.git_ran = true;
            self.wrote |= !cmd.writes.is_empty();
            return;
        }
        let prog = program_name(leaf);
        let fed = self.fed(cmd, ctx);
        let mut inner = ctx.clone();
        inner.nested = true;
        inner.depth = ctx.depth + 1;
        match peel(&prog, args, self.ps) {
            Peel::Runs {
                at,
                env: more,
                more_args,
            } => {
                inner.env = env;
                inner.env.extend(more);
                inner.more_args |= more_args;
                inner.depth = ctx.depth;
                if i + 1 + at < words.len() {
                    self.simple(cmd, i + 1 + at, &inner, true);
                }
            }
            Peel::Script(k) => {
                self.out.runs.push(run);
                inner.env = env;
                match args.get(k) {
                    Some(script) if self.may_expand_to_git(script) => self.hide(BUILT),
                    Some(script) => {
                        let text = script.text.clone();
                        self.list(&text, &inner);
                    }
                    None => {}
                }
                for w in args.iter().skip(k + 1) {
                    self.note(w);
                }
            }
            Peel::Eval => {
                self.out.runs.push(run);
                if args.iter().any(|w| self.may_expand_to_git(w)) {
                    self.hide(BUILT);
                    return;
                }
                inner.env = env;
                let text: Vec<&str> = args.iter().map(|w| w.text.as_str()).collect();
                self.list(&text.join(" "), &inner);
            }
            Peel::Stdin => {
                self.out.runs.push(run);
                // `sh <<'EOF'`, `bash <<< '…'`: the script is in the command.
                // The last input wins.
                let script = cmd
                    .reads
                    .last()
                    .filter(|(_, text)| *text && !cmd.pipe_in && !ctx.more_args);
                if script.is_some_and(|(w, _)| self.may_expand_to_git(w)) {
                    self.hide(FED);
                } else if let Some((w, _)) = script {
                    inner.env = env;
                    let text = w.text.clone();
                    self.list(&text, &inner);
                } else if fed || (self.wrote && cmd.reads.iter().any(|(_, text)| !text)) {
                    self.hide(FED);
                }
            }
            Peel::Data => {
                self.out.runs.push(run);
                self.data(&prog, cmd, args, ctx);
            }
            Peel::Unknown => {
                self.out.runs.push(run);
                if args.iter().any(|w| self.could_be_git(w)) {
                    self.hide(GIVEN);
                }
                if fed {
                    self.hide(FED);
                }
                // A script the command wrote: `… > x.sh; sh x.sh`, `./x.sh`.
                let runs_a_file = SHELLS.contains(&prog.as_str())
                    || matches!(prog.as_str(), "." | "source")
                    || head.text.contains(['/', '\\']);
                if self.wrote && runs_a_file {
                    self.hide(WRITTEN);
                }
                for w in args {
                    self.note(w);
                }
            }
        }
    }

    /// A program that only reads its words as data.
    fn data(&mut self, prog: &str, cmd: &Cmd, args: &[Word], ctx: &Ctx) {
        let writes = !cmd.writes.is_empty() || FILE_WRITERS.contains(&prog);
        let piped = cmd.pipe_in || ctx.more_args;
        if writes
            && (args.iter().any(|w| self.could_be_git(w))
                || cmd
                    .reads
                    .iter()
                    .any(|(w, text)| *text && self.could_be_git(w))
                || (prog == "tee" && piped && self.git_text))
        {
            self.hide(WRITTEN);
        }
        if matches!(
            prog,
            "export" | "declare" | "typeset" | "local" | "readonly"
        ) {
            for w in args {
                self.keep(w);
            }
        }
        // `read x y`: set from its input, which [`Reader::fed`] reads.
        if prog == "read" && args.iter().any(|w| w.text == "IFS") {
            self.ifs = true;
        }
        let formats = prog == "printf" && args.first().is_some_and(|w| w.text.contains('%'));
        let builds = (BUILDERS.contains(&prog) && prog != "printf") || formats;
        if writes && (builds || (piped && (self.git_text || self.git_ran || self.built))) {
            self.wrote = true;
        }
        if builds {
            self.built = true;
        }
        for w in args {
            self.note(w);
        }
        for (w, text) in &cmd.reads {
            if *text {
                self.note(w);
            }
        }
    }

    /// The command reads text that may hold git and may run it: on a pipe
    /// after one, from `xargs`, a here-document or a here-string.
    fn fed(&self, cmd: &Cmd, ctx: &Ctx) -> bool {
        ((cmd.pipe_in || ctx.more_args) && (self.git_text || self.git_ran || self.built))
            || cmd
                .reads
                .iter()
                .any(|(w, text)| *text && self.could_be_git(w))
    }

    /// Whether an expansion in `w` may give a git name when it runs. A
    /// variable the command doesn't set has the app's own value, which the
    /// command can't change, and a substitution gives a program's output:
    /// both are outside what the text shows, unless the command holds text
    /// that may be git (a git word, git's own output, a builder's), the shell
    /// builds the value from parts (`${x}it`, `${x/a/b}`, `b=$a$c`), or
    /// `IFS` may split it anywhere.
    fn may_expand_to_git(&self, w: &Word) -> bool {
        if !w.text.contains(HIDDEN) {
            return false;
        }
        w.ops
            || self.git_text
            || self.git_ran
            || self.built
            || (self.ifs && w.split)
            || w.vars
                .iter()
                .any(|v| self.shell_built.iter().any(|b| b == v))
            || parts_may_be_git(&w.text)
    }

    /// A `NAME=value` word: `IFS`, or a value the shell builds from parts.
    fn assign(&mut self, w: &Word) {
        let Some(name) = assignment_name(&w.text) else {
            return;
        };
        if name == "IFS" {
            self.ifs = true;
        }
        let value = w.text.split_once('=').map_or("", |(_, v)| v);
        let built = !w.vars.is_empty()
            || w.ops
            || parts_may_be_git(value)
            || (w.glob && glob_may_be_git(leaf_of(value)));
        if built {
            self.shell_built.push(name.to_string());
        }
    }

    /// `for x in …`: `x` takes each word, built by the shell when one is.
    fn loop_var(&mut self, rest: &[Word]) {
        let Some(name) = rest.first().map(|w| w.text.clone()) else {
            return;
        };
        let built = rest.iter().skip(2).any(|w| {
            !w.vars.is_empty()
                || w.ops
                || parts_may_be_git(&w.text)
                || (w.glob && glob_may_be_git(leaf_of(&w.text)))
        });
        if name == "IFS" {
            self.ifs = true;
        }
        if built {
            self.shell_built.push(name);
        }
    }

    /// PowerShell's `$name = <words>`. `$env:NAME = …` sets it for what comes
    /// after.
    fn ps_assign(&mut self, var: &Word, value: &[Word]) {
        let name = var.vars.first().cloned().unwrap_or_default();
        let text: Vec<&str> = value.iter().map(|w| w.text.as_str()).collect();
        let word = Word {
            text: format!(
                "{}={}",
                name.rsplit(':').next().unwrap_or(&name),
                text.join(" ")
            ),
            vars: value.iter().flat_map(|w| w.vars.clone()).collect(),
            ops: value.iter().any(|w| w.ops),
            glob: value.iter().any(|w| w.glob),
            ..Word::default()
        };
        if let Some(env) = name
            .strip_prefix("env:")
            .or_else(|| name.strip_prefix("ENV:"))
            .or_else(|| name.strip_prefix("Env:"))
        {
            self.keep(&Word {
                text: format!("{env}={}", text.join(" ")),
                ..word.clone()
            });
        } else if is_name(&name) {
            self.assign(&word);
        }
        for w in value {
            self.note(w);
        }
    }

    /// A `NAME=value` set for the commands after it. One that isn't plain
    /// and holds git (`EDITOR='git push …'`) may run in a later command of
    /// the same shell.
    fn keep(&mut self, w: &Word) {
        if assignment_name(&w.text).is_none() {
            return;
        }
        self.assign(w);
        let text = w.text.replace(HIDDEN, " ");
        if !plain_assignment(&w.text)
            && (holds_git(&text)
                || (text.contains('\\') && holds_git(&decode_escapes(&text, false))))
        {
            self.hide(KEPT);
        }
        self.out.sets.push(w.text.clone());
    }

    fn note(&mut self, w: &Word) {
        if self.could_be_git(w) {
            self.git_text = true;
        }
    }

    /// Whether `w` could be a git program's name when it runs: a git word in
    /// it (`git`, `git-push`, `/usr/bin/git`, also after `\x67`-style
    /// escapes), an expansion the reader can't see, or a glob that could
    /// match one.
    fn could_be_git(&self, w: &Word) -> bool {
        if w.text.contains(HIDDEN) && !self.exempt(w) && self.may_expand_to_git(w) {
            return true;
        }
        if holds_git(&w.text)
            || (w.text.contains('\\') && holds_git(&decode_escapes(&w.text, false)))
        {
            return true;
        }
        w.glob && glob_may_be_git(leaf_of(&w.text))
    }

    /// Every expansion in `w` is of a system variable the command doesn't
    /// set, in a folder part: its program name is the literal text after.
    fn exempt(&self, w: &Word) -> bool {
        !w.complex
            && w.subs.is_empty()
            && !leaf_of(&w.text).contains(HIDDEN)
            && w.vars.iter().all(|v| {
                SYSTEM_VARS.iter().any(|s| {
                    if self.ps {
                        s.eq_ignore_ascii_case(v)
                    } else {
                        s == v
                    }
                }) && !self.assigned(v)
            })
    }

    /// The command names `var` other than as an expansion (`HOME=…`, `read
    /// HOME`, `export HOME`).
    fn assigned(&self, var: &str) -> bool {
        let name = var.rsplit(':').next().unwrap_or(var);
        let raw = &self.raw;
        let mut from = 0;
        while let Some(at) = raw[from..].find(name).map(|k| k + from) {
            from = at + name.len();
            let before = raw[..at].chars().next_back();
            let after = raw[from..].chars().next();
            let word_char = |c: Option<char>| c.is_some_and(|c| c == '_' || c.is_alphanumeric());
            if word_char(before) || word_char(after) {
                continue;
            }
            let expanded = before == Some('$')
                || raw[..at].ends_with("${")
                || raw[..at].to_ascii_lowercase().ends_with("$env:")
                || raw[..at].to_ascii_lowercase().ends_with("${env:");
            if !expanded {
                return true;
            }
        }
        false
    }
}

/// The name in a `NAME=value` (or `NAME+=value`, `NAME[k]=value`) word.
fn assignment_name(word: &str) -> Option<&str> {
    let (left, _) = word.split_once('=')?;
    let left = left.strip_suffix('+').unwrap_or(left);
    let name = match left.split_once('[') {
        Some((name, rest)) if rest.ends_with(']') => name,
        Some(_) => return None,
        None => left,
    };
    is_name(name).then_some(name)
}

/// The text could name git when the shell runs it, read loosely: for a
/// command the reader can't follow.
fn could_hold_git(text: &str) -> bool {
    let plain: String = text
        .chars()
        .filter(|c| !matches!(c, '\\' | '\'' | '"' | '`'))
        .collect::<String>()
        .to_ascii_lowercase();
    plain.contains("git") || text.contains('$') || text.contains('`')
}

/// The last part of a path, `/` or `\` separated.
fn leaf_of(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word)
}

/// A program's name as the reader matches it: lower case, with no `.exe`.
fn program_name(leaf: &str) -> String {
    let lower = leaf.to_ascii_lowercase();
    [".exe", ".com", ".cmd", ".bat"]
        .iter()
        .find_map(|ext| lower.strip_suffix(ext))
        .unwrap_or(&lower)
        .to_string()
}

/// `git`, `git.exe`, or a `git-<name>` program, in any letter case (on
/// macOS and Windows the file system doesn't tell them apart).
pub fn is_git_name(leaf: &str) -> bool {
    let name = program_name(leaf);
    name == "git" || (name.len() > 4 && name.starts_with("git-"))
}

/// A git program's name is one of the words in `text`, split where a shell
/// or a program would split it (`=`, `:`, `,`, blanks, quotes).
fn holds_git(text: &str) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || "_.-/\\~+@%".contains(c)))
        .any(|token| is_git_name(leaf_of(token)))
}

/// A glob in a program's name could match `git` or a `git-<name>`
/// program: its literal start fits `git-`, and it has no literal extension
/// other than `.exe`.
fn glob_may_be_git(leaf: &str) -> bool {
    let lower = leaf.to_ascii_lowercase();
    let start: String = lower
        .chars()
        .take_while(|c| !matches!(c, '*' | '?' | '[' | '{'))
        .collect();
    let fits = "git-".starts_with(&start) || start.starts_with("git");
    let ext = lower
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty() && !ext.contains(['*', '?', '[', ']', '{', '}', ',']));
    fits && ext.is_none_or(|ext| matches!(ext, "exe" | "com" | "cmd" | "bat"))
}

/// A word with an expansion and other parts (`${x}it`, `$a$b`) could make
/// a git program's name when it runs: its parts fit one, an expansion
/// standing for any text.
fn parts_may_be_git(text: &str) -> bool {
    if !text.contains(HIDDEN) || text == HIDDEN.to_string() {
        return false;
    }
    let starred = text.replace(HIDDEN, "*");
    let leaf = leaf_of(&starred);
    !leaf.is_empty() && glob_may_be_git(leaf)
}

/// `$name = …`, `$name += …` in PowerShell.
fn is_ps_assignment(var: &Word, op: &Word) -> bool {
    var.text == HIDDEN.to_string()
        && var.vars.len() == 1
        && !op.quoted
        && op.text.ends_with('=')
        && op.text.len() <= 3
        && op.text[..op.text.len() - 1]
            .chars()
            .all(|c| "+-*/%?".contains(c))
}

enum Peel {
    /// The command it runs starts at this word of its own, with more
    /// `NAME=value` for it; `true` when more words come on stdin.
    Runs {
        at: usize,
        env: Vec<String>,
        more_args: bool,
    },
    /// Runs this word of its own as a script (`sh -c`, `flock -c`).
    Script(usize),
    /// Runs its words joined as a script (`eval`).
    Eval,
    /// Reads stdin as commands (`sh` with no script).
    Stdin,
    /// Its words are data.
    Data,
    /// A program the reader doesn't know, or a wrapper it can't follow: it
    /// may run any word it is given. A script file run by a shell is read
    /// so too: what the file holds is outside the reading.
    Unknown,
}

/// What `prog` (lower case, no `.exe`) does with its words.
fn peel(prog: &str, args: &[Word], ps: bool) -> Peel {
    let text: Vec<&str> = args.iter().map(|w| w.text.as_str()).collect();
    let runs = |at: usize| Peel::Runs {
        at,
        env: Vec::new(),
        more_args: false,
    };
    match prog {
        "nohup" | "builtin" | "busybox" | "&" => match text.first() {
            Some(w) if w.starts_with('-') => Peel::Unknown,
            _ => runs(0),
        },
        "command" => {
            let mut k = 0;
            while let Some(w) = text.get(k).filter(|w| w.starts_with('-')) {
                match *w {
                    "-p" => k += 1,
                    "-v" | "-V" => return Peel::Data,
                    _ => return Peel::Unknown,
                }
            }
            runs(k)
        }
        "exec" => options(&text, "cl", "a", &[], &[]).map_or(Peel::Unknown, runs),
        "time" => options(
            &text,
            "apqv",
            "fo",
            &["--append", "--verbose", "--quiet", "--portability"],
            &["--format", "--output"],
        )
        .map_or(Peel::Unknown, runs),
        "setsid" => options(&text, "cfw", "", &["--ctty", "--fork", "--wait"], &[])
            .map_or(Peel::Unknown, runs),
        "nice" => {
            // `nice -5 cmd`, `nice -n 5 cmd`
            let mut k = 0;
            while let Some(w) = text.get(k) {
                if w.len() > 1 && w.starts_with('-') && w[1..].chars().all(|c| c.is_ascii_digit()) {
                    k += 1;
                } else {
                    break;
                }
            }
            options(&text[k..], "", "n", &[], &["--adjustment"])
                .map_or(Peel::Unknown, |at| runs(k + at))
        }
        "ionice" => options(&text, "t", "cn", &["--ignore"], &["--class", "--classdata"])
            .map_or(Peel::Unknown, runs),
        "stdbuf" => options(&text, "", "ioe", &[], &["--input", "--output", "--error"])
            .map_or(Peel::Unknown, runs),
        "chronic" => options(&text, "ev", "", &[], &[]).map_or(Peel::Unknown, runs),
        "unbuffer" => options(&text, "p", "", &[], &[]).map_or(Peel::Unknown, runs),
        "timeout" => match options(
            &text,
            "fv",
            "sk",
            &["--foreground", "--preserve-status", "--verbose"],
            &["--signal", "--kill-after"],
        ) {
            // The duration comes before the command.
            Some(at) if at < text.len() => runs(at + 1),
            _ => Peel::Unknown,
        },
        "sudo" => {
            if text.iter().any(|w| {
                matches!(
                    *w,
                    "-e" | "--edit"
                        | "-l"
                        | "--list"
                        | "-v"
                        | "--validate"
                        | "-K"
                        | "-k"
                        | "-V"
                        | "--version"
                        | "-h"
                        | "--help"
                )
            }) && text.first().is_some_and(|w| w.starts_with('-'))
            {
                // sudoedit, list, validate, reset: nothing of the words runs
                // as a command, or it can't be told what does.
                return Peel::Unknown;
            }
            match options(
                &text,
                "AbEHinPSsBN",
                "CDghpRrTtUu",
                &[
                    "--login",
                    "--shell",
                    "--non-interactive",
                    "--stdin",
                    "--background",
                    "--set-home",
                    "--askpass",
                    "--preserve-env",
                ],
                &[
                    "--user",
                    "--group",
                    "--chdir",
                    "--prompt",
                    "--close-from",
                    "--role",
                    "--type",
                    "--other-user",
                    "--host",
                    "--command-timeout",
                ],
            ) {
                Some(mut at) => {
                    let mut env = Vec::new();
                    while let Some(w) = text.get(at).filter(|w| assignment_name(w).is_some()) {
                        env.push(w.to_string());
                        at += 1;
                    }
                    Peel::Runs {
                        at,
                        env,
                        more_args: false,
                    }
                }
                None => Peel::Unknown,
            }
        }
        "doas" => options(&text, "ns", "uC", &[], &[]).map_or(Peel::Unknown, runs),
        "env" => peel_env(&text),
        "xargs" => match options(
            &text,
            "0oprtx",
            "adEILnPs",
            &[
                "--null",
                "--open-tty",
                "--interactive",
                "--no-run-if-empty",
                "--verbose",
                "--exit",
            ],
            &[
                "--arg-file",
                "--delimiter",
                "--eof",
                "--replace",
                "--max-lines",
                "--max-args",
                "--max-procs",
                "--max-chars",
                "--process-slot-var",
            ],
        ) {
            Some(at) if at < text.len() => Peel::Runs {
                at,
                env: Vec::new(),
                more_args: true,
            },
            // `xargs` alone runs echo.
            Some(_) => Peel::Data,
            None => Peel::Unknown,
        },
        "flock" => {
            match options(
                &text,
                "sxunoF",
                "wE",
                &[
                    "--shared",
                    "--exclusive",
                    "--unlock",
                    "--nonblock",
                    "--close",
                    "--no-fork",
                    "--verbose",
                ],
                &["--wait", "--timeout", "--conflict-exit-code"],
            ) {
                // The lock file, then a command or `-c <script>`.
                Some(at) if at + 1 < text.len() => {
                    if matches!(text[at + 1], "-c" | "--command") {
                        Peel::Script(at + 2)
                    } else {
                        runs(at + 1)
                    }
                }
                Some(_) => Peel::Data,
                None => Peel::Unknown,
            }
        }
        "eval" | "iex" | "invoke-expression" => Peel::Eval,
        _ if SHELLS.contains(&prog) => peel_shell(&text),
        _ if DATA_PROGRAMS.contains(&prog) && data_program(prog, &text, ps) => Peel::Data,
        _ => Peel::Unknown,
    }
}

/// Where a wrapper's command starts, past its options: `flags` are its
/// short options without a value, `valued` those with one (`-n 5`,
/// `-n5`), `long` and `long_valued` the same for `--name`. `None` for an
/// option it doesn't have.
fn options(
    text: &[&str],
    flags: &str,
    valued: &str,
    long: &[&str],
    long_valued: &[&str],
) -> Option<usize> {
    let mut k = 0;
    while let Some(w) = text.get(k) {
        if *w == "--" {
            return Some(k + 1);
        }
        if let Some(name) = w.strip_prefix("--") {
            let (name, has_value) = match name.split_once('=') {
                Some((name, _)) => (name, true),
                None => (name, false),
            };
            let name = format!("--{name}");
            if long_valued.contains(&name.as_str()) {
                k += if has_value { 1 } else { 2 };
            } else if long.contains(&name.as_str()) {
                k += 1;
            } else {
                return None;
            }
            continue;
        }
        let Some(cluster) = w.strip_prefix('-').filter(|c| !c.is_empty()) else {
            return Some(k);
        };
        k += 1;
        for (at, c) in cluster.char_indices() {
            if valued.contains(c) {
                if at + c.len_utf8() == cluster.len() {
                    k += 1;
                }
                break;
            }
            if !flags.contains(c) {
                return None;
            }
        }
    }
    Some(k)
}

/// `env`'s options and `NAME=value` words, then the command.
fn peel_env(text: &[&str]) -> Peel {
    let mut env = Vec::new();
    let mut k = 0;
    while let Some(w) = text.get(k) {
        match *w {
            "--" => {
                k += 1;
                break;
            }
            "-" | "-i" | "-0" | "-v" | "--ignore-environment" | "--null" | "--debug" => k += 1,
            "-u" | "--unset" | "-C" | "--chdir" | "-a" | "--argv0" => k += 2,
            // BSD `env -P <path>` looks the command up there.
            "-P" => {
                env.push(format!("PATH={}", text.get(k + 1).unwrap_or(&"")));
                k += 2;
            }
            _ if w.starts_with("--unset=")
                || w.starts_with("--chdir=")
                || w.starts_with("--argv0=") =>
            {
                k += 1
            }
            _ if w.starts_with("-u") || w.starts_with("-C") => k += 1,
            _ if w.starts_with('-') => return Peel::Unknown,
            _ if assignment_name(w).is_some() => {
                env.push(w.to_string());
                k += 1;
            }
            _ => break,
        }
    }
    if k >= text.len() {
        // No command: it prints the environment.
        return Peel::Data;
    }
    Peel::Runs {
        at: k,
        env,
        more_args: false,
    }
}

/// A shell's options: `-c` takes the first word after them as the script;
/// with no script and no file it reads stdin.
fn peel_shell(text: &[&str]) -> Peel {
    let mut k = 0;
    let mut script = false;
    while let Some(w) = text.get(k) {
        if *w == "--" || *w == "-" {
            k += 1;
            break;
        }
        if w.starts_with("--") {
            match *w {
                "--norc" | "--noprofile" | "--login" | "--posix" | "--noediting"
                | "--restricted" | "--verbose" => k += 1,
                "--rcfile" | "--init-file" => k += 2,
                _ => return Peel::Unknown,
            }
            continue;
        }
        let Some(cluster) = w.strip_prefix(['-', '+']).filter(|c| !c.is_empty()) else {
            break;
        };
        k += 1;
        for c in cluster.chars() {
            match c {
                'c' => script = true,
                'o' | 'O' => k += 1,
                c if c.is_ascii_alphabetic() => {}
                _ => return Peel::Unknown,
            }
        }
    }
    if script {
        Peel::Script(k)
    } else if k < text.len() {
        Peel::Unknown
    } else {
        Peel::Stdin
    }
}

/// [`DATA_PROGRAMS`] that run a command with some options: `find -exec`,
/// `sed`'s `e` command and a script file, `rg --pre`, `fd -x`, `sort
/// --compress-program`.
fn data_program(prog: &str, text: &[&str], ps: bool) -> bool {
    match prog {
        "find" => !text
            .iter()
            .any(|w| matches!(*w, "-exec" | "-execdir" | "-ok" | "-okdir")),
        "sed" => {
            let mut scripts = Vec::new();
            let mut k = 0;
            let mut named = false;
            while let Some(w) = text.get(k) {
                if *w == "--sandbox" {
                    return true;
                }
                if *w == "-f"
                    || w.starts_with("--file")
                    || (w.starts_with('-') && !w.starts_with("--") && w.contains('f'))
                {
                    return false;
                }
                if *w == "-e" || *w == "--expression" {
                    scripts.extend(text.get(k + 1));
                    named = true;
                    k += 2;
                    continue;
                }
                if let Some(s) = w.strip_prefix("--expression=") {
                    scripts.push(s);
                    named = true;
                } else if !w.starts_with('-') && !named && scripts.is_empty() {
                    scripts.push(w);
                }
                k += 1;
            }
            !scripts.iter().any(|s| s.contains('e'))
        }
        "rg" => !text.iter().any(|w| w.starts_with("--pre")),
        "fd" => !text
            .iter()
            .any(|w| matches!(*w, "-x" | "-X") || w.starts_with("--exec")),
        "sort" => !text.iter().any(|w| w.starts_with("--compress")),
        // In PowerShell `type` and `cat` are Get-Content; elsewhere they
        // only read too.
        _ => {
            let _ = ps;
            true
        }
    }
}

// ---- whether a git command can push ----

/// One git command, read as git reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCall<'a> {
    /// git's own options before the verb, each with its value: `("-C",
    /// Some("dir"))`, `("--no-pager", None)`.
    pub options: Vec<(&'a str, Option<&'a str>)>,
    pub verb: Option<&'a str>,
    pub args: &'a [String],
    /// Run as a `git-<name>` program (`/usr/lib/git-core/git-push`).
    pub dashed: bool,
    /// An option before the verb git's reader here doesn't know.
    pub odd: bool,
}

/// git's own options before the verb that take a value.
const GIT_VALUED: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--attr-source",
    "--config-env",
];

/// git's own options before the verb that take none.
const GIT_FLAGS: &[&str] = &[
    "-p",
    "--paginate",
    "-P",
    "--no-pager",
    "--bare",
    "--literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--icase-pathspecs",
    "--no-replace-objects",
    "--no-lazy-fetch",
    "--no-optional-locks",
    "--no-advice",
    "--exec-path",
    "--html-path",
    "--man-path",
    "--info-path",
];

/// The git command `run` is, if its program is git or a `git-<name>`.
pub fn git_call(run: &Run) -> Option<GitCall<'_>> {
    let head = run.words.first()?;
    let leaf = leaf_of(head);
    let name = program_name(leaf);
    if name != "git" {
        if !is_git_name(leaf) {
            return None;
        }
        // The name as written, after `git-` and before any `.exe`.
        let verb = &leaf[4..4 + name.len() - 4];
        return Some(GitCall {
            options: Vec::new(),
            verb: Some(verb),
            args: &run.words[1..],
            dashed: true,
            odd: false,
        });
    }
    let rest = &run.words[1..];
    let mut options = Vec::new();
    let mut odd = false;
    let mut k = 0;
    while let Some(w) = rest.get(k) {
        if !w.starts_with('-') {
            return Some(GitCall {
                options,
                verb: Some(w),
                args: &rest[k + 1..],
                dashed: false,
                odd,
            });
        }
        let (name, value) = match w.split_once('=').filter(|_| w.starts_with("--")) {
            Some((name, value)) => (name, Some(value)),
            None => (w.as_str(), None),
        };
        match name {
            // `git --version …` runs `git version …`, `--help` `git help`.
            "--version" | "-v" | "--help" | "-h" => {
                let verb = if name.contains('h') {
                    "help"
                } else {
                    "version"
                };
                return Some(GitCall {
                    options,
                    verb: Some(verb),
                    args: &rest[k + 1..],
                    dashed: false,
                    odd,
                });
            }
            _ if GIT_VALUED.contains(&name) => match value {
                Some(v) => options.push((name, Some(v))),
                None => {
                    let v = rest.get(k + 1).map(String::as_str);
                    odd |= v.is_none();
                    options.push((name, v));
                    k += 1;
                }
            },
            _ if GIT_FLAGS.contains(&name) || name.starts_with("--list-cmds") => {
                options.push((name, value));
            }
            _ => {
                odd = true;
                options.push((name, value));
            }
        }
        k += 1;
    }
    Some(GitCall {
        options,
        verb: None,
        args: &[],
        dashed: false,
        odd,
    })
}

/// git's own commands that can't push: compiled into every git since 2.0,
/// or shipped with it as a program in its exec path (`stash`, `rebase`).
/// Those that run a command they are given only with some options are
/// read in [`may_push`].
const CANT_PUSH: &[&str] = &[
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-tree",
    "config",
    "count-objects",
    "describe",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "fast-export",
    "fast-import",
    "fetch",
    "fetch-pack",
    "fmt-merge-msg",
    "for-each-ref",
    "format-patch",
    "fsck",
    "gc",
    "get-tar-commit-id",
    "grep",
    "hash-object",
    "help",
    "index-pack",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "merge",
    "merge-base",
    "merge-file",
    "merge-tree",
    "mktag",
    "mktree",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-refs",
    "patch-id",
    "prune",
    "prune-packed",
    "pull",
    "read-tree",
    "rebase",
    "reflog",
    "remote",
    "repack",
    "replace",
    "rerere",
    "reset",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "shortlog",
    "show",
    "show-branch",
    "show-index",
    "show-ref",
    "stage",
    "stash",
    "status",
    "stripspace",
    "submodule",
    "symbolic-ref",
    "tag",
    "unpack-file",
    "unpack-objects",
    "update-index",
    "update-ref",
    "update-server-info",
    "var",
    "verify-pack",
    "verify-tag",
    "version",
    "whatchanged",
    "write-tree",
];

/// git's own commands that can't push, added after 2.0: read as git's own
/// only when the installed git has them compiled in.
const CANT_PUSH_NEWER: &[&str] = &[
    "switch",
    "restore",
    "worktree",
    "sparse-checkout",
    "range-diff",
    "commit-graph",
    "multi-pack-index",
    "interpret-trailers",
    "verify-commit",
    "diagnose",
    "bugreport",
    "replay",
    "refs",
];

/// Commands that reach a remote, and their options that name a program
/// git runs there or set config in the copy it makes.
const TRANSPORT: &[(&str, &[&str])] = &[
    ("fetch", &["--upload-pack"]),
    ("pull", &["--upload-pack"]),
    ("clone", &["-u", "--upload-pack", "-c", "--config"]),
    ("ls-remote", &["--upload-pack", "--exec"]),
    ("fetch-pack", &["--upload-pack", "--exec"]),
    ("archive", &["--exec"]),
    ("remote", &[]),
    ("submodule", &[]),
];

/// Config keys whose values never name a program, set with `-c` or `git
/// config`.
const PLAIN_CONFIG: &[&str] = &[
    "user.name",
    "user.email",
    "color.ui",
    "color.diff",
    "color.status",
    "color.branch",
    "color.interactive",
    "core.quotepath",
    "core.autocrlf",
    "core.safecrlf",
    "core.filemode",
    "core.ignorecase",
    "init.defaultbranch",
    "advice.detachedhead",
    "pull.rebase",
    "pull.ff",
    "merge.ff",
    "commit.verbose",
    "log.decorate",
    "diff.renames",
    "status.short",
    "column.ui",
    "i18n.commitencoding",
    "i18n.logoutputencoding",
];

/// Variables a command may set without changing what git runs.
const PLAIN_VARS: &[&str] = &[
    "LANG",
    "LANGUAGE",
    "TZ",
    "TERM",
    "COLUMNS",
    "LINES",
    "NO_COLOR",
    "FORCE_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    "GIT_TERMINAL_PROMPT",
    "GIT_OPTIONAL_LOCKS",
    "GIT_FLUSH",
    "GIT_MERGE_VERBOSITY",
    "GIT_REFLOG_ACTION",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "CARGO_TERM_COLOR",
    "CI",
];

/// A `NAME=value` that can't change what git runs or reads: a plain
/// variable ([`PLAIN_VARS`], `LC_*`, `GIT_TRACE*`), on Linux and macOS a
/// lower-case one (on Windows names don't tell case apart, so `path=` is
/// `PATH=`), and any other that git, a program it runs or the loader
/// doesn't read for what to run or where its config is ([`moves_git`]).
pub fn plain_assignment(word: &str) -> bool {
    let Some(name) = assignment_name(word) else {
        return false;
    };
    let upper = name.to_ascii_uppercase();
    PLAIN_VARS.contains(&upper.as_str())
        || upper.starts_with("LC_")
        || upper.starts_with("GIT_TRACE")
        || (!cfg!(windows)
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
        || !moves_git(&upper)
}

/// Variables (upper case) that point git, a program it runs or the loader
/// at another program or config: `PATH` and the home and config folders
/// (aliases, editors), git's own `GIT_*`, editors, pagers, ssh and askpass
/// programs, the shells' start files, the loader's and the script
/// runtimes' (git's Perl and Python commands).
fn moves_git(upper: &str) -> bool {
    const NAMES: &[&str] = &[
        "PATH",
        "PATHEXT",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "COMSPEC",
        "ENV",
        "BASH_ENV",
        "ZDOTDIR",
        "PS4",
        "IFS",
        "MORE",
        "LV",
        "PREFIX",
    ];
    const STARTS: &[&str] = &[
        "GIT_", "XDG_", "LD_", "DYLD_", "LESS", "PERL", "PYTHON", "RUBY", "GNUPG", "GPG",
    ];
    const HOLDS: &[&str] = &[
        "EDITOR", "VISUAL", "PAGER", "ASKPASS", "SSH", "SHELL", "PROXY", "BROWSER",
    ];
    NAMES.contains(&upper)
        || STARTS.iter().any(|p| upper.starts_with(p))
        || HOLDS.iter().any(|p| upper.contains(p))
}

/// The run, or a variable set before it, points git at other programs:
/// `PATH`, `GIT_EXEC_PATH`, or `--exec-path=<dir>`.
pub fn git_path_changed(run: &Run, sets: &[String]) -> bool {
    let moved = |a: &String| {
        assignment_name(a).is_some_and(|name| {
            name.eq_ignore_ascii_case("PATH") || name.eq_ignore_ascii_case("GIT_EXEC_PATH")
        })
    };
    run.env.iter().chain(sets).any(moved)
        || git_call(run).is_some_and(|call| {
            call.options
                .iter()
                .any(|(name, value)| *name == "--exec-path" && value.is_some())
        })
}

/// Whether `command` may push, by the closed rule (module doc): some part
/// of it could run git that [`read_shell`] doesn't read, or a git in it may
/// push ([`git_may_push`]).
pub fn command_may_push(command: &str) -> bool {
    let read = read_shell(command);
    read.hidden.is_some() || read.runs.iter().any(|run| git_may_push(run, &read.sets))
}

/// Whether the git command `run` may push, by the closed rule (module
/// doc). `sets` are the variables the command sets before it
/// ([`ShellRead::sets`]). `false` when `run` isn't git.
pub fn git_may_push(run: &Run, sets: &[String]) -> bool {
    may_push(run, sets, &git_compiled_in)
}

fn may_push(run: &Run, sets: &[String], compiled_in: &dyn Fn(&str) -> bool) -> bool {
    let Some(call) = git_call(run) else {
        return false;
    };
    if call.odd || !run.env.iter().chain(sets).all(|a| plain_assignment(a)) {
        return true;
    }
    for (name, value) in &call.options {
        let plain = match *name {
            "-c" => value.is_some_and(|v| {
                !v.contains(HIDDEN) && plain_config(v.split_once('=').map_or(v, |(key, _)| key))
            }),
            "--config-env" => false,
            "--exec-path" => value.is_none(),
            _ => true,
        };
        if !plain {
            return true;
        }
    }
    let Some(verb) = call.verb else {
        // `xargs git`: the verb comes when it runs.
        return run.more_args;
    };
    let known = CANT_PUSH.contains(&verb) || (CANT_PUSH_NEWER.contains(&verb) && compiled_in(verb));
    if !known {
        return true;
    }
    let args = call.args;
    if args.iter().any(|a| {
        let lower = a.to_ascii_lowercase();
        lower.starts_with("ext::") || lower.starts_with("fd::")
    }) {
        return true;
    }
    // A word the reader can't see, or more that come on stdin (`xargs`).
    let hidden = run.more_args || args.iter().any(|a| a.contains(HIDDEN));
    match verb {
        "config" => hidden || config_writes(args),
        "rebase" => hidden || has_option(args, &["-x", "--exec"]),
        "bisect" => hidden || first_word(args) == Some("run"),
        "grep" => hidden || has_option(args, &["-O", "--open-files-in-pager"]),
        _ => match TRANSPORT.iter().find(|(name, _)| *name == verb) {
            Some((_, opts)) => {
                hidden
                    || args.iter().any(|a| a.contains("::"))
                    || has_option(args, opts)
                    || (verb == "submodule" && first_word(args) == Some("foreach"))
            }
            None => false,
        },
    }
}

fn plain_config(key: &str) -> bool {
    PLAIN_CONFIG.contains(&key.to_ascii_lowercase().as_str())
}

/// The first word that isn't an option.
fn first_word(args: &[String]) -> Option<&str> {
    args.iter()
        .map(String::as_str)
        .find(|a| !a.starts_with('-'))
}

/// One of `opts` is given, as git's option reader takes it: `--name`,
/// `--name=…`, a long name's start (`--upload`), or a short one in a
/// cluster (`-nO`).
fn has_option(args: &[String], opts: &[&str]) -> bool {
    for a in args {
        if a == "--" {
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let name = long.split_once('=').map_or(long, |(name, _)| name);
            if !name.is_empty()
                && opts
                    .iter()
                    .filter_map(|o| o.strip_prefix("--"))
                    .any(|o| o.starts_with(name))
            {
                return true;
            }
        } else if let Some(cluster) = a.strip_prefix('-') {
            if opts
                .iter()
                .filter(|o| o.len() == 2 && !o.starts_with("--"))
                .any(|o| cluster.contains(&o[1..]))
            {
                return true;
            }
        }
    }
    false
}

/// `git config` with these words changes config other than a
/// [`PLAIN_CONFIG`] key, or opens an editor.
fn config_writes(args: &[String]) -> bool {
    const VALUED: &[&str] = &[
        "-f",
        "--file",
        "--blob",
        "--type",
        "--default",
        "--comment",
        "--value",
    ];
    const READS: &[&str] = &[
        "--get",
        "--get-all",
        "--get-regexp",
        "--get-urlmatch",
        "-l",
        "--list",
        "--get-color",
        "--get-colorbool",
    ];
    const KEYED: &[&str] = &["--add", "--replace-all", "--unset", "--unset-all"];
    const SCOPES: &[&str] = &[
        "--global",
        "--system",
        "--local",
        "--worktree",
        "--show-origin",
        "--show-scope",
        "-z",
        "--null",
        "--name-only",
        "--bool",
        "--int",
        "--bool-or-int",
        "--path",
        "--expiry-date",
        "--includes",
        "--no-includes",
        "--fixed-value",
        "--no-type",
    ];
    let mut words: Vec<&str> = Vec::new();
    let mut flags: Vec<&str> = Vec::new();
    let mut k = 0;
    while let Some(a) = args.get(k) {
        k += 1;
        if a == "--" {
            words.extend(args[k..].iter().map(String::as_str));
            break;
        }
        if a.len() > 1 && a.starts_with('-') {
            let name = a.split_once('=').map_or(a.as_str(), |(name, _)| name);
            flags.push(name);
            if VALUED.contains(&name) && !a.contains('=') {
                k += 1;
            }
        } else {
            words.push(a);
        }
    }
    let key_plain = |k: usize| words.get(k).is_some_and(|key| plain_config(key));
    match words.first().copied() {
        Some("get" | "list") => return false,
        Some("set" | "unset") => return !key_plain(1),
        Some("edit" | "rename-section" | "remove-section") => return true,
        _ => {}
    }
    if flags.iter().any(|f| {
        !(VALUED.contains(f) || READS.contains(f) || KEYED.contains(f) || SCOPES.contains(f))
    }) {
        return true;
    }
    if flags.iter().any(|f| READS.contains(f)) {
        return false;
    }
    if words.len() <= 1 && !flags.iter().any(|f| KEYED.contains(f)) {
        return false;
    }
    !key_plain(0)
}

/// Commands compiled into every git since 2.0 (2014). They are git's own on
/// any git a person runs, so these are read without asking git.
const BUILTINS_SINCE_2_0: &[&str] = &[
    "add",
    "annotate",
    "apply",
    "archive",
    "blame",
    "branch",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-tree",
    "config",
    "count-objects",
    "credential",
    "describe",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "fast-export",
    "fetch",
    "fetch-pack",
    "fmt-merge-msg",
    "for-each-ref",
    "format-patch",
    "fsck",
    "gc",
    "grep",
    "hash-object",
    "help",
    "index-pack",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "merge",
    "merge-base",
    "merge-file",
    "merge-tree",
    "mktag",
    "mktree",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-refs",
    "patch-id",
    "prune",
    "push",
    "read-tree",
    "reflog",
    "remote",
    "repack",
    "replace",
    "rerere",
    "reset",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-pack",
    "shortlog",
    "show",
    "show-branch",
    "show-ref",
    "status",
    "stripspace",
    "symbolic-ref",
    "tag",
    "unpack-objects",
    "update-index",
    "update-ref",
    "var",
    "verify-pack",
    "verify-tag",
    "write-tree",
];

/// What the installed git lists for `git --list-cmds=<kinds>`, or `None`
/// when git can't be run or lists nothing. It runs outside any repository,
/// so no repository's config is read.
fn list_cmds(kinds: &str) -> Option<Vec<String>> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg(format!("--list-cmds={kinds}"))
        .current_dir(std::env::temp_dir());
    let out = grokhub_core::proc_util::output_within(&mut cmd, Duration::from_secs(3), 1 << 20)?;
    let list: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    (!list.is_empty()).then_some(list)
}

/// The commands compiled into the installed git. They don't change while
/// the app runs, so they are asked for once; a failed ask is asked again.
fn installed_builtins() -> Option<&'static [String]> {
    static BUILTINS: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(list) = BUILTINS.get() {
        return Some(list);
    }
    let list = list_cmds("builtins")?;
    Some(BUILTINS.get_or_init(|| list))
}

/// Whether `verb` is a command compiled into git: one of every git's since
/// 2.0, or the installed git's. Unlike a `git-<name>` program, which git
/// looks up on the command's `PATH`, no change to the command's
/// environment can turn one into an alias.
pub fn git_compiled_in(verb: &str) -> bool {
    BUILTINS_SINCE_2_0.contains(&verb)
        || installed_builtins().is_some_and(|list| list.iter().any(|c| c == verb))
}

/// Whether `git <verb>` runs git's own command on this machine: a command
/// compiled into the installed git, or a `git-<verb>` program in its exec
/// path or on the app's PATH. git runs either before it looks for an
/// alias, so no alias can stand in for one. Any other word may be an alias
/// that pushes. A command that changes `PATH` or `GIT_EXEC_PATH` can find
/// other programs, so only [`git_compiled_in`] holds for it.
///
/// Programs are looked up on every call, not kept: one removed since could
/// have left an alias of its name in its place.
pub fn git_runs_own(verb: &str) -> bool {
    if git_compiled_in(verb) {
        return true;
    }
    // Not a name git could look up as a command.
    if verb.is_empty()
        || verb.starts_with('-')
        || verb.contains(|c: char| {
            c.is_whitespace() || c == '/' || c == '\\' || c == '=' || c == HIDDEN
        })
    {
        return false;
    }
    list_cmds("main,others").is_some_and(|list| list.iter().any(|c| c == verb))
}

#[cfg(test)]
#[path = "git_words_tests.rs"]
mod tests;
