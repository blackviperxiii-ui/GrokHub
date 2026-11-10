//! The best local model lineup for this PC: one model per goal (Speed,
//! Everyday, Performance, Image, Video) picked from the Download models rows,
//! so the hot-swap router has the right set on disk to switch between.
//!
//! Pure. It reads the same hardware profile and picker rows as the rest of
//! the Download models section (`local_picker`, `model_library`); it stores
//! nothing and sends nothing anywhere. The shells own the downloading.
//!
//! How each goal maps to hot-swap (`grokhub_agent::route::local_swap`):
//! - Everyday is the auto setup's default: the most capable model that is
//!   still quick here (speed first, `local_picker::default_among`).
//! - Speed is smaller than Everyday, so it is what runs on low battery, while
//!   a game runs, or when the machine saves power.
//! - Performance is bigger than Everyday and still fast enough, so it is what
//!   hard tasks step up to when the computer is free.

use crate::local_picker::{Memory, Profile};
use crate::local_setup::{self as ls, LocalModel, ModelKind, DISK_HEADROOM_MB, STANDARD_MODEL_CAP};
use crate::model_library::{Fit, Row};

/// Hot-swap only steps up to a model this fast (mirrors
/// `local_scale::PERFORMANCE_FLOOR_TOK_S`).
pub const PERFORMANCE_FLOOR_TOK_S: u32 = 12;

/// What a lineup slot is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Speed,
    Everyday,
    Performance,
    Image,
    Video,
}

impl Goal {
    /// Display order.
    pub const ALL: [Goal; 5] = [
        Goal::Speed,
        Goal::Everyday,
        Goal::Performance,
        Goal::Image,
        Goal::Video,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Goal::Speed => "Speed",
            Goal::Everyday => "Everyday",
            Goal::Performance => "Performance",
            Goal::Image => "Image",
            Goal::Video => "Video",
        }
    }
}

/// One goal's pick.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub goal: Goal,
    /// The model that fills the goal, or `None` with `note` saying why.
    pub pick: Option<&'static LocalModel>,
    /// What comes with the pick: its draft model, or the picture guard.
    pub extras: Vec<&'static LocalModel>,
    pub on_disk: bool,
    /// Why nothing fills the goal, or why the pick can't download now.
    pub note: String,
}

/// The recommended lineup and what downloading it fetches.
#[derive(Debug, Clone, PartialEq)]
pub struct Lineup {
    /// One slot per [`Goal`], in [`Goal::ALL`] order.
    pub slots: Vec<Slot>,
    /// What isn't on disk yet, most needed first, each draft model right
    /// after its chat model. Stops before the disk runs out.
    pub queue: Vec<&'static LocalModel>,
}

impl Lineup {
    pub fn slot(&self, g: Goal) -> Option<&Slot> {
        self.slots.iter().find(|s| s.goal == g)
    }
}

/// Rough tok/s for `m` on `p` with the whole GPU free, the way hot-swap
/// estimates it (`local_scale::est_tok_s`).
pub fn est_tok_s(p: &Profile, m: &LocalModel) -> u32 {
    let (mem, vram) = p.budget();
    match mem {
        Memory::Unified => m.tok_s.unified,
        Memory::System => m.tok_s.cpu,
        Memory::Gpu => {
            if m.run_mb <= vram {
                return m.tok_s.gpu;
            }
            if m.badges.contains(&"moe") && vram * 3 >= m.run_mb {
                return m.tok_s.gpu * 2 / 3;
            }
            let on_gpu = vram * 100 / m.run_mb.max(1);
            let cpu = u64::from(m.tok_s.cpu);
            let gpu = u64::from(m.tok_s.gpu);
            u32::try_from(cpu + gpu.saturating_sub(cpu) * on_gpu / 100 / 2).unwrap_or(u32::MAX)
        }
    }
}

/// A model the app may pick for the user: never an optional one.
fn pickable(r: &Row, kind: ModelKind) -> bool {
    r.model.kind == kind && !r.model.optional
}

fn row_of<'a>(rows: &'a [Row], id: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.model.id == id)
}

/// The recommended lineup for `p`. `free_disk_mb` bounds the queue (`None`
/// skips the disk check).
pub fn recommend(rows: &[Row], p: &Profile, free_disk_mb: Option<u64>) -> Lineup {
    let chat = |r: &&Row| pickable(r, ModelKind::Chat);
    // Everyday: the picker's default rule (speedy, then capable) among the
    // chat models that run well.
    let well: Vec<&'static LocalModel> = rows
        .iter()
        .filter(chat)
        .filter(|r| r.fit == Fit::Yes)
        .map(|r| r.model)
        .collect();
    let everyday = crate::local_picker::default_among(p, &well).and_then(|m| row_of(rows, m.id));
    let home_mb = everyday.map(|r| r.model.run_mb);
    // Speed: the fastest one smaller than Everyday; on a tie, the smaller.
    let speed = home_mb.and_then(|home| {
        rows.iter()
            .filter(chat)
            .filter(|r| r.fit == Fit::Yes && r.model.run_mb < home)
            .max_by(|a, b| {
                est_tok_s(p, a.model)
                    .cmp(&est_tok_s(p, b.model))
                    .then(b.model.run_mb.cmp(&a.model.run_mb))
            })
    });
    // Performance: the biggest one past Everyday that still runs fast enough.
    let performance = home_mb.and_then(|home| {
        rows.iter()
            .filter(chat)
            .filter(|r| r.fit != Fit::No && r.model.run_mb > home)
            .filter(|r| r.model.run_mb <= p.capacity())
            .filter(|r| est_tok_s(p, r.model) >= PERFORMANCE_FLOOR_TOK_S)
            .max_by_key(|r| r.model.run_mb)
    });
    // Image: the biggest picture model memory holds, with the picture guard.
    let image = rows
        .iter()
        .filter(|r| pickable(r, ModelKind::Image) && r.fit != Fit::No)
        .filter(|r| r.model.run_mb <= p.capacity())
        .max_by_key(|r| r.model.run_mb);
    let guard = rows
        .iter()
        .find(|r| pickable(r, ModelKind::ImageGuard) && r.fit != Fit::No);

    let drafter_of = |r: &Row| -> Vec<&'static LocalModel> {
        r.model
            .drafter
            .and_then(|id| row_of(rows, id))
            .map(|d| d.model)
            .into_iter()
            .collect()
    };
    let slot =
        |goal: Goal, pick: Option<&Row>, extras: Vec<&'static LocalModel>, none: String| Slot {
            goal,
            pick: pick.map(|r| r.model),
            extras,
            on_disk: pick.is_some_and(|r| r.on_disk),
            note: if pick.is_some() { String::new() } else { none },
        };
    let no_chat = || match rows.iter().find(|r| chat(r)) {
        Some(r) if !r.why.is_empty() => r.why.clone(),
        _ => "No chat model runs well on this PC.".to_string(),
    };
    let mut slots = vec![
        slot(
            Goal::Speed,
            speed,
            speed.map(drafter_of).unwrap_or_default(),
            if everyday.is_some() {
                "Everyday is already the quickest model that runs well here.".into()
            } else {
                no_chat()
            },
        ),
        slot(
            Goal::Everyday,
            everyday,
            everyday.map(drafter_of).unwrap_or_default(),
            no_chat(),
        ),
        slot(
            Goal::Performance,
            performance,
            performance.map(drafter_of).unwrap_or_default(),
            if everyday.is_some() {
                "Everyday is already the strongest model that runs well here.".into()
            } else {
                no_chat()
            },
        ),
        slot(
            Goal::Image,
            image,
            guard.map(|g| vec![g.model]).unwrap_or_default(),
            "No picture model fits this PC.".into(),
        ),
        Slot {
            goal: Goal::Video,
            pick: None,
            extras: Vec::new(),
            on_disk: false,
            note: "No on-device video models yet.".into(),
        },
    ];

    // The queue: most needed first. Each pick with its extras, or none of them.
    let mut room = free_disk_mb.map(|f| f.saturating_sub(DISK_HEADROOM_MB));
    let mut standard = rows
        .iter()
        .filter(|r| r.on_disk && ls::counts_toward_cap(r.model))
        .count();
    let on_disk = |m: &LocalModel| row_of(rows, m.id).is_some_and(|r| r.on_disk);
    let mut queue: Vec<&'static LocalModel> = Vec::new();
    for goal in [Goal::Everyday, Goal::Speed, Goal::Image, Goal::Performance] {
        let Some(s) = slots.iter_mut().find(|s| s.goal == goal) else {
            continue;
        };
        let Some(pick) = s.pick else { continue };
        let want: Vec<&'static LocalModel> = std::iter::once(pick)
            .chain(s.extras.iter().copied())
            .filter(|m| !on_disk(m) && !queue.iter().any(|q| q.id == m.id))
            .collect();
        if want.is_empty() {
            continue;
        }
        let new_standard = want.iter().filter(|m| ls::counts_toward_cap(m)).count();
        if standard + new_standard > STANDARD_MODEL_CAP {
            s.note = format!("You already have {STANDARD_MODEL_CAP} chat models.");
            continue;
        }
        let mb: u64 = want.iter().map(|m| m.download_mb).sum();
        if let Some(left) = room.as_mut() {
            if *left < mb {
                s.note = format!(
                    "Needs {} more free disk.",
                    ls::size_label(mb.saturating_sub(*left))
                );
                continue;
            }
            *left -= mb;
        }
        standard += new_standard;
        queue.extend(want);
    }
    Lineup { slots, queue }
}

/// One goal in a few words: "Speed: Gemma 4 E2B", "Video: none yet".
pub fn slot_line(s: &Slot) -> String {
    match s.pick {
        Some(m) if s.on_disk => format!("{}: {} (downloaded)", s.goal.label(), m.name),
        Some(m) if s.note.is_empty() => format!("{}: {}", s.goal.label(), m.name),
        Some(m) => format!("{}: {} ({})", s.goal.label(), m.name, s.note),
        None => format!("{}: {}", s.goal.label(), s.note),
    }
}

/// What the button will fetch, said before it does: how many, how big, and
/// which goal each fills.
pub fn lineup_line(l: &Lineup) -> String {
    let head = if l.queue.is_empty() {
        "This PC has its best lineup.".to_string()
    } else {
        let mb: u64 = l.queue.iter().map(|m| m.download_mb).sum();
        match l.queue.len() {
            1 => format!("1 model, {}.", ls::size_label(mb)),
            n => format!("{n} models, {} in all.", ls::size_label(mb)),
        }
    };
    let roles: Vec<String> = l.slots.iter().map(slot_line).collect();
    format!("{head} {}.", roles.join(" · "))
}

#[cfg(test)]
#[path = "model_lineup_tests.rs"]
mod tests;
