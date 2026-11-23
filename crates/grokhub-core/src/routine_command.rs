//! Where each `{{ }}` sits in a step's command line, read the way the shell
//! reads it.
//!
//! The runner quotes every value as one word ([`Quote::apply`]). A quoted
//! word stays data, except where the shell or a program reads a word as code
//! (`sh -c`, `eval`, `[[ ]]`, a pipe into `sh`) or as an option
//! (`--upload-pack=...`). [`problem`] refuses the first kind when a routine
//! is saved or loaded. [`option_checks`] tells `fill` where to refuse a value
//! of the second kind.
//!
//! These are known places, not a proof: a program that reads an ordinary
//! argument as code in its own way can still be handed one.

use super::{Place, Quote};

const QUOTED: &str = "a value in a command is quoted for you; take the quotes off around it";
const ITSELF: &str = "a value can't be the command itself; name the program in the step";
const HEREDOC: &str = "a value can't go in a heredoc; pass it as an argument";
const COMMENT: &str = "a value can't go in a comment";
const MATH: &str =
    "a value can't go where the shell does arithmetic ([[ ]], (( )), let); compare it with [ ] instead";
const EXPANSION: &str = "a value can't go inside ${...}";
const VERBATIM: &str = "a value can't come after --%, where PowerShell stops quoting";
const UNCLEAR: &str = "a value can't go in a command with a part nested in quotes or substitutions that this check can't follow; make that part plain (\"$(date +%F)\", not \"$(date +\"%F\")\"), or move it into a script step";
const NAMES: &str = "a value can't be a variable's name, where bash reads an array index as code; name the variable in the step";

/// Why a value in `command` would run as code. `None` when every value
/// stays data, and always for [`Quote::Plain`].
pub(super) fn problem(command: &str, quote: Quote) -> Option<String> {
    read_line(command, quote).err()
}

/// For each placeholder in template order, where it sits ([`Place`]): a
/// placeholder glued to text before it, after `--`, or naming a file
/// (`> {{f}}`) can't be read as an option, and one a PowerShell cmdlet gets
/// never goes through a Windows command line. Empty when the line has a
/// [`problem`]; a missing entry means check everything.
pub(super) fn places(command: &str, quote: Quote) -> Vec<Place> {
    read_line(command, quote).unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Posix,
    PowerShell,
}

/// One word of the line, as the program gets it.
#[derive(Debug, Default)]
struct Word {
    /// The word with quotes and escapes taken off.
    text: String,
    /// Its placeholders, by template order.
    slots: Vec<usize>,
    /// Some of it was quoted or escaped: `"]]"` is no keyword.
    quoted: bool,
    /// A placeholder comes before any `=` in it: in `declare {{x}}=1` the
    /// value is part of a variable's name.
    bare: bool,
}

impl Word {
    fn has_slot(&self) -> bool {
        !self.slots.is_empty()
    }

    /// The program name: lowercase, no folder, no `.exe`.
    fn name(&self) -> String {
        let lower = self.text.to_ascii_lowercase();
        let file = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
        file.strip_suffix(".exe").unwrap_or(file).to_string()
    }
}

#[derive(Debug)]
enum Tok {
    Word(Word),
    /// `;` `&&` `||` `&` a newline `(` `)` `$(`: a new command starts.
    Sep,
    /// `|`: the next command reads this one's output.
    Pipe,
    /// `>` `<` `2>` `<<EOF`: the next word names a file (or a heredoc's end).
    Redirect,
    /// `<<<`: the next word is the command's input.
    HereString,
}

fn read_line(command: &str, quote: Quote) -> Result<Vec<Place>, String> {
    let shell = match quote {
        Quote::Plain => return Ok(Vec::new()),
        Quote::Posix => Shell::Posix,
        Quote::PowerShell => Shell::PowerShell,
    };
    let (toks, checked, in_substitution) = Reader::new(command, shell).read()?;
    // `bash <(echo {{x}})`, `< <(echo {{x}}) bash`, `echo {{x}} > >(sh)`,
    // `exec 3< <(...)`: where a substitution's output goes, or what feeds
    // one, can't be told, so a line with one holds no value where any
    // program could run a script.
    if in_substitution
        && command.contains("{{")
        && let Some(name) = toks.iter().find_map(|t| match t {
            Tok::Word(w) if w.text.starts_with('$') => Some(w.text.clone()),
            Tok::Word(w) if runs_files(&w.name()) => Some(w.name()),
            _ => None,
        })
    {
        return Err(format!(
            "a value can't go in a <(...) in a line where {name} runs a script; pass it as an argument"
        ));
    }
    let mut places: Vec<Place> = checked
        .into_iter()
        .map(|option| Place {
            option,
            native: true,
            batch: true,
        })
        .collect();
    // A file name or an input: the shell itself opens it.
    for pair in toks.windows(2) {
        if let [Tok::Redirect | Tok::HereString, Tok::Word(w)] = pair {
            for &k in &w.slots {
                places[k] = Place {
                    option: false,
                    native: false,
                    batch: false,
                };
            }
        }
    }
    if shell == Shell::Posix {
        test_brackets(&toks)?;
    }
    for c in commands(&toks) {
        for k in command_problem(&c.words, shell)? {
            places[k].option = false;
        }
        // A cmdlet gets its values as strings, not through a command line;
        // Start-Process builds one. A program named with `.exe` isn't a
        // batch file that cmd.exe reads again.
        let program = program(&c.words, shell)
            .ok()
            .flatten()
            .map(|(p, _)| c.words[p]);
        let cmdlet = program.is_none_or(|w| {
            let text = &w.text;
            let name = w.name();
            shell == Shell::PowerShell
                && !text.contains(['/', '\\', '.'])
                && is_cmdlet(&name)
                && !matches!(name.as_str(), "start-process" | "saps")
        });
        let exe = program.is_some_and(|w| w.text.to_ascii_lowercase().ends_with(".exe"));
        for w in &c.words {
            for &k in &w.slots {
                places[k].native &= !cmdlet;
                places[k].batch &= !cmdlet && !exe;
            }
        }
        let runs = || runs_input(&c.words, shell);
        if c.input.is_some_and(Word::has_slot)
            && let Some(name) = runs()
        {
            return Err(format!(
                "a value can't be the input of {name}, which runs what it reads"
            ));
        }
        // xargs only makes its input words arguments; an interpreter runs
        // whatever reaches it, even through a file.
        // A `{ ...; }` or `( ... )` piped in reads as a pipeline of its own.
        let group = c.start >= 2
            && match &toks[c.start - 2] {
                Tok::Sep => true,
                Tok::Word(w) => {
                    matches!(w.text.as_str(), "}" | "fi" | "done" | "esac") && !w.quoted
                }
                _ => false,
            };
        let reaches = |name: &str| match name {
            "xargs" if !group => slot_in_pipeline(&toks, c.start),
            _ => slot_before(&toks, c.start),
        };
        if c.piped
            && let Some(name) = runs()
            && reaches(&name)
        {
            return Err(format!(
                "a value can't be piped into {name}, which runs what it reads"
            ));
        }
    }
    Ok(places)
}

/// A PowerShell cmdlet or function name, `Verb-Noun` with a standard verb
/// (`Copy-Item`, `Write-Output`). Anything else may be a program.
fn is_cmdlet(name: &str) -> bool {
    const VERBS: &[&str] = &[
        "add",
        "approve",
        "assert",
        "backup",
        "block",
        "build",
        "checkpoint",
        "clear",
        "close",
        "compare",
        "complete",
        "compress",
        "confirm",
        "connect",
        "convert",
        "convertfrom",
        "convertto",
        "copy",
        "debug",
        "deny",
        "deploy",
        "disable",
        "disconnect",
        "dismount",
        "edit",
        "enable",
        "enter",
        "exit",
        "expand",
        "export",
        "find",
        "foreach",
        "format",
        "get",
        "grant",
        "group",
        "hide",
        "import",
        "initialize",
        "install",
        "invoke",
        "join",
        "limit",
        "lock",
        "measure",
        "merge",
        "mount",
        "move",
        "new",
        "open",
        "optimize",
        "out",
        "ping",
        "pop",
        "protect",
        "publish",
        "push",
        "read",
        "receive",
        "redo",
        "register",
        "remove",
        "rename",
        "repair",
        "request",
        "reset",
        "resize",
        "resolve",
        "restart",
        "restore",
        "resume",
        "revoke",
        "save",
        "search",
        "select",
        "send",
        "set",
        "show",
        "skip",
        "sort",
        "split",
        "start",
        "step",
        "stop",
        "submit",
        "suspend",
        "switch",
        "sync",
        "tee",
        "test",
        "trace",
        "unblock",
        "undo",
        "uninstall",
        "unlock",
        "unprotect",
        "unpublish",
        "unregister",
        "update",
        "use",
        "wait",
        "watch",
        "where",
        "write",
    ];
    name.split_once('-').is_some_and(|(verb, noun)| {
        VERBS.contains(&verb) && noun.starts_with(|c: char| c.is_ascii_alphabetic())
    })
}

/// Splits a line into words and operators, refusing a placeholder the
/// runner's quoting can't keep as one word of data.
struct Reader<'a> {
    src: &'a str,
    shell: Shell,
    at: usize,
    toks: Vec<Tok>,
    /// Per placeholder: does it start its word.
    checked: Vec<bool>,
    word: Option<Word>,
    /// `<<` or `<<-` seen: the next word names a heredoc's end.
    heredoc_next: Option<bool>,
    /// Heredocs whose bodies start after the next newline: the end word,
    /// `<<-`, and whether the end word was quoted.
    heredocs: Vec<(String, bool, bool)>,
    /// A `<(...)` or `>(...)` in the line.
    in_substitution: bool,
    /// A heredoc whose end word holds an expansion, which bash takes as
    /// written: where its body ends can't be told.
    unclear_heredoc: bool,
}

const SINGLE: &[char] = &['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];
const DOUBLE: &[char] = &['"', '\u{201c}', '\u{201d}', '\u{201e}'];

impl<'a> Reader<'a> {
    fn new(src: &'a str, shell: Shell) -> Self {
        Reader {
            src,
            shell,
            at: 0,
            toks: Vec::new(),
            checked: Vec::new(),
            word: None,
            heredoc_next: None,
            heredocs: Vec::new(),
            in_substitution: false,
            unclear_heredoc: false,
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.at..]
    }

    fn word(&mut self) -> &mut Word {
        self.word.get_or_insert_with(Word::default)
    }

    fn push(&mut self, c: char) {
        self.word().text.push(c);
    }

    fn end_word(&mut self) {
        if let Some(w) = self.word.take() {
            if let Some(strip) = self.heredoc_next.take() {
                self.unclear_heredoc |= w.text.contains('$') || w.has_slot();
                self.heredocs.push((w.text.clone(), strip, w.quoted));
            }
            self.toks.push(Tok::Word(w));
        }
    }

    fn op(&mut self, tok: Tok, len: usize) {
        self.end_word();
        self.toks.push(tok);
        self.at += len;
    }

    fn read(mut self) -> Result<(Vec<Tok>, Vec<bool>, bool), String> {
        while let Some(c) = self.rest().chars().next() {
            if self.rest().starts_with("{{") {
                self.slot()?;
                continue;
            }
            match self.shell {
                Shell::Posix => self.posix(c)?,
                Shell::PowerShell => self.powershell(c)?,
            }
        }
        self.end_word();
        if self.unclear_heredoc {
            self.unclear()?;
        }
        Ok((self.toks, self.checked, self.in_substitution))
    }

    fn slot(&mut self) -> Result<(), String> {
        let end = self.rest()[2..]
            .find("}}")
            .map_or(self.src.len(), |c| self.at + 2 + c + 2);
        let posix = self.shell == Shell::Posix;
        let word = self.word();
        // `arr[{{i}}]=1`: bash reads an array index as arithmetic.
        if is_index(&word.text) {
            return Err(MATH.into());
        }
        // `--{{x}}`: the value would pick the option.
        if !word.text.is_empty() && word.text.chars().all(|c| c == '-') {
            return Err("a value can't name an option; write the option in the step".into());
        }
        // `{main,{{x}}}`: brace expansion makes the value a word of its own.
        let braced = posix
            && word.text.contains('{')
            && (word.text.ends_with('{') || word.text.ends_with(','));
        let starts = word.text.is_empty() || braced;
        if !word.text.contains('=') {
            word.bare = true;
        }
        let k = self.checked.len();
        self.word().slots.push(k);
        self.checked.push(starts);
        self.at = end;
        Ok(())
    }

    fn posix(&mut self, c: char) -> Result<(), String> {
        let rest = self.rest();
        match c {
            '\\' => self.escape('\\')?,
            '\'' => self.quoted(1, &['\''], None, false)?,
            '"' => self.quoted(1, &['"'], Some('\\'), true)?,
            '`' => self.quoted(1, &['`'], Some('\\'), false)?,
            '$' if rest.starts_with("${{") => return Err(after('$')),
            '$' if rest.starts_with("$'") => self.quoted(2, &['\''], Some('\\'), false)?,
            '$' if rest.starts_with("$\"") => self.quoted(2, &['"'], Some('\\'), true)?,
            '$' if rest.starts_with("$((") => {
                self.push('$');
                self.balanced(3, '(', ')', 2, MATH)?;
            }
            '$' if rest.starts_with("$(") => self.op(Tok::Sep, 2),
            '$' if rest.starts_with("${") => {
                self.push('$');
                self.balanced(2, '{', '}', 1, EXPANSION)?;
            }
            '$' if rest.starts_with("$[") => {
                self.push('$');
                self.balanced(2, '[', ']', 1, MATH)?;
            }
            '#' if self.word.is_none() => self.comment()?,
            '\n' => {
                self.op(Tok::Sep, 1);
                self.heredoc_bodies()?;
            }
            ';' | ')' => self.op(Tok::Sep, 1),
            '(' if rest.starts_with("((") => {
                self.end_word();
                self.balanced(2, '(', ')', 2, MATH)?;
            }
            '(' => self.op(Tok::Sep, 1),
            '&' if rest.starts_with("&>>") => self.op(Tok::Redirect, 3),
            '&' if rest.starts_with("&>") => self.op(Tok::Redirect, 2),
            '&' if rest.starts_with("&&") => self.op(Tok::Sep, 2),
            '&' => self.op(Tok::Sep, 1),
            '|' if rest.starts_with("||") => self.op(Tok::Sep, 2),
            '|' if rest.starts_with("|&") => self.op(Tok::Pipe, 2),
            '|' => self.op(Tok::Pipe, 1),
            '<' | '>' => {
                self.drop_fd();
                if rest.starts_with("<<<") {
                    self.op(Tok::HereString, 3);
                } else if rest.starts_with("<<-") || rest.starts_with("<<") {
                    let strip = rest.starts_with("<<-");
                    self.op(Tok::Redirect, if strip { 3 } else { 2 });
                    self.heredoc_next = Some(strip);
                } else if rest.starts_with("<(") || rest.starts_with(">(") {
                    // `bash <(echo {{x}})`: a program may run the
                    // substitution's output as its script (see `read_line`).
                    self.in_substitution = true;
                    self.op(Tok::Sep, 2);
                } else if [">>", ">&", ">|", "<&", "<>"]
                    .iter()
                    .any(|o| rest.starts_with(o))
                {
                    self.op(Tok::Redirect, 2);
                } else {
                    self.op(Tok::Redirect, 1);
                }
            }
            c if c.is_whitespace() => {
                self.end_word();
                self.at += c.len_utf8();
            }
            c => {
                self.push(c);
                self.at += c.len_utf8();
            }
        }
        Ok(())
    }

    fn powershell(&mut self, c: char) -> Result<(), String> {
        let rest = self.rest();
        match c {
            '`' => self.escape('`')?,
            c if SINGLE.contains(&c) => self.quoted(c.len_utf8(), SINGLE, None, false)?,
            c if DOUBLE.contains(&c) => self.quoted(c.len_utf8(), DOUBLE, Some('`'), true)?,
            '@' if here_string(rest) => {
                let close = if rest.starts_with("@'") {
                    "\n'@"
                } else {
                    "\n\"@"
                };
                let end = rest.find(close).map_or(rest.len(), |e| e + close.len());
                if rest[..end].contains("{{") {
                    return Err(QUOTED.into());
                }
                self.at += end;
            }
            '@' if rest.starts_with("@(") || rest.starts_with("@{") => self.op(Tok::Sep, 2),
            '$' if rest.starts_with("${{") => return Err(after('$')),
            '$' if rest.starts_with("$(") => self.op(Tok::Sep, 2),
            '$' if rest.starts_with("${") => {
                self.push('$');
                self.balanced(2, '{', '}', 1, EXPANSION)?;
            }
            '<' if rest.starts_with("<#") => {
                let end = rest.find("#>").map_or(rest.len(), |e| e + 2);
                if rest[..end].contains("{{") {
                    return Err(COMMENT.into());
                }
                self.at += end;
            }
            '#' if self.word.is_none() => self.comment()?,
            '-' if self.word.is_none()
                && rest.starts_with("--%")
                && rest[3..].chars().next().is_none_or(char::is_whitespace) =>
            {
                if rest.contains("{{") {
                    return Err(VERBATIM.into());
                }
                self.at = self.src.len();
            }
            '\n' | ';' | '(' | ')' | '{' | '}' => self.op(Tok::Sep, 1),
            '&' if rest.starts_with("&&") => self.op(Tok::Sep, 2),
            // The call operator: `& prog` runs prog.
            '&' => {
                self.end_word();
                self.toks.push(Tok::Word(Word {
                    text: "&".into(),
                    ..Word::default()
                }));
                self.at += 1;
            }
            '|' if rest.starts_with("||") => self.op(Tok::Sep, 2),
            '|' => self.op(Tok::Pipe, 1),
            '<' | '>' => {
                self.drop_fd();
                let len = if rest.starts_with(">>") || rest.starts_with(">&") {
                    2
                } else {
                    1
                };
                self.op(Tok::Redirect, len);
            }
            c if c.is_whitespace() => {
                self.end_word();
                self.at += c.len_utf8();
            }
            c => {
                self.push(c);
                self.at += c.len_utf8();
            }
        }
        Ok(())
    }

    /// `\x` (bash) or `` `x `` (PowerShell) outside quotes: `x` as itself.
    fn escape(&mut self, mark: char) -> Result<(), String> {
        let next = &self.rest()[1..];
        if next.starts_with("{{") {
            return Err(after(mark));
        }
        match next.chars().next() {
            Some('\n') => self.at += 2,
            Some(e) => {
                self.push(e);
                self.word().quoted = true;
                self.at += 1 + e.len_utf8();
            }
            None => self.at += 1,
        }
        Ok(())
    }

    /// A quoted part of a word, from after its `open` bytes to a `close`
    /// character; `escape` skips the character after it, and with `nested`
    /// a `$(...)` inside keeps its own quotes.
    fn quoted(
        &mut self,
        open: usize,
        close: &[char],
        escape: Option<char>,
        nested: bool,
    ) -> Result<(), String> {
        let start = self.at + open;
        let mut j = start;
        while let Some(c) = self.src[j..].chars().next() {
            if self.src[j..].starts_with("{{") {
                return Err(QUOTED.into());
            }
            if Some(c) == escape {
                if self.src[j + 1..].starts_with("{{") {
                    return Err(QUOTED.into());
                }
                j += 1 + self.src[j + 1..].chars().next().map_or(0, char::len_utf8);
                continue;
            }
            if close.contains(&c) {
                let text = self.src[start..j].to_string();
                let word = self.word();
                word.text.push_str(&text);
                word.quoted = true;
                self.at = j + c.len_utf8();
                return Ok(());
            }
            // `"$(date +"%F")"`, `"`git -C "x" log`"`, `"${A:-"x"}"`: the
            // inner quotes belong to the inner part, where a value is refused.
            if nested && self.src[j..].starts_with("$(") {
                j = self.skip_parens(j + 1)?;
                continue;
            }
            if nested && self.shell == Shell::Posix && c == '`' {
                j = self.skip_backticks(j)?;
                continue;
            }
            if nested && self.src[j..].starts_with("${") {
                j = self.skip_balanced(j + 2, '{', '}', 1, QUOTED, false)?;
                continue;
            }
            j += c.len_utf8();
        }
        let text = self.src[start..].to_string();
        let word = self.word();
        word.text.push_str(&text);
        word.quoted = true;
        self.at = self.src.len();
        Ok(())
    }

    /// From an opening backtick at `j`, past its match; a placeholder
    /// inside is refused. Like [`Self::skip_balanced`], only plain contents
    /// are read: with quotes, an escape or a substitution inside, the rest
    /// of the command counts as inside.
    fn skip_backticks(&self, j: usize) -> Result<usize, String> {
        let mut j = j + 1;
        while let Some(c) = self.src[j..].chars().next() {
            let rest = &self.src[j..];
            if rest.starts_with("{{") {
                return Err(QUOTED.into());
            }
            if matches!(c, '\\' | '\'' | '"')
                || (c == '$' && rest[1..].starts_with(['(', '{', '[']))
            {
                return self.unclear();
            }
            if c == '`' {
                return Ok(j + 1);
            }
            j += c.len_utf8();
        }
        Ok(self.src.len())
    }

    /// A part whose end this reader can't be sure of. What follows it
    /// could be inside it or not, and could pipe an earlier value into a
    /// program, so a command with a value anywhere is refused; one without
    /// is read no further.
    fn unclear(&self) -> Result<usize, String> {
        if self.src.contains("{{") {
            Err(UNCLEAR.into())
        } else {
            Ok(self.src.len())
        }
    }

    /// From an opening `(` at `j`, past its match; a placeholder anywhere
    /// inside is in quotes.
    fn skip_parens(&self, j: usize) -> Result<usize, String> {
        self.skip_balanced(j + 1, '(', ')', 1, QUOTED, true)
    }

    /// From `j`, `depth` deep in `inc`/`dec` pairs, past the match. A
    /// placeholder anywhere inside is refused with `why`. Only plain
    /// contents are read. Once the part holds a quote, an escape, a nested
    /// `$(`, `${` or `$[`, a nested `{` in `${...}`, or, in `code` (a
    /// command inside `$(...)`), a comment, a heredoc, a here-string or
    /// `case`, where it ends turns on rules this reader doesn't follow
    /// (nested quoting, `)` in a `case` pattern), so the rest of the
    /// command counts as inside it.
    fn skip_balanced(
        &self,
        mut j: usize,
        inc: char,
        dec: char,
        mut depth: usize,
        why: &str,
        code: bool,
    ) -> Result<usize, String> {
        let posix = self.shell == Shell::Posix;
        // Bash takes `((` or `$((` as arithmetic only when the first `)`
        // that matches the inner `(` is followed straight away by `)`;
        // otherwise it reads nested `(`s, where `#` starts a comment.
        let math = inc == '(' && depth == 2 && !code;
        while let Some(c) = self.src[j..].chars().next() {
            let rest = &self.src[j..];
            if rest.starts_with("{{") {
                return Err(why.into());
            }
            let quote = if posix {
                matches!(c, '\'' | '"' | '`' | '\\')
            } else {
                SINGLE.contains(&c) || DOUBLE.contains(&c) || c == '`'
            };
            let nested = c == '$' && rest[1..].starts_with(['(', '{', '[']);
            let unsure = quote
                || nested
                || (inc == '{' && c == '{')
                || (code
                    && (c == '#'
                        || rest.starts_with("<<")
                        || (posix && starts_word(rest, "case"))
                        || (!posix && here_string(rest))));
            if unsure {
                return self.unclear();
            }
            if c == inc {
                depth += 1;
            } else if c == dec {
                depth -= 1;
                if math && depth == 1 {
                    return if rest[1..].starts_with(dec) {
                        Ok(j + 2)
                    } else {
                        self.unclear()
                    };
                }
                if depth == 0 {
                    return Ok(j + c.len_utf8());
                }
            }
            j += c.len_utf8();
        }
        Ok(self.src.len())
    }

    /// `$(( ))`, `(( ))`, `${ }`, `$[ ]`: from after `open` bytes, `depth`
    /// deep, to the match. A placeholder inside is refused with `why`.
    fn balanced(
        &mut self,
        open: usize,
        inc: char,
        dec: char,
        depth: usize,
        why: &str,
    ) -> Result<(), String> {
        self.at = self.skip_balanced(self.at + open, inc, dec, depth, why, false)?;
        Ok(())
    }

    /// `#` to the end of the line.
    fn comment(&mut self) -> Result<(), String> {
        let end = self
            .rest()
            .find('\n')
            .map_or(self.src.len(), |e| self.at + e);
        if self.src[self.at..end].contains("{{") {
            return Err(COMMENT.into());
        }
        self.at = end;
        Ok(())
    }

    /// After a newline: the bodies of the heredocs opened on that line.
    fn heredoc_bodies(&mut self) -> Result<(), String> {
        for (end, strip, quoted) in std::mem::take(&mut self.heredocs) {
            // A line ending in an odd run of `\` goes on to the next one, so
            // the end word on that next line doesn't end the body. Bash
            // joins them only when the end word isn't quoted.
            let mut joined = String::new();
            while self.at < self.src.len() {
                let rest = self.rest();
                let line_end = rest.find('\n').map_or(rest.len(), |e| e + 1);
                let line = rest[..line_end].trim_end_matches('\n');
                self.at += line_end;
                if line.contains("{{") {
                    return Err(HEREDOC.into());
                }
                let odd = line.len() - line.trim_end_matches('\\').len();
                if !quoted && odd % 2 == 1 {
                    joined.push_str(&line[..line.len() - 1]);
                    continue;
                }
                joined.push_str(line);
                let line_text = if strip {
                    joined.trim_start_matches('\t')
                } else {
                    joined.as_str()
                };
                if line_text == end {
                    break;
                }
                joined.clear();
            }
        }
        Ok(())
    }

    /// `2>` and `*>`: the number is part of the redirection, not a word.
    fn drop_fd(&mut self) {
        if self.word.as_ref().is_some_and(|w| {
            !w.has_slot()
                && !w.text.is_empty()
                && (w.text.chars().all(|c| c.is_ascii_digit()) || w.text == "*")
        }) {
            self.word = None;
        }
    }
}

/// A path that may be under `/dev` or `/proc`, however it is written: `..`
/// steps back (no further than `/`), a relative path that steps up may get
/// there, and a glob may match it.
fn device(path: &str) -> bool {
    if path.contains(['*', '?', '[']) {
        return true;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if !path.starts_with('/') => return true,
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    path.starts_with('/') && matches!(parts.first(), Some(&"dev" | &"proc"))
}

/// A program that runs a file it is given: an interpreter, `source`, `.`,
/// `eval`, `exec`, or sed and awk with `-f`.
fn runs_files(name: &str) -> bool {
    lang(name).is_some()
        || matches!(
            name,
            "source" | "." | "eval" | "exec" | "sed" | "awk" | "gawk" | "mawk" | "nawk"
        )
}

fn after(mark: char) -> String {
    format!("a value can't come right after \"{mark}\"; put a space before it")
}

/// `rest` starts with the word `word`.
fn starts_word(rest: &str, word: &str) -> bool {
    rest.strip_prefix(word).is_some_and(|after| {
        after
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || c == ';')
    })
}

/// `@'` or `@"` that ends its line: a PowerShell here-string.
fn here_string(rest: &str) -> bool {
    (rest.starts_with("@'") || rest.starts_with("@\""))
        && rest[2..]
            .chars()
            .find(|c| *c != ' ' && *c != '\t' && *c != '\r')
            .is_none_or(|c| c == '\n')
}

/// `arr[` with the index still open.
fn is_index(text: &str) -> bool {
    text.split_once('[')
        .is_some_and(|(name, index)| ident(name) && !index.contains(']'))
}

fn ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `NAME=value` before a program: a variable for it, not a word.
fn is_assignment(text: &str) -> bool {
    text.split_once('=').is_some_and(|(name, _)| {
        let name = name.strip_suffix('+').unwrap_or(name);
        let name = name.split_once('[').map_or(name, |(n, _)| n);
        ident(name)
    })
}

/// A placeholder anywhere in `toks` before `end`: a value can reach a
/// pipe's reader through a file an earlier command wrote.
fn slot_before(toks: &[Tok], end: usize) -> bool {
    toks[..end]
        .iter()
        .any(|t| matches!(t, Tok::Word(w) if w.has_slot()))
}

/// A placeholder in the pipeline that ends at `end`.
fn slot_in_pipeline(toks: &[Tok], end: usize) -> bool {
    let start = toks[..end]
        .iter()
        .rposition(|t| matches!(t, Tok::Sep))
        .map_or(0, |s| s + 1);
    slot_before(&toks[start..end], end - start)
}

/// `[[ ... ]]` reads the sides of `-gt` and friends as arithmetic, and
/// `-v` a variable's name: an array index there runs `$(...)`, quoted or
/// not. Only unquoted `[[` and `]]` count.
fn test_brackets(toks: &[Tok]) -> Result<(), String> {
    let keyword = |w: &Word, k: &str| w.text == k && !w.quoted && !w.has_slot();
    let mut inside: Option<Vec<&Word>> = None;
    for t in toks {
        let Tok::Word(w) = t else { continue };
        match &mut inside {
            None if keyword(w, "[[") => inside = Some(Vec::new()),
            None => {}
            Some(words) if keyword(w, "]]") => {
                arithmetic_slot(words)?;
                inside = None;
            }
            Some(words) => words.push(w),
        }
    }
    inside.map_or(Ok(()), |words| arithmetic_slot(&words))
}

/// A placeholder next to `-eq` and friends, or after `-v`, in `[[ ]]`.
fn arithmetic_slot(words: &[&Word]) -> Result<(), String> {
    const ARITH: &[&str] = &["-eq", "-ne", "-lt", "-le", "-gt", "-ge"];
    let is = |w: Option<&&Word>, ops: &[&str]| {
        w.is_some_and(|w| !w.quoted && ops.contains(&w.text.as_str()))
    };
    for (i, w) in words.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| words.get(p));
        if w.has_slot() && (is(prev, ARITH) || is(words.get(i + 1), ARITH) || is(prev, &["-v"])) {
            return Err(MATH.into());
        }
    }
    Ok(())
}

struct Command<'t> {
    words: Vec<&'t Word>,
    /// Its `<<<` input.
    input: Option<&'t Word>,
    /// Where it starts in the tokens.
    start: usize,
    /// It reads the output of the command before it.
    piped: bool,
}

fn commands(toks: &[Tok]) -> Vec<Command<'_>> {
    let empty = |start, piped| Command {
        words: Vec::new(),
        input: None,
        start,
        piped,
    };
    let mut out = Vec::new();
    let mut cur = empty(0, false);
    let mut target: Option<bool> = None;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Word(w) => match target.take() {
                Some(true) => cur.input = Some(w),
                Some(false) => {}
                None => cur.words.push(w),
            },
            Tok::Redirect => target = Some(false),
            Tok::HereString => target = Some(true),
            // `a |` then a newline or a comment: the next line reads the pipe.
            Tok::Sep if cur.piped && cur.words.is_empty() && cur.input.is_none() => {
                target = None;
            }
            Tok::Sep | Tok::Pipe => {
                target = None;
                let next = empty(i + 1, matches!(t, Tok::Pipe));
                out.push(std::mem::replace(&mut cur, next));
            }
        }
    }
    out.push(cur);
    out
}

/// Shell words that run the word after them as a command.
const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "{", "coproc",
];

/// A program that runs the rest of its command as a program, with its
/// options that take the next word (`sudo -u root`) and how many words come
/// before the program (`timeout 5s`).
struct Wrapper {
    name: &'static str,
    takes: &'static [&'static str],
    leading: usize,
}

const fn wrap(name: &'static str, takes: &'static [&'static str], leading: usize) -> Wrapper {
    Wrapper {
        name,
        takes,
        leading,
    }
}

const WRAPPERS: &[Wrapper] = &[
    wrap(
        "sudo",
        &[
            "-u",
            "-g",
            "-h",
            "-p",
            "-C",
            "-D",
            "-r",
            "-t",
            "-U",
            "-T",
            "-R",
            "--user",
            "--group",
            "--host",
            "--prompt",
            "--close-from",
            "--chdir",
            "--role",
            "--type",
            "--other-user",
            "--command-timeout",
            "--chroot",
        ],
        0,
    ),
    wrap("doas", &["-u", "-C"], 0),
    wrap("env", &["-u", "-C", "--unset", "--chdir"], 0),
    wrap("exec", &["-a"], 0),
    wrap("nohup", &[], 0),
    wrap("nice", &["-n", "--adjustment"], 0),
    wrap("time", &["-f", "-o", "--format", "--output"], 0),
    wrap(
        "xargs",
        &[
            "-I",
            "-L",
            "-n",
            "-P",
            "-s",
            "-d",
            "-E",
            "-a",
            "--arg-file",
            "--delimiter",
            "--max-args",
            "--max-procs",
            "--max-chars",
            "--max-lines",
            "--replace",
            "--eof",
        ],
        0,
    ),
    wrap("command", &[], 0),
    wrap("builtin", &[], 0),
    wrap("timeout", &["-s", "-k", "--signal", "--kill-after"], 1),
    wrap(
        "stdbuf",
        &["-i", "-o", "-e", "--input", "--output", "--error"],
        0,
    ),
    wrap(
        "ionice",
        &["-c", "-n", "-p", "-P", "-u", "--class", "--classdata"],
        0,
    ),
    wrap("setsid", &[], 0),
    wrap("chroot", &["--userspec", "--groups"], 1),
    wrap("taskset", &[], 1),
    wrap("chrt", &[], 1),
    wrap(
        "flock",
        &["-w", "-E", "--timeout", "--conflict-exit-code"],
        1,
    ),
    wrap("busybox", &[], 0),
    wrap("unbuffer", &[], 0),
    wrap("caffeinate", &["-w", "-t"], 0),
    wrap("&", &[], 0),
];

/// Words that run what follows them as code.
const RUNS_REST: &[&str] = &[
    "eval",
    "iex",
    "invoke-expression",
    "source",
    ".",
    "watch",
    "trap",
    "wsl",
];

/// Where a command's program is in `words`, past `A=1`, `then`, `sudo -u x`
/// and the like, and whether `xargs` hands it its words. `None` when
/// nothing runs (`command -v`).
fn program(words: &[&Word], shell: Shell) -> Result<Option<(usize, bool)>, String> {
    let mut i = 0;
    let mut xargs = false;
    while let Some(w) = words.get(i) {
        if shell == Shell::Posix && is_assignment(&w.text) {
            i += 1;
            continue;
        }
        if w.has_slot() {
            // PowerShell prints a string that starts a command: `& 'x'` runs it.
            if shell == Shell::PowerShell && i == 0 {
                return Ok(None);
            }
            return Err(ITSELF.into());
        }
        let name = w.name();
        if shell == Shell::Posix && KEYWORDS.contains(&name.as_str()) {
            i += 1;
            continue;
        }
        let Some(wrapper) = WRAPPERS.iter().find(|x| x.name == name) else {
            return Ok(Some((i, xargs)));
        };
        if name == "command"
            && words
                .get(i + 1)
                .is_some_and(|w| matches!(w.text.as_str(), "-v" | "-V"))
        {
            return Ok(None);
        }
        xargs |= name == "xargs";
        i += 1;
        while let Some(o) = words.get(i) {
            let t = o.text.as_str();
            if t == "--" {
                i += 1;
                break;
            }
            if t.len() < 2 || !t.starts_with('-') {
                break;
            }
            // `env -S 'prog args'` splits its value into a command.
            let split = t == "--split-string" || (!t.starts_with("--") && t.contains('S'));
            if name == "env" && split && words[i..].iter().any(|w| w.has_slot()) {
                return Err(ITSELF.into());
            }
            i += if wrapper.takes.contains(&t) { 2 } else { 1 };
        }
        for _ in 0..wrapper.leading {
            if words
                .get(i)
                .is_some_and(|w| w.has_slot() || !w.text.starts_with('-'))
            {
                i += 1;
            }
        }
        // `flock lockfile -c 'script'`
        if name == "flock"
            && words
                .get(i)
                .is_some_and(|w| matches!(w.text.as_str(), "-c" | "--command"))
        {
            if words.get(i + 1).is_some_and(|w| w.has_slot()) {
                return Err(script("flock"));
            }
            return Ok(None);
        }
    }
    Ok(None)
}

fn script(name: &str) -> String {
    format!("a value can't be the script {name} runs; pass it as an argument")
}

fn any_slot(words: &[&Word]) -> bool {
    words.iter().any(|w| w.has_slot())
}

/// Refuses a value in a place where the command runs it as code. Returns the
/// placeholders after `--`, where a value can start with `-`.
fn command_problem(words: &[&Word], shell: Shell) -> Result<Vec<usize>, String> {
    let Some((p, _)) = program(words, shell)? else {
        return Ok(Vec::new());
    };
    let name = words[p].name();
    let args = &words[p + 1..];
    if RUNS_REST.contains(&name.as_str()) && any_slot(args) {
        return Err(format!(
            "a value can't follow {name}, which runs it as code"
        ));
    }
    // `-i` makes values arithmetic; `-n` makes a value a variable's name.
    let int_flag = |w: &&Word| {
        (w.text.starts_with('-') || w.text.starts_with('+')) && w.text[1..].contains(['i', 'n'])
    };
    let declares = matches!(
        name.as_str(),
        "declare" | "typeset" | "local" | "readonly" | "export"
    );
    if shell == Shell::Posix
        && (name == "let" || (declares && args.iter().any(int_flag)))
        && any_slot(args)
    {
        return Err(MATH.into());
    }
    // Builtins that take a variable's name evaluate an array index in it.
    if shell == Shell::Posix {
        // `printf -v {{x}}`, glued on as `printf -v{{x}}` or `'-v'{{x}}`, or
        // in a cluster (`wait -np{{x}}`): bash reads the text after quotes.
        let letter = |w: &Word, flag: &str| {
            let f = &flag[1..];
            w.text.len() > 1
                && w.text.starts_with('-')
                && !w.text.starts_with("--")
                && w.text[1..].contains(f)
        };
        let after = |flag: &str| {
            args.windows(2)
                .any(|w| !w[0].has_slot() && letter(w[0], flag) && w[1].has_slot())
                || args.iter().any(|w| w.has_slot() && letter(w, flag))
        };
        let names = match name.as_str() {
            "read" | "mapfile" | "readarray" | "getopts" | "unset" => any_slot(args),
            "printf" => after("-v"),
            "test" | "[" => after("-v") || after("-R"),
            "wait" => after("-p"),
            _ if declares => args.iter().any(|w| w.bare && w.has_slot()),
            _ => false,
        };
        if names {
            return Err(NAMES.into());
        }
    }
    // The program, and any interpreter later on (`docker exec web sh -c`).
    for j in p..words.len() {
        let interpreter = words[j].name();
        let Some(lang) = lang(&interpreter) else {
            continue;
        };
        let rest = &words[j + 1..];
        let runs_value = match lang.reads(rest) {
            Reads::Word(k) => rest.get(k).is_some_and(|w| w.has_slot()),
            Reads::Rest(k) => any_slot(&rest[k.min(rest.len())..]),
            // Only as the program: `Get-Process powershell` names one.
            Reads::Bare(k) => j == p && any_slot(&rest[k..]),
            Reads::File | Reads::Input => false,
        };
        if runs_value {
            return Err(script(&interpreter));
        }
    }
    match name.as_str() {
        "ssh" => ssh(args)?,
        "find" => find(args, shell)?,
        "git" => git(args)?,
        "sed" | "awk" | "gawk" | "mawk" | "nawk" => program_first(&name, args)?,
        "start-process" | "saps" | "start" if shell == Shell::PowerShell => start_process(args)?,
        _ => {}
    }
    // `sh -c 'git fetch "$1"' -- {{x}}`, `bash -s -- {{x}} <<EOF`,
    // `python3 deploy.py -- {{x}}`: with an interpreter in the command, a
    // `--` is the script's own, and the script may hand the value on.
    // find's `--` comes before its paths, not its -exec command.
    let script_reads = (p..words.len()).any(|j| lang(&words[j].name()).is_some());
    if script_reads || name == "find" {
        return Ok(Vec::new());
    }
    // `kubectl exec app -- git fetch {{x}}`: the first `--` ends the
    // runner's own options, and the program after it reads the value.
    let need = if PASSES_ON.contains(&name.as_str()) {
        2
    } else {
        1
    };
    Ok(args
        .iter()
        .enumerate()
        .filter(|(_, w)| w.text == "--" && !w.has_slot())
        .nth(need - 1)
        .map(|(d, _)| {
            args[d + 1..]
                .iter()
                .flat_map(|w| w.slots.iter().copied())
                .collect()
        })
        .unwrap_or_default())
}

/// Programs that end their own options at `--` and hand the words after it
/// to another program, `--` and all removed.
const PASSES_ON: &[&str] = &[
    "cargo",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "bun",
    "deno",
    "uv",
    "uvx",
    "poetry",
    "pipx",
    "pdm",
    "hatch",
    "dotnet",
    "kubectl",
    "oc",
    "nsenter",
    "unshare",
    "flatpak",
    "firejail",
    "bwrap",
    "systemd-run",
];

/// The name of a program that runs what it reads in (`| sh`, `| iex`,
/// `xargs`, which makes its input words a command).
fn runs_input(words: &[&Word], shell: Shell) -> Option<String> {
    let (p, xargs) = program(words, shell).ok()??;
    if xargs {
        return Some("xargs".into());
    }
    let name = words[p].name();
    if matches!(name.as_str(), "source" | "." | "iex" | "invoke-expression") {
        return Some(name);
    }
    (lang(&name)?.reads(&words[p + 1..]) == Reads::Input).then_some(name)
}

/// Where an interpreter finds its script in the words after its name.
#[derive(Debug, PartialEq, Eq)]
enum Reads {
    /// This word is the script.
    Word(usize),
    /// Every word from here on is the script.
    Rest(usize),
    /// Windows PowerShell's first bare word: it and every word after it are
    /// a command (`powershell Get-Item x`).
    Bare(usize),
    /// A file or module; the words after it are data.
    File,
    /// Its input.
    Input,
}

/// How an interpreter's flags read.
struct Lang {
    /// Short flags whose value is the script: `-c`, `-e`.
    code: &'static [char],
    /// Long flags whose value is the script: `--eval`.
    long_code: &'static [&'static str],
    /// Short flags with a value: the rest of the cluster, or the next word
    /// when nothing follows (`-o pipefail`, `-W error`).
    takes: &'static [char],
    /// Short flags whose value is only ever glued on (`perl -Mstrict`).
    glued: &'static [char],
    /// Long flags with a value in the next word.
    long_takes: &'static [&'static str],
    /// Short flags that run a file or module (`python -m`).
    file: &'static [char],
    /// Short flags that read the script from the input (`bash -s`).
    input: &'static [char],
    /// A shell: `-c` makes the first word after the options the script.
    shell: bool,
    /// What runs when no script is named.
    none: Reads,
    /// Flags that read their own way.
    custom: Option<Custom>,
}

#[derive(Clone, Copy)]
enum Custom {
    /// `pwsh -Command ...`
    Pwsh,
    /// `powershell ...`: like pwsh, but a bare word is a command.
    WindowsPowerShell,
    /// `cmd /c ...`
    Cmd,
    /// `deno eval 'code'`
    Deno,
    /// `-c` anywhere, options after the user name too (`su user -c`).
    Anywhere,
}

const BASE: Lang = Lang {
    code: &[],
    long_code: &[],
    takes: &[],
    glued: &[],
    long_takes: &[],
    file: &[],
    input: &[],
    shell: false,
    none: Reads::Input,
    custom: None,
};

const SHELL: Lang = Lang {
    code: &['c'],
    long_code: &["command"],
    takes: &['o', 'O'],
    long_takes: &["rcfile", "init-file"],
    input: &['s'],
    shell: true,
    ..BASE
};
const PYTHON: Lang = Lang {
    code: &['c'],
    takes: &['W', 'X'],
    file: &['m'],
    ..BASE
};
const PERL: Lang = Lang {
    code: &['e', 'E'],
    takes: &['I'],
    glued: &['0', 'C', 'd', 'D', 'i', 'l', 'm', 'M', 'x'],
    ..BASE
};
const RUBY: Lang = Lang {
    code: &['e'],
    takes: &['r', 'I', 'C', 'E', 'F'],
    glued: &['0', 'i', 'K', 'T', 'W', 'x'],
    ..BASE
};
const NODE: Lang = Lang {
    code: &['e', 'p'],
    long_code: &["eval", "print"],
    takes: &['r', 'C'],
    long_takes: &[
        "require",
        "import",
        "loader",
        "experimental-loader",
        "conditions",
        "input-type",
    ],
    ..BASE
};
const BUN: Lang = Lang {
    code: &['e', 'p'],
    long_code: &["eval", "print"],
    none: Reads::File,
    ..BASE
};
const PHP: Lang = Lang {
    code: &['r', 'B', 'R', 'E'],
    takes: &['c', 'd', 'z'],
    file: &['f', 'F'],
    ..BASE
};
const E_FLAG: Lang = Lang {
    code: &['e'],
    takes: &['l'],
    ..BASE
};
const RSCRIPT: Lang = Lang {
    code: &['e'],
    none: Reads::File,
    ..BASE
};
const fn custom(c: Custom) -> Lang {
    Lang {
        custom: Some(c),
        ..BASE
    }
}
const PWSH: Lang = custom(Custom::Pwsh);
const WINDOWS_POWERSHELL: Lang = custom(Custom::WindowsPowerShell);
const CMD: Lang = custom(Custom::Cmd);
const DENO: Lang = custom(Custom::Deno);
const ANYWHERE: Lang = custom(Custom::Anywhere);

fn lang(name: &str) -> Option<&'static Lang> {
    Some(match name {
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "ash" | "yash" | "fish" | "rbash"
        | "csh" | "tcsh" => &SHELL,
        "python" | "python2" | "python3" | "py" | "pypy" | "pypy3" | "pythonw" => &PYTHON,
        n if n.starts_with("python2.") || n.starts_with("python3.") => &PYTHON,
        "perl" => &PERL,
        "ruby" | "jruby" => &RUBY,
        "node" | "nodejs" => &NODE,
        "bun" => &BUN,
        "php" => &PHP,
        "osascript" | "lua" | "luajit" => &E_FLAG,
        "rscript" => &RSCRIPT,
        "su" | "runuser" | "script" | "expect" => &ANYWHERE,
        "deno" => &DENO,
        "pwsh" => &PWSH,
        "powershell" => &WINDOWS_POWERSHELL,
        "cmd" => &CMD,
        _ => return None,
    })
}

impl Lang {
    fn reads(&self, rest: &[&Word]) -> Reads {
        match self.custom {
            Some(Custom::Pwsh) => return pwsh_reads(rest, false),
            Some(Custom::WindowsPowerShell) => return pwsh_reads(rest, true),
            Some(Custom::Cmd) => return cmd_reads(rest),
            Some(Custom::Deno) => {
                return match rest.iter().position(|w| !w.text.starts_with('-')) {
                    Some(k) if rest[k].text == "eval" => Reads::Word(k + 1),
                    _ => Reads::File,
                };
            }
            Some(Custom::Anywhere) => return anywhere_reads(rest),
            None => {}
        }
        let mut command = false;
        let mut input = false;
        let mut k = 0;
        while let Some(w) = rest.get(k) {
            let t = w.text.as_str();
            if t == "-" {
                if !command {
                    return Reads::Input;
                }
                break;
            }
            if t == "--" {
                k += 1;
                break;
            }
            let option = t.len() > 1 && (t.starts_with('-') || (self.shell && t.starts_with('+')));
            if !option {
                break;
            }
            if let Some(long) = t.strip_prefix("--") {
                let (key, glued) = long
                    .split_once('=')
                    .map_or((long, false), |(key, _)| (key, true));
                if self.long_code.contains(&key) {
                    if glued {
                        return Reads::Word(k);
                    }
                    if self.shell {
                        command = true;
                        k += 1;
                        continue;
                    }
                    return Reads::Word(k + 1);
                }
                k += if !glued && self.long_takes.contains(&key) {
                    2
                } else {
                    1
                };
                continue;
            }
            let dash = t.starts_with('-');
            let flags = &t[1..];
            let mut skip = false;
            for (i, f) in flags.char_indices() {
                let after = &flags[i + f.len_utf8()..];
                if dash && self.file.contains(&f) {
                    return Reads::File;
                }
                if dash && self.code.contains(&f) {
                    if self.shell {
                        command = true;
                        continue;
                    }
                    // `-pe` is two flags; `-c'print(1)'` has the script glued on.
                    return if after.chars().all(|c| self.code.contains(&c)) {
                        Reads::Word(k + 1)
                    } else {
                        Reads::Word(k)
                    };
                }
                if dash && self.input.contains(&f) {
                    input = true;
                }
                if self.takes.contains(&f) {
                    skip = after.is_empty();
                    break;
                }
                if self.glued.contains(&f) {
                    break;
                }
            }
            k += if skip { 2 } else { 1 };
        }
        match rest.get(k) {
            Some(_) if command => Reads::Word(k),
            _ if command => Reads::File,
            _ if input => Reads::Input,
            // `bash /dev/stdin <<< ...`, `bash /dev/fd/3 3<<< ...`: a script
            // from a device or a process's open file is an input.
            Some(w) if device(&w.text) => Reads::Input,
            Some(_) => Reads::File,
            None => match self.none {
                Reads::File => Reads::File,
                _ => Reads::Input,
            },
        }
    }
}

/// `pwsh -Command ...`: everything after the flag is the script. Windows
/// PowerShell reads a bare first word as a command too; pwsh as a file.
fn pwsh_reads(rest: &[&Word], windows_powershell: bool) -> Reads {
    const TAKES: &[&str] = &[
        "executionpolicy",
        "workingdirectory",
        "windowstyle",
        "inputformat",
        "outputformat",
        "settingsfile",
        "configurationname",
        "configurationfile",
        "custompipename",
        "psconsolefile",
        "version",
    ];
    let mut k = 0;
    while let Some(w) = rest.get(k) {
        let t = w.text.to_ascii_lowercase();
        let Some(flag) = t.strip_prefix('-').or_else(|| t.strip_prefix('/')) else {
            return if windows_powershell {
                Reads::Bare(k)
            } else {
                Reads::File
            };
        };
        if flag.is_empty() {
            return Reads::Input;
        }
        let encoded =
            flag == "e" || flag == "ec" || (flag.len() > 1 && "encodedcommand".starts_with(flag));
        if "commandwithargs".starts_with(flag) || flag == "cwa" || encoded {
            return Reads::Rest(k + 1);
        }
        if "file".starts_with(flag) {
            return Reads::File;
        }
        let takes = matches!(flag, "ep" | "ex" | "wd" | "w" | "if" | "of" | "o" | "v")
            || (flag.len() > 2 && TAKES.iter().any(|name| name.starts_with(flag)));
        k += if takes { 2 } else { 1 };
    }
    Reads::Input
}

/// `cmd /c ...`, `/k`, `/r`: everything after is the script.
fn cmd_reads(rest: &[&Word]) -> Reads {
    for (k, w) in rest.iter().enumerate() {
        let t = w.text.to_ascii_lowercase();
        if ["/c", "/k", "/r"].iter().any(|f| t.starts_with(f)) {
            return if t.len() > 2 {
                Reads::Rest(k)
            } else {
                Reads::Rest(k + 1)
            };
        }
    }
    Reads::Input
}

/// `su user -c 'script'`, `script -qc 'script'`: a `-c` anywhere.
fn anywhere_reads(rest: &[&Word]) -> Reads {
    for (k, w) in rest.iter().enumerate() {
        let t = w.text.as_str();
        if let Some(long) = t.strip_prefix("--") {
            match long.split_once('=') {
                Some(("command" | "session-command", _)) => return Reads::Word(k),
                None if matches!(long, "command" | "session-command") => return Reads::Word(k + 1),
                _ => {}
            }
        } else if let Some(flags) = t.strip_prefix('-')
            && let Some(at) = flags.find('c')
        {
            return if at + 1 == flags.len() {
                Reads::Word(k + 1)
            } else {
                Reads::Word(k)
            };
        }
    }
    Reads::Input
}

/// After the host, ssh's words are a command line the other computer's
/// shell runs; `-o` sets an option such as `ProxyCommand`.
fn ssh(args: &[&Word]) -> Result<(), String> {
    const TAKES: &str = "BbcDEeFIiJLlmOopQRSWw";
    let mut k = 0;
    while let Some(w) = args.get(k) {
        let t = w.text.as_str();
        if t == "--" {
            k += 1;
            break;
        }
        if t.len() < 2 || !t.starts_with('-') {
            break;
        }
        let flags = &t[1..];
        let mut skip = false;
        for (i, f) in flags.char_indices() {
            if TAKES.contains(f) {
                let last = i + 1 == flags.len();
                let value = if last { k + 1 } else { k };
                if f == 'o' && args.get(value).is_some_and(|w| w.has_slot()) {
                    return Err("a value can't set an ssh option".into());
                }
                skip = last;
                break;
            }
        }
        k += if skip { 2 } else { 1 };
    }
    if args.get(k + 1..).is_some_and(any_slot) {
        return Err("a value can't be the command ssh runs on the other computer".into());
    }
    Ok(())
}

/// `find -exec prog ... ;` runs prog.
fn find(args: &[&Word], shell: Shell) -> Result<(), String> {
    for (k, w) in args.iter().enumerate() {
        if matches!(w.text.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            let rest = &args[k + 1..];
            let end = rest
                .iter()
                .position(|w| matches!(w.text.as_str(), ";" | "+"))
                .unwrap_or(rest.len());
            command_problem(&rest[..end], shell)?;
        }
    }
    Ok(())
}

/// `git -c core.sshCommand=...` and `git config core.pager ...` run what
/// the setting names.
fn git(args: &[&Word]) -> Result<(), String> {
    const CONFIG: &str = "a value can't name a git setting; name it in the step";
    let mut k = 0;
    while let Some(w) = args.get(k) {
        let t = w.text.as_str();
        if t == "-c" || t == "--config-env" {
            if args.get(k + 1).is_some_and(|w| w.has_slot()) {
                return Err(CONFIG.into());
            }
            k += 2;
        } else if t.starts_with("--config-env=") && w.has_slot() {
            return Err(CONFIG.into());
        } else if matches!(t, "-C" | "--git-dir" | "--work-tree" | "--namespace") {
            k += 2;
        } else if t.starts_with('-') {
            k += 1;
        } else {
            break;
        }
    }
    if args.get(k).is_none_or(|w| w.text != "config") {
        return Ok(());
    }
    let rest = &args[k + 1..];
    if rest.iter().any(|w| {
        matches!(
            w.text.as_str(),
            "get" | "--get" | "--get-all" | "--get-regexp" | "-l" | "--list"
        )
    }) {
        return Ok(());
    }
    let mut names = Vec::new();
    let mut j = 0;
    while let Some(w) = rest.get(j) {
        let t = w.text.as_str();
        if matches!(
            t,
            "-f" | "--file" | "--blob" | "--type" | "--default" | "--comment"
        ) {
            j += 2;
            continue;
        }
        if !t.starts_with('-') {
            names.push(*w);
        }
        j += 1;
    }
    let key = match names.first().map(|w| w.text.as_str()) {
        Some("set" | "unset" | "rename-section" | "remove-section") => names.get(1),
        _ => names.first(),
    };
    if key.is_some_and(|w| w.has_slot()) {
        return Err(CONFIG.into());
    }
    Ok(())
}

/// sed and awk read their first word as a program of their own (sed's `e`
/// runs commands, awk has `system()`), unless `-e` or `-f` names one.
fn program_first(name: &str, args: &[&Word]) -> Result<(), String> {
    let sed = name == "sed";
    let takes: &[char] = if sed {
        &['l']
    } else {
        &['v', 'F', 'i', 'l', 'W']
    };
    let why = || format!("a value can't be the program {name} runs; pass it as a file name");
    let mut named = false;
    let mut k = 0;
    while let Some(w) = args.get(k) {
        let t = w.text.as_str();
        if t == "--" {
            k += 1;
            break;
        }
        if t.len() < 2 || !t.starts_with('-') {
            break;
        }
        if let Some(long) = t.strip_prefix("--") {
            let (key, glued) = long
                .split_once('=')
                .map_or((long, false), |(key, _)| (key, true));
            let code = matches!(key, "expression" | "source");
            named |= code || key == "file";
            if code
                && args
                    .get(if glued { k } else { k + 1 })
                    .is_some_and(|w| w.has_slot())
            {
                return Err(why());
            }
            let value = code
                || matches!(
                    key,
                    "file" | "line-length" | "assign" | "field-separator" | "include" | "load"
                );
            k += if value && !glued { 2 } else { 1 };
            continue;
        }
        let flags = &t[1..];
        let mut skip = false;
        for (i, f) in flags.char_indices() {
            let last = i + f.len_utf8() == flags.len();
            if f == 'e' || f == 'f' {
                named = true;
                if f == 'e'
                    && args
                        .get(if last { k + 1 } else { k })
                        .is_some_and(|w| w.has_slot())
                {
                    return Err(why());
                }
                skip = last;
                break;
            }
            if takes.contains(&f) {
                skip = last;
                break;
            }
            if sed && f == 'i' {
                break;
            }
        }
        k += if skip { 2 } else { 1 };
    }
    if !named && args.get(k).is_some_and(|w| w.has_slot()) {
        return Err(why());
    }
    Ok(())
}

/// `Start-Process prog`: its first word, or `-FilePath`'s, is the program.
fn start_process(args: &[&Word]) -> Result<(), String> {
    const SWITCHES: &[&str] = &[
        "wait",
        "nonewwindow",
        "passthru",
        "usenewenvironment",
        "loaduserprofile",
    ];
    let mut k = 0;
    while let Some(w) = args.get(k) {
        let t = w.text.to_ascii_lowercase();
        let Some(flag) = t.strip_prefix('-') else {
            return if w.has_slot() {
                Err(ITSELF.into())
            } else {
                Ok(())
            };
        };
        if !flag.is_empty() && "filepath".starts_with(flag) {
            if args.get(k + 1).is_some_and(|w| w.has_slot()) {
                return Err(ITSELF.into());
            }
            return Ok(());
        }
        k += if SWITCHES
            .iter()
            .any(|s| !flag.is_empty() && s.starts_with(flag))
        {
            1
        } else {
            2
        };
    }
    Ok(())
}
