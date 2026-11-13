//! Tool calls a model wrote into its reply as text (LO-51).
//!
//! A model that can't, or didn't, call a tool sometimes writes the call as a
//! tag in its reply: `<report_findings>{…}</report_findings>`. That is never
//! meant for the user. [`ToolTagFilter`] holds such a tag back from the
//! visible text, streaming or whole, and keeps the calls so the agent can
//! carry out the one it trusts (the findings card) and drop the rest.
//!
//! A tag counts when its name is snake_case with an underscore (every
//! two-word tool name), so prose like `<b>` or `a < b` passes through, and
//! nothing inside a ``` fence is touched.

/// Longest tag name taken as a tool name.
const NAME_MAX: usize = 48;

/// One call found in the text: the tag's name and what was between the tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenCall {
    pub name: String,
    pub body: String,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Text,
    /// After `<`: maybe the start of a tool tag.
    Open,
    /// Inside `<name>…`, waiting for `</name>`.
    Inside(String),
}

/// Streaming filter: feed reply chunks, get back the text to show.
#[derive(Debug, Default)]
pub struct ToolTagFilter {
    state: State,
    held: String,
    body: String,
    calls: Vec<WrittenCall>,
    fence: bool,
    /// Backticks seen at the start of the current line; `None` once the line
    /// has other text.
    ticks: Option<u8>,
}

impl ToolTagFilter {
    pub fn new() -> Self {
        Self {
            ticks: Some(0),
            ..Self::default()
        }
    }

    /// The part of `chunk` (and of what was held back before it) to show.
    pub fn feed(&mut self, chunk: &str) -> String {
        let mut out = String::new();
        for c in chunk.chars() {
            self.push(c, &mut out);
        }
        out
    }

    /// Whatever is still held back that turned out to be text.
    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        match std::mem::take(&mut self.state) {
            State::Text => {}
            State::Open => out.push_str(&std::mem::take(&mut self.held)),
            State::Inside(name) => {
                let body = std::mem::take(&mut self.body);
                let trimmed = body.trim_start();
                // An unclosed call (`<report_findings>{…`) is still a call;
                // anything else was prose after all.
                if trimmed.starts_with(['{', '[']) {
                    self.calls.push(WrittenCall {
                        name,
                        body: body.trim().to_string(),
                    });
                } else {
                    out.push_str(&format!("<{name}>{body}"));
                }
            }
        }
        out
    }

    /// The calls found so far, oldest first.
    pub fn take_calls(&mut self) -> Vec<WrittenCall> {
        std::mem::take(&mut self.calls)
    }

    fn push(&mut self, c: char, out: &mut String) {
        match &mut self.state {
            State::Text => {
                if c == '<' && !self.fence {
                    self.held.push(c);
                    self.state = State::Open;
                } else {
                    out.push(c);
                }
                self.track_fence(c);
            }
            State::Open => {
                self.held.push(c);
                let name = &self.held[1..self.held.len() - c.len_utf8()];
                let name_ok = |n: &str| is_tool_name(n);
                match c {
                    'a'..='z' | '0'..='9' | '_' if self.held.len() <= NAME_MAX + 2 => {
                        if self.held.len() == 2 && !c.is_ascii_lowercase() {
                            self.flush_held(out);
                        }
                    }
                    '>' if name_ok(name) => {
                        self.state = State::Inside(name.to_string());
                        self.held.clear();
                        self.body.clear();
                    }
                    '/' if name_ok(name.trim_end()) => {}
                    ' ' if name_ok(name) => {}
                    '>' if self.held.ends_with("/>") => {
                        let name = self.held[1..self.held.len() - 2].trim().to_string();
                        if is_tool_name(&name) {
                            self.calls.push(WrittenCall {
                                name,
                                body: String::new(),
                            });
                            self.held.clear();
                            self.state = State::Text;
                        } else {
                            self.flush_held(out);
                        }
                    }
                    _ => {
                        // Not a tool tag: show what was held, and read `c`
                        // again as text (it may open a new tag).
                        self.held.pop();
                        self.flush_held(out);
                        self.push(c, out);
                    }
                }
            }
            State::Inside(name) => {
                self.body.push(c);
                // A call's body is JSON or nested tags; `<dest_dir> here` is
                // a placeholder in prose, shown at once.
                if self
                    .body
                    .trim_start()
                    .starts_with(|c: char| !matches!(c, '{' | '[' | '<'))
                {
                    out.push_str(&format!("<{name}>{}", self.body));
                    self.body.clear();
                    self.state = State::Text;
                    return;
                }
                let close = format!("</{name}>");
                if self.body.ends_with(&close) {
                    let body = self.body[..self.body.len() - close.len()]
                        .trim()
                        .to_string();
                    let name = std::mem::take(name);
                    self.calls.push(WrittenCall { name, body });
                    self.body.clear();
                    self.state = State::Text;
                }
            }
        }
    }

    fn flush_held(&mut self, out: &mut String) {
        out.push_str(&std::mem::take(&mut self.held));
        self.state = State::Text;
    }

    fn track_fence(&mut self, c: char) {
        match (c, self.ticks) {
            ('\n', _) => self.ticks = Some(0),
            ('`', Some(n)) => {
                if n + 1 == 3 {
                    self.fence = !self.fence;
                    self.ticks = None;
                } else {
                    self.ticks = Some(n + 1);
                }
            }
            (' ' | '\t', Some(0)) => {}
            _ => self.ticks = None,
        }
    }
}

/// A snake_case name with an underscore, like every two-word tool name.
fn is_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name.contains('_')
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `text` without the tool calls written into it.
pub fn strip_tool_tags(text: &str) -> String {
    if !text.contains('<') {
        return text.to_string();
    }
    let mut filter = ToolTagFilter::new();
    let mut out = filter.feed(text);
    out.push_str(&filter.finish());
    out
}

/// The tool calls written into `text`, oldest first.
pub fn written_calls(text: &str) -> Vec<WrittenCall> {
    if !text.contains('<') {
        return Vec::new();
    }
    let mut filter = ToolTagFilter::new();
    filter.feed(text);
    filter.finish();
    filter.take_calls()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAK: &str = "Here's what I found.\n\n<report_findings>\n{\"title\":\"Aqir tokens\",\"items\":[]}\n</report_findings>";

    #[test]
    fn a_written_tool_call_leaves_the_text_and_is_kept() {
        assert_eq!(strip_tool_tags(LEAK).trim(), "Here's what I found.");
        let calls = written_calls(LEAK);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "report_findings");
        assert!(calls[0].body.starts_with("{\"title\""));
    }

    #[test]
    fn streamed_in_pieces_it_is_held_back_the_same() {
        let mut f = ToolTagFilter::new();
        let mut shown = String::new();
        for piece in LEAK.as_bytes().chunks(3) {
            shown.push_str(&f.feed(std::str::from_utf8(piece).unwrap()));
        }
        shown.push_str(&f.finish());
        assert_eq!(shown.trim(), "Here's what I found.");
        assert_eq!(f.take_calls().len(), 1);
    }

    #[test]
    fn prose_html_and_code_pass_through() {
        for text in [
            "a < b and c > d",
            "use <b>bold</b> here",
            "<html> is a tag",
            "x<y_",
            "```\n<report_findings>{}</report_findings>\n```",
            "Press <Enter> to go",
        ] {
            assert_eq!(strip_tool_tags(text), text, "{text}");
        }
    }

    #[test]
    fn self_closing_and_unclosed_calls_are_dropped_too() {
        assert_eq!(strip_tool_tags("ok <load_tool_schema/> done"), "ok  done");
        assert_eq!(strip_tool_tags("ok <report_findings>{\"items\":["), "ok ");
        // An unclosed tag around prose stays prose.
        assert_eq!(
            strip_tool_tags("see <some_name>notes here"),
            "see <some_name>notes here"
        );
        for text in ["cp file <dest_dir>", "run `cp <src_dir> <dest_dir>` now"] {
            assert_eq!(strip_tool_tags(text), text, "{text}");
        }
        // A placeholder in prose is shown as it streams, not held to the end.
        let mut f = ToolTagFilter::new();
        assert_eq!(f.feed("cp <dest_dir> now"), "cp <dest_dir> now");
    }
}
