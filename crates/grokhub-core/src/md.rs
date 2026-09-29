//! Reply markdown: block and inline parse, plus a small code tokenizer.
//!
//! The cabin paints what this returns. Parsing lives here so Linux, Windows,
//! and the tests agree on what a table, a numbered list, or a link is.

/// One painted block of a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdBlock {
    /// `#` .. `######`. Level is 1..=6.
    Heading(u8, String),
    /// `- `, `* `, or `+ `. Depth counts two-space indents.
    Bullet { depth: u8, text: String },
    /// `1. ` or `1) `. The number is what the reply wrote, not a recount.
    Numbered { depth: u8, num: String, text: String },
    /// `- [ ]` / `- [x]`.
    Task { depth: u8, done: bool, text: String },
    /// `> ` lines, joined.
    Quote(String),
    /// `---`, `***`, `___`.
    Rule,
    Table {
        header: Vec<String>,
        align: Vec<MdAlign>,
        rows: Vec<Vec<String>>,
    },
    /// A fenced block. `lang` is the info string's first word, lowercased.
    Code { lang: String, body: String },
    Para(String),
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdAlign {
    Left,
    Center,
    Right,
}

/// Inline run inside a paragraph, list item, quote, or table cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdSpan {
    Text(String),
    Bold(String),
    Italic(String),
    Strike(String),
    Code(String),
    /// Only http(s). Anything else stays [`MdSpan::Text`].
    Link { text: String, url: String },
}

/// A code block or a table takes the full bubble width.
pub fn md_wants_full_width(text: &str) -> bool {
    md_blocks(text)
        .iter()
        .any(|b| matches!(b, MdBlock::Code { .. } | MdBlock::Table { .. }))
}

fn fence_open(line: &str) -> Option<(&'static str, String)> {
    let t = line.trim_start();
    let marker = if t.starts_with("```") {
        "```"
    } else if t.starts_with("~~~") {
        "~~~"
    } else {
        return None;
    };
    let info = t[3..].trim_start_matches(marker.chars().next().unwrap_or('`'));
    let lang = info
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c == '{' || c == '}' || c == '.')
        .to_ascii_lowercase();
    Some((marker, lang))
}

fn indent_depth(line: &str) -> u8 {
    let mut cols = 0usize;
    for c in line.chars() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4,
            _ => break,
        }
    }
    (cols / 2).min(6) as u8
}

fn heading(line: &str) -> Option<(u8, String)> {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some((hashes as u8, text.to_string()))
}

fn is_rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3
        && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn bullet(line: &str) -> Option<(u8, &str)> {
    let depth = indent_depth(line);
    let t = line.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some((depth, rest));
        }
    }
    if matches!(t, "-" | "*" | "+") {
        return Some((depth, ""));
    }
    None
}

fn task(text: &str) -> Option<(bool, &str)> {
    let lower = text.get(..3).map(|s| s.to_ascii_lowercase());
    match lower.as_deref() {
        Some("[ ]") => Some((false, text[3..].trim_start())),
        Some("[x]") => Some((true, text[3..].trim_start())),
        _ => None,
    }
}

fn numbered(line: &str) -> Option<(u8, String, &str)> {
    let depth = indent_depth(line);
    let t = line.trim_start();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let after = &t[digits..];
    let rest = after
        .strip_prefix(". ")
        .or_else(|| after.strip_prefix(") "))
        .or_else(|| (after == "." || after == ")").then_some(""))?;
    Some((depth, t[..digits].to_string(), rest))
}

/// Split one `| a | b |` row. Escaped `\|` stays in the cell.
pub fn md_table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    let mut in_code = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '`' => {
                in_code = !in_code;
                cur.push(c);
            }
            '|' if !in_code => cells.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

fn table_align(line: &str) -> Option<Vec<MdAlign>> {
    if !line.contains('-') {
        return None;
    }
    let cells = md_table_cells(line);
    if cells.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(cells.len());
    for c in cells {
        let c = c.trim();
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        let dashes = c.trim_matches(':');
        if dashes.is_empty() || !dashes.chars().all(|ch| ch == '-') {
            return None;
        }
        out.push(match (left, right) {
            (true, true) => MdAlign::Center,
            (false, true) => MdAlign::Right,
            _ => MdAlign::Left,
        });
    }
    Some(out)
}

fn looks_like_row(line: &str) -> bool {
    let t = line.trim();
    t.contains('|') && !t.is_empty()
}

/// Walk a reply into blocks. An unclosed fence runs to the end (a live stream).
pub fn md_blocks(text: &str) -> Vec<MdBlock> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some((marker, lang)) = fence_open(line) {
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(marker) {
                body.push(lines[i]);
                i += 1;
            }
            // Skip the closing fence when there is one.
            i += 1;
            out.push(MdBlock::Code {
                lang,
                body: body.join("\n"),
            });
            continue;
        }
        if line.trim().is_empty() {
            out.push(MdBlock::Blank);
            i += 1;
            continue;
        }
        if let Some((level, text)) = heading(line) {
            out.push(MdBlock::Heading(level, text));
            i += 1;
            continue;
        }
        if is_rule(line) {
            out.push(MdBlock::Rule);
            i += 1;
            continue;
        }
        if looks_like_row(line) {
            if let Some(align) = lines.get(i + 1).and_then(|l| table_align(l)) {
                let header = md_table_cells(line);
                if header.len() == align.len() {
                    let mut rows = Vec::new();
                    i += 2;
                    while i < lines.len() && looks_like_row(lines[i]) {
                        let mut cells = md_table_cells(lines[i]);
                        cells.resize(header.len(), String::new());
                        rows.push(cells);
                        i += 1;
                    }
                    out.push(MdBlock::Table { header, align, rows });
                    continue;
                }
            }
        }
        if line.trim_start().starts_with('>') {
            let mut quote = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim_start();
                let Some(rest) = t.strip_prefix('>') else {
                    break;
                };
                quote.push(rest.strip_prefix(' ').unwrap_or(rest));
                i += 1;
            }
            out.push(MdBlock::Quote(quote.join("\n")));
            continue;
        }
        if let Some((depth, rest)) = bullet(line) {
            out.push(match task(rest) {
                Some((done, text)) => MdBlock::Task {
                    depth,
                    done,
                    text: text.to_string(),
                },
                None => MdBlock::Bullet {
                    depth,
                    text: rest.to_string(),
                },
            });
            i += 1;
            continue;
        }
        if let Some((depth, num, rest)) = numbered(line) {
            out.push(MdBlock::Numbered {
                depth,
                num,
                text: rest.to_string(),
            });
            i += 1;
            continue;
        }
        out.push(MdBlock::Para(line.to_string()));
        i += 1;
    }
    out
}

/// A link the cabin may open. Only http(s), no whitespace or quotes.
pub fn md_link_ok(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let host = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    host.is_some_and(|h| !h.is_empty() && !h.starts_with('/'))
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>')
}

fn push_text(out: &mut Vec<MdSpan>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(MdSpan::Text(prev)) = out.last_mut() {
        prev.push_str(s);
    } else {
        out.push(MdSpan::Text(s.to_string()));
    }
}

fn word_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric())
}

/// Length of a bare `https://…` run at the start of `s`, minus trailing punctuation.
fn bare_url_len(s: &str) -> Option<usize> {
    let lower_head: String = s.chars().take(8).collect::<String>().to_ascii_lowercase();
    if !(lower_head.starts_with("https://") || lower_head.starts_with("http://")) {
        return None;
    }
    let mut end = s
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    // Trailing sentence punctuation is not part of the URL.
    while end > 0 {
        let last = s[..end].chars().next_back().unwrap_or(' ');
        let unbalanced_paren = last == ')' && s[..end].matches('(').count() < s[..end].matches(')').count();
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | ']') || unbalanced_paren {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    md_link_ok(&s[..end]).then_some(end)
}

/// `[text](url)` at the start of `s`. Returns (text, url, consumed).
fn link_at(s: &str) -> Option<(String, String, usize)> {
    let rest = s.strip_prefix('[')?;
    let close = rest.find("](")?;
    let text = &rest[..close];
    if text.contains('\n') {
        return None;
    }
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let url = after[..end].trim();
    let url = url.split_whitespace().next().unwrap_or("");
    if !md_link_ok(url) {
        return None;
    }
    Some((text.to_string(), url.to_string(), 1 + close + 2 + end + 1))
}

/// Inline spans for one line. Unclosed markers stay text.
pub fn md_spans(line: &str) -> Vec<MdSpan> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = line.as_bytes();
    let mut plain_from = 0;
    while i < line.len() {
        if !line.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rest = &line[i..];
        let prev = line[..i].chars().next_back();
        let mut hit: Option<(MdSpan, usize)> = None;
        match bytes[i] {
            b'`' => {
                let ticks = rest.chars().take_while(|&c| c == '`').count();
                let fence = &rest[..ticks];
                if let Some(end) = rest[ticks..].find(fence) {
                    let body = rest[ticks..ticks + end].trim();
                    hit = Some((MdSpan::Code(body.to_string()), ticks + end + ticks));
                }
            }
            b'*' | b'_'
                if rest.starts_with("**") || (rest.starts_with("__") && !word_char(prev)) =>
            {
                let m = &rest[..2];
                if let Some(end) = rest[2..].find(m) {
                    if end > 0 {
                        hit = Some((MdSpan::Bold(rest[2..2 + end].to_string()), 2 + end + 2));
                    }
                }
            }
            b'*' | b'_' => {
                let m = &rest[..1];
                // `snake_case` and `2*3*4` are not emphasis.
                let opens = !word_char(prev) || bytes[i] == b'*';
                let next = rest[1..].chars().next();
                if opens && next.is_some_and(|c| !c.is_whitespace()) {
                    if let Some(end) = rest[1..].find(m) {
                        let inner = &rest[1..1 + end];
                        let after = rest[1 + end + 1..].chars().next();
                        let closes = !inner.ends_with(char::is_whitespace)
                            && (bytes[i] == b'*' || !word_char(after));
                        if end > 0 && closes {
                            hit = Some((MdSpan::Italic(inner.to_string()), 1 + end + 1));
                        }
                    }
                }
            }
            b'~' if rest.starts_with("~~") => {
                if let Some(end) = rest[2..].find("~~") {
                    if end > 0 {
                        hit = Some((MdSpan::Strike(rest[2..2 + end].to_string()), 2 + end + 2));
                    }
                }
            }
            b'[' => {
                if let Some((text, url, used)) = link_at(rest) {
                    hit = Some((MdSpan::Link { text, url }, used));
                }
            }
            b'h' | b'H' if !word_char(prev) => {
                if let Some(len) = bare_url_len(rest) {
                    let url = rest[..len].to_string();
                    hit = Some((
                        MdSpan::Link {
                            text: url.clone(),
                            url,
                        },
                        len,
                    ));
                }
            }
            _ => {}
        }
        match hit {
            Some((span, used)) => {
                push_text(&mut out, &line[plain_from..i]);
                out.push(span);
                i += used;
                plain_from = i;
            }
            None => i += 1,
        }
    }
    push_text(&mut out, &line[plain_from.min(line.len())..]);
    out
}

/// Plain text of a line with markers dropped. Used for width and search.
pub fn md_plain(line: &str) -> String {
    md_spans(line)
        .into_iter()
        .map(|s| match s {
            MdSpan::Text(t)
            | MdSpan::Bold(t)
            | MdSpan::Italic(t)
            | MdSpan::Strike(t)
            | MdSpan::Code(t) => t,
            MdSpan::Link { text, .. } => text,
        })
        .collect()
}

/// Token class for code colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeTok {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
}

const KEYWORDS: &[&str] = &[
    // Shared across C-family, Rust, Go, JS/TS, Python, shell.
    "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "def",
    "default", "del", "do", "elif", "else", "enum", "esac", "except", "export", "extends",
    "false", "fi", "finally", "fn", "for", "from", "func", "function", "go", "if", "impl",
    "import", "in", "interface", "is", "lambda", "let", "loop", "match", "mod", "mut", "new",
    "nil", "None", "not", "null", "or", "and", "package", "pass", "pub", "raise", "return",
    "self", "Self", "static", "struct", "super", "switch", "then", "this", "throw", "trait",
    "True", "False", "true", "try", "type", "typeof", "use", "var", "void", "where", "while",
    "with", "yield", "done", "echo", "local", "unsafe", "undefined", "private", "public",
    "protected", "defer", "chan", "select", "range", "map", "crate", "ref", "dyn", "move",
];

/// Line comment marker for a fence language. `None` means try both `//` and `#`.
fn line_comment(lang: &str) -> &'static [&'static str] {
    match lang {
        "py" | "python" | "sh" | "bash" | "zsh" | "shell" | "console" | "toml" | "yaml" | "yml"
        | "rb" | "ruby" | "ps1" | "powershell" | "pwsh" | "r" | "dockerfile" | "make"
        | "makefile" | "ini" | "conf" => &["#"],
        "sql" | "lua" | "haskell" | "hs" => &["--"],
        "rs" | "rust" | "c" | "cpp" | "c++" | "h" | "hpp" | "go" | "js" | "javascript" | "ts"
        | "typescript" | "jsx" | "tsx" | "java" | "kt" | "kotlin" | "swift" | "cs" | "csharp"
        | "zig" | "dart" | "scala" | "json5" | "jsonc" => &["//"],
        "json" | "md" | "markdown" | "text" | "txt" | "diff" | "patch" => &[],
        _ => &["//", "#"],
    }
}

/// Colour one code line. `lang` is the fence info word. Unknown languages still
/// get strings, numbers, and `//` / `#` comments.
pub fn code_tokens(lang: &str, line: &str) -> Vec<(CodeTok, String)> {
    let mut out: Vec<(CodeTok, String)> = Vec::new();
    let push = |out: &mut Vec<(CodeTok, String)>, tok: CodeTok, s: &str| {
        if s.is_empty() {
            return;
        }
        if let Some((last, buf)) = out.last_mut() {
            if *last == tok {
                buf.push_str(s);
                return;
            }
        }
        out.push((tok, s.to_string()));
    };
    let plain_lang = matches!(lang, "text" | "txt" | "md" | "markdown" | "diff" | "patch");
    if plain_lang {
        push(&mut out, CodeTok::Plain, line);
        return out;
    }
    let comments = line_comment(lang);
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut k = 0;
    while k < chars.len() {
        let (i, c) = chars[k];
        let rest = &line[i..];
        let prev = k.checked_sub(1).map(|p| chars[p].1);
        if comments.iter().any(|m| {
            rest.starts_with(m)
                // `#` inside `a#b` or a shell `$#` is not a comment.
                && (*m != "#" || prev.is_none_or(|p| p.is_whitespace()))
        }) {
            push(&mut out, CodeTok::Comment, rest);
            return out;
        }
        if c == '"' || c == '\'' || c == '`' {
            // A Rust lifetime `'a` is not a string.
            let lifetime = c == '\'' && matches!(lang, "rs" | "rust") && {
                let ident = chars[k + 1..]
                    .iter()
                    .take_while(|(_, ch)| ch.is_alphanumeric() || *ch == '_')
                    .count();
                ident > 0 && chars.get(k + 1 + ident).map(|(_, ch)| *ch) != Some('\'')
            };
            if !lifetime {
                let mut j = k + 1;
                while j < chars.len() {
                    if chars[j].1 == '\\' {
                        j += 2;
                        continue;
                    }
                    if chars[j].1 == c {
                        break;
                    }
                    j += 1;
                }
                let end = chars.get(j + 1).map(|(b, _)| *b).unwrap_or(line.len());
                push(&mut out, CodeTok::Str, &line[i..end]);
                k = (j + 1).min(chars.len());
                continue;
            }
        }
        if c.is_ascii_digit() && !prev.is_some_and(|p| p.is_alphanumeric() || p == '_') {
            let mut j = k;
            while j < chars.len()
                && (chars[j].1.is_ascii_alphanumeric() || chars[j].1 == '.' || chars[j].1 == '_')
            {
                j += 1;
            }
            let end = chars.get(j).map(|(b, _)| *b).unwrap_or(line.len());
            push(&mut out, CodeTok::Number, &line[i..end]);
            k = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = k;
            while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_') {
                j += 1;
            }
            let end = chars.get(j).map(|(b, _)| *b).unwrap_or(line.len());
            let word = &line[i..end];
            let tok = if KEYWORDS.contains(&word) {
                CodeTok::Keyword
            } else {
                CodeTok::Plain
            };
            push(&mut out, tok, word);
            k = j;
            continue;
        }
        push(&mut out, CodeTok::Plain, &line[i..i + c.len_utf8()]);
        k += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_bullets_tasks_and_depth() {
        let b = md_blocks("1. one\n2) two\n  - nested\n- [x] done\n- [ ] todo\n* star");
        assert_eq!(
            b[0],
            MdBlock::Numbered {
                depth: 0,
                num: "1".into(),
                text: "one".into()
            }
        );
        assert_eq!(
            b[1],
            MdBlock::Numbered {
                depth: 0,
                num: "2".into(),
                text: "two".into()
            }
        );
        assert_eq!(
            b[2],
            MdBlock::Bullet {
                depth: 1,
                text: "nested".into()
            }
        );
        assert_eq!(
            b[3],
            MdBlock::Task {
                depth: 0,
                done: true,
                text: "done".into()
            }
        );
        assert_eq!(
            b[4],
            MdBlock::Task {
                depth: 0,
                done: false,
                text: "todo".into()
            }
        );
        assert_eq!(
            b[5],
            MdBlock::Bullet {
                depth: 0,
                text: "star".into()
            }
        );
    }

    #[test]
    fn a_year_or_version_is_not_a_list() {
        let b = md_blocks("2026 was a year.\n1.0.38 shipped");
        assert!(matches!(b[0], MdBlock::Para(_)), "{b:?}");
        assert!(matches!(b[1], MdBlock::Para(_)), "{b:?}");
    }

    #[test]
    fn headings_rules_and_quotes() {
        let b = md_blocks("#### Four\n#nope\n---\n> a\n> b\nafter");
        assert_eq!(b[0], MdBlock::Heading(4, "Four".into()));
        assert!(matches!(b[1], MdBlock::Para(_)));
        assert_eq!(b[2], MdBlock::Rule);
        assert_eq!(b[3], MdBlock::Quote("a\nb".into()));
        assert_eq!(b[4], MdBlock::Para("after".into()));
    }

    #[test]
    fn tables_need_a_separator_row() {
        let src = "| Name | Size |\n|:-----|-----:|\n| a | 1 |\n| b |\nnot a row";
        let b = md_blocks(src);
        match &b[0] {
            MdBlock::Table { header, align, rows } => {
                assert_eq!(header, &vec!["Name".to_string(), "Size".to_string()]);
                assert_eq!(align, &vec![MdAlign::Left, MdAlign::Right]);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[1], vec!["b".to_string(), String::new()]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b[1], MdBlock::Para("not a row".into()));
        let lone = md_blocks("a | b\nplain");
        assert!(matches!(lone[0], MdBlock::Para(_)), "{lone:?}");
    }

    #[test]
    fn table_cells_keep_escaped_pipes_and_code() {
        assert_eq!(
            md_table_cells("| `a|b` | c \\| d |"),
            vec!["`a|b`".to_string(), "c | d".to_string()]
        );
    }

    #[test]
    fn fences_keep_lang_and_run_to_end_while_streaming() {
        let b = md_blocks("```Rust\nfn main() {}\n```\nafter\n~~~py\nprint(1)");
        assert_eq!(
            b[0],
            MdBlock::Code {
                lang: "rust".into(),
                body: "fn main() {}".into()
            }
        );
        assert_eq!(b[1], MdBlock::Para("after".into()));
        assert_eq!(
            b[2],
            MdBlock::Code {
                lang: "py".into(),
                body: "print(1)".into()
            }
        );
        assert!(md_wants_full_width("```\nx\n```"));
        assert!(md_wants_full_width("| a |\n|---|"));
        assert!(!md_wants_full_width("just words"));
    }

    #[test]
    fn inline_spans() {
        let s = md_spans("**bold** and *it* and `co*de` ~~gone~~ [x](https://x.ai) snake_case_name");
        assert_eq!(s[0], MdSpan::Bold("bold".into()));
        assert_eq!(s[1], MdSpan::Text(" and ".into()));
        assert_eq!(s[2], MdSpan::Italic("it".into()));
        assert_eq!(s[4], MdSpan::Code("co*de".into()));
        assert_eq!(s[6], MdSpan::Strike("gone".into()));
        assert_eq!(
            s[8],
            MdSpan::Link {
                text: "x".into(),
                url: "https://x.ai".into()
            }
        );
        assert_eq!(s[9], MdSpan::Text(" snake_case_name".into()));
    }

    #[test]
    fn unclosed_markers_stay_text() {
        assert_eq!(md_spans("a ** b"), vec![MdSpan::Text("a ** b".into())]);
        assert_eq!(md_spans("2 * 3 * 4"), vec![MdSpan::Text("2 * 3 * 4".into())]);
        assert_eq!(md_spans("`open"), vec![MdSpan::Text("`open".into())]);
        assert_eq!(md_spans(""), Vec::<MdSpan>::new());
        assert_eq!(md_plain("é **ü** ✓"), "é ü ✓");
    }

    #[test]
    fn only_web_links_are_links() {
        assert!(md_link_ok("https://github.com/x"));
        assert!(!md_link_ok("javascript:alert(1)"));
        assert!(!md_link_ok("file:///etc/passwd"));
        assert!(!md_link_ok("https://a b"));
        assert!(!md_link_ok("https://"));
        let s = md_spans("[bad](file:///etc/passwd)");
        assert!(s.iter().all(|x| !matches!(x, MdSpan::Link { .. })), "{s:?}");
    }

    #[test]
    fn bare_urls_drop_trailing_punctuation() {
        let s = md_spans("See https://x.ai/cli. Or (https://github.com/a_(b)).");
        let links: Vec<_> = s
            .iter()
            .filter_map(|x| match x {
                MdSpan::Link { url, .. } => Some(url.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(links, vec!["https://x.ai/cli", "https://github.com/a_(b)"]);
    }

    #[test]
    fn code_tokens_colour_keywords_strings_comments_numbers() {
        let t = code_tokens("rust", "let x = \"a // b\"; // note 42");
        assert_eq!(t[0], (CodeTok::Keyword, "let".into()));
        assert!(t.contains(&(CodeTok::Str, "\"a // b\"".into())), "{t:?}");
        assert_eq!(t.last().unwrap(), &(CodeTok::Comment, "// note 42".into()));
        let py = code_tokens("python", "x = 10  # ten");
        assert!(py.contains(&(CodeTok::Number, "10".into())), "{py:?}");
        assert_eq!(py.last().unwrap(), &(CodeTok::Comment, "# ten".into()));
        let sh = code_tokens("sh", "echo $#");
        assert!(sh.iter().all(|(k, _)| *k != CodeTok::Comment), "{sh:?}");
        let life = code_tokens("rust", "fn f<'a>(x: &'a str)");
        assert!(life.iter().all(|(k, _)| *k != CodeTok::Str), "{life:?}");
        let joined: String = code_tokens("js", "const a1 = b2 + 3;")
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        assert_eq!(joined, "const a1 = b2 + 3;");
        assert_eq!(
            code_tokens("diff", "+ let x"),
            vec![(CodeTok::Plain, "+ let x".into())]
        );
    }

    #[test]
    fn unterminated_string_runs_to_end_of_line() {
        let t = code_tokens("js", "let s = 'open");
        assert_eq!(t.last().unwrap(), &(CodeTok::Str, "'open".into()));
    }
}
