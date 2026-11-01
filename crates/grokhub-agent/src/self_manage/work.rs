//! KB-02: the agent's lazy `work_*` tools for its own cards on the board.
//!
//! They stay out of the default tool list and out of the deferred index (KB-04):
//! no schema byte rides a normal turn. `load_tool_schema` takes them by name,
//! and the MCP `grokhub-self` list leaves them out too. Each call is a plain
//! local read or a locked read-change-write of `workboard.json`; no model is
//! involved. Reads are capped ([`WORK_GET_MAX`], [`WORK_LIST_MAX`]).
//!
//! The agent's own cards are the ones it filed (origin Agent). It reads and
//! updates only those, archives only those you never touched, and has no
//! delete.

use serde_json::{json, Value};

use grokhub_core::work_item::{
    self as wi, append_work_log, clip_line, StepStatus, WorkKind, WorkOrigin, WorkStep,
    CHECKPOINT_BLOCKER_MAX, CHECKPOINT_GOAL_MAX, CHECKPOINT_NEXT_MAX,
};
use grokhub_core::work_run as wr;
use grokhub_core::{BoardCard, BoardStatus};

use super::ops::SelfCtx;
use super::SelfClass;

/// Every `work_*` tool and its class. None deletes.
pub const WORK_TOOLS: &[(&str, SelfClass)] = &[
    ("work_get", SelfClass::Read),
    ("work_list", SelfClass::Read),
    ("work_update", SelfClass::Soft),
    ("work_add", SelfClass::Soft),
    ("work_archive", SelfClass::Soft),
];

/// Most bytes `work_get` returns.
pub const WORK_GET_MAX: usize = 1_200;
/// Most bytes `work_list` returns.
pub const WORK_LIST_MAX: usize = 1_500;
/// Most rows `work_list` returns.
pub const WORK_LIST_ROWS: usize = 20;
/// Longest note `work_update` logs.
pub const WORK_NOTE_MAX: usize = 160;
/// Longest title `work_add` keeps, and a `work_list` row shows.
const TITLE_MAX: usize = 80;

pub fn work_tool(leaf: &str) -> Option<&'static str> {
    WORK_TOOLS.iter().find(|(n, _)| *n == leaf).map(|(n, _)| *n)
}

pub fn class(leaf: &str) -> Option<SelfClass> {
    WORK_TOOLS.iter().find(|(n, _)| *n == leaf).map(|(_, c)| *c)
}

fn function(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }
    })
}

/// The native function schema for one `work_*` tool, for `load_tool_schema`.
pub fn schema(name: &str) -> Option<Value> {
    let id = json!({ "type": "string", "description": "Card id from work_list." });
    Some(match name {
        "work_get" => function(
            "work_get",
            "One of your board cards: status, goal, next, blocker, steps and refs.",
            json!({ "id": id }),
            &["id"],
        ),
        "work_list" => function(
            "work_list",
            "Your open board cards: id, status and title, newest first, at most 20.",
            json!({}),
            &[],
        ),
        "work_update" => function(
            "work_update",
            "Checkpoint one of your cards: a step's status, next, blocker, or a short note for its log.",
            json!({
                "id": id,
                "step": { "type": "integer", "description": "Step number from work_get, from 1." },
                "status": { "type": "string", "description": "The step's status: planned, doing, done, skipped or failed." },
                "next": { "type": "string", "description": "Where to resume. At most 200 bytes." },
                "blocker": { "type": "string", "description": "What stops the work. Empty clears it." },
                "note": { "type": "string", "description": "One log line, at most 160 bytes." }
            }),
            &["id"],
        ),
        "work_add" => function(
            "work_add",
            "File a new card for work you will do later, with an optional goal and steps.",
            json!({
                "title": { "type": "string" },
                "goal": { "type": "string" },
                "steps": { "type": "array", "items": { "type": "string" } }
            }),
            &["title"],
        ),
        "work_archive" => function(
            "work_archive",
            "Archive one of your cards that the user never touched. There is no delete.",
            json!({ "id": id }),
            &["id"],
        ),
        _ => return None,
    })
}

/// Cut to `max` bytes on a character boundary, keeping lines.
fn cap(text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max.saturating_sub(4);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n...", text[..end].trim_end())
}

fn status_word(s: BoardStatus) -> &'static str {
    match s {
        BoardStatus::Proposed | BoardStatus::Approved | BoardStatus::Staged => "proposed",
        BoardStatus::Todo => "todo",
        BoardStatus::InProgress => "doing",
        BoardStatus::Blocked => "blocked",
        BoardStatus::FollowUp => "follow_up",
        BoardStatus::Done => "done",
        BoardStatus::Dismissed => "archived",
    }
}

fn kind_word(k: WorkKind) -> &'static str {
    match k {
        WorkKind::Task => "task",
        WorkKind::Automation => "automation",
        WorkKind::Background => "background",
        WorkKind::Episode => "desktop",
        WorkKind::Flow => "flow",
    }
}

fn step_mark(s: StepStatus) -> &'static str {
    match s {
        StepStatus::Planned => "[ ]",
        StepStatus::Doing => "[>]",
        StepStatus::Done => "[x]",
        StepStatus::Skipped => "[-]",
        StepStatus::Failed => "[!]",
    }
}

fn mine(c: &BoardCard) -> bool {
    c.origin == Some(WorkOrigin::Agent)
}

fn arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

/// One card, compact, within [`WORK_GET_MAX`].
pub fn render_card(c: &BoardCard) -> String {
    let mut out = format!(
        "{} · {}\nstatus: {} · kind: {}",
        c.id,
        clip_line(&c.title, TITLE_MAX),
        status_word(c.status),
        kind_word(c.kind)
    );
    if let Some(cp) = &c.checkpoint {
        if cp.attempt > 0 {
            out.push_str(&format!(" · attempt {}", cp.attempt));
        }
        for (label, value) in [
            ("goal", &cp.goal),
            ("next", &cp.next),
            ("blocker", &cp.blocker),
        ] {
            if !value.is_empty() {
                out.push_str(&format!("\n{label}: {value}"));
            }
        }
    }
    if !c.steps.is_empty() {
        out.push_str("\nsteps:");
        for (i, s) in c.steps.iter().enumerate() {
            out.push_str(&format!("\n{} {}. {}", step_mark(s.status), i + 1, s.text));
        }
    }
    if let Some(cp) = c.checkpoint.as_ref().filter(|cp| !cp.refs.is_empty()) {
        out.push_str(&format!("\nrefs: {}", cp.refs.join(", ")));
    }
    cap(out, WORK_GET_MAX)
}

/// Your open cards, newest first, within [`WORK_LIST_ROWS`] and [`WORK_LIST_MAX`].
pub fn render_list(cards: &[BoardCard]) -> String {
    let open: Vec<&BoardCard> = cards
        .iter()
        .rev()
        .filter(|c| mine(c) && c.status != BoardStatus::Dismissed)
        .collect();
    if open.is_empty() {
        return "No open cards of yours.".into();
    }
    let mut out = String::new();
    let mut shown = 0;
    for c in open.iter().take(WORK_LIST_ROWS) {
        let row = format!(
            "{} · {} · {}\n",
            c.id,
            status_word(c.status),
            clip_line(&c.title, TITLE_MAX)
        );
        // Room for the "+N more" line.
        if out.len() + row.len() > WORK_LIST_MAX - 16 {
            break;
        }
        out.push_str(&row);
        shown += 1;
    }
    if open.len() > shown {
        out.push_str(&format!("+{} more", open.len() - shown));
    }
    cap(out.trim_end().to_string(), WORK_LIST_MAX)
}

fn find_mine<'a>(cards: &'a mut [BoardCard], id: &str) -> Result<&'a mut BoardCard, String> {
    let c = cards
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("No card `{}`.", clip_line(id, 64)))?;
    if !mine(c) {
        return Err(format!(
            "`{}` is the user's card. You can read and change only your own.",
            c.id
        ));
    }
    if c.status == BoardStatus::Dismissed {
        return Err(format!("`{}` is archived.", c.id));
    }
    Ok(c)
}

/// Run one `work_*` call.
pub fn run(ctx: &SelfCtx<'_>, tool: &str, args: &Value) -> Result<String, String> {
    let path = wr::board_path(ctx.config_dir);
    let now = ctx.now_ms;
    match tool {
        "work_list" => Ok(render_list(&wr::read_board_file(&path)?)),
        "work_get" => {
            let mut cards = wr::read_board_file(&path)?;
            find_mine(&mut cards, arg(args, "id")).map(|c| render_card(c))
        }
        "work_update" => work_update(ctx, args),
        "work_add" => {
            let title = clip_line(arg(args, "title"), TITLE_MAX);
            if title.is_empty() {
                return Err("work_add needs a title.".into());
            }
            let goal = clip_line(arg(args, "goal"), CHECKPOINT_GOAL_MAX);
            let steps: Vec<WorkStep> = args
                .get("steps")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|t| WorkStep {
                            text: t.to_string(),
                            status: StepStatus::Planned,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let id = wr::edit_board_file(&path, now, |cards| {
                let id = wr::open_run_card(cards, WorkKind::Task, &title, "", "", now);
                if let Some(c) = cards.iter_mut().find(|c| c.id == id) {
                    c.status = BoardStatus::Todo;
                    c.steps = wi::clean_steps(steps);
                    if let Some(cp) = c.checkpoint.as_mut() {
                        cp.attempt = 0;
                        if !goal.is_empty() {
                            cp.goal = goal.clone();
                        }
                        cp.next = wr::next_step(&c.steps);
                    }
                    wi::clean_card(c);
                }
                id
            })?;
            let _ = append_work_log(
                &wr::work_dir(ctx.config_dir),
                &id,
                now,
                wr::LOG_START,
                "filed",
            );
            Ok(format!("Added {id} in Todo."))
        }
        "work_archive" => {
            let id = arg(args, "id").to_string();
            wr::edit_board_file(&path, now, |cards| {
                let c = find_mine(cards, &id)?;
                if c.touched {
                    return Err(format!(
                        "`{}` is one the user touched. It stays until they archive it.",
                        c.id
                    ));
                }
                wi::archive(cards, &id, now);
                Ok::<String, String>(format!("Archived {id}. It can be restored from Archived."))
            })?
        }
        other => Err(format!("unknown work tool `{other}`")),
    }
}

fn work_update(ctx: &SelfCtx<'_>, args: &Value) -> Result<String, String> {
    let path = wr::board_path(ctx.config_dir);
    let now = ctx.now_ms;
    let id = arg(args, "id").to_string();
    let step = args.get("step").and_then(Value::as_u64);
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(wr::step_status);
    let next = args.get("next").and_then(Value::as_str);
    let blocker = args.get("blocker").and_then(Value::as_str);
    let note = clip_line(arg(args, "note"), WORK_NOTE_MAX);
    if step.is_none() && next.is_none() && blocker.is_none() && note.is_empty() {
        return Err("work_update needs a step and status, next, blocker or note.".into());
    }
    if step.is_some() != status.is_some() {
        return Err("A step change needs both step and status.".into());
    }
    let reply = wr::edit_board_file(&path, now, |cards| {
        let c = find_mine(cards, &id)?;
        if let (Some(n), Some(st)) = (step, status) {
            let at = (n as usize)
                .checked_sub(1)
                .filter(|i| *i < c.steps.len())
                .ok_or_else(|| format!("`{}` has no step {n}.", c.id))?;
            c.steps[at].status = st;
        }
        let auto_next = wr::next_step(&c.steps);
        let has_steps = !c.steps.is_empty();
        let title = c.title.clone();
        let cp = c.checkpoint.get_or_insert_with(Default::default);
        if cp.goal.is_empty() {
            cp.goal = title;
        }
        match next {
            Some(n) => cp.next = clip_line(n, CHECKPOINT_NEXT_MAX),
            None if step.is_some() && has_steps => cp.next = auto_next,
            None => {}
        }
        if let Some(b) = blocker {
            cp.blocker = clip_line(b, CHECKPOINT_BLOCKER_MAX);
        }
        cp.at_ms = now;
        wi::clean_card(c);
        Ok::<String, String>(render_card(c))
    })??;
    if !note.is_empty() {
        let _ = append_work_log(&wr::work_dir(ctx.config_dir), &id, now, wr::LOG_NOTE, &note);
    }
    Ok(reply)
}
