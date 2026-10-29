//! The "Sources" list at the end of a web-search reply. The model ends the
//! answer with a `Sources:` heading and one link per line (or the links on the
//! heading's own line); the chat paints them as compact chips with the site
//! name instead of a column of raw URLs.

/// One link from the list: the full URL and the site name a chip shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplySource {
    pub url: String,
    pub site: String,
}

/// Split `body` into the answer and its trailing Sources list. `None` when the
/// reply doesn't end in one, so the bubble paints exactly as before.
pub fn split_sources(body: &str) -> Option<(&str, Vec<ReplySource>)> {
    let trimmed = body.trim_end();
    let lines: Vec<(usize, &str)> = line_starts(trimmed).collect();
    let mut urls = Vec::new();
    let mut i = lines.len();
    while i > 0 {
        let line = lines[i - 1].1.trim();
        if !line.is_empty() {
            let Some(url) = list_url(line) else { break };
            urls.push(url);
        }
        i -= 1;
    }
    let (at, line) = lines[i.checked_sub(1)?];
    let (cut, inline) = if urls.is_empty() {
        // "… Sources: url url" on the last line, after the answer or alone.
        let lower = line.to_ascii_lowercase();
        let idx = lower.rfind("sources")?;
        let lead = &line[..idx];
        if !(lead.is_empty() || lead.ends_with([' ', '*', '_', '#'])) {
            return None;
        }
        let rest = heading_rest(&line[idx..])?;
        let lead = lead.trim_end().trim_end_matches(['*', '_', '#']).trim_end();
        (at + lead.len(), rest)
    } else {
        urls.reverse();
        (
            at,
            heading_rest(line.trim().trim_start_matches('#').trim_start())?,
        )
    };
    if !inline.is_empty() {
        if !urls.is_empty() {
            return None;
        }
        for token in inline.split([' ', ',', ';']).filter(|t| !t.is_empty()) {
            urls.push(list_url(token)?);
        }
    }
    let mut out: Vec<ReplySource> = Vec::new();
    for url in urls {
        if out.iter().any(|s| s.url == url) {
            continue;
        }
        let site = crate::pulse::source_host(&url)?;
        out.push(ReplySource { url, site });
    }
    if out.is_empty() {
        return None;
    }
    Some((trimmed[..cut].trim_end(), out))
}

fn line_starts(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split('\n').map(move |line| {
        let start = at;
        at += line.len() + 1;
        (start, line.trim_end_matches('\r'))
    })
}

/// The text after a `Sources:` heading (`Sources`, `**Sources:**`), or
/// `None` when the line isn't one.
fn heading_rest(line: &str) -> Option<&str> {
    let line = line.trim_start_matches(['*', '_']);
    if !line.get(..7)?.eq_ignore_ascii_case("sources") {
        return None;
    }
    let rest = line[7..].trim_start_matches(['*', '_']);
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let rest = rest.trim_start_matches(['*', '_']);
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// The link on one list line: `1. url`, `- [Title](url)`, `<url>`, or a bare
/// URL, optionally followed by a title. `None` for anything else.
fn list_url(line: &str) -> Option<String> {
    let mut rest = line.trim();
    if let Some(r) = rest.strip_prefix(['-', '*', '•']) {
        rest = r.trim_start();
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            rest = rest[digits..]
                .strip_prefix(['.', ')'])
                .map(str::trim_start)
                .unwrap_or(rest);
        }
    }
    let url = if let Some(r) = rest.strip_prefix('[') {
        let (_, after) = r.split_once("](")?;
        after.split(')').next()?
    } else {
        let r = rest.strip_prefix('<').unwrap_or(rest);
        r.split([' ', '>', '\t']).next()?
    };
    let url = url.trim_end_matches(['.', ',', ';']);
    let ok = (url.starts_with("https://") || url.starts_with("http://")) && url.len() > 8;
    ok.then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sites(s: &[ReplySource]) -> Vec<&str> {
        s.iter().map(|x| x.site.as_str()).collect()
    }

    #[test]
    fn a_numbered_list_splits_off_with_site_names() {
        let body = "Rust 1.92 is out.\n\nSources:\n1. https://blog.rust-lang.org/2026/10/30/Rust-1.92.html\n2. https://www.phoronix.com/news/Rust-1.92\n";
        let (answer, list) = split_sources(body).expect("sources");
        assert_eq!(answer, "Rust 1.92 is out.");
        assert_eq!(sites(&list), vec!["blog.rust-lang.org", "phoronix.com"]);
        assert_eq!(
            list[1].url,
            "https://www.phoronix.com/news/Rust-1.92".to_string()
        );
    }

    #[test]
    fn bold_headings_markdown_links_and_bullets_all_read() {
        let body = "Answer.\n\n**Sources:**\n- [Arch Wiki](https://wiki.archlinux.org/title/Framework)\n* <https://frame.work/laptop16> Framework\n";
        let (answer, list) = split_sources(body).expect("sources");
        assert_eq!(answer, "Answer.");
        assert_eq!(sites(&list), vec!["wiki.archlinux.org", "frame.work"]);
        assert_eq!(list[0].url, "https://wiki.archlinux.org/title/Framework");
        let heading = "Answer.\n## Sources\n- https://example.com/a";
        assert_eq!(
            sites(&split_sources(heading).unwrap().1),
            vec!["example.com"]
        );
    }

    #[test]
    fn links_on_the_heading_line_split_too_and_repeats_drop() {
        let body = "The Framework is repairable. Sources: https://frame.work/laptop16 https://www.dell.com/xps15, https://frame.work/laptop16";
        let (answer, list) = split_sources(body).expect("sources");
        assert_eq!(answer, "The Framework is repairable.");
        assert_eq!(sites(&list), vec!["frame.work", "dell.com"]);
        let own_line = "The Framework is repairable.\nSources: https://frame.work/laptop16 https://www.dell.com/xps15";
        let (answer, list) = split_sources(own_line).expect("sources");
        assert_eq!(answer, "The Framework is repairable.");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn replies_without_a_trailing_list_stay_whole() {
        assert_eq!(split_sources("Just an answer."), None);
        assert_eq!(split_sources("Sources:\n1. not a link"), None);
        assert_eq!(
            split_sources("Sources:\n1. https://a.com\n\nThen more text after."),
            None,
            "a list in the middle is part of the answer"
        );
        assert_eq!(split_sources("Sourcesless: https://a.com"), None);
        assert_eq!(
            split_sources("Answer.\n- https://a.com"),
            None,
            "no heading"
        );
    }
}
