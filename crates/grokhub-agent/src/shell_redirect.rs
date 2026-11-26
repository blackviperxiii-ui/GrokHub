//! A shell command's output redirections, read for the file each writes.
//!
//! `cargo test 2>&1 | tail -20` and `npm test > /dev/null 2>&1` write no
//! file: a descriptor copy or close (`2>&1`, `>&2`, `*>&1`, `>&-`) and a
//! sink (`/dev/null`, `NUL`, PowerShell's `$null`) only move or drop output.
//! `> out.txt`, `>> log`, `&> all.log`, `2> err.txt`, `*> all.txt` and
//! `>&out.txt` write the file they name. Text in quotes, a comment and a
//! here-document's body are data, and `>(…)` is a process substitution, not
//! a redirection. A pipe into `Out-Null` drops output too, but it is a
//! command, not a redirection, so it is not read here.
//!
//! Only the text is read: what a program writes on its own (`sed -i`, `cp`,
//! a script) is the caller's to judge. [`code`] gives the same text with
//! that data blanked, so a caller can read the commands themselves.

/// One output redirection in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Redirect {
    /// The byte where it starts: its descriptor (`2`, `&`, `*`) or its `>`.
    pub at: usize,
    /// The file it writes, as written; `None` when it writes no file.
    pub file: Option<String>,
}

/// Targets that keep no output, compared lowercased.
const SINKS: &[&str] = &[
    "/dev/null",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "nul",
    "$null",
];

/// Every output redirection in `cmd`, in text order.
pub(crate) fn redirects(cmd: &str) -> Vec<Redirect> {
    scan(cmd).0
}

/// `cmd` with its data blanked, byte for byte, so an offset in it is the
/// same offset in `cmd`: quoted text becomes `_` (still one word), and a
/// comment, a here-document (its `<<WORD` and its body), an output
/// redirection (its operator and target) and a line continuation become
/// spaces. What is left is the commands, their separators and their plain
/// words: `git commit -m "fix test" && cargo test 2>&1 | tail` reads
/// `git commit -m __________ && cargo test      | tail`.
pub(crate) fn code(cmd: &str) -> String {
    scan(cmd).1
}

/// The redirections in `cmd` and its [`code`].
fn scan(cmd: &str) -> (Vec<Redirect>, String) {
    let chars: Vec<(usize, char)> = cmd.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let byte = |i: usize| chars.get(i).map_or(cmd.len(), |&(b, _)| b);
    let mut code = cmd.as_bytes().to_vec();
    let mut out = Vec::new();
    // Here-documents opened on this line: their bodies start at its end.
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    let mut word_start = true;
    let mut i = 0;
    while let Some(c) = at(i) {
        match c {
            '\'' | '"' => {
                let end = past_quote(&chars, i);
                blank(&mut code, byte(i), byte(end), b'_');
                i = end;
                word_start = false;
            }
            '\\' => {
                // `\` at a line end joins the next line to this command.
                if at(i + 1) == Some('\n') {
                    code[byte(i)..byte(i + 2)].fill(b' ');
                }
                i += 2;
                word_start = false;
            }
            '#' if word_start => {
                let from = i;
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
                blank(&mut code, byte(from), byte(i), b' ');
            }
            '\n' => {
                i += 1;
                for (delim, tabs) in heredocs.drain(..) {
                    let from = i;
                    i = past_heredoc(cmd, &chars, i, &delim, tabs);
                    blank(&mut code, byte(from), byte(i), b' ');
                }
                word_start = true;
            }
            '<' => {
                let from = i;
                let run = (i..).take_while(|&k| at(k) == Some('<')).count();
                i += run;
                if run == 2 {
                    let tabs = at(i) == Some('-');
                    if tabs {
                        i += 1;
                    }
                    let (delim, end) = word(&chars, skip_blanks(&chars, i));
                    // `$((1<<2))` shifts; a here-document names a word.
                    if !delim.is_empty() && !delim.bytes().all(|b| b.is_ascii_digit()) {
                        heredocs.push((delim, tabs));
                        blank(&mut code, byte(from), byte(end), b' ');
                    }
                    i = end;
                }
                word_start = true;
            }
            '>' => {
                let start = descriptor_start(&chars, i);
                let mut j = i + 1;
                if at(j) == Some('(') {
                    // `>(…)`: a command reads the output, inside the parens.
                    i = j;
                    word_start = true;
                    continue;
                }
                if matches!(at(j), Some('>' | '|')) {
                    j += 1;
                }
                let copy = at(j) == Some('&');
                if copy {
                    j += 1;
                }
                let (target, end) = word(&chars, skip_blanks(&chars, j));
                blank(&mut code, byte(start), byte(end), b' ');
                i = end;
                word_start = false;
                if target.is_empty() {
                    continue;
                }
                let lower = target.to_ascii_lowercase();
                let moved = copy && (target == "-" || target.bytes().all(|b| b.is_ascii_digit()));
                out.push(Redirect {
                    at: chars[start].0,
                    file: (!moved && !SINKS.contains(&lower.as_str())).then_some(target),
                });
            }
            c if c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')') => {
                i += 1;
                word_start = true;
            }
            _ => {
                i += 1;
                word_start = false;
            }
        }
    }
    // Only whole characters were blanked, with ASCII, so this is still UTF-8.
    let code = String::from_utf8(code).unwrap_or_default();
    (out, code)
}

/// Sets `code[from..to]` to `fill`. A blank keeps line ends, so the lines
/// stay apart; a quote's `_` doesn't, so a quoted line break stays inside
/// its word.
fn blank(code: &mut [u8], from: usize, to: usize, fill: u8) {
    for b in &mut code[from..to] {
        if fill == b'_' || *b != b'\n' {
            *b = fill;
        }
    }
}

/// Where the redirection at the `>` at `i` starts: a descriptor written
/// bare at the start of a word (`2>`, `&>`, `*>`) belongs to it.
fn descriptor_start(chars: &[(usize, char)], i: usize) -> usize {
    let mut k = i;
    while k > 0 && chars[k - 1].1.is_ascii_digit() {
        k -= 1;
    }
    if k == i && k > 0 && matches!(chars[k - 1].1, '&' | '*') {
        k -= 1;
    }
    let bare = k == 0
        || chars[k - 1].1.is_whitespace()
        || matches!(chars[k - 1].1, ';' | '|' | '&' | '(' | ')');
    if bare {
        k
    } else {
        i
    }
}

fn skip_blanks(chars: &[(usize, char)], mut i: usize) -> usize {
    while chars.get(i).is_some_and(|&(_, c)| c == ' ' || c == '\t') {
        i += 1;
    }
    i
}

/// Past the quoted text that opens at `i` (a backslash or, in PowerShell,
/// a backtick escapes a double quote).
fn past_quote(chars: &[(usize, char)], i: usize) -> usize {
    let quote = chars[i].1;
    let mut k = i + 1;
    while let Some(&(_, c)) = chars.get(k) {
        if c == quote {
            return k + 1;
        }
        k += if quote == '"' && matches!(c, '\\' | '`') {
            2
        } else {
            1
        };
    }
    k
}

/// The word at `i`, quotes removed, and where it ends. A backslash stays
/// with the character after it, so a Windows path keeps its separators.
fn word(chars: &[(usize, char)], mut i: usize) -> (String, usize) {
    let mut text = String::new();
    while let Some(&(_, c)) = chars.get(i) {
        if c.is_whitespace() || matches!(c, ';' | '|' | '&' | '<' | '>' | '(' | ')') {
            break;
        }
        if c == '\'' || c == '"' {
            let end = past_quote(chars, i);
            let inner = chars[i + 1..end.min(chars.len())]
                .iter()
                .map(|&(_, c)| c)
                .collect::<String>();
            text.push_str(inner.strip_suffix(c).unwrap_or(&inner));
            i = end;
            continue;
        }
        text.push(c);
        if c == '\\' {
            if let Some(&(_, next)) = chars.get(i + 1) {
                text.push(next);
                i += 1;
            }
        }
        i += 1;
    }
    (text, i)
}

/// Past a here-document body that starts at `i` and ends at a line holding
/// only `delim` (after leading tabs, for `<<-`).
fn past_heredoc(
    cmd: &str,
    chars: &[(usize, char)],
    mut i: usize,
    delim: &str,
    tabs: bool,
) -> usize {
    while i < chars.len() {
        let end = (i..chars.len())
            .find(|&k| chars[k].1 == '\n')
            .unwrap_or(chars.len());
        let line = &cmd[chars[i].0..chars.get(end).map_or(cmd.len(), |&(b, _)| b)];
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = if tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        i = end + 1;
        if line == delim {
            break;
        }
    }
    i.min(chars.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(cmd: &str) -> Vec<Option<String>> {
        redirects(cmd).into_iter().map(|r| r.file).collect()
    }

    fn file(name: &str) -> Option<String> {
        Some(name.to_string())
    }

    #[test]
    fn a_descriptor_copy_or_a_sink_writes_no_file() {
        assert_eq!(
            redirects("cargo test 2>&1 | tail -20"),
            [Redirect { at: 11, file: None }]
        );
        assert_eq!(files("npm test > /dev/null 2>&1"), [None, None]);
        assert_eq!(files("make >&2; make >&-"), [None, None]);
        // PowerShell and cmd sinks, in any case.
        assert_eq!(
            files("cargo build *>$null; Invoke-Pester 2>$NULL; dir >$null 3>&1; dir > NUL"),
            [None, None, None, None, None]
        );
        // A pipe into Out-Null is a command, not a redirection.
        assert_eq!(files("cargo build | Out-Null"), []);
    }

    #[test]
    fn a_redirect_to_a_file_names_it_and_where_it_starts() {
        assert_eq!(
            redirects("echo hi > out.txt"),
            [Redirect {
                at: 8,
                file: file("out.txt")
            }]
        );
        assert_eq!(
            files("a >> log.txt; b &> all.log; c 2> err.txt; d *> ps.txt; e >&both.txt; f >| x"),
            [
                file("log.txt"),
                file("all.log"),
                file("err.txt"),
                file("ps.txt"),
                file("both.txt"),
                file("x"),
            ]
        );
        assert_eq!(redirects("cargo test 2>err.txt")[0].at, 11);
        assert_eq!(redirects("cargo test &>all.log")[0].at, 11);
        // Digits inside a word are the word's, not a descriptor.
        assert_eq!(redirects("echo a2>f")[0].at, 7);
        assert_eq!(files("cargo test 2>&1 >out.txt"), [None, file("out.txt")]);
        assert_eq!(
            files(r#"echo hi > "my notes.txt"; dir > C:\temp\out.txt"#),
            [file("my notes.txt"), file(r"C:\temp\out.txt")]
        );
        // A redirect inside a process substitution still writes.
        assert_eq!(files("tee >(gzip > x.gz)"), [file("x.gz")]);
    }

    #[test]
    fn the_code_view_blanks_data_and_keeps_every_offset() {
        assert_eq!(
            code(r#"git commit -m "fix test" && cargo test 2>&1 | tail"#),
            "git commit -m __________ && cargo test      | tail"
        );
        let script = "cat > a.md <<'EOF'\nx > y\nEOF\nls";
        assert_eq!(
            code(script),
            format!(
                "cat{}\n{}\n{}\nls",
                " ".repeat(15),
                " ".repeat(5),
                " ".repeat(3)
            )
        );
        assert_eq!(code("ls # cargo test"), "ls             ");
        assert_eq!(
            code("cargo test \\\n  --workspace"),
            "cargo test     --workspace"
        );
        // A quoted line break stays in its word; a multi-byte character keeps its bytes.
        assert_eq!(code("echo 'a\nb'; ls"), "echo _____; ls");
        let wide = r#"echo "héllo" > x"#;
        assert_eq!(code(wide), "echo ________    ");
        assert_eq!(code(wide).len(), wide.len());
    }

    #[test]
    fn quotes_comments_and_here_document_bodies_are_data() {
        assert_eq!(files(r#"echo "a > b" 'c > d'"#), []);
        assert_eq!(files(r#"git commit -m "fix a -> b \" > c""#), []);
        assert_eq!(files("echo hi # > not.txt"), []);
        assert_eq!(files("echo $# > n.txt"), [file("n.txt")]);
        let script = "cat > src/main.rs <<'EOF'\nfn main() -> i32 { 0 }\nEOF\necho done > d.txt";
        assert_eq!(files(script), [file("src/main.rs"), file("d.txt")]);
        let tabs = "cat <<-END > a.txt\n\tx > y\n\tEND\necho > b.txt";
        assert_eq!(files(tabs), [file("a.txt"), file("b.txt")]);
        assert_eq!(files(r#"cat <<< "x > y""#), []);
        assert_eq!(files("echo $((1<<2)) > n.txt"), [file("n.txt")]);
    }
}
