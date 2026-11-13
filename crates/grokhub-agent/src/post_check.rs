//! Deterministic checks before a turn ends (round 2 item 24).
//!
//! A reply can say the tests pass, that it committed or pushed, that it
//! changed a file, or give a total, while this run's own tool results say
//! otherwise. Agents catch loud errors far more often than plausible wrong
//! values, and a "check your results" prompt line does not help (arXiv
//! 2610.10062), so these checks run in code (arXiv 2608.02464):
//!
//! - totals: a table's Total row and an inline `a + b = c` are added up again;
//! - counts: "N passed" and "N files changed" are read against the last test
//!   run's summary and git's own line;
//! - coverage: "the tests pass", "committed", "pushed" and "updated `path`"
//!   each need a call this run that did it;
//! - results: "verified", "gone" or "empty" and "N GB freed" are read against
//!   what the commands printed: the last command's error and its `KEY=VALUE`
//!   facts (`EXISTS=True`, `COUNT=3`), and every `FREED=` this run. Like a
//!   tool receipt (arXiv 2603.10060), the facts come from the output, never
//!   from the model's words about it.
//!
//! A finding sends the model one note naming what doesn't match. It never
//! blocks the turn and never edits the reply.

/// How much of one tool result is kept: the head and the tail, where the
/// command line and the test summary sit.
const HEAD_KEEP: usize = 2_000;
const TAIL_KEEP: usize = 14_000;

/// One tool call this run, as the checks see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub name: String,
    /// The shell command, for `run_terminal_command`; empty otherwise.
    pub command: String,
    pub args: String,
    pub ok: bool,
    pub output: String,
    /// Pages this call opened or listed (LO-51): web_search results, the
    /// page web_fetch read, the page a browser went to.
    pub urls: Vec<String>,
}

/// Notes one run can get from these checks: one per new round of calls, so a
/// reply can't loop on them.
pub const NOTES_PER_RUN: usize = 3;

/// Calls kept for one run; the oldest go first.
const RAN_CAP: usize = 400;

/// Notes one finished call for the end-of-turn checks. App-side session tools
/// (todos and the like) are not work, so they are left out.
pub(crate) fn note_ran(
    ran: &mut Vec<Ran>,
    call: &crate::client::FunctionCall,
    output: &crate::tools::ToolOutput,
) {
    if crate::drive::is_session_tool(&call.name) {
        return;
    }
    if ran.len() >= RAN_CAP {
        ran.remove(0);
    }
    ran.push(Ran::new(
        &call.name,
        &call.arguments,
        !output.failed,
        &output.text,
    ));
}

impl Ran {
    pub fn new(name: &str, arguments: &str, ok: bool, output: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let command = if name == "run_terminal_command" {
            value
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            String::new()
        };
        let urls = if !ok {
            Vec::new()
        } else if name == "web_search" {
            urls_in(output)
        } else if name == "web_fetch" || name.contains("navigate") {
            value
                .get("url")
                .and_then(serde_json::Value::as_str)
                .map(|u| vec![u.trim().to_string()])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        // Only a command's output is read (test summaries, git's lines).
        let output = if command.is_empty() {
            String::new()
        } else {
            head_and_tail(output)
        };
        Self {
            name: name.to_string(),
            command,
            args: arguments.chars().take(HEAD_KEEP).collect(),
            ok,
            output,
            urls,
        }
    }

    /// Work the checks can't see into: a subagent, or a command left running
    /// in the background.
    fn opaque(&self) -> bool {
        self.name == "spawn_subagent"
            || self.name == "get_command_or_subagent_output"
            || (self.name == "run_terminal_command"
                && self
                    .args
                    .split_whitespace()
                    .collect::<String>()
                    .contains("\"is_background\":true"))
    }
}

fn head_and_tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= HEAD_KEEP + TAIL_KEEP {
        return text.to_string();
    }
    let head: String = text.chars().take(HEAD_KEEP).collect();
    let tail: String = text.chars().skip(count - TAIL_KEEP).collect();
    format!("{head}\n…\n{tail}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: &'static str,
    pub detail: String,
}

pub const TABLE_TOTAL: &str = "table_total";
pub const INLINE_SUM: &str = "inline_sum";
pub const TESTS_FAILED: &str = "tests_failed";
pub const TESTS_NOT_RUN: &str = "tests_not_run";
pub const TEST_COUNT: &str = "test_count";
pub const FILES_CHANGED: &str = "files_changed";
pub const NOT_COMMITTED: &str = "not_committed";
pub const NOT_PUSHED: &str = "not_pushed";
pub const FILE_NOT_WRITTEN: &str = "file_not_written";
pub const CHECK_FAILED: &str = "check_failed";
pub const STILL_THERE: &str = "still_there";
pub const FREED_MISMATCH: &str = "freed_mismatch";
pub const UNCHECKED_CONNECTION: &str = "unchecked_connection";
pub const PUSHED_TEST: &str = "pushed_test";
pub const UNOPENED_LINK: &str = "unopened_link";

/// Every check for a reply that ends a run.
pub fn check(reply: &str, ran: &[Ran]) -> Vec<Finding> {
    let mut out = arithmetic(reply);
    out.extend(counts(reply, ran));
    if !ran.is_empty() && !ran.iter().any(Ran::opaque) {
        out.extend(coverage(reply, ran));
    }
    out.extend(results(reply, ran));
    out
}

/// The turn found nothing for the ask: every search or page this run tried
/// failed or listed nothing, every call failed, or the reply says it came up
/// empty. The next turn is told, so "thanks" isn't taken for a result.
pub fn came_up_empty(reply: &str, ran: &[Ran]) -> bool {
    let lookup =
        |r: &&Ran| r.name == "web_search" || r.name == "web_fetch" || r.name.contains("navigate");
    let mut lookups = ran.iter().filter(lookup).peekable();
    let lookups_empty = lookups.peek().is_some() && lookups.all(|r| !r.ok || r.urls.is_empty());
    let all_failed = !ran.is_empty() && ran.iter().all(|r| !r.ok);
    lookups_empty || all_failed || grokhub_core::conversation::admits_empty(reply)
}

/// The note the model gets for `findings`; `None` when there are none.
pub fn note(findings: &[Finding]) -> Option<String> {
    if findings.is_empty() {
        return None;
    }
    let mut text =
        String::from("Before this turn ends: your reply doesn't match this run's own results.\n");
    for f in findings {
        text.push_str(&format!("- {}\n", f.detail));
    }
    text.push_str(
        "Check each one. If the work isn't done, finish it. If the reply was wrong, say what is actually true.",
    );
    Some(text)
}

// --- Sources -------------------------------------------------------------

/// Links named in one note, at most.
const LINKS_NOTED: usize = 3;

/// LO-51: every link in the reply is a page this run opened or listed (a
/// web_search result, a web_fetch, a browser visit, a hosted search
/// citation) or one the person gave. A link from memory, or from an earlier
/// turn's half-finished lookup, reads as checked when it isn't.
pub fn sources(reply: &str, ran: &[Ran], cited: &[String], asked: &str) -> Vec<Finding> {
    let seen: Vec<String> = ran
        .iter()
        .flat_map(|r| r.urls.iter())
        .chain(cited)
        .chain(urls_in(asked).iter())
        .map(|u| link_key(u))
        .collect();
    let mut unopened: Vec<String> = Vec::new();
    for url in urls_in(reply) {
        let key = link_key(&url);
        let opened = seen
            .iter()
            .any(|s| *s == key || (s.contains('/') && key.starts_with(&format!("{s}/"))));
        if !opened && !unopened.contains(&url) {
            unopened.push(url);
        }
    }
    if unopened.is_empty() {
        return Vec::new();
    }
    let shown: Vec<&str> = unopened
        .iter()
        .take(LINKS_NOTED)
        .map(String::as_str)
        .collect();
    let more = unopened.len().saturating_sub(LINKS_NOTED);
    vec![Finding {
        kind: UNOPENED_LINK,
        detail: format!(
            "the reply links {}{}, which no tool opened this turn. web_fetch the page, drop the link, or say that part is from memory, not checked.",
            shown.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ),
    }]
}

/// The http(s) links in `text`, without trailing punctuation.
pub(crate) fn urls_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("http") {
        let tail = &rest[at..];
        if !(tail.starts_with("https://") || tail.starts_with("http://")) {
            rest = &rest[at + 4..];
            continue;
        }
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`' | '|'))
            .unwrap_or(tail.len());
        let url = tail[..end]
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '*', '_']);
        if url.len() > "https://".len() && !out.iter().any(|u| u == url) {
            out.push(url.to_string());
        }
        rest = &tail[end..];
    }
    out
}

/// A link as it is compared: no scheme, no `www.`, no fragment, no trailing
/// slash, lower-case host.
fn link_key(url: &str) -> String {
    let url = url.trim();
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let url = url.strip_prefix("www.").unwrap_or(url);
    let url = url.split('#').next().unwrap_or(url).trim_end_matches('/');
    match url.split_once('/') {
        Some((host, path)) => format!("{}/{path}", host.to_ascii_lowercase()),
        None => url.to_ascii_lowercase(),
    }
}

// --- Totals --------------------------------------------------------------

/// Totals the reply states and can be added up from the reply alone.
pub fn arithmetic(reply: &str) -> Vec<Finding> {
    let mut out = table_totals(reply);
    out.extend(inline_sums(reply));
    out
}

/// A number as written, with how many decimals it shows.
fn number(cell: &str) -> Option<(f64, u32)> {
    let mut t = cell.trim();
    for wrap in ["**", "__", "`", "*", "_"] {
        t = t.trim_start_matches(wrap).trim_end_matches(wrap).trim();
    }
    let negative = t.starts_with('-') || t.starts_with('−');
    let t = t.trim_start_matches(['-', '−', '+']);
    let t = t.trim_start_matches(['$', '€', '£', '¥']).trim();
    if t.is_empty() || !t.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let mut digits = String::new();
    let chars: Vec<char> = t.chars().collect();
    let mut decimals = 0u32;
    let mut seen_point = false;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '0'..='9' => {
                digits.push(c);
                if seen_point {
                    decimals += 1;
                }
            }
            '.' if !seen_point => {
                seen_point = true;
                digits.push('.');
            }
            // A thousands comma: exactly three digits follow before the next
            // non-digit.
            ',' if !seen_point => {
                let run = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
                if run != 3 {
                    return None;
                }
            }
            _ => return None,
        }
    }
    let value: f64 = digits.parse().ok()?;
    Some((if negative { -value } else { value }, decimals))
}

fn close_enough(stated: f64, sum: f64, decimals: u32, terms: usize) -> bool {
    let unit = 10f64.powi(-(decimals as i32));
    (stated - sum).abs() <= unit * 0.5 * terms.max(1) as f64 + 1e-9
}

fn show(value: f64, decimals: u32) -> String {
    format!("{value:.prec$}", prec = decimals as usize)
}

fn is_total_label(cell: &str) -> bool {
    let label = cell
        .trim()
        .trim_matches(|c| c == '*' || c == '_' || c == ':')
        .trim()
        .to_lowercase();
    matches!(
        label.as_str(),
        "total" | "totals" | "sum" | "overall" | "grand total" | "all"
    )
}

/// Columns that a Total row doesn't sum.
fn not_summed(header: &str) -> bool {
    let h = header.to_lowercase();
    [
        "avg", "average", "mean", "median", "rate", "%", "percent", "per ", "/", "ratio", "max",
        "min", "rank", "#", "id", "year",
    ]
    .iter()
    .any(|w| h.contains(w))
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn table_totals(reply: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let lines: Vec<&str> = reply.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if !(lines[i].trim_start().starts_with('|')
            && lines.get(i + 1).is_some_and(|l| is_separator(l)))
        {
            i += 1;
            continue;
        }
        let header = cells(lines[i]);
        let mut rows = Vec::new();
        let mut j = i + 2;
        while j < lines.len() && lines[j].trim_start().starts_with('|') {
            rows.push(cells(lines[j]));
            j += 1;
        }
        i = j;
        let totals: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.first().is_some_and(|c| is_total_label(c)))
            .map(|(k, _)| k)
            .collect();
        // One Total row, and it closes the table; subtotals are left alone.
        let [at] = totals[..] else { continue };
        if at + 1 != rows.len() || at < 2 {
            continue;
        }
        for (col, name) in header.iter().enumerate().skip(1) {
            if not_summed(name)
                || rows
                    .iter()
                    .any(|r| r.get(col).is_some_and(|c| c.contains('%')))
            {
                continue;
            }
            let Some((stated, stated_dec)) = rows[at].get(col).and_then(|c| number(c)) else {
                continue;
            };
            let parts: Option<Vec<(f64, u32)>> = rows[..at]
                .iter()
                .map(|r| r.get(col).and_then(|c| number(c)))
                .collect();
            let Some(parts) = parts else { continue };
            let sum: f64 = parts.iter().map(|p| p.0).sum();
            let decimals = parts.iter().map(|p| p.1).max().unwrap_or(0).max(stated_dec);
            if !close_enough(stated, sum, decimals, parts.len()) {
                let label = if name.is_empty() {
                    "the"
                } else {
                    name.as_str()
                };
                out.push(Finding {
                    kind: TABLE_TOTAL,
                    detail: format!(
                        "Your table's {label} total says {}, but its rows add up to {}.",
                        show(stated, decimals),
                        show(sum, decimals)
                    ),
                });
            }
        }
    }
    out
}

/// `a + b - c = d` written out in the reply, with plain numbers only.
fn inline_sums(reply: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in reply.lines() {
        let chars: Vec<char> = line.chars().collect();
        for (eq, _) in chars.iter().enumerate().filter(|(_, c)| **c == '=') {
            // `==`, `<=`, `>=`, `!=` are code, not sums.
            if chars.get(eq + 1) == Some(&'=')
                || eq > 0 && matches!(chars[eq - 1], '=' | '<' | '>' | '!')
            {
                continue;
            }
            let mut start = eq;
            while start > 0 && is_expr_char(chars[start - 1]) {
                start -= 1;
            }
            while start < eq && chars[start] == ' ' {
                start += 1;
            }
            // The expression has to start the line or follow a space or
            // bracket: `x3+4=7` is code.
            if start > 0 && !matches!(chars[start - 1], ' ' | '(' | ':' | '\t') {
                continue;
            }
            let left: String = chars[start..eq].iter().collect();
            let right: String = chars[eq + 1..]
                .iter()
                .skip_while(|c| **c == ' ')
                .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '$' | '-' | '−'))
                .collect();
            let right = right.trim_end_matches(['.', ',']);
            let Some((stated, stated_dec)) = number(right) else {
                continue;
            };
            let Some((sum, terms, decimals)) = evaluate(&left) else {
                continue;
            };
            let decimals = decimals.max(stated_dec);
            // The terms are written out exactly, so only the result's own
            // rounding is allowed: 2 + 2 = 5 is wrong, not rounded.
            if terms >= 2 && !close_enough(stated, sum, decimals, 1) {
                out.push(Finding {
                    kind: INLINE_SUM,
                    detail: format!(
                        "Your reply says {} = {}, but that comes to {}.",
                        left.trim(),
                        right,
                        show(sum, decimals)
                    ),
                });
            }
        }
    }
    out
}

fn is_expr_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, ' ' | '.' | ',' | '+' | '-' | '−' | '*' | '×' | '$')
}

/// `+`, `-`, `*` and `×` over plain numbers; `None` unless it has an operator
/// between two numbers.
fn evaluate(expr: &str) -> Option<(f64, usize, u32)> {
    let expr = expr.trim();
    let mut terms: Vec<(char, String)> = Vec::new();
    let mut current = String::new();
    let mut op = '+';
    for c in expr.chars() {
        match c {
            '+' | '−' | '*' | '×' => {
                terms.push((op, std::mem::take(&mut current)));
                op = if c == '−' { '-' } else { c };
            }
            '-' if !current.trim().is_empty() => {
                terms.push((op, std::mem::take(&mut current)));
                op = '-';
            }
            _ => current.push(c),
        }
    }
    terms.push((op, current));
    if terms.len() < 2 {
        return None;
    }
    let mut sum = 0f64;
    let mut product: Option<f64> = None;
    let mut decimals = 0u32;
    let mut sign = 1f64;
    for (op, raw) in &terms {
        let (value, dec) = number(raw)?;
        decimals = decimals.max(dec);
        match op {
            '*' | '×' => product = Some(product? * value),
            _ => {
                if let Some(p) = product.take() {
                    sum += sign * p;
                }
                sign = if *op == '-' { -1.0 } else { 1.0 };
                product = Some(value);
            }
        }
    }
    if let Some(p) = product {
        sum += sign * p;
    }
    Some((sum, terms.len(), decimals))
}

// --- Counts --------------------------------------------------------------

fn is_test_command(command: &str) -> bool {
    let c = command.to_lowercase();
    [
        "cargo test",
        "cargo nextest",
        "npm test",
        "npm run test",
        "pnpm test",
        "pnpm run test",
        "yarn test",
        "bun test",
        "pytest",
        "go test",
        "jest",
        "vitest",
        "dotnet test",
        "mvn test",
        "gradle test",
        "gradlew test",
        "ctest",
        "make test",
        "phpunit",
        "rspec",
        "mix test",
        "deno test",
        "flutter test",
        "unittest",
    ]
    .iter()
    .any(|t| c.contains(t))
}

/// What a test run's own summary says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Summary {
    /// Each summary line's passed count (one per crate for cargo).
    passed: Vec<u64>,
    failed: u64,
}

/// `<number> <word>` pairs on one line, e.g. `12 passed; 1 failed`.
/// The first `word` with a number before it, so cargo's `FAILED.` heading
/// doesn't hide its `1 failed`.
fn counted(line: &str, word: &str) -> Option<u64> {
    let lower = line.to_lowercase();
    lower.match_indices(word).find_map(|(at, _)| {
        let digits: String = lower[..at]
            .trim_end()
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        digits.parse().ok()
    })
}

fn summary(output: &str) -> Summary {
    let cargo: Vec<&str> = output
        .lines()
        .filter(|l| l.trim_start().starts_with("test result:"))
        .collect();
    let lines: Vec<&str> = if cargo.is_empty() {
        output
            .lines()
            .rev()
            .find(|l| {
                let l = l.to_lowercase();
                counted(&l, "passed").is_some() || counted(&l, "failed").is_some()
            })
            .into_iter()
            .collect()
    } else {
        cargo
    };
    let mut out = Summary::default();
    for line in lines {
        if let Some(p) = counted(line, "passed") {
            out.passed.push(p);
        }
        out.failed += counted(line, "failed").unwrap_or(0);
    }
    // Go prints no counts, only one `--- FAIL` per failing test.
    if out.passed.is_empty() && out.failed == 0 {
        out.failed = output.matches("--- FAIL").count() as u64;
    }
    out
}

fn sentences(reply: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_code = false;
    let mut chars = reply.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if c == '`' {
            in_code = !in_code;
        }
        // `lib.rs` and `1.5` are not sentence ends.
        let ends = c == '\n'
            || !in_code
                && matches!(c, '.' | '!' | '?')
                && chars.peek().is_none_or(|n| n.is_whitespace());
        if ends {
            in_code = false;
            let s = current.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// A sentence that only might be true, or talks about another time.
fn hedged(sentence: &str) -> bool {
    let s = format!(" {} ", sentence.to_lowercase());
    s.trim_end().ends_with('?')
        || [
            " not ",
            "n't ",
            " never ",
            " if ",
            " once ",
            " should ",
            " will ",
            "'ll ",
            " would ",
            " could ",
            " can ",
            " until ",
            " unless ",
            " might ",
            " may ",
            " earlier",
            " before",
            " previous",
            " already ",
            " last time",
            " yesterday",
            " ready to ",
            " want ",
            " need to ",
            " yet",
            " try ",
            " let me ",
            " going to ",
            " when ",
        ]
        .iter()
        .any(|w| s.contains(w))
}

fn words(sentence: &str) -> Vec<String> {
    sentence
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn says_tests_pass(sentence: &str) -> bool {
    let w = words(sentence);
    let about_tests = w
        .iter()
        .any(|w| w.starts_with("test") || w == "suite" || w == "ci");
    let pass = w.iter().any(|w| {
        matches!(
            w.as_str(),
            "pass" | "passes" | "passed" | "passing" | "green"
        )
    });
    let fail = w
        .iter()
        .any(|w| w.starts_with("fail") || w == "red" || w == "broken");
    about_tests && pass && !fail && !hedged(sentence)
}

/// The number in "N passed" / "N tests pass" / "all N tests pass".
fn stated_passed(sentence: &str) -> Option<u64> {
    let w = words(sentence);
    for (i, word) in w.iter().enumerate() {
        let Ok(n) = word.parse::<u64>() else { continue };
        let next = w.get(i + 1).map(String::as_str);
        let after = w.get(i + 2).map(String::as_str);
        let passes = |x: Option<&str>| matches!(x, Some("pass" | "passed" | "passing" | "passes"));
        if passes(next) || (next.is_some_and(|x| x.starts_with("test")) && passes(after)) {
            return Some(n);
        }
    }
    None
}

fn counts(reply: &str, ran: &[Ran]) -> Vec<Finding> {
    let mut out = Vec::new();
    let last_test = ran.iter().rev().find(|r| is_test_command(&r.command));
    for sentence in sentences(reply) {
        if hedged(&sentence) {
            continue;
        }
        if let (Some(n), Some(test)) = (stated_passed(&sentence), last_test) {
            let s = summary(&test.output);
            let total: u64 = s.passed.iter().sum();
            if !s.passed.is_empty() && total != n && !s.passed.contains(&n) {
                out.push(Finding {
                    kind: TEST_COUNT,
                    detail: format!(
                        "Your reply says {n} passed, but the last test run (`{}`) reports {total} passed.",
                        clip(&test.command)
                    ),
                });
            }
        }
        if let Some(n) = stated_count(&sentence, "files changed")
            .or_else(|| stated_count(&sentence, "file changed"))
        {
            let seen: Vec<u64> = ran
                .iter()
                .filter(|r| r.command.contains("git"))
                .flat_map(|r| {
                    r.output
                        .lines()
                        .filter_map(|l| counted(l, "file"))
                        .collect::<Vec<_>>()
                })
                .collect();
            if !seen.is_empty() && !seen.contains(&n) && seen.iter().sum::<u64>() != n {
                out.push(Finding {
                    kind: FILES_CHANGED,
                    detail: format!(
                        "Your reply says {n} files changed, but git reported {}.",
                        seen.iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                });
            }
        }
    }
    out
}

fn stated_count(sentence: &str, phrase: &str) -> Option<u64> {
    let lower = sentence.to_lowercase();
    let at = lower.find(phrase)?;
    counted(&lower[..at + phrase.len()], phrase.split(' ').next()?)
}

fn clip(text: &str) -> String {
    let t: String = text.chars().take(80).collect();
    if text.chars().count() > 80 {
        format!("{t}…")
    } else {
        t
    }
}

// --- Coverage ------------------------------------------------------------

fn ran_ok(ran: &[Ran], any_of: &[&str]) -> bool {
    ran.iter()
        .filter(|r| r.ok)
        .any(|r| any_of.iter().any(|c| r.command.contains(c)))
}

fn coverage(reply: &str, ran: &[Ran]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let last_test = ran.iter().rev().find(|r| is_test_command(&r.command));
    for sentence in sentences(reply) {
        if hedged(&sentence) {
            continue;
        }
        let w = words(&sentence);
        if says_tests_pass(&sentence) {
            let finding = match last_test {
                // A script or `make check` may well be the test run; only
                // a turn with no command at all can't have tested.
                None if ran.iter().all(|r| r.command.is_empty()) => Some(Finding {
                    kind: TESTS_NOT_RUN,
                    detail: "Your reply says the tests pass, but no test ran this turn.".into(),
                }),
                None => None,
                Some(test) => {
                    let s = summary(&test.output);
                    (s.failed > 0 || !test.ok).then(|| Finding {
                        kind: TESTS_FAILED,
                        detail: if s.failed > 0 {
                            format!(
                                "Your reply says the tests pass, but the last test run (`{}`) shows {} failed.",
                                clip(&test.command),
                                s.failed
                            )
                        } else {
                            format!(
                                "Your reply says the tests pass, but the last test run (`{}`) ended with an error.",
                                clip(&test.command)
                            )
                        },
                    })
                }
            };
            if let Some(f) = finding {
                if seen.insert(f.kind) {
                    out.push(f);
                }
            }
        }
        if w.iter().any(|w| w == "committed")
            && !ran_ok(
                ran,
                &[
                    "git commit",
                    "git merge",
                    "git cherry-pick",
                    "git am",
                    "git rebase",
                    "git revert",
                ],
            )
            && seen.insert(NOT_COMMITTED)
        {
            out.push(Finding {
                kind: NOT_COMMITTED,
                detail: "Your reply says it committed, but no commit ran this turn.".into(),
            });
        }
        let pushed_code = w.iter().any(|w| w == "pushed")
            && w.iter().any(|w| {
                matches!(
                    w.as_str(),
                    "branch"
                        | "origin"
                        | "remote"
                        | "github"
                        | "commit"
                        | "commits"
                        | "changes"
                        | "fix"
                        | "to"
                )
            });
        if pushed_code && !ran_ok(ran, &["git push"]) && seen.insert(NOT_PUSHED) {
            out.push(Finding {
                kind: NOT_PUSHED,
                detail: "Your reply says it pushed, but no push ran this turn.".into(),
            });
        }
        for path in claimed_paths(&sentence) {
            if !wrote(ran, &path) && seen.insert(FILE_NOT_WRITTEN) {
                out.push(Finding {
                    kind: FILE_NOT_WRITTEN,
                    detail: format!(
                        "Your reply says you changed `{path}`, but no call this turn wrote to it."
                    ),
                });
            }
        }
    }
    out
}

const CHANGE_VERBS: [&str; 9] = [
    "created", "wrote", "saved", "added", "updated", "edited", "modified", "changed", "fixed",
];

/// Backticked paths after a change verb in one sentence.
fn claimed_paths(sentence: &str) -> Vec<String> {
    let lower = sentence.to_lowercase();
    let Some(verb_at) = CHANGE_VERBS
        .iter()
        .filter_map(|v| {
            lower
                .match_indices(v)
                .find(|(i, _)| {
                    let before = lower[..*i].chars().last();
                    let after = lower[i + v.len()..].chars().next();
                    !before.is_some_and(char::is_alphanumeric)
                        && !after.is_some_and(char::is_alphanumeric)
                })
                .map(|(i, _)| i)
        })
        .min()
    else {
        return Vec::new();
    };
    sentence[verb_at..]
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|p| looks_like_path(p))
        .map(str::to_string)
        .collect()
}

fn looks_like_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() < 200
        && !p.contains(char::is_whitespace)
        && !p.contains(['(', ')', '{', '}', '=', '<', '>', ';', '"', '\''])
        && !p.starts_with("--")
        && (p.contains('/')
            || p.rsplit_once('.').is_some_and(|(stem, ext)| {
                !stem.is_empty()
                    && (1..=5).contains(&ext.len())
                    && ext.chars().all(|c| c.is_ascii_alphanumeric())
            }))
}

/// A call this run that wrote `path`: a write tool on it, or a shell command
/// or other tool whose arguments name its file.
fn wrote(ran: &[Ran], path: &str) -> bool {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    ran.iter().filter(|r| r.ok).any(|r| {
        crate::compact::path_from_write_call(&r.name, &r.args)
            .is_some_and(|p| p.ends_with(path) || path.ends_with(p.as_str()) || p.ends_with(file))
            || (!r.command.is_empty() && r.command.contains(file))
            || (r.name != "read_file"
                && r.name != "grep"
                && r.name != "glob"
                && r.name != "list_dir"
                && r.command.is_empty()
                && crate::compact::path_from_write_call(&r.name, &r.args).is_none()
                && r.args.contains(file))
    })
}

// --- Results -------------------------------------------------------------

/// `KEY=VALUE` facts a command printed, keys upper-cased. A key is letters,
/// digits and `_`, so `$a=1` in an echoed script and `a==b` are left out.
fn facts(output: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for token in output.split(|c: char| c.is_whitespace() || c == ';' || c == ',') {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        let value = value.trim_matches(['"', '\'']);
        if key.is_empty()
            || value.is_empty()
            || value.starts_with('=')
            || !key.starts_with(|c: char| c.is_ascii_alphabetic())
            || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        out.push((key.to_ascii_uppercase(), value.to_string()));
    }
    out
}

fn truthy(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn is_existence_key(key: &str) -> bool {
    matches!(
        key,
        "EXISTS" | "EXIST" | "FOUND" | "PRESENT" | "STILL_THERE"
    ) || key.ends_with("_EXISTS")
        || key.ends_with("_FOUND")
        || key.ends_with("_PRESENT")
}

fn is_count_key(key: &str) -> bool {
    matches!(key, "COUNT" | "REMAINING" | "LEFT" | "ITEMS" | "FILES")
        || key.ends_with("_COUNT")
        || key.ends_with("_REMAINING")
        || key.ends_with("_LEFT")
}

/// The first line of an output that shows the command itself went wrong.
/// "Cannot find path" and "No such file" are left out: for a check that
/// something is gone, they are the answer.
fn error_line(output: &str) -> Option<String> {
    const MARKS: [&str; 10] = [
        "ParserError",
        "FullyQualifiedErrorId",
        "Unexpected token",
        "is not recognized as",
        "Traceback (most recent call last)",
        ": command not found",
        "Access is denied",
        "UnauthorizedAccessException",
        "Missing closing",
        "syntax error",
    ];
    output
        .lines()
        .map(str::trim)
        .find(|l| {
            MARKS.iter().any(|m| l.contains(m))
                || l.starts_with("error:")
                || l.starts_with("error[")
                || l.starts_with("Error:")
        })
        .map(clip)
}

fn says_verified(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    let w = words(sentence);
    w.iter().any(|w| {
        matches!(
            w.as_str(),
            "verified" | "confirmed" | "succeeded" | "double'checked"
        )
    }) || [
        "double-checked",
        "checked and",
        "error gone",
        "error is gone",
        "errors are gone",
        "works again",
        "working again",
        "all good",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

/// "Error gone" is about an error, not about a file, so it's read by
/// [`says_verified`] instead.
fn says_gone(sentence: &str) -> bool {
    let mut lower = sentence.to_lowercase();
    for p in ["errors are gone", "error is gone", "error gone"] {
        lower = lower.replace(p, "");
    }
    words(&lower)
        .iter()
        .any(|w| matches!(w.as_str(), "gone" | "removed" | "deleted"))
        || lower.contains("no longer exist")
}

fn says_empty(sentence: &str) -> bool {
    words(sentence)
        .iter()
        .any(|w| matches!(w.as_str(), "empty" | "emptied" | "cleared"))
}

/// "33 GB free" is how much is free now, not what was freed, so it isn't here.
const FREE_WORDS: [&str; 4] = ["freed", "reclaimed", "recovered", "cleared"];

/// Bytes in one `value unit` pair: the decimal and binary reading, since
/// "GB" is often a GiB. The check uses the lower one, so a GiB never flags.
fn bytes(value: f64, unit: &str) -> Option<(f64, f64)> {
    let u = unit.to_ascii_lowercase();
    let (dec, bin) = match u.trim_end_matches('s').trim_end_matches('b') {
        "" => return None,
        "k" | "ki" | "kilo" | "kilobyte" => (1e3, 1024f64),
        "m" | "mi" | "mega" | "megabyte" => (1e6, 1024f64.powi(2)),
        "g" | "gi" | "giga" | "gigabyte" => (1e9, 1024f64.powi(3)),
        "t" | "ti" | "tera" | "terabyte" => (1e12, 1024f64.powi(4)),
        _ => return None,
    };
    Some((value * dec, value * bin))
}

/// A size the sentence says was freed, as the low and high byte reading.
fn stated_freed(sentence: &str) -> Option<(f64, f64)> {
    let w: Vec<String> = sentence
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '(' | ')' | '*'))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_end_matches(['.', '!', ';']).to_string())
        .collect();
    if !w.iter().any(|t| FREE_WORDS.contains(&t.as_str())) {
        return None;
    }
    for i in 0..w.len() {
        // "12.8 GB" and "12.8GB".
        let (num, unit) = match w[i].find(|c: char| c.is_ascii_alphabetic()) {
            Some(at) if at > 0 => (w[i][..at].to_string(), w[i][at..].to_string()),
            _ => (w[i].clone(), w.get(i + 1).cloned().unwrap_or_default()),
        };
        let Ok(value) = num.replace(',', "").parse::<f64>() else {
            continue;
        };
        if let Some(b) = bytes(value, &unit) {
            return Some(b);
        }
    }
    None
}

/// Bytes this run's commands say they freed: `FREED=` style facts, in bytes
/// unless a unit follows the number.
fn run_freed(ran: &[Ran]) -> Option<f64> {
    let mut total = None;
    for r in ran.iter().filter(|r| !r.command.is_empty()) {
        for (key, value) in facts(&r.output) {
            if !(key.contains("FREED") || key.contains("RECLAIM")) {
                continue;
            }
            let at = value
                .find(|c: char| c.is_ascii_alphabetic())
                .unwrap_or(value.len());
            let Ok(n) = value[..at].replace(',', "").parse::<f64>() else {
                continue;
            };
            let b = if at == value.len() {
                n
            } else {
                bytes(n, &value[at..]).map_or(n, |(dec, _)| dec)
            };
            *total.get_or_insert(0.0) += b.max(0.0);
        }
    }
    total
}

fn gb(b: f64) -> String {
    format!("{:.2} GB", b / 1e9)
}

/// What a reply says the checks found, read against what they printed.
fn results(reply: &str, ran: &[Ran]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let last = ran.iter().rev().find(|r| !r.command.is_empty());
    let last_error = last.and_then(|r| {
        error_line(&r.output).or_else(|| (!r.ok).then(|| "it ended with an error".to_string()))
    });
    let last_facts = last.map(|r| facts(&r.output)).unwrap_or_default();
    let still_there = last_facts.iter().rev().find_map(|(k, v)| {
        if is_existence_key(k) && truthy(v) == Some(true) {
            Some((format!("{k}={v}"), true))
        } else if is_count_key(k) && v.parse::<u64>().is_ok_and(|n| n > 0) {
            Some((format!("{k}={v}"), false))
        } else {
            None
        }
    });
    let freed = run_freed(ran);
    for sentence in sentences(reply) {
        if hedged(&sentence) {
            continue;
        }
        if let (Some(r), Some(err)) = (last, &last_error) {
            if says_verified(&sentence) && seen.insert(CHECK_FAILED) {
                out.push(Finding {
                    kind: CHECK_FAILED,
                    detail: format!(
                        "Your reply says \"{}\", but the last command (`{}`) didn't work: {err}.",
                        clip(&sentence),
                        clip(&r.command)
                    ),
                });
            }
        }
        if let Some((fact, exists)) = &still_there {
            // A folder that exists can still be empty; only a count says it isn't.
            let claimed = says_gone(&sentence) || (!exists && says_empty(&sentence));
            if claimed && seen.insert(STILL_THERE) {
                out.push(Finding {
                    kind: STILL_THERE,
                    detail: format!(
                        "Your reply says \"{}\", but the last check printed `{fact}`.",
                        clip(&sentence)
                    ),
                });
            }
        }
        if let (Some(total), Some((low, _))) = (freed, stated_freed(&sentence)) {
            if low > total * 1.05 + 1e7 && seen.insert(FREED_MISMATCH) {
                out.push(Finding {
                    kind: FREED_MISMATCH,
                    detail: format!(
                        "Your reply says \"{}\", but this run's commands freed {} in all.",
                        clip(&sentence),
                        gb(total)
                    ),
                });
            }
        }
    }
    out
}

// --- Online models ---------------------------------------------------------

/// Words that put a sentence about the online models, a key or the router.
const PROVIDER_WORDS: [&str; 9] = [
    "claude",
    "anthropic",
    "openrouter",
    "open router",
    "api key",
    "your key",
    "online model",
    "the router",
    "auto router",
];

/// Words that state how one stands, or that something was done to it.
const STATUS_WORDS: [&str; 14] = [
    "connected",
    "available",
    "working",
    " works",
    " ready",
    " active",
    " live",
    "tested",
    "verified",
    "confirmed",
    "pinged",
    "switched",
    "triggered",
    "routed",
];

/// Asks that hand a test or a setup step back to the user.
const PUSHBACK_WORDS: [&str; 12] = [
    "update the app",
    "update loadout",
    "restart",
    "relaunch",
    "sign in again",
    "log in again",
    "re-auth",
    "reauth",
    "re-enter",
    "harder task",
    "complex task",
    "harder question",
];

/// A sentence that says it doesn't know, or asks.
fn unsure(sentence: &str) -> bool {
    let s = format!(" {} ", sentence.to_lowercase());
    s.trim_end().ends_with('?')
        || [
            " whether ",
            " if ",
            "can't tell",
            "cannot tell",
            "don't know",
            "haven't checked",
            "not checked",
            "not sure",
            " might ",
            " may ",
            " would ",
        ]
        .iter()
        .any(|w| s.contains(w))
}

/// A reply about the online models: one that says how a model or key
/// stands with nothing this turn having checked (`checked`: a models_check
/// ran, or the turn carried the app's own connection check), and one that
/// asks the user to update, restart, sign in again or send a harder task to
/// test it. The app picks the model and runs the test itself, so both are
/// the reply making things up or handing the work back.
pub fn connections(reply: &str, ran: &[Ran], checked: bool) -> Vec<Finding> {
    let checked = checked || ran.iter().any(|r| r.name == "models_check" && r.ok);
    let mut out = Vec::new();
    for sentence in sentences(reply) {
        let low = format!(" {}", sentence.to_lowercase());
        if !PROVIDER_WORDS.iter().any(|w| low.contains(w)) {
            continue;
        }
        if PUSHBACK_WORDS.iter().any(|w| low.contains(w)) && !kinds_has(&out, PUSHED_TEST) {
            out.push(Finding {
                kind: PUSHED_TEST,
                detail: format!(
                    "You asked the user to \"{}\". The app picks the model each turn and models_check is the test: run it if it's offered, and don't hand testing or setup back to them.",
                    clip(&sentence)
                ),
            });
            continue;
        }
        if !checked
            && STATUS_WORDS.iter().any(|w| low.contains(w))
            && !unsure(&sentence)
            && !kinds_has(&out, UNCHECKED_CONNECTION)
        {
            out.push(Finding {
                kind: UNCHECKED_CONNECTION,
                detail: format!(
                    "Your reply says \"{}\", but nothing this turn checked the online models. Call models_check if it's offered, or say you haven't checked.",
                    clip(&sentence)
                ),
            });
        }
    }
    out
}

fn kinds_has(found: &[Finding], kind: &str) -> bool {
    found.iter().any(|f| f.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(command: &str, ok: bool, output: &str) -> Ran {
        Ran::new(
            "run_terminal_command",
            &serde_json::json!({ "command": command }).to_string(),
            ok,
            output,
        )
    }

    fn write(path: &str) -> Ran {
        Ran::new(
            "write",
            &serde_json::json!({ "path": path, "contents": "x" }).to_string(),
            true,
            "ok",
        )
    }

    fn kinds(f: &[Finding]) -> Vec<&'static str> {
        f.iter().map(|f| f.kind).collect()
    }

    const CARGO_OK: &str = "running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored\n\nrunning 9 tests\ntest result: ok. 9 passed; 0 failed; 0 ignored\n";
    const CARGO_RED: &str =
        "test result: FAILED. 11 passed; 1 failed; 0 ignored\nerror: test failed\n";

    #[test]
    fn a_turn_whose_searches_found_nothing_or_failed_came_up_empty() {
        let search = |out: &str| Ran::new("web_search", r#"{"query":"patch 2.1"}"#, true, out);
        let found = search("1. Patch notes - https://example.com/patch-2-1\n");
        let nothing = search("No results.");
        let fetch_failed = Ran::new(
            "web_fetch",
            r#"{"url":"https://example.com/patch-2-1"}"#,
            false,
            "HTTP 404",
        );
        let reply = "Here are the notes.";
        assert!(came_up_empty(reply, std::slice::from_ref(&nothing)));
        assert!(came_up_empty(
            reply,
            &[nothing.clone(), fetch_failed.clone()]
        ));
        assert!(came_up_empty(reply, std::slice::from_ref(&fetch_failed)));
        assert!(came_up_empty(
            reply,
            &[shell("ls /nope", false, "No such file")]
        ));
        assert!(!came_up_empty(reply, &[nothing, found.clone()]));
        assert!(!came_up_empty(reply, &[found, fetch_failed]));
        assert!(!came_up_empty("Hey, good to see you!", &[]));
        // The reply's own word counts, even with no tool at all.
        assert!(came_up_empty(
            "Sorry, I couldn't find that patch anywhere.",
            &[]
        ));
    }

    #[test]
    fn a_link_no_tool_opened_gets_a_note_and_an_opened_one_does_not() {
        let fetched = Ran::new(
            "web_fetch",
            r#"{"url":"https://www.wowhead.com/item=226995/aqir-research-token"}"#,
            true,
            "page text",
        );
        let searched = Ran::new(
            "web_search",
            r#"{"query":"aqir research token turn in"}"#,
            true,
            "1. Aqir token guide - https://www.icy-veins.com/wow/aqir-tokens\n",
        );
        let reply = "Turn them in at the vendor ([Wowhead](https://wowhead.com/item=226995/aqir-research-token)), see https://www.icy-veins.com/wow/aqir-tokens.";
        assert!(sources(reply, &[fetched.clone(), searched.clone()], &[], "").is_empty());
        // The old turn's link nobody opened this turn.
        let stale = "Per https://www.wowhead.com/guide/old-aqir-guide, go to the camp.";
        let found = sources(stale, std::slice::from_ref(&fetched), &[], "");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, UNOPENED_LINK);
        assert!(
            found[0].detail.contains("old-aqir-guide"),
            "{}",
            found[0].detail
        );
        // Hosted citations and links the person gave count as seen.
        assert!(sources(
            stale,
            &[],
            &["https://wowhead.com/guide/old-aqir-guide".into()],
            ""
        )
        .is_empty());
        assert!(sources(
            stale,
            &[],
            &[],
            "is https://www.wowhead.com/guide/old-aqir-guide right?"
        )
        .is_empty());
        // A failed fetch opened nothing.
        let failed = Ran::new(
            "web_fetch",
            r#"{"url":"https://www.wowhead.com/guide/old-aqir-guide"}"#,
            false,
            "404",
        );
        assert_eq!(sources(stale, &[failed], &[], "").len(), 1);
    }

    #[test]
    fn a_table_total_that_doesnt_add_up_is_caught_and_a_right_one_is_not() {
        let wrong = "| Item | Cost |\n|---|---|\n| CPU | $320 |\n| GPU | $1,199.99 |\n| RAM | $89.50 |\n| **Total** | **$1,599.49** |\n";
        let f = check(wrong, &[]);
        assert_eq!(kinds(&f), vec![TABLE_TOTAL]);
        assert!(
            f[0].detail.contains("1599.49") && f[0].detail.contains("1609.49"),
            "{}",
            f[0].detail
        );
        let right = wrong.replace("1,599.49", "1,609.49");
        assert!(check(&right, &[]).is_empty());
        // An average column, a percent column and subtotals are left alone.
        let avg = "| Game | FPS avg | Share |\n|---|---|---|\n| A | 90 | 40% |\n| B | 60 | 60% |\n| Total | 75 | 100% |\n";
        assert!(check(avg, &[]).is_empty());
    }

    #[test]
    fn inline_sums_are_redone_and_code_comparisons_are_not_sums() {
        let f = check("That's 12 + 30 + 7 = 50 GB in all.", &[]);
        assert_eq!(kinds(&f), vec![INLINE_SUM]);
        assert!(f[0].detail.contains("comes to 49"), "{}", f[0].detail);
        assert!(check("So 2 × 16 + 8 = 40 GB free.", &[]).is_empty());
        assert!(check("Price: $19.99 - $5 = $14.99", &[]).is_empty());
        for code in [
            "if a == 3 + 4 {",
            "x = 3 + 4",
            "let n=1+1;",
            "check 3 + 4 >= 9",
        ] {
            assert!(check(code, &[]).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_pass_claim_meets_the_last_test_run() {
        let red = [shell("cargo test -p app", false, CARGO_RED)];
        let f = check("Fixed it, all tests pass now.", &red);
        assert_eq!(kinds(&f), vec![TESTS_FAILED]);
        assert!(f[0].detail.contains("1 failed"), "{}", f[0].detail);
        // Fixed and rerun green: nothing to say.
        let green = [
            shell("cargo test -p app", false, CARGO_RED),
            shell("cargo test -p app", true, CARGO_OK),
        ];
        assert!(check("Fixed it, all tests pass now.", &green).is_empty());
        // A crash with no summary still isn't a pass.
        let crashed = [shell("npm test", false, "Error: cannot find module")];
        assert_eq!(
            kinds(&check("The tests pass.", &crashed)),
            vec![TESTS_FAILED]
        );
        // Claimed with no test run at all.
        let edits = [write("src/app.rs")];
        assert_eq!(
            kinds(&check("Updated `src/app.rs` and the tests pass.", &edits)),
            vec![TESTS_NOT_RUN]
        );
        // Hedged, failing on purpose, or about another time: no claim.
        for said in [
            "The tests should pass once you rebuild.",
            "Tests passed earlier but 2 fail now.",
            "Do the tests pass for you?",
            "I'll run the tests next.",
        ] {
            assert!(check(said, &edits).is_empty(), "{said}");
        }
    }

    #[test]
    fn stated_counts_meet_the_tools_own_numbers() {
        let green = [shell("cargo test", true, CARGO_OK)];
        assert!(check("All 12 tests pass.", &green).is_empty());
        assert!(check("9 passed in the core crate.", &green).is_empty());
        let f = check("All 14 tests pass.", &green);
        assert_eq!(kinds(&f), vec![TEST_COUNT]);
        assert!(f[0].detail.contains("reports 12 passed"), "{}", f[0].detail);
        let pytest = [shell(
            "python -m pytest -q",
            true,
            "....\n========== 41 passed in 2.31s ==========\n",
        )];
        assert!(check("41 passed.", &pytest).is_empty());
        assert_eq!(kinds(&check("40 passed.", &pytest)), vec![TEST_COUNT]);
        let jest = [shell("npx jest", false, "Test Suites: 1 failed, 3 passed, 4 total\nTests:       2 failed, 30 passed, 32 total\n")];
        let f = check("Done, the tests are green.", &jest);
        assert_eq!(kinds(&f), vec![TESTS_FAILED]);
        assert!(f[0].detail.contains("2 failed"), "{}", f[0].detail);
        let commit = [shell(
            "git commit -am wip",
            true,
            "[beta 1a2b3c] wip\n 3 files changed, 40 insertions(+)\n",
        )];
        assert!(check("Committed: 3 files changed.", &commit).is_empty());
        assert_eq!(
            kinds(&check("Committed: 4 files changed.", &commit)),
            vec![FILES_CHANGED]
        );
    }

    #[test]
    fn commits_pushes_and_file_edits_need_a_call_that_did_them() {
        let edits = [write("crates/app/src/lib.rs")];
        let f = check(
            "Updated `crates/app/src/lib.rs`, committed it and pushed the branch to origin.",
            &edits,
        );
        assert_eq!(kinds(&f), vec![NOT_COMMITTED, NOT_PUSHED]);
        let done = [
            write("crates/app/src/lib.rs"),
            shell("git commit -am 'fix' && git push -u origin fix", true, "ok"),
        ];
        assert!(check(
            "Updated `crates/app/src/lib.rs`, committed it and pushed the branch to origin.",
            &done
        )
        .is_empty());
        let f = check("I also updated `README.md` with the new flag.", &edits);
        assert_eq!(kinds(&f), vec![FILE_NOT_WRITTEN]);
        // A shell edit names the file; a read does not count as a write.
        let sed = [shell("sed -i 's/a/b/' README.md", true, "")];
        assert!(check("I updated `README.md`.", &sed).is_empty());
        let read = [Ran::new(
            "read_file",
            r#"{"path":"README.md"}"#,
            true,
            "# hi",
        )];
        assert_eq!(
            kinds(&check("I updated `README.md`.", &read)),
            vec![FILE_NOT_WRITTEN]
        );
        // Nothing ran this turn, or a subagent did the work: coverage stays quiet.
        assert!(check("Committed and pushed to origin.", &[]).is_empty());
        let sub = [
            Ran::new("spawn_subagent", "{}", true, "done"),
            write("a.rs"),
        ];
        assert!(check("Committed and pushed to origin.", &sub).is_empty());
        // Not a claim: plans, questions and other people's pushes.
        for said in [
            "Want me to commit and push it?",
            "I haven't committed anything yet.",
            "Once it's committed, I'll push the branch.",
            "The deadline got pushed.",
        ] {
            assert!(check(said, &edits).is_empty(), "{said}");
        }
    }

    #[test]
    fn the_note_lists_each_finding_and_long_output_keeps_its_tail() {
        assert_eq!(note(&[]), None);
        let n = note(&check("Total: 2 + 2 = 5.", &[])).unwrap();
        assert!(
            n.starts_with("Before this turn ends") && n.contains("- Your reply says 2 + 2 = 5"),
            "{n}"
        );
        let long = format!(
            "{}{}",
            "x\n".repeat(20_000),
            "test result: ok. 7 passed; 0 failed"
        );
        let r = shell("cargo test", true, &long);
        assert!(r.output.ends_with("7 passed; 0 failed") && r.output.chars().count() < 16_100);
    }

    // --- Results: the PC tune-up chat log, Nov 6 ---

    const WINRE_CHECK: &str = "$winre = 'C:\\$WinREAgent'; $e = Test-Path $winre; \"EXISTS=$e\"";

    #[test]
    fn verified_empty_after_a_check_that_failed_and_says_exists_is_caught() {
        let ran = [shell(
            WINRE_CHECK,
            true,
            "EXISTS=True CURRENT_FREE=33257828352\nAt line:1 char:40\nUnexpected token 'if' in expression or statement.\n    + FullyQualifiedErrorId : UnexpectedToken",
        )];
        let f = check(
            "Recovery verified empty. Error gone, space works again. Task complete.",
            &ran,
        );
        assert_eq!(kinds(&f), vec![CHECK_FAILED]);
        assert!(f[0].detail.contains("Unexpected token"), "{}", f[0].detail);
        assert!(note(&f).is_some());
    }

    #[test]
    fn gone_against_exists_true_is_caught_but_an_empty_folder_that_exists_is_not() {
        let ran = [shell(
            WINRE_CHECK,
            true,
            "EXISTS=True CURRENT_FREE=33257828352",
        )];
        assert_eq!(
            kinds(&check("The recovery leftovers are gone.", &ran)),
            vec![STILL_THERE]
        );
        assert!(check("The recovery folder is empty.", &ran).is_empty());
        let counted = [shell(WINRE_CHECK, true, "EXISTS=True COUNT=4")];
        assert_eq!(
            kinds(&check("The recovery folder is empty.", &counted)),
            vec![STILL_THERE]
        );
    }

    #[test]
    fn a_check_that_backs_the_claim_passes() {
        let ran = [shell(
            WINRE_CHECK,
            true,
            "EXISTS=False CURRENT_FREE=33257828352",
        )];
        assert!(check("Recovery leftovers verified gone. Task complete.", &ran).is_empty());
        let missing = [shell(
            "Get-ChildItem C:\\old",
            false,
            "Get-ChildItem : Cannot find path 'C:\\old' because it does not exist.",
        )];
        assert!(check("The old folder is gone.", &missing).is_empty());
    }

    #[test]
    fn a_failed_check_with_no_error_text_still_counts() {
        let ran = [shell("Test-Path C:\\x", false, "")];
        assert_eq!(
            kinds(&check("Verified, the folder is clean.", &ran)),
            vec![CHECK_FAILED]
        );
    }

    #[test]
    fn hedged_and_honest_replies_are_left_alone() {
        let ran = [shell(WINRE_CHECK, false, "Unexpected token 'if'")];
        assert!(check("The check failed, so I couldn't confirm it.", &ran).is_empty());
        assert!(check("I'll check again once it's verified.", &ran).is_empty());
        assert!(check("Recovery cleanup: the check hit a syntax error.", &ran).is_empty());
    }

    #[test]
    fn freed_more_than_the_commands_printed_is_caught() {
        let ran = [
            shell(
                "Remove-Item target",
                true,
                "BEFORE_FREE=20466192384 AFTER_FREE=33258516480 FREED=12792324096 DELETED",
            ),
            shell("Remove-Item $winre", true, "FREED=0"),
        ];
        let f = check("Rust and recovery cleaned. 28.41 GB freed total.", &ran);
        assert_eq!(kinds(&f), vec![FREED_MISMATCH]);
        assert!(f[0].detail.contains("12.79 GB"), "{}", f[0].detail);
        assert!(check("The Rust target was deleted, freeing 12.79 GB.", &ran).is_empty());
        assert!(check("That freed 11.9 GiB.", &ran).is_empty());
        assert!(check(
            "28.41 GB freed in all, with the VM cleanup from earlier.",
            &ran
        )
        .is_empty());
        assert!(check("You now have 33.26 GB free.", &ran).is_empty());
        assert!(
            check("Freed 28 GB.", &[shell("dir", true, "nothing")]).is_empty(),
            "no FREED fact, nothing to read against"
        );
    }

    #[test]
    fn facts_skip_script_text_and_comparisons() {
        assert_eq!(
            facts("$e=1 a==b EXISTS=True Count=3 'x'"),
            vec![
                ("EXISTS".to_string(), "True".to_string()),
                ("COUNT".to_string(), "3".to_string())
            ]
        );
    }

    #[test]
    fn an_unchecked_word_on_the_online_models_is_caught() {
        // The reported chat: a status nobody checked, and the test handed back.
        let reply = "Claude is connected and available. To test it, send a complex task and Auto will escalate to Claude.";
        assert_eq!(
            kinds(&connections(reply, &[], false)),
            vec![UNCHECKED_CONNECTION, PUSHED_TEST]
        );
        // After the app's own check, the status is the check's: only the
        // hand-back is still wrong.
        assert_eq!(kinds(&connections(reply, &[], true)), vec![PUSHED_TEST]);
        let check = Ran::new(
            "models_check",
            "{}",
            true,
            "Claude (Anthropic): Auto can use it.",
        );
        assert_eq!(
            kinds(&connections(
                "Claude is connected: Auto can use it.",
                &[check],
                false
            )),
            Vec::<&str>::new()
        );
        // Saying it hasn't checked, asking, or not talking about the models
        // at all is fine.
        for fine in [
            "I can't tell whether Claude is connected from here.",
            "Is your Claude key the one you added last week?",
            "The build is ready and the tests are working.",
        ] {
            assert!(connections(fine, &[], false).is_empty(), "{fine}");
        }
        assert_eq!(
            kinds(&connections(
                "Please restart the app so the router picks up your key.",
                &[],
                true
            )),
            vec![PUSHED_TEST]
        );
    }
}
