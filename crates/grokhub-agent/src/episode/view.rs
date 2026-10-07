//! The episode view (Spike-3b add-on, the OptChat pattern): a long episode
//! never grows one transcript. Each step the worker starts fresh with
//! [system] [this view] [goal step + latest observation].
//!
//! The view is a fixed byte budget ([`VIEW_BUDGET_BYTES`], UTF-8 bytes, not
//! tokens). Recent steps are one line each in the fixed step shape. When the
//! view overflows, the two oldest adjacent lines of the same level fold into
//! one parent of at most [`FOLD_CAP_BYTES`], so the oldest part is the
//! coarsest. New lines append at the end and a fold only swaps two lines for
//! their parent, so the view's prefix stays byte-identical between steps
//! until a fold happens (prompt caching holds).
//!
//! Folds are built only from already-redacted step lines, then passed
//! through `redact_secrets` and `redact_held_secrets`. Typed values, secrets,
//! screenshots and frames never go in. `zoom(ref, n)` opens a fold back into
//! its children, down to the raw redacted span row.

use std::collections::HashMap;

use crate::route::CallTokens;

/// The view's byte budget (UTF-8 bytes).
pub const VIEW_BUDGET_BYTES: usize = 48_000;
/// A fold (parent summary) is at most this many bytes.
pub const FOLD_CAP_BYTES: usize = 512;
/// The read-only tool that opens a folded line.
pub const ZOOM_TOOL: &str = "zoom";
/// First line of the view.
pub const VIEW_HEAD: &str = "Episode so far, oldest first. zoom(ref, n) opens a folded line:\n";

/// Instructions for the fold call (`background:compact`).
pub const FOLD_SYSTEM: &str = "You merge two summaries of steps in a desktop session into one summary. \
Keep tool names, decisions, failures and anything still waiting on approval. One plain line, no more than 400 characters.";

/// One line of the view: a step (level 0) or a fold of two lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewNode {
    pub id: String,
    pub level: u8,
    pub text: String,
    /// The two lines this one folds (empty on a step).
    pub children: Vec<String>,
    /// A step's raw redacted span row (JSON). Empty on a fold.
    pub raw: String,
}

impl ViewNode {
    fn line(&self) -> String {
        format!("[{}] {}\n", self.id, self.text)
    }
}

/// Builds a fold's text from its two children's lines.
pub trait Folder {
    /// The parent summary, plus the call's tokens when a model wrote it.
    fn fold(&self, left: &str, right: &str) -> Result<(String, Option<CallTokens>), String>;
}

/// One fold the view made while pushing a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldDone {
    pub id: String,
    pub children: [String; 2],
    pub tokens: Option<CallTokens>,
}

#[derive(Debug, Clone)]
pub struct EpisodeView {
    budget: usize,
    nodes: HashMap<String, ViewNode>,
    top: Vec<String>,
}

impl Default for EpisodeView {
    fn default() -> Self {
        Self::new(VIEW_BUDGET_BYTES)
    }
}

/// The longest prefix of `s` that fits in `max` bytes, on a char boundary.
pub fn clip_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl EpisodeView {
    pub fn new(budget: usize) -> Self {
        Self { budget, nodes: HashMap::new(), top: Vec::new() }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// The lines the worker sees now, oldest first.
    pub fn lines(&self) -> Vec<&ViewNode> {
        self.top.iter().filter_map(|id| self.nodes.get(id)).collect()
    }

    /// The view as the worker gets it.
    pub fn render(&self) -> String {
        let mut out = String::from(VIEW_HEAD);
        for node in self.lines() {
            out.push_str(&node.line());
        }
        out
    }

    pub fn node(&self, id: &str) -> Option<&ViewNode> {
        self.nodes.get(id)
    }

    /// Add one step line (already in the fixed step shape, from redacted span
    /// fields) and its raw redacted span row, then fold while over budget.
    pub fn push(&mut self, id: &str, line: &str, raw: &str, held: &[String], folder: &dyn Folder) -> Vec<FoldDone> {
        let text = clean(line, held).replace('\n', " ");
        self.nodes.insert(
            id.into(),
            ViewNode { id: id.into(), level: 0, text, children: Vec::new(), raw: clean(raw, held) },
        );
        self.top.push(id.into());
        let mut done = Vec::new();
        while self.render().len() > self.budget && self.top.len() >= 2 {
            done.push(self.fold_once(held, folder));
        }
        done
    }

    /// Fold the oldest adjacent pair of the same level (else the oldest pair).
    /// Both children exist before their parent is built.
    fn fold_once(&mut self, held: &[String], folder: &dyn Folder) -> FoldDone {
        let level = |id: &String| self.nodes.get(id).map_or(0, |n| n.level);
        let at = (0..self.top.len() - 1)
            .find(|&i| level(&self.top[i]) == level(&self.top[i + 1]))
            .unwrap_or(0);
        let (left, right) = (self.nodes[&self.top[at]].clone(), self.nodes[&self.top[at + 1]].clone());
        // A parent never outgrows its children, so every fold shrinks the view.
        let cap = FOLD_CAP_BYTES.min(left.line().len() + right.line().len()).saturating_sub(left.id.len() + 4);
        let (text, tokens) = match folder.fold(&left.text, &right.text) {
            Ok((text, tokens)) if !text.trim().is_empty() => (text, tokens),
            _ => (format!("{} | {}", left.text, right.text), None),
        };
        let text = clip_bytes(&clean(&text, held).replace('\n', " "), cap).trim().to_string();
        let id = format!("f{}", fold_span(&left.id, &right.id));
        self.nodes.insert(
            id.clone(),
            ViewNode {
                id: id.clone(),
                level: left.level.max(right.level).saturating_add(1),
                text,
                children: vec![left.id.clone(), right.id.clone()],
                raw: String::new(),
            },
        );
        self.top.splice(at..at + 2, [id.clone()]);
        FoldDone { id, children: [left.id, right.id], tokens }
    }

    /// `zoom(ref, n)`: a fold opens `n` levels (at least one) into its
    /// children's lines; a step returns its raw redacted span row. Read only.
    pub fn zoom(&self, id: &str, n: u32) -> Result<String, String> {
        let node = self.nodes.get(id.trim()).ok_or_else(|| format!("no line `{id}` in this episode"))?;
        if node.level == 0 {
            return Ok(node.raw.clone());
        }
        let mut frontier = vec![node];
        for _ in 0..n.max(1) {
            if frontier.iter().all(|n| n.children.is_empty()) {
                break;
            }
            frontier = frontier
                .into_iter()
                .flat_map(|n| {
                    if n.children.is_empty() {
                        vec![n]
                    } else {
                        n.children.iter().filter_map(|c| self.nodes.get(c)).collect()
                    }
                })
                .collect();
        }
        Ok(frontier.iter().map(|n| n.line()).collect())
    }
}

/// `s3`..`s4` → `3-4`; folds keep their outer bounds (`f1-2` + `f3-4` → `1-4`).
fn fold_span(left: &str, right: &str) -> String {
    let lo = left.trim_start_matches(['s', 'f']).split('-').next().unwrap_or("");
    let hi = right.trim_start_matches(['s', 'f']).rsplit('-').next().unwrap_or("");
    format!("{lo}-{hi}")
}

fn clean(text: &str, held: &[String]) -> String {
    grokhub_core::redact_held_secrets(&grokhub_core::redact_secrets(text), held)
}

/// The zoom tool schema for the worker.
pub fn zoom_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "name": ZOOM_TOOL,
        "description": "Open a folded line of the episode view into the lines it summarizes (read only, runs nothing).",
        "parameters": {
            "type": "object",
            "properties": {
                "span_ref": {"type": "string", "description": "The [ref] of a line in the episode view."},
                "n": {"type": "integer", "description": "How many levels to open. Defaults to 1."}
            },
            "required": ["span_ref"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Echoes both children, so a test sees exactly what a fold was built from.
    struct Echo(RefCell<Vec<(String, String)>>);

    impl Folder for Echo {
        fn fold(&self, left: &str, right: &str) -> Result<(String, Option<CallTokens>), String> {
            self.0.borrow_mut().push((left.into(), right.into()));
            Ok((format!("{} + {}", clip_bytes(left, 30), clip_bytes(right, 30)), None))
        }
    }

    fn step(n: u32) -> String {
        format!(r#"#{n} goal="open the settings" tool=click decision=allow ui_changed=true result="ok""#)
    }

    #[test]
    fn small_view_keeps_every_line_and_render_is_byte_measured() {
        let mut v = EpisodeView::new(VIEW_BUDGET_BYTES);
        let echo = Echo(RefCell::new(Vec::new()));
        for n in 1..=3 {
            assert!(v.push(&format!("s{n}"), &step(n), "{}", &[], &echo).is_empty());
        }
        assert_eq!(
            v.render(),
            format!("{VIEW_HEAD}[s1] {}\n[s2] {}\n[s3] {}\n", step(1), step(2), step(3))
        );
        assert!(echo.0.borrow().is_empty());
        assert_eq!(clip_bytes("héllo", 2), "h");
        assert_eq!(clip_bytes("héllo", 3), "hé");
    }

    #[test]
    fn overflow_folds_the_oldest_pair_and_zoom_opens_it() {
        let mut v = EpisodeView::new(VIEW_HEAD.len() + 3 * (step(1).len() + 6));
        let echo = Echo(RefCell::new(Vec::new()));
        for n in 1..=3 {
            v.push(&format!("s{n}"), &step(n), &format!(r#"{{"step":{n}}}"#), &[], &echo);
        }
        let folds = v.push("s4", &step(4), r#"{"step":4}"#, &[], &echo);
        assert_eq!(folds.len(), 1);
        assert_eq!(folds[0].id, "f1-2");
        assert_eq!(folds[0].children, ["s1".to_string(), "s2".to_string()]);
        let ids: Vec<&str> = v.lines().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["f1-2", "s3", "s4"]);
        assert_eq!(v.zoom("f1-2", 1).unwrap(), format!("[s1] {}\n[s2] {}\n", step(1), step(2)));
        assert_eq!(v.zoom("s1", 1).unwrap(), r#"{"step":1}"#);
        assert_eq!(v.zoom("nope", 1).unwrap_err(), "no line `nope` in this episode");
        assert!(v.render().len() <= v.budget());
    }
}
