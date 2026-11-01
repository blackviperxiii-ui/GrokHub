//! The chat page ports the cabin's reply parser (`grokhub_core::md`) to
//! TypeScript (`ui/src/md.ts`). This runs the Rust parser over the inputs below
//! and checks `ui/src/md.fixtures.json` holds what it returns; the vitest suite
//! checks the TypeScript port returns the same. Change the parser or an input,
//! then rerun with `UPDATE_FIXTURES=1` to rewrite the file.

use std::path::PathBuf;

use grokhub_core::{code_tokens, md_blocks, md_spans, CodeTok, MdAlign, MdBlock, MdSpan};
use serde_json::{json, Value};

const INPUTS: &[&str] = &[
    "",
    "plain words",
    "# One\n## Two ##\n###### Six\n####### seven\n#nope",
    "1. one\n2) two\n  - nested\n- [x] done\n- [ ] todo\n- [X] caps\n* star\n+ plus",
    "2026 was a year.\n1.0.38 shipped\n10) ten\n1.\n-",
    "> a\n> b\n>c\nafter",
    "---\n* * *\n___\n--",
    "| Name | Size |\n|:-----|-----:|\n| a | 1 |\n| b |\nnot a row",
    "| L | C | R |\n|:--|:-:|--:|\n| `a|b` | c \\| d | **e** |\n| 1 | 2 | 3 | 4 |",
    "a | b\nplain",
    "| a |\n|---|",
    "```Rust\nfn main() {}\n```\nafter\n~~~py\nprint(1)",
    "```{.Python} extra\nx = 1\n\n```",
    "````\nnested ``` fence\n````",
    "text\r\nwith crlf\r\n",
    "**bold** and *it* and `co*de` ~~gone~~ [x](https://x.ai) snake_case_name",
    "a ** b\n2 * 3 * 4\n`open\n**\n__init__ and _it_ and x__y__z",
    "``code with ` tick`` and `` ` ``",
    "[bad](javascript:alert(1)) [data](data:text/html,x) [file](file:///etc/passwd) [ok](https://a.b/c \"title\")",
    "See https://x.ai/cli. Or (https://github.com/a_(b)).\nnohttps://x.ai and HTTPS://UP.CASE/x!",
    "<script>alert('x')</script>\n<b>bold?</b> <img src=x onerror=alert(1)>",
    "é **ü** ✓ 😀 *émoji* ~~ẞ~~",
    "  - two deep\n    - four\n\t- tab\n            - capped",
    "- \n- [ ]\n- [x]",
    "Intro\n\n- a\n- b\n\n| h |\n|---|\n| r |\n\n```sh\necho hi # note\n```",
    "### Heading with *marks* and [link](https://x.ai) ###",
    "*a**b*\n**a*b**\n_a_b_\n*a *\n* a*",
    "[nested [brackets]](https://x.ai) and [](https://empty.text)",
    "```js\nconst s = \"unterminated",
    ">\n> \n> > deep",
];

const CODE: &[(&str, &str)] = &[
    ("rust", "let x = \"a // b\"; // note 42"),
    ("python", "x = 10  # ten"),
    ("sh", "echo $#"),
    ("rust", "fn f<'a>(x: &'a str) -> char { 'c' }"),
    ("js", "const a1 = b2 + 3; let s = 'open"),
    ("diff", "+ let x"),
    ("", "a#b // c"),
    ("", "x = 1 # c"),
    ("sql", "SELECT 1 -- note"),
    ("json", "{\"a\": 1.5e3, \"b\": true}"),
    ("ts", "type T = `t${x}` // é"),
    ("go", "if x := 0x1F; x > 0 { return \"a\\\"b\" }"),
    ("py", "def café(): return None"),
];

fn spans(line: &str) -> Value {
    Value::Array(
        md_spans(line)
            .into_iter()
            .map(|s| match s {
                MdSpan::Text(t) => json!({ "kind": "text", "text": t }),
                MdSpan::Bold(t) => json!({ "kind": "bold", "text": t }),
                MdSpan::Italic(t) => json!({ "kind": "italic", "text": t }),
                MdSpan::Strike(t) => json!({ "kind": "strike", "text": t }),
                MdSpan::Code(t) => json!({ "kind": "code", "text": t }),
                MdSpan::Link { text, url } => json!({ "kind": "link", "text": text, "url": url }),
            })
            .collect(),
    )
}

fn align(a: MdAlign) -> &'static str {
    match a {
        MdAlign::Left => "left",
        MdAlign::Center => "center",
        MdAlign::Right => "right",
    }
}

fn block(b: MdBlock) -> Value {
    match b {
        MdBlock::Heading(level, text) => json!({ "kind": "heading", "level": level, "text": text }),
        MdBlock::Bullet { depth, text } => {
            json!({ "kind": "bullet", "depth": depth, "text": text })
        }
        MdBlock::Numbered { depth, num, text } => {
            json!({ "kind": "numbered", "depth": depth, "num": num, "text": text })
        }
        MdBlock::Task { depth, done, text } => {
            json!({ "kind": "task", "depth": depth, "done": done, "text": text })
        }
        MdBlock::Quote(text) => json!({ "kind": "quote", "text": text }),
        MdBlock::Rule => json!({ "kind": "rule" }),
        MdBlock::Table {
            header,
            align: al,
            rows,
        } => json!({
            "kind": "table",
            "header": header,
            "align": al.into_iter().map(align).collect::<Vec<_>>(),
            "rows": rows,
        }),
        MdBlock::Code { lang, body } => json!({ "kind": "code", "lang": lang, "body": body }),
        MdBlock::Para(text) => json!({ "kind": "para", "text": text }),
        MdBlock::Blank => json!({ "kind": "blank" }),
    }
}

fn tok(t: CodeTok) -> &'static str {
    match t {
        CodeTok::Plain => "plain",
        CodeTok::Keyword => "keyword",
        CodeTok::Str => "str",
        CodeTok::Comment => "comment",
        CodeTok::Number => "number",
    }
}

fn expected() -> Value {
    let cases: Vec<Value> = INPUTS
        .iter()
        .map(|input| {
            json!({
                "input": input,
                "blocks": md_blocks(input).into_iter().map(block).collect::<Vec<_>>(),
                "spans": input.lines().map(spans).collect::<Vec<_>>(),
            })
        })
        .collect();
    let code: Vec<Value> = CODE
        .iter()
        .map(|(lang, line)| {
            json!({
                "lang": lang,
                "line": line,
                "tokens": code_tokens(lang, line)
                    .into_iter()
                    .map(|(t, s)| json!([tok(t), s]))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "about": "Generated by src-tauri/src/md_fixtures.rs from grokhub_core::md. Rerun it with UPDATE_FIXTURES=1; do not edit by hand.",
        "cases": cases,
        "code": code,
    })
}

#[test]
fn ui_markdown_fixtures_match_the_core_parser() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/src/md.fixtures.json");
    let want = expected();
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        let text = serde_json::to_string_pretty(&want).expect("fixtures serialize");
        std::fs::write(&path, text + "\n").expect("write fixtures");
        return;
    }
    let have: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("read fixtures"))
        .expect("fixtures parse");
    assert!(
        have == want,
        "{} is out of date with grokhub_core::md; rerun with UPDATE_FIXTURES=1",
        path.display()
    );
}
