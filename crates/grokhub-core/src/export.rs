//! `/export html` and `/export json`. Markdown stays in the app (`export_markdown`).
//!
//! HTML is one standalone page: the visible chat (what the pane shows), replies
//! rendered from markdown, every string escaped, and only http(s) links.
//! JSON is the raw transcript with roles, for tools and diffs.

use crate::chat_view::{visible_chat_refs, ChatKind};
use crate::md::{md_blocks, md_link_ok, md_spans, MdAlign, MdBlock, MdSpan};

/// Formats `/export` writes. Markdown is the bare `/export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatExport {
    Markdown,
    Html,
    Json,
}

impl ChatExport {
    pub fn parse(arg: &str) -> Option<Self> {
        match arg.trim().to_ascii_lowercase().as_str() {
            "" | "md" | "markdown" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// `export.md`, `export.html`, `export.json`.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Markdown => "export.md",
            Self::Html => "export.html",
            Self::Json => "export.json",
        }
    }
}

pub const EXPORT_FORMATS_HINT: &str = "Export takes md, html, or json";

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn spans_html(line: &str) -> String {
    let mut out = String::new();
    for span in md_spans(line) {
        match span {
            MdSpan::Text(t) => out.push_str(&html_escape(&t)),
            MdSpan::Bold(t) => out.push_str(&format!("<strong>{}</strong>", html_escape(&t))),
            MdSpan::Italic(t) => out.push_str(&format!("<em>{}</em>", html_escape(&t))),
            MdSpan::Strike(t) => out.push_str(&format!("<del>{}</del>", html_escape(&t))),
            MdSpan::Code(t) => out.push_str(&format!("<code>{}</code>", html_escape(&t))),
            MdSpan::Link { text, url } if md_link_ok(&url) => out.push_str(&format!(
                "<a href=\"{}\" rel=\"noopener noreferrer\">{}</a>",
                html_escape(&url),
                html_escape(&text)
            )),
            MdSpan::Link { text, .. } => out.push_str(&html_escape(&text)),
        }
    }
    out
}

#[derive(PartialEq, Eq)]
enum OpenList {
    None,
    Ul,
    Ol,
}

fn close_list(out: &mut String, open: &mut OpenList) {
    match open {
        OpenList::Ul => out.push_str("</ul>\n"),
        OpenList::Ol => out.push_str("</ol>\n"),
        OpenList::None => {}
    }
    *open = OpenList::None;
}

fn align_attr(a: Option<&MdAlign>) -> &'static str {
    match a {
        Some(MdAlign::Center) => " style=\"text-align:center\"",
        Some(MdAlign::Right) => " style=\"text-align:right\"",
        _ => "",
    }
}

/// A reply's markdown as HTML. Same parse the cabin paints with.
pub fn md_to_html(text: &str) -> String {
    let mut out = String::new();
    let mut open = OpenList::None;
    for block in md_blocks(text) {
        let want = match &block {
            MdBlock::Bullet { .. } | MdBlock::Task { .. } => OpenList::Ul,
            MdBlock::Numbered { .. } => OpenList::Ol,
            _ => OpenList::None,
        };
        if want != open {
            close_list(&mut out, &mut open);
            match (&want, &block) {
                (OpenList::Ul, _) => out.push_str("<ul>\n"),
                (OpenList::Ol, MdBlock::Numbered { num, .. }) => {
                    let start = num.parse::<u64>().unwrap_or(1);
                    if start == 1 {
                        out.push_str("<ol>\n");
                    } else {
                        out.push_str(&format!("<ol start=\"{start}\">\n"));
                    }
                }
                _ => {}
            }
            open = want;
        }
        match block {
            MdBlock::Heading(level, t) => {
                // The page title is h1; reply headings start one below.
                let h = (level + 1).min(6);
                out.push_str(&format!("<h{h}>{}</h{h}>\n", spans_html(&t)));
            }
            MdBlock::Bullet { depth, text } => out.push_str(&format!(
                "<li style=\"margin-left:{}em\">{}</li>\n",
                depth as f32 * 1.2,
                spans_html(&text)
            )),
            MdBlock::Task { depth, done, text } => out.push_str(&format!(
                "<li class=\"task\" style=\"margin-left:{}em\"><input type=\"checkbox\" disabled{}> {}</li>\n",
                depth as f32 * 1.2,
                if done { " checked" } else { "" },
                spans_html(&text)
            )),
            MdBlock::Numbered { depth, text, .. } => out.push_str(&format!(
                "<li style=\"margin-left:{}em\">{}</li>\n",
                depth as f32 * 1.2,
                spans_html(&text)
            )),
            MdBlock::Quote(q) => {
                let lines: Vec<String> = q.lines().map(spans_html).collect();
                out.push_str(&format!("<blockquote>{}</blockquote>\n", lines.join("<br>")));
            }
            MdBlock::Rule => out.push_str("<hr>\n"),
            MdBlock::Table { header, align, rows } => {
                out.push_str("<table>\n<thead><tr>");
                for (i, h) in header.iter().enumerate() {
                    out.push_str(&format!("<th{}>{}</th>", align_attr(align.get(i)), spans_html(h)));
                }
                out.push_str("</tr></thead>\n<tbody>\n");
                for row in rows {
                    out.push_str("<tr>");
                    for (i, c) in row.iter().enumerate() {
                        out.push_str(&format!("<td{}>{}</td>", align_attr(align.get(i)), spans_html(c)));
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</tbody>\n</table>\n");
            }
            MdBlock::Code { lang, body } => {
                let class = if lang.is_empty() {
                    String::new()
                } else {
                    format!(" class=\"language-{}\"", html_escape(&lang))
                };
                out.push_str(&format!("<pre><code{class}>{}</code></pre>\n", html_escape(&body)));
            }
            MdBlock::Para(t) => out.push_str(&format!("<p>{}</p>\n", spans_html(&t))),
            MdBlock::Blank => {}
        }
    }
    close_list(&mut out, &mut open);
    out
}

const EXPORT_CSS: &str = "\
:root{--bg:#fff;--fg:#0a0a0a;--muted:#737373;--bubble:#eeeef0;--border:#e4e4e7;--link:#1d9bf0;--code:#f4f4f5}\
@media (prefers-color-scheme:dark){:root{--bg:#000;--fg:#e7e9ea;--muted:#71767b;--bubble:#16181c;--border:#2f3336;--code:#0b0c0e}}\
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.55 Inter,system-ui,sans-serif}\
main{max-width:760px;margin:0 auto;padding:32px 16px 64px}h1{font-size:22px;margin:0 0 24px}\
.msg{margin:0 0 14px;display:flex}.msg .b{padding:10px 14px;border-radius:16px;background:var(--bubble);max-width:100%;overflow-wrap:anywhere}\
.user{justify-content:flex-end}.user .b{max-width:84%;white-space:pre-wrap}\
.role{font-size:12px;color:var(--muted);margin:0 0 4px}\
details{margin:0 0 12px;color:var(--muted);font-size:13px}summary{cursor:pointer}\
details .body{white-space:pre-wrap;margin-top:6px}\
a{color:var(--link)}code{font-family:ui-monospace,Menlo,Consolas,monospace;font-size:13px;background:var(--code);padding:1px 4px;border-radius:4px}\
pre{background:var(--code);border:1px solid var(--border);border-radius:8px;padding:10px;overflow-x:auto}pre code{padding:0;background:none}\
table{border-collapse:collapse;margin:6px 0}th,td{border:1px solid var(--border);padding:4px 10px}\
blockquote{margin:6px 0;padding-left:10px;border-left:3px solid var(--border);color:var(--muted)}\
ul,ol{padding-left:1.4em;margin:6px 0}li.task{list-style:none;margin-left:-1.2em}p{margin:6px 0}\
footer{margin-top:32px;font-size:12px;color:var(--muted)}";

/// One standalone page of the visible chat. Workload turns stay out, as in the pane.
pub fn chat_export_html<'a>(
    title: &str,
    messages: impl IntoIterator<Item = (&'a str, &'a str)>,
    footer: &str,
) -> String {
    let title = if title.trim().is_empty() {
        "Chat"
    } else {
        title.trim()
    };
    let mut body = String::new();
    for view in visible_chat_refs(messages) {
        match view.kind {
            ChatKind::User => body.push_str(&format!(
                "<div class=\"msg user\"><div class=\"b\">{}</div></div>\n",
                html_escape(&view.body)
            )),
            ChatKind::Assistant | ChatKind::Result => body.push_str(&format!(
                "<div class=\"msg assistant\"><div class=\"b\">{}</div></div>\n",
                md_to_html(&view.body)
            )),
            ChatKind::Thought => body.push_str(&format!(
                "<details><summary>Thought process</summary><div class=\"body\">{}</div></details>\n",
                html_escape(&view.body)
            )),
            ChatKind::Tool => {
                let t = if view.title.trim().is_empty() {
                    "Work"
                } else {
                    view.title.as_str()
                };
                let rows = match crate::turn_timeline::decode_tool_rows(&view.body) {
                    Some(rows) => rows
                        .iter()
                        .map(|r| {
                            let name = crate::turn_timeline::tool_display_title(&r.title);
                            if r.detail.is_empty() {
                                name
                            } else {
                                format!("{name} — {}", r.detail)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    None => view.body.clone(),
                };
                body.push_str(&format!(
                    "<details><summary>{}</summary><div class=\"body\">{}</div></details>\n",
                    html_escape(t),
                    html_escape(&rows)
                ));
            }
        }
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{t}</title>\n<style>{EXPORT_CSS}</style>\n</head>\n<body>\n<main>\n<h1>{t}</h1>\n{body}\
<footer>{f}</footer>\n</main>\n</body>\n</html>\n",
        t = html_escape(title),
        f = html_escape(footer),
    )
}

/// The whole transcript, roles and all, as pretty JSON.
pub fn chat_export_json<'a>(
    title: &str,
    thread_id: &str,
    messages: impl IntoIterator<Item = (&'a str, &'a str)>,
    exported_ms: u64,
) -> String {
    let messages: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|(role, content)| serde_json::json!({ "role": role, "content": content }))
        .collect();
    let doc = serde_json::json!({
        "app": "GrokHub",
        "version": env!("CARGO_PKG_VERSION"),
        "title": title,
        "threadId": thread_id,
        "exportedMs": exported_ms,
        "messages": messages,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_parse_and_name_their_files() {
        assert_eq!(ChatExport::parse(""), Some(ChatExport::Markdown));
        assert_eq!(ChatExport::parse(" HTML "), Some(ChatExport::Html));
        assert_eq!(ChatExport::parse("json"), Some(ChatExport::Json));
        assert_eq!(ChatExport::parse("pdf"), None);
        assert_eq!(ChatExport::Markdown.file_name(), "export.md");
        assert_eq!(ChatExport::Html.file_name(), "export.html");
        assert_eq!(ChatExport::Json.file_name(), "export.json");
    }

    #[test]
    fn html_escapes_everything_the_chat_says() {
        let msgs = [
            ("user", "<script>alert(1)</script> & \"quotes\""),
            ("assistant", "Try `<b>` and [x](javascript:alert(1)) or [ok](https://x.ai?a=1&b=2)"),
        ];
        let page = chat_export_html("A <title>", msgs, "GrokHub");
        assert!(!page.contains("<script>alert"), "{page}");
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quotes&quot;"));
        assert!(page.contains("<code>&lt;b&gt;</code>"));
        assert!(!page.contains("href=\"javascript"), "{page}");
        assert!(page.contains("href=\"https://x.ai?a=1&amp;b=2\" rel=\"noopener noreferrer\""));
        assert!(page.contains("<title>A &lt;title&gt;</title>"));
        assert!(page.starts_with("<!doctype html>"));
    }

    #[test]
    fn replies_render_lists_tables_and_code() {
        let html = md_to_html(
            "# Plan\n1. one\n2. two\n- a\n- [x] done\n\n| k | v |\n|:-|-:|\n| a | 1 |\n```rust\nfn main() { 1 < 2 }\n```\n> note",
        );
        assert!(html.contains("<h2>Plan</h2>"), "{html}");
        assert!(html.contains("<ol>\n<li style=\"margin-left:0em\">one</li>"), "{html}");
        assert!(html.contains("</ol>\n<ul>\n"), "{html}");
        assert!(html.contains("<input type=\"checkbox\" disabled checked> done"), "{html}");
        assert!(html.contains("<td style=\"text-align:right\">1</td>"), "{html}");
        assert!(html.contains("<pre><code class=\"language-rust\">fn main() { 1 &lt; 2 }</code></pre>"), "{html}");
        assert!(html.contains("<blockquote>note</blockquote>"), "{html}");
        let started = md_to_html("3. three\n4. four");
        assert!(started.starts_with("<ol start=\"3\">"), "{started}");
    }

    #[test]
    fn json_keeps_every_turn_and_role() {
        let msgs = [("user", "hi \"there\""), ("assistant", "yo\nline"), ("system", "GOAL PIN: x")];
        let json = chat_export_json("T", "t1", msgs, 42);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["title"], "T");
        assert_eq!(v["threadId"], "t1");
        assert_eq!(v["exportedMs"], 42);
        assert_eq!(v["messages"].as_array().unwrap().len(), 3);
        assert_eq!(v["messages"][0]["content"], "hi \"there\"");
        assert_eq!(v["messages"][2]["role"], "system");
    }
}
