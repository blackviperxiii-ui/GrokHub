//! What each self-manage tool does once `harness::decide` let it through.
//! Every write goes through the ChangeLedger writers, so each one keeps the
//! version it replaced and shows as a Work-tree row with Undo.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::harness::{self as hx, Origin};
use crate::tools::ToolOutput;

use super::{self_tool, SKILL_CREATE_DAY_CAP};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Longest skill name, description, or trigger line kept.
const LINE_MAX: usize = 120;

/// Where a call runs and who started it.
pub struct SelfCtx<'a> {
    pub config_dir: &'a Path,
    pub origin: Origin,
    pub now_ms: u64,
}

impl<'a> SelfCtx<'a> {
    pub fn new(config_dir: &'a Path) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self { config_dir, origin: Origin::SelfManage, now_ms }
    }

    fn skills_dir(&self) -> PathBuf {
        self.config_dir.join("skills")
    }
}

/// Run one self-manage tool. The caller has already asked `harness::decide`
/// (and parked and got the user's click for delete and credentials).
/// `ask_token` shows the masked token card: the card's message in, the typed
/// value out. Connections and automations go through Spike-5b's writers.
pub fn run(ctx: &SelfCtx<'_>, name: &str, args: &Value, ask_token: &mut dyn FnMut(&str) -> Option<String>) -> ToolOutput {
    use crate::tools::{connections, control};
    let Some(tool) = self_tool(name) else {
        return ToolOutput::err(format!("unknown self-manage tool `{name}`"));
    };
    let out = match tool {
        "skill_list" => Ok(skill_list(ctx)),
        "skill_create" => skill_create(ctx, args),
        "skill_modify" => skill_modify(ctx, args),
        "skill_disable" => skill_move(ctx, args, false),
        "skill_enable" => skill_move(ctx, args, true),
        "skill_delete" => skill_delete(ctx, args),
        "connection_list" => return connections::list(),
        "connection_add" => return connections::add_with(args, ask_token),
        "connection_modify" => return connections::modify_with(args, ask_token),
        "connection_disable" => return connections::disable(args),
        "connection_remove" => return connections::delete(args),
        "automation_list" => control::list_automations(),
        "automation_create" => automation_write(ctx, args, None),
        "automation_modify" => {
            let id = arg(args, "id");
            automation_write(ctx, args, Some(&id))
        }
        "automation_disable" => control::disable_automation(&arg(args, "id"), &reason_of(args, "turned off by Grok")),
        "automation_delete" => control::delete_automation(&arg(args, "id")),
        other => Err(format!("unknown self-manage tool `{other}`")),
    };
    match out {
        Ok(text) => ToolOutput::ok(text),
        Err(e) => ToolOutput::err(e),
    }
}

/// `automation_create` / `automation_modify`: Spike-5b's scheduler writer
/// (ledger, Work-tree row, the 2-a-week cap). A modify keeps what it leaves out.
fn automation_write(ctx: &SelfCtx<'_>, args: &Value, id: Option<&str>) -> Result<String, String> {
    let current = match id {
        Some(id) => Some(crate::tools::control::find_automation(id)?.ok_or_else(|| format!("automation {id} not found"))?),
        None => None,
    };
    let mut instructions = arg(args, "instructions");
    if instructions.is_empty() {
        instructions = current.as_ref().map(|a| a.instructions.clone()).ok_or("an automation needs instructions")?;
    }
    let every = arg(args, "every");
    let mins = match (every.is_empty(), &current) {
        (true, Some(a)) => a.heartbeat_every_min.max(1),
        (true, None) => return Err("an automation needs `every` (10m, 1h, 1d)".into()),
        (false, _) => crate::tools::control::interval_minutes(&every)?,
    };
    crate::tools::control::write_automation(&instructions, mins, false, id, ctx.now_ms)
}

fn arg(args: &Value, key: &str) -> String {
    args.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
}

/// One frontmatter line: no newlines, capped.
fn line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(LINE_MAX).collect()
}

fn reason_of(args: &Value, fallback: &str) -> String {
    let r = line(&arg(args, "reason"));
    if r.is_empty() {
        fallback.into()
    } else {
        r
    }
}

fn skill_path(ctx: &SelfCtx<'_>, name: &str) -> Result<PathBuf, String> {
    let dir = grokhub_core::skill_dir_name(name);
    if dir.is_empty() {
        return Err("a skill needs a name".into());
    }
    Ok(ctx.skills_dir().join(dir).join("SKILL.md"))
}

fn read_skill(path: &Path) -> Option<grokhub_core::SkillMd> {
    let raw = std::fs::read_to_string(path).ok()?;
    grokhub_core::skill_safe(&raw).then(|| grokhub_core::parse_skill_md(&raw))
}

fn write_skill(path: &Path, s: &grokhub_core::SkillMd) -> Result<(), String> {
    for body in [&s.name, &s.description, &s.trigger, &s.instructions] {
        if !grokhub_core::skill_safe(body) {
            return Err("Secrets never in markdown".into());
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, grokhub_core::render_skill_md(s)).map_err(|e| e.to_string())
}

fn skill_list(ctx: &SelfCtx<'_>) -> String {
    let mut rows: Vec<String> = std::fs::read_dir(ctx.skills_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| read_skill(&e.path().join("SKILL.md")))
        .map(|s| format!("{}: {}", s.name, s.description))
        .collect();
    rows.sort();
    if rows.is_empty() {
        "no skills".into()
    } else {
        rows.join("\n")
    }
}

/// Skills these tools created in the 24 hours before `now_ms`.
pub fn skill_creates_today(config_dir: &Path, now_ms: u64) -> usize {
    hx::ChangeLedger::load(config_dir)
        .all()
        .iter()
        .filter(|c| c.op == hx::ChangeOp::Create && c.origin == Origin::SelfManage)
        .filter(|c| c.at + DAY_MS > now_ms)
        .count()
}

fn skill_create(ctx: &SelfCtx<'_>, args: &Value) -> Result<String, String> {
    let name = line(&arg(args, "name"));
    let path = skill_path(ctx, &name)?;
    if path.exists() {
        return Err(format!("a skill named {name} already exists; use skill_modify"));
    }
    if ctx.origin == Origin::SelfManage && skill_creates_today(ctx.config_dir, ctx.now_ms) >= SKILL_CREATE_DAY_CAP {
        return Err(format!(
            "skill_create is capped at {SKILL_CREATE_DAY_CAP} new skills a day; ask the user before adding more"
        ));
    }
    let instructions = arg(args, "instructions");
    if instructions.is_empty() {
        return Err("a skill needs instructions".into());
    }
    let skill = grokhub_core::SkillMd {
        name: name.clone(),
        description: line(&arg(args, "description")),
        trigger: line(&arg(args, "trigger")),
        instructions,
        ..Default::default()
    };
    let reason = reason_of(args, "created by Grok");
    let done = hx::record_skill_change(ctx.config_dir, &ctx.skills_dir(), &name, ctx.origin, &reason, || {
        write_skill(&path, &skill)
    })?;
    Ok(match done {
        Some(c) => format!("created skill {name} (change #{}; the user can Undo it)", c.seq),
        None => format!("skill {name} unchanged"),
    })
}

fn skill_modify(ctx: &SelfCtx<'_>, args: &Value) -> Result<String, String> {
    let name = line(&arg(args, "name"));
    let path = skill_path(ctx, &name)?;
    let Some(mut skill) = read_skill(&path) else {
        return Err(format!("no skill named {name}"));
    };
    for (key, field) in [
        ("description", &mut skill.description),
        ("trigger", &mut skill.trigger),
    ] {
        if args.get(key).is_some() {
            *field = line(&arg(args, key));
        }
    }
    if args.get("instructions").is_some() {
        skill.instructions = arg(args, "instructions");
    }
    let reason = reason_of(args, "changed by Grok");
    let done = hx::record_skill_change(ctx.config_dir, &ctx.skills_dir(), &name, ctx.origin, &reason, || {
        write_skill(&path, &skill)
    })?;
    Ok(match done {
        Some(c) => format!("changed skill {name} (change #{}; the user can Undo it)", c.seq),
        None => format!("skill {name} unchanged"),
    })
}

fn skill_delete(ctx: &SelfCtx<'_>, args: &Value) -> Result<String, String> {
    let name = line(&arg(args, "name"));
    let path = skill_path(ctx, &name)?;
    if !path.exists() {
        return Err(format!("no skill named {name}"));
    }
    let reason = reason_of(args, "deleted by Grok");
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let done = hx::record_skill_change(ctx.config_dir, &ctx.skills_dir(), &name, ctx.origin, &reason, || {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())
    })?;
    Ok(match done {
        Some(c) => format!("deleted skill {name} (change #{}; its last version is kept)", c.seq),
        None => format!("skill {name} unchanged"),
    })
}

/// `skill_disable` moves the folder to `skills/.disabled/` (the cabin and the
/// native engine skip dot folders); `skill_enable` moves it back. Each move
/// is one ledger line, so Undo brings the skill back as it was.
fn skill_move(ctx: &SelfCtx<'_>, args: &Value, enable: bool) -> Result<String, String> {
    let name = line(&arg(args, "name"));
    let on = skill_path(ctx, &name)?;
    let on_dir = on.parent().map(Path::to_path_buf).unwrap_or_default();
    let off_dir = ctx.skills_dir().join(".disabled").join(grokhub_core::skill_dir_name(&name));
    let (from, to) = if enable { (&off_dir, &on_dir) } else { (&on_dir, &off_dir) };
    if enable && on.exists() {
        let _ = std::fs::remove_dir_all(&off_dir);
        return Ok(format!("skill {name} is already on"));
    }
    if !from.join("SKILL.md").exists() {
        return Err(if enable { format!("no turned-off skill named {name}") } else { format!("no skill named {name}") });
    }
    let reason = reason_of(args, if enable { "turned back on by Grok" } else { "turned off by Grok" });
    let done = hx::record_skill_change(ctx.config_dir, &ctx.skills_dir(), &name, ctx.origin, &reason, || {
        if to.exists() {
            std::fs::remove_dir_all(to).map_err(|e| e.to_string())?;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(from, to).map_err(|e| e.to_string())
    })?;
    let verb = if enable { "turned on" } else { "turned off" };
    Ok(match done {
        Some(c) => format!("{verb} skill {name} (change #{}; the user can Undo it)", c.seq),
        None => format!("skill {name} unchanged"),
    })
}
