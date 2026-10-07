//! PII masking for recall packs (Spike-4b).
//!
//! Recalled memory is masked before it goes into a model prompt: emails,
//! phone numbers, card numbers (IIN + Luhn), SSN-shaped numbers and US street
//! addresses / PO boxes become `[email]`, `[phone]`, `[card]`, `[ssn]` and
//! `[address]`. Hand-rolled and deterministic (no regex crate). Each matcher
//! needs a word boundary on both sides and a strict shape, so code
//! identifiers, versions, hashes, timestamps, IPs and `git@host:` remotes stay.
//! The user's own views (`/recall`, Settings) are not masked.

/// What [`redact_recall`] did: the masked text and how many spans it replaced.
pub fn redact_recall(text: &str) -> (String, u32) {
    let before = text.matches("[redacted]").count();
    let secrets = crate::redact_secrets(text);
    let secret_hits = secrets.matches("[redacted]").count().saturating_sub(before) as u32;
    let (out, pii_hits) = redact_pii(&secrets);
    (out, secret_hits + pii_hits)
}

/// Mask emails, phones, cards, SSNs and street addresses. Returns the text
/// and the number of replacements.
pub fn redact_pii(text: &str) -> (String, u32) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut hits = 0u32;
    let mut i = 0;
    while i < chars.len() {
        if let Some((len, token)) = match_at(&chars, i) {
            out.push_str(token);
            hits += 1;
            i += len;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    (out, hits)
}

fn match_at(c: &[char], i: usize) -> Option<(usize, &'static str)> {
    let ch = c[i];
    if is_local_char(ch) && (i == 0 || !is_local_char(c[i - 1])) {
        if let Some(n) = email(c, i) {
            return Some((n, "[email]"));
        }
    }
    let prev = if i == 0 { None } else { Some(c[i - 1]) };
    // Number-shaped matches start on a clean boundary: not inside a word,
    // identifier, version, path or another number.
    let clean = prev.is_none_or(|p| !(p.is_alphanumeric() || matches!(p, '_' | '.' | '-' | '/' | '\\' | '#' | '$' | '@' | '+')));
    if ch == '+' && prev.is_none_or(|p| !(p.is_alphanumeric() || matches!(p, '_' | '+'))) {
        if let Some(n) = intl_phone(c, i) {
            return Some((n, "[phone]"));
        }
    }
    if ch == '(' && clean {
        if let Some(n) = nanp_paren(c, i) {
            return Some((n, "[phone]"));
        }
    }
    if (ch == 'P' || ch == 'p') && prev.is_none_or(|p| !p.is_alphanumeric()) {
        if let Some(n) = po_box(c, i) {
            return Some((n, "[address]"));
        }
    }
    if !ch.is_ascii_digit() || !clean {
        return None;
    }
    if let Some(n) = ssn(c, i) {
        return Some((n, "[ssn]"));
    }
    if let Some(n) = card(c, i) {
        return Some((n, "[card]"));
    }
    if let Some(n) = nanp_plain(c, i) {
        return Some((n, "[phone]"));
    }
    if let Some(n) = street(c, i) {
        return Some((n, "[address]"));
    }
    None
}

/// The match ends cleanly: no letter, digit or `_` next, and no `.`/`-`
/// that continues into another digit (a longer number or version).
fn ends_clean(c: &[char], end: usize) -> bool {
    match c.get(end) {
        None => true,
        Some(n) if n.is_alphanumeric() || *n == '_' => false,
        Some('.' | '-' | '/') => !c.get(end + 1).is_some_and(|x| x.is_ascii_alphanumeric()),
        Some(_) => true,
    }
}

fn digits(c: &[char], i: usize, n: usize) -> bool {
    i + n <= c.len() && c[i..i + n].iter().all(|x| x.is_ascii_digit())
}

fn num(c: &[char], i: usize, n: usize) -> u32 {
    c[i..i + n].iter().fold(0, |a, x| a * 10 + x.to_digit(10).unwrap_or(0))
}

// ---- email ---------------------------------------------------------------

fn is_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// File extensions and code-ish endings that are never a mail TLD here
/// (`logo@2x.png`, `module@scope.js`, `pkg@1.2.3`).
const NOT_TLDS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "bmp", "js", "mjs", "cjs", "ts", "tsx", "jsx", "rs", "py",
    "rb", "go", "java", "kt", "c", "h", "cpp", "hpp", "cs", "json", "toml", "yaml", "yml", "md", "txt", "lock",
    "html", "css", "scss", "sh", "zip", "gz", "tar", "wasm", "map", "log", "lib", "dll", "exe", "so", "pdf",
];

fn email(c: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    while j < c.len() && is_local_char(c[j]) {
        j += 1;
    }
    let local: String = c[i..j].iter().collect();
    if j == i || c.get(j) != Some(&'@') || local.starts_with('.') || local.ends_with('.') {
        return None;
    }
    if local.eq_ignore_ascii_case("git") {
        return None; // git@github.com:org/repo
    }
    let mut k = j + 1;
    let mut labels: Vec<String> = Vec::new();
    loop {
        let start = k;
        while k < c.len() && (c[k].is_ascii_alphanumeric() || c[k] == '-') {
            k += 1;
        }
        if k == start {
            return None;
        }
        labels.push(c[start..k].iter().collect());
        if c.get(k) == Some(&'.') && c.get(k + 1).is_some_and(|x| x.is_ascii_alphanumeric()) {
            k += 1;
            continue;
        }
        break;
    }
    let tld = labels.last()?;
    if labels.len() < 2
        || !(2..=24).contains(&tld.len())
        || !tld.chars().all(|x| x.is_ascii_alphabetic())
        || NOT_TLDS.contains(&tld.to_ascii_lowercase().as_str())
        || labels.iter().any(|l| l.starts_with('-') || l.ends_with('-'))
    {
        return None;
    }
    if c.get(k).is_some_and(|x| x.is_alphanumeric() || matches!(x, '_' | '@' | ':' | '/')) {
        return None;
    }
    Some(k - i)
}

// ---- cards and SSNs --------------------------------------------------------

/// 13–19 digits, optionally in groups split by one space or dash, with a
/// real card prefix and a valid Luhn check digit.
fn card(c: &[char], i: usize) -> Option<usize> {
    // Collect digit groups and where each ends.
    let mut ends: Vec<(usize, String)> = Vec::new();
    let mut all = String::new();
    let mut j = i;
    let mut sep: Option<char> = None;
    loop {
        let start = j;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            break;
        }
        all.extend(&c[start..j]);
        ends.push((j, all.clone()));
        if all.len() > 19 {
            break;
        }
        match c.get(j) {
            Some(&s @ (' ' | '-')) if c.get(j + 1).is_some_and(|x| x.is_ascii_digit()) && sep.is_none_or(|p| p == s) => {
                sep = Some(s);
                j += 1;
            }
            _ => break,
        }
    }
    for (end, digits) in ends.iter().rev() {
        if (13..=19).contains(&digits.len()) && ends_clean(c, *end) && card_prefix(digits) && luhn(digits) {
            // A bare run must be the whole number, not the head of a longer one.
            return Some(end - i);
        }
    }
    None
}

fn card_prefix(d: &str) -> bool {
    let p = |n: usize| d[..n].parse::<u32>().unwrap_or(0);
    d.starts_with('4')
        || (51..=55).contains(&p(2))
        || (2221..=2720).contains(&p(4))
        || matches!(p(2), 34 | 37)
        || p(4) == 6011
        || p(2) == 65
        || (3528..=3589).contains(&p(4))
}

fn luhn(d: &str) -> bool {
    let mut sum = 0;
    for (n, ch) in d.chars().rev().enumerate() {
        let mut v = ch.to_digit(10).unwrap_or(0);
        if n % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum % 10 == 0
}

/// `ddd-dd-dddd` with a possible area (not 000, 666 or 9xx), group and serial.
fn ssn(c: &[char], i: usize) -> Option<usize> {
    let ok = digits(c, i, 3)
        && c.get(i + 3) == Some(&'-')
        && digits(c, i + 4, 2)
        && c.get(i + 6) == Some(&'-')
        && digits(c, i + 7, 4)
        && ends_clean(c, i + 11);
    if !ok {
        return None;
    }
    let (area, group, serial) = (num(c, i, 3), num(c, i + 4, 2), num(c, i + 7, 4));
    (area != 0 && area != 666 && area < 900 && group != 0 && serial != 0).then_some(11)
}

// ---- phones ----------------------------------------------------------------

/// `+` then 8–15 digits in groups split by one space, dash, dot or a
/// `(…)` area group. `+1 (312) 555-0199`, `+44 20 7946 0958`.
fn intl_phone(c: &[char], i: usize) -> Option<usize> {
    let mut j = i + 1;
    let mut count = 0;
    let end = loop {
        let paren = c.get(j) == Some(&'(');
        if paren {
            j += 1;
        }
        let start = j;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        if j == start || j - start > 5 {
            return None;
        }
        count += j - start;
        if paren {
            if c.get(j) != Some(&')') {
                return None;
            }
            j += 1;
        }
        match c.get(j) {
            Some(' ' | '-' | '.') if c.get(j + 1).is_some_and(|x| x.is_ascii_digit() || *x == '(') => j += 1,
            _ => break j,
        }
    };
    ((8..=15).contains(&count) && ends_clean(c, end)).then(|| end - i)
}

fn nanp_ok(c: &[char], area: usize, exch: usize) -> bool {
    matches!(c[area], '2'..='9') && matches!(c[exch], '2'..='9')
}

/// `(312) 555-0199` or `(312)555-0199`.
fn nanp_paren(c: &[char], i: usize) -> Option<usize> {
    if !(digits(c, i + 1, 3) && c.get(i + 4) == Some(&')')) {
        return None;
    }
    let mut j = i + 5;
    if c.get(j) == Some(&' ') {
        j += 1;
    }
    let ok = digits(c, j, 3)
        && matches!(c.get(j + 3), Some('-' | '.'))
        && digits(c, j + 4, 4)
        && nanp_ok(c, i + 1, j)
        && ends_clean(c, j + 8);
    ok.then_some(j + 8 - i)
}

/// `312-555-0199`, `312.555.0199`, `1-312-555-0199`. One separator kind,
/// and never with spaces only (too much like a list of numbers).
fn nanp_plain(c: &[char], i: usize) -> Option<usize> {
    let lead = if c[i] == '1' && c.get(i + 1) == Some(&'-') { 2 } else { 0 };
    let a = i + lead;
    let sep = *c.get(a + 3)?;
    if !matches!(sep, '-' | '.') || (lead == 2 && sep != '-') {
        return None;
    }
    let ok = digits(c, a, 3)
        && digits(c, a + 4, 3)
        && c.get(a + 7) == Some(&sep)
        && digits(c, a + 8, 4)
        && nanp_ok(c, a, a + 4)
        && ends_clean(c, a + 12);
    ok.then_some(a + 12 - i)
}

// ---- addresses -------------------------------------------------------------

const STREET_SUFFIXES: &[&str] = &[
    "Street", "St", "Avenue", "Ave", "Road", "Rd", "Boulevard", "Blvd", "Lane", "Ln", "Drive", "Dr", "Court", "Ct",
    "Way", "Place", "Pl", "Terrace", "Ter", "Parkway", "Pkwy", "Circle", "Cir", "Highway", "Hwy", "Square", "Sq",
    "Trail", "Trl",
];
const DIRECTIONS: &[&str] = &["N", "S", "E", "W", "NE", "NW", "SE", "SW"];

fn word_at(c: &[char], i: usize) -> Option<(String, usize)> {
    let mut j = i;
    while j < c.len() && (c[j].is_alphanumeric() || c[j] == '\'') {
        j += 1;
    }
    (j > i).then(|| (c[i..j].iter().collect(), j))
}

/// A street name word: Capitalized (`Elm`, `O'Hare`) or an ordinal (`5th`).
fn name_word(w: &str) -> bool {
    let first = w.chars().next().unwrap_or(' ');
    if first.is_ascii_digit() {
        let n = w.trim_end_matches(|x: char| x.is_ascii_alphabetic());
        let suf = &w[n.len()..];
        return !n.is_empty() && n.len() <= 3 && matches!(suf, "st" | "nd" | "rd" | "th");
    }
    first.is_uppercase() && w.chars().skip(1).all(|x| x.is_lowercase() || x == '\'')
}

/// `221B Baker Street`, `1600 Pennsylvania Avenue NW`, `42 N 5th St., Apt 3`.
fn street(c: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    while j < c.len() && c[j].is_ascii_digit() {
        j += 1;
    }
    if j - i > 6 {
        return None;
    }
    if c.get(j).is_some_and(|x| x.is_ascii_uppercase()) && c.get(j + 1) == Some(&' ') {
        j += 1; // 221B
    }
    if c.get(j) != Some(&' ') {
        return None;
    }
    j += 1;
    let mut names = 0;
    loop {
        let (w, end) = word_at(c, j)?;
        let suffix = STREET_SUFFIXES.contains(&w.as_str());
        if suffix && names > 0 {
            let mut end = end;
            if c.get(end) == Some(&'.') {
                end += 1;
            }
            // An optional direction and unit, kept inside the mask.
            if c.get(end) == Some(&' ') {
                if let Some((d, e)) = word_at(c, end + 1) {
                    if DIRECTIONS.contains(&d.as_str()) && ends_clean(c, e) {
                        end = e;
                    }
                }
            }
            end = unit(c, end).unwrap_or(end);
            return (ends_clean(c, end) || c.get(end) == Some(&'.')).then_some(end - i);
        }
        if names >= 4 || !(name_word(&w) || (names == 0 && DIRECTIONS.contains(&w.as_str()))) {
            return None;
        }
        names += 1;
        if c.get(end) != Some(&' ') {
            return None;
        }
        j = end + 1;
    }
}

/// `, Apt 3`, ` Suite 200`, ` Unit 4B`, ` #12` after a street.
fn unit(c: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    if c.get(j) == Some(&',') {
        j += 1;
    }
    if c.get(j) != Some(&' ') {
        return None;
    }
    j += 1;
    if c.get(j) == Some(&'#') {
        j += 1;
    } else {
        let (w, e) = word_at(c, j)?;
        if !matches!(w.as_str(), "Apt" | "Suite" | "Unit" | "Ste") {
            return None;
        }
        j = e;
        if c.get(j) == Some(&'.') {
            j += 1;
        }
        if c.get(j) != Some(&' ') {
            return None;
        }
        j += 1;
    }
    let start = j;
    while j < c.len() && c[j].is_ascii_alphanumeric() && j - start < 6 {
        j += 1;
    }
    (j > start && c[start].is_ascii_digit()).then_some(j)
}

/// `PO Box 123`, `P.O. Box 123`, `Post Office Box 123` (any case).
fn po_box(c: &[char], i: usize) -> Option<usize> {
    let rest: String = c[i..c.len().min(i + 20)].iter().collect::<String>().to_ascii_lowercase();
    let head = ["p.o. box ", "po box ", "p.o.box ", "post office box "]
        .iter()
        .find(|h| rest.starts_with(**h))?;
    let j = i + head.chars().count();
    let mut k = j;
    while k < c.len() && c[k].is_ascii_digit() && k - j < 8 {
        k += 1;
    }
    (k > j && ends_clean(c, k)).then_some(k - i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(s: &str) -> String {
        redact_pii(s).0
    }

    #[test]
    fn masks_each_kind() {
        let cases = [
            ("mail jane.doe+news@example.co.uk today", "mail [email] today"),
            ("Reach me at J_Smith@Mail.Example.COM.", "Reach me at [email]."),
            ("call +1 (312) 555-0199 or +44 20 7946 0958", "call [phone] or [phone]"),
            ("office (312) 555-0199, cell 312-555-0142", "office [phone], cell [phone]"),
            ("fax 312.555.0177 or 1-800-555-0100", "fax [phone] or [phone]"),
            ("card 4111 1111 1111 1111 exp 12/29", "card [card] exp 12/29"),
            ("amex 3782-822463-10005 and 5555555555554444", "amex [card] and [card]"),
            ("ssn 123-45-6789.", "ssn [ssn]."),
            ("Phone:312-555-0199 SSN:123-45-6789", "Phone:[phone] SSN:[ssn]"),
            ("I live at 221B Baker Street, London", "I live at [address], London"),
            ("ship to 1600 Pennsylvania Avenue NW", "ship to [address]"),
            ("42 N 5th St., Apt 3 is the door", "[address] is the door"),
            ("mail it to PO Box 1234 please", "mail it to [address] please"),
            ("or P.O. Box 77.", "or [address]."),
        ];
        for (input, want) in cases {
            assert_eq!(masked(input), want, "{input}");
        }
        assert_eq!(redact_pii("a@b.co and 123-45-6789").1, 2);
    }

    #[test]
    fn leaves_code_and_numbers_alone() {
        let keep = [
            "git@github.com:blackviperxiii-ui/GrokHub.git",
            "icon@2x.png and logo@3x.svg and mod@scope.js",
            "npm i react@18.2.0 @types/node",
            "fn send_mail(user@host) -> Result<(), Error>",
            "#[derive(Debug)] @property def name(self):",
            "version 1.2.3-alpha.4 and 10.0.0.1 and 192.168.1.10",
            "sha 4f9c2a1e5b7d8c3f and id 1696780800000 and port 48988",
            "let x = 4111111111111112; // not Luhn",
            "1234567890123456789012 is too long",
            "0x4111111111111111 and v4111111111111111",
            "range 000-12-3456 and 666-12-3456 and 900-12-3456 and 123-00-4567",
            "2026-10-07 and 12:30:45 and 2026-10-07T12:00:00Z",
            "call 555-0199 and ext 123-4567",
            "issue #312-555-0199 and path/312-555-0199",
            "a 404 Not Found page and 200 OK",
            "step 3 of 10 Main parts",
            "Vec<String> at line 42 Drive::new()",
            "x = 12 + 34 - 5678",
            "UUID 123e4567-e89b-12d3-a456-426614174000",
            "+1 thanks, +100 votes, a+b@c",
            "diff +10,8 @@",
            "Box 12 and PO box",
            "SomeStruct::field and obj.user_email and EMAIL@ENV_VAR",
        ];
        for text in keep {
            assert_eq!(masked(text), text, "over-redacted: {text}");
            assert_eq!(redact_pii(text).1, 0, "{text}");
        }
    }

    #[test]
    fn recall_masks_secrets_and_pii_together() {
        let (out, n) = redact_recall("key sk-abcdefghijklmnopqrstuv for jane@example.org");
        assert_eq!(out, "key [redacted] for [email]");
        assert_eq!(n, 2);
        let (out, n) = redact_recall("already [redacted] here");
        assert_eq!((out.as_str(), n), ("already [redacted] here", 0));
        let unicode = "café ☕ at 10 Rue Street — naïve";
        assert_eq!(redact_recall(unicode).0, "café ☕ at [address] — naïve");
    }
}
