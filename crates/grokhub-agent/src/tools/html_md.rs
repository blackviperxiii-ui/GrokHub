//! Small HTML to markdown conversion for `web_fetch`.
//! Headings, links, lists, code, and paragraphs. Script and style are dropped.
//! No HTML crate: the workspace does not already depend on one.

#[derive(Debug, Clone)]
enum Tok {
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        self_close: bool,
    },
    End(String),
    Text(String),
}

pub(crate) fn html_to_markdown(html: &str) -> String {
    let stripped = strip_raw(html);
    let toks = tokenize(&stripped);
    let mut out = String::new();
    let mut i = 0;
    render_blocks(&toks, &mut i, &mut out, None, 0);
    trim_blank_edges(&out)
}

fn strip_raw(html: &str) -> String {
    const NAMES: &[&str] = &[
        "script", "style", "noscript", "svg", "iframe", "object", "embed",
    ];
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len().min(RAW_HINT));
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(end) = skip_raw_at(html, i, NAMES) {
                i = end;
                continue;
            }
        }
        let ch = html[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

const RAW_HINT: usize = 64 * 1024;

fn skip_raw_at(html: &str, i: usize, names: &[&str]) -> Option<usize> {
    let rest = html.get(i + 1..)?;
    for name in names {
        if !starts_with_ignore_ascii(rest, name) {
            continue;
        }
        let after = name.len();
        let boundary = rest.as_bytes().get(after).copied().unwrap_or(b'>');
        if boundary.is_ascii_alphanumeric() {
            continue;
        }
        let open_rel = rest[after..].find('>')?;
        let open_end = i + 1 + after + open_rel;
        if html.as_bytes().get(open_end.saturating_sub(1)) == Some(&b'/') {
            return Some(open_end + 1);
        }
        return Some(find_close_tag(html, open_end + 1, name).unwrap_or(html.len()));
    }
    None
}

fn find_close_tag(html: &str, from: usize, name: &str) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = from;
    let needle_len = name.len() + 2;
    while i + needle_len <= bytes.len() {
        if bytes[i] == b'<' && bytes.get(i + 1) == Some(&b'/') {
            let rest = html.get(i + 2..)?;
            if starts_with_ignore_ascii(rest, name) {
                let after = name.len();
                let boundary = rest.as_bytes().get(after).copied().unwrap_or(b'>');
                if !boundary.is_ascii_alphanumeric() {
                    let rel = rest[after..].find('>')?;
                    return Some(i + 2 + after + rel + 1);
                }
            }
        }
        i += 1;
    }
    None
}

fn starts_with_ignore_ascii(hay: &str, needle: &str) -> bool {
    hay.len() >= needle.len()
        && hay.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

fn tokenize(html: &str) -> Vec<Tok> {
    let bytes = html.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &html[i..];
            if rest.starts_with("<!--") {
                i = rest
                    .find("-->")
                    .map(|rel| i + rel + 3)
                    .unwrap_or(html.len());
                continue;
            }
            if rest.starts_with("<!") || rest.starts_with("<?") {
                i = rest.find('>').map(|rel| i + rel + 1).unwrap_or(html.len());
                continue;
            }
            if let Some((tok, next)) = parse_tag(html, i) {
                if next > i {
                    toks.push(tok);
                    i = next;
                    continue;
                }
            }
            push_text(&mut toks, "<");
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'<' {
            let ch = html[i..].chars().next().unwrap_or('\u{fffd}');
            i += ch.len_utf8();
        }
        if i > start {
            push_text(&mut toks, &html[start..i]);
        }
    }
    toks
}

fn push_text(toks: &mut Vec<Tok>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Tok::Text(have)) = toks.last_mut() {
        have.push_str(text);
    } else {
        toks.push(Tok::Text(text.to_string()));
    }
}

fn parse_tag(html: &str, i: usize) -> Option<(Tok, usize)> {
    let rest = html.get(i + 1..)?;
    let end = rest.starts_with('/');
    let mut j = usize::from(end);
    let name_start = j;
    while rest
        .as_bytes()
        .get(j)
        .is_some_and(|b| b.is_ascii_alphanumeric())
    {
        j += 1;
    }
    if j == name_start {
        return None;
    }
    let name = rest[name_start..j].to_ascii_lowercase();
    if end {
        let close = rest[j..].find('>')?;
        return Some((Tok::End(name), i + 1 + j + close + 1));
    }
    let mut attrs = Vec::new();
    let mut self_close = false;
    let mut closed = false;
    while j < rest.len() {
        let c = rest.as_bytes()[j];
        if c == b'>' {
            j += 1;
            closed = true;
            break;
        }
        if c == b'/' {
            self_close = true;
            j += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            j += 1;
            continue;
        }
        let ns = j;
        while j < rest.len() && is_attr_name(rest.as_bytes()[j]) {
            j += 1;
        }
        if j == ns {
            return None;
        }
        let aname = rest[ns..j].to_ascii_lowercase();
        while j < rest.len() && rest.as_bytes()[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut aval = String::new();
        if j < rest.len() && rest.as_bytes()[j] == b'=' {
            j += 1;
            while j < rest.len() && rest.as_bytes()[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < rest.len() {
                let q = rest.as_bytes()[j];
                if q == b'"' || q == b'\'' {
                    j += 1;
                    let vs = j;
                    while j < rest.len() && rest.as_bytes()[j] != q {
                        j += 1;
                    }
                    aval = decode_entities(&rest[vs..j]);
                    if j < rest.len() {
                        j += 1;
                    }
                } else {
                    let vs = j;
                    while j < rest.len()
                        && !rest.as_bytes()[j].is_ascii_whitespace()
                        && rest.as_bytes()[j] != b'>'
                    {
                        j += 1;
                    }
                    aval = decode_entities(&rest[vs..j]);
                }
            }
        }
        attrs.push((aname, aval));
    }
    if !closed {
        return None;
    }
    Some((
        Tok::Start {
            name,
            attrs,
            self_close,
        },
        i + 1 + j,
    ))
}

fn is_attr_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b':' || b == b'_'
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

fn render_blocks(toks: &[Tok], i: &mut usize, out: &mut String, stop: Option<&str>, depth: u8) {
    while *i < toks.len() {
        let kind = clone_tok(&toks[*i]);
        match kind {
            Tok::End(name) => {
                *i += 1;
                if stop == Some(name.as_str()) {
                    return;
                }
            }
            Tok::Text(text) => {
                *i += 1;
                if !text.trim().is_empty() {
                    ensure_gap(out);
                    push_inline(out, &text, false);
                    trim_trailing_space(out);
                    out.push('\n');
                }
            }
            Tok::Start {
                name, self_close, ..
            } => {
                if self_close || is_void(&name) {
                    *i += 1;
                    if name == "br" || name == "hr" {
                        out.push('\n');
                    }
                    continue;
                }
                match name.as_str() {
                    "h1" => render_heading(toks, i, out, 1),
                    "h2" => render_heading(toks, i, out, 2),
                    "h3" => render_heading(toks, i, out, 3),
                    "h4" => render_heading(toks, i, out, 4),
                    "h5" => render_heading(toks, i, out, 5),
                    "h6" => render_heading(toks, i, out, 6),
                    "ul" => render_list(toks, i, out, false),
                    "ol" => render_list(toks, i, out, true),
                    "pre" => render_pre(toks, i, out),
                    "p" | "blockquote" => render_paragraph(toks, i, out, &name),
                    "head" | "script" | "style" | "noscript" => skip_element(toks, i, &name),
                    _ => {
                        if depth >= 32 {
                            skip_element(toks, i, &name);
                        } else {
                            *i += 1;
                            render_blocks(
                                toks,
                                i,
                                out,
                                Some(name.as_str()),
                                depth.saturating_add(1),
                            );
                        }
                    }
                }
            }
        }
    }
}

fn skip_element(toks: &[Tok], i: &mut usize, name: &str) {
    *i += 1;
    let mut depth = 1i32;
    while *i < toks.len() && depth > 0 {
        match &toks[*i] {
            Tok::Start {
                name: inner,
                self_close,
                ..
            } if inner == name && !*self_close && !is_void(inner) => depth += 1,
            Tok::End(inner) if inner == name => depth -= 1,
            _ => {}
        }
        *i += 1;
    }
}

fn render_heading(toks: &[Tok], i: &mut usize, out: &mut String, level: usize) {
    let name = format!("h{level}");
    *i += 1;
    ensure_gap(out);
    for _ in 0..level {
        out.push('#');
    }
    out.push(' ');
    render_inline(toks, i, out, &name);
    trim_trailing_space(out);
    if out.ends_with('#') {
        out.push(' ');
    }
    out.push('\n');
}

fn render_paragraph(toks: &[Tok], i: &mut usize, out: &mut String, name: &str) {
    *i += 1;
    let start = out.len();
    ensure_gap(out);
    let mark = out.len();
    render_inline(toks, i, out, name);
    trim_trailing_space(out);
    if out.len() == mark || out[mark..].trim().is_empty() {
        out.truncate(start);
        return;
    }
    out.push('\n');
}

fn render_list(toks: &[Tok], i: &mut usize, out: &mut String, ordered: bool) {
    let stop = if ordered { "ol" } else { "ul" };
    *i += 1;
    ensure_gap(out);
    let mut n = 1u32;
    while *i < toks.len() {
        let kind = clone_tok(&toks[*i]);
        match kind {
            Tok::End(name) if name == stop => {
                *i += 1;
                return;
            }
            Tok::Text(text) if text.trim().is_empty() => {
                *i += 1;
            }
            Tok::Start { name, .. } if name == "li" => {
                *i += 1;
                if ordered {
                    out.push_str(&format!("{n}. "));
                    n = n.saturating_add(1);
                } else {
                    out.push_str("- ");
                }
                render_inline(toks, i, out, "li");
                trim_trailing_space(out);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ => {
                *i += 1;
            }
        }
    }
}

fn render_pre(toks: &[Tok], i: &mut usize, out: &mut String) {
    *i += 1;
    ensure_gap(out);
    out.push_str("```\n");
    let mut body = String::new();
    collect_pre(toks, i, &mut body);
    if body.ends_with('\n') {
        out.push_str(&body);
    } else {
        out.push_str(&body);
        out.push('\n');
    }
    out.push_str("```\n");
}

fn collect_pre(toks: &[Tok], i: &mut usize, out: &mut String) {
    let mut depth = 1i32;
    while *i < toks.len() && depth > 0 {
        let kind = clone_tok(&toks[*i]);
        *i += 1;
        match kind {
            Tok::End(name) if name == "pre" => depth -= 1,
            Tok::Start {
                name, self_close, ..
            } if name == "pre" && !self_close => depth += 1,
            Tok::Text(text) => out.push_str(&decode_entities(&text)),
            Tok::Start { .. } | Tok::End(_) => {}
        }
    }
}

fn render_inline(toks: &[Tok], i: &mut usize, out: &mut String, stop: &str) {
    while *i < toks.len() {
        let kind = clone_tok(&toks[*i]);
        *i += 1;
        match kind {
            Tok::End(name) if name == stop => return,
            Tok::End(_) => {}
            Tok::Text(text) => push_inline(out, &text, false),
            Tok::Start {
                name,
                attrs,
                self_close,
            } => {
                if self_close || is_void(&name) {
                    if name == "br" {
                        out.push('\n');
                    }
                    continue;
                }
                if name == "a" {
                    let href = attr(&attrs, "href");
                    let mut label = String::new();
                    render_inline(toks, i, &mut label, "a");
                    let label = label.trim();
                    if href.is_empty() {
                        push_inline(out, label, false);
                    } else {
                        out.push('[');
                        out.push_str(label);
                        out.push_str("](");
                        out.push_str(href.trim());
                        out.push(')');
                    }
                } else if name == "code" {
                    let mut label = String::new();
                    render_inline(toks, i, &mut label, "code");
                    out.push('`');
                    out.push_str(label.trim());
                    out.push('`');
                } else if matches!(name.as_str(), "ul" | "ol" | "pre" | "p")
                    || (name.starts_with('h') && name.len() == 2)
                {
                    // A block inside a paragraph ends the inline run. The block
                    // renderer picks it up because we stepped back onto its start.
                    *i = i.saturating_sub(1);
                    return;
                } else {
                    render_inline(toks, i, out, &name);
                }
            }
        }
    }
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> &'a str {
    attrs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .unwrap_or("")
}

fn clone_tok(tok: &Tok) -> Tok {
    tok.clone()
}

fn ensure_gap(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn trim_trailing_space(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
}

fn push_inline(out: &mut String, text: &str, pre: bool) {
    let decoded = decode_entities(text);
    if pre {
        out.push_str(&decoded);
        return;
    }
    let lead = decoded.chars().next().is_some_and(char::is_whitespace);
    let trail = decoded.chars().next_back().is_some_and(char::is_whitespace);
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        if lead && out.chars().next_back().is_some_and(|c| !c.is_whitespace()) {
            out.push(' ');
        }
        return;
    }
    let punct = collapsed.starts_with([',', '.', ';', ':', '!', '?']);
    if punct && out.ends_with(' ') {
        out.pop();
    } else if lead && out.chars().next_back().is_some_and(|c| !c.is_whitespace()) {
        out.push(' ');
    }
    out.push_str(&collapsed);
    if trail {
        out.push(' ');
    }
}

fn trim_blank_edges(text: &str) -> String {
    let mut out = text.trim().to_string();
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    if out.is_empty() {
        out
    } else {
        out.push('\n');
        out
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some((ch, next)) = entity_at(s, i) {
                out.push(ch);
                i = next;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn entity_at(s: &str, i: usize) -> Option<(char, usize)> {
    let rest = s.get(i + 1..)?;
    let end = rest.find(';')?;
    if end == 0 || end > 16 {
        return None;
    }
    let body = &rest[..end];
    let next = i + 1 + end + 1;
    let ch = if let Some(hex) = body.strip_prefix('#') {
        let code = if let Some(h) = hex.strip_prefix(['x', 'X']) {
            u32::from_str_radix(h, 16).ok()?
        } else {
            hex.parse().ok()?
        };
        char::from_u32(code)?
    } else {
        match body {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => ' ',
            _ => return None,
        }
    };
    Some((ch, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_links_lists_code_and_skipped_script() {
        let html = r#"<!doctype html><html><head><style>.x{color:red}</style><script>alert(1)</script></head>
<body>
<h1>Cabin</h1>
<p>See <a href="https://example.com/docs">docs</a>.</p>
<ul><li>pine</li><li>oak</li></ul>
<pre><code>fn cabin() {}</code></pre>
</body></html>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("# Cabin\n"), "{md}");
        assert!(md.contains("[docs](https://example.com/docs)"), "{md}");
        assert!(md.contains("- pine\n"), "{md}");
        assert!(md.contains("- oak\n"), "{md}");
        assert!(md.contains("fn cabin() {}"), "{md}");
        assert!(!md.contains("alert"), "{md}");
        assert!(!md.contains("color:red"), "{md}");
        assert!(md.contains('`') || md.contains("```"), "{md}");
    }
}
