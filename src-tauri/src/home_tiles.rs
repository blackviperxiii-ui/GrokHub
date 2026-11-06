//! Home tiles the shell draws from the shared tile model
//! (`grokhub_core::home::TileView`). Every catalog module that has no
//! dedicated tile in `home.rs` gets one here, from data this computer
//! already has: drives, the game guard, the egress log, the internet log,
//! the Steam library, the workboard, the journal and playbooks. A module
//! whose data isn't read yet says what it will show in one sentence and
//! offers one button that helps today.
//!
//! The words are core's (`grokhub_core::home::text`), so the desktop app
//! and the shell say the same thing about the same facts. Reads here never
//! touch a game: the guard looks at process names only.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_core::home::text::{self as tt, NetLink, SendFacts};
use grokhub_core::home::tiles;
use grokhub_core::home::{
    TileAction, TileRow, TileText, TileView, Tone, FREE_STORAGE_ASK, HOME_CHIPS, TUNE_UP_ASK,
};
use grokhub_core::local_profile::Gpu;

use crate::home::Sources;
use crate::{journal, library, local_ai, playbooks, pulse, workboard};

/// Shown on a phone for a tile that reads the PC itself.
fn brand() -> &'static str {
    grokhub_core::brand::product_name()
}

pub fn on_pc() -> String {
    format!("Shows on the PC that runs {}", brand())
}
/// Rows a list tile builds; each size shows as many as fit.
const ROWS: usize = 8;
/// GPU and RAM barely change; read them again after this.
const MACHINE_EVERY: Duration = Duration::from_secs(10 * 60);

/// The tile for `module`, or `None` for a module `home.rs` draws itself.
pub fn view(module: &str, src: &Sources<'_>) -> Option<TileView> {
    let now = src.clock.now_ms;
    let pc_only = |build: &dyn Fn() -> TileView| {
        if cfg!(mobile) {
            TileView::unsupported(&on_pc())
        } else {
            build()
        }
    };
    let v = match module {
        "pc_loadout" => pc_only(&|| pc_view(&machine(src.config_dir))),
        "speeds" => speeds_view(&grokhub_core::net_log::load(src.config_dir), now),
        "frame_rate" => frame_rate_view(),
        "storage" => pc_only(&|| storage_view(&drives())),
        "updates" => updates_view(&crate::shell_update::version()),
        "game_mode" => pc_only(&|| {
            game_mode_view(
                guard(),
                grokhub_agent::route::local::switched_on()
                    && grokhub_agent::route::local_gov::paused(),
            )
        }),
        "connection" => connection_view(src.config_dir, now),
        "usage" => usage_view(src.config_dir, now),
        "local_ai" => local_ai_view(
            &local_ai::load(src.config_dir),
            grokhub_agent::route::local::installed(),
            grokhub_agent::route::local::available(),
        ),
        "now_playing" => library_or(src, |games| tiles::now_playing_view(&played(games), now)),
        "game_library" => library_or(src, |games| tiles::game_library_view(&played(games))),
        "what_to_play" => library_or(src, |games| {
            tiles::what_to_play_view(&played(games), now, !src.web())
        }),
        "workboard" => match workboard::load(src.config_dir) {
            Ok(page) => workboard_view(&page),
            Err(e) => TileView::error(&e),
        },
        "decisions" => match workboard::load(src.config_dir) {
            Ok(page) => decisions_view(&page),
            Err(e) => TileView::error(&e),
        },
        "journal" => match journal::load(src.config_dir) {
            Ok(page) => journal_view(&page),
            Err(e) => TileView::error(&e),
        },
        "playbooks" => playbooks_view(&playbooks::load(src.config_dir)),
        "daily_drop" => match pulse::peek(src.config_dir, now) {
            Ok(page) => daily_drop_view(&page),
            Err(e) => TileView::error(&e),
        },
        "systems_brief" => pc_only(&|| tiles::systems_brief_view(&drives(), guard())),
        "coach_tip" => pc_only(&|| tiles::coach_tip_view(&drives(), guard())),
        "clips" | "achievements" | "patch_notes" | "deals" => {
            library_or(src, |games| tiles::soon_view(module, games.len()))
        }
        "squad" | "tonight" | "audio_check" | "stream_check" | "session_timer" | "hype_line" => {
            tiles::waiting_view(module)
        }
        _ => return None,
    };
    Some(v)
}

impl Sources<'_> {
    fn web(&self) -> bool {
        matches!(self.viewer, crate::home::Viewer::Web(_))
    }
}

/// What the game guard sees. Phones run no games beside the app and don't
/// build the desktop crate, so they see none.
#[cfg(desktop)]
fn guard() -> grokhub_core::game_guard::GameGuard {
    grokhub_desktop::game_guard::peek()
}

#[cfg(mobile)]
fn guard() -> grokhub_core::game_guard::GameGuard {
    grokhub_core::game_guard::GameGuard::default()
}

fn played(games: &[library::Game]) -> Vec<tiles::PlayedGame> {
    games
        .iter()
        .map(|g| tiles::PlayedGame {
            app_id: g.app_id,
            name: g.name.clone(),
            installed: g.installed,
            last_played: g.last_played,
            playtime_min: g.playtime_min,
        })
        .collect()
}

/// Tiles that scan the disk (the Steam library, drives) are kept for their
/// refresh time, so a screen asking every few seconds doesn't rescan.
/// Everything else is a small file read and is built fresh each ask.
fn scans_disk(module: &str) -> bool {
    matches!(
        module,
        "now_playing"
            | "game_library"
            | "what_to_play"
            | "clips"
            | "achievements"
            | "patch_notes"
            | "deals"
            | "storage"
            | "systems_brief"
            | "coach_tip"
    )
}

type Kept = Vec<((String, bool), TileView)>;
static KEPT: Mutex<Kept> = Mutex::new(Vec::new());

/// A kept tile still inside its refresh time, for a web or local viewer.
pub fn cached(module: &str, web: bool, now: u64) -> Option<TileView> {
    if !scans_disk(module) {
        return None;
    }
    let secs = grokhub_core::home::refresh_secs(module)?;
    let kept = KEPT.lock().ok()?;
    kept.iter()
        .find(|((m, w), _)| m == module && *w == web)
        .map(|(_, v)| v)
        .filter(|v| {
            v.updated_ms
                .is_some_and(|at| now.saturating_sub(at) < secs * 1000)
        })
        .cloned()
}

/// Keep a freshly built disk-scanning tile. Error tiles aren't kept, so the
/// next ask tries again.
pub fn keep(module: &str, web: bool, view: &TileView) {
    if !scans_disk(module) || view.state == grokhub_core::home::TileState::Error {
        return;
    }
    if let Ok(mut kept) = KEPT.lock() {
        kept.retain(|((m, w), _)| !(m == module && *w == web));
        kept.push(((module.to_string(), web), view.clone()));
    }
}

/// The Steam library, or why there isn't one.
fn library_or(src: &Sources<'_>, build: impl Fn(&[library::Game]) -> TileView) -> TileView {
    if cfg!(mobile) {
        return TileView::unsupported(&on_pc());
    }
    if !src.caps.library {
        return TileView::unsupported("The game library isn't on this computer");
    }
    let page = library::page(grokhub_core::steam::detect_root().as_deref());
    if !page.steam_found {
        return TileView::empty(
            "Install Steam to see your games here",
            "Open Library",
            TileAction::nav("library"),
        );
    }
    build(&page.games)
}

fn drives() -> Vec<grokhub_core::disk::DiskUse> {
    use grokhub_agent::indexers::{paths::PlatformDirs, system_state};
    system_state::local_drives(&system_state::OsProbe, &PlatformDirs::from_env())
}

/// This computer's GPUs and RAM, read at most every ten minutes.
fn machine(config_dir: &Path) -> local_ai::Machine {
    static SLOT: Mutex<Option<(Instant, local_ai::Machine)>> = Mutex::new(None);
    let mut slot = SLOT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, m)) = slot.as_ref() {
        if at.elapsed() < MACHINE_EVERY {
            return m.clone();
        }
    }
    let m = local_ai::read_machine(config_dir);
    *slot = Some((Instant::now(), m.clone()));
    m
}

fn mb_text(mb: u64) -> String {
    if mb >= 1024 {
        format!("{:.0} GB", mb as f64 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

fn main_gpu(gpus: &[Gpu]) -> Option<&Gpu> {
    gpus.iter()
        .filter(|g| !g.integrated)
        .max_by_key(|g| g.vram_mb)
        .or_else(|| gpus.first())
}

pub fn pc_view(m: &local_ai::Machine) -> TileView {
    let gpu = main_gpu(&m.gpus);
    let big = gpu.map_or_else(|| "This PC".to_string(), |g| g.name.clone());
    let caption = match (gpu.filter(|g| g.vram_mb > 0), m.ram_mb) {
        (Some(g), Some(ram)) => format!("{} VRAM · {} RAM", mb_text(g.vram_mb), mb_text(ram)),
        (None, Some(ram)) => format!("{} RAM", mb_text(ram)),
        (Some(g), None) => format!("{} VRAM", mb_text(g.vram_mb)),
        (None, None) => "Hardware not read yet".to_string(),
    };
    let mut rows = Vec::new();
    if let Some(g) = gpu.filter(|g| !g.driver.trim().is_empty()) {
        rows.push(TileRow::new("GPU driver", g.driver.trim(), Tone::Normal));
    }
    if let Some(ram) = m.ram_mb {
        rows.push(TileRow::new("Memory", mb_text(ram), Tone::Normal));
    }
    if let Some(free) = m.free_disk_mb {
        let tone = if free < 20 * 1024 {
            Tone::Warn
        } else {
            Tone::Normal
        };
        rows.push(TileRow::new("Free for models", mb_text(free), tone));
    }
    TileView::ready(TileText::new(big, caption, Tone::Normal))
        .rows(rows)
        .primary("Tune-up", TileAction::ask(TUNE_UP_ASK))
        .on_tap(TileAction::ask(TUNE_UP_ASK))
}

pub fn speeds_view(log: &grokhub_core::net_log::NetLog, now: u64) -> TileView {
    if !log.on {
        return TileView::off(
            "net_log",
            "Turn on the internet log to see outages and download speed",
        );
    }
    if log.down() {
        return TileView::ready(TileText::new(
            "Offline",
            "Neither public DNS server answered",
            Tone::Alert,
        ))
        .tag("DOWN");
    }
    let week = now.saturating_sub(7 * 86_400_000);
    let drops = log.outages.iter().filter(|o| o.start_ms >= week).count();
    let drops_line = match drops {
        0 => "No drops this week".to_string(),
        1 => "1 drop this week".to_string(),
        n => format!("{n} drops this week"),
    };
    let text = match log.speed.last() {
        Some(s) => TileText::new(
            format!("{:.0} Mbps", s.mbps),
            format!("Down · {}", grokhub_core::pulse::ago_label(s.at_ms, now)),
            Tone::Normal,
        )
        .detail(drops_line),
        None => TileText::new(
            "Online",
            "No speed test yet",
            if drops > 0 { Tone::Warn } else { Tone::Normal },
        )
        .detail(drops_line),
    };
    let rows = log
        .speed
        .iter()
        .rev()
        .take(ROWS)
        .map(|s| {
            TileRow::new(
                grokhub_core::pulse::ago_label(s.at_ms, now),
                format!("{:.0} Mbps", s.mbps),
                Tone::Normal,
            )
        })
        .collect();
    let at = log.speed.last().map(|s| s.at_ms);
    let mut v = TileView::ready(text).rows(rows);
    v.updated_ms = at;
    v
}

pub fn frame_rate_view() -> TileView {
    TileView::ready(
        TileText::new(
            "Not read",
            format!("{} never hooks into games", brand()),
            Tone::Quiet,
        )
        .detail("Xbox Game Bar (Win+G) or Steam's in-game counter show FPS safely"),
    )
    .button("Fix my stutter", TileAction::ask(HOME_CHIPS[0]))
}

pub fn storage_view(drives: &[grokhub_core::disk::DiskUse]) -> TileView {
    if drives.iter().all(|d| d.total == 0) {
        return TileView::error("Couldn't read this PC's drives");
    }
    TileView::ready(tt::storage_text(drives, None))
        .rows(tt::storage_rows(drives))
        .primary("Free up storage", TileAction::ask(FREE_STORAGE_ASK))
}

pub fn updates_view(version: &str) -> TileView {
    TileView::ready(TileText::new(
        format!("{} {version}", brand()),
        "Settings → About checks for a newer one",
        Tone::Normal,
    ))
    .button("Check for updates", TileAction::settings("about"))
    .on_tap(TileAction::settings("about"))
}

pub fn game_mode_view(guard: grokhub_core::game_guard::GameGuard, local_paused: bool) -> TileView {
    let on = guard.gaming || guard.anticheat.is_some();
    TileView::ready(tt::game_mode_text(guard, None, local_paused))
        .tag(if on { "ON" } else { "OFF" })
        .button("Library", TileAction::nav("library"))
        .on_tap(TileAction::nav("library"))
}

fn connection_view(config_dir: &Path, now: u64) -> TileView {
    let read = grokhub_agent::harness::read_egress_report(config_dir);
    let (sends, top) = match read.locked {
        Some(_) => (None, None),
        None => {
            let (n, top) =
                tt::count_sends(read.lines.iter().map(|l| (l.ts_ms, l.dest.as_str())), now);
            (Some(n), top)
        }
    };
    let facts = SendFacts {
        sends_24h: sends,
        top_dest: top,
        local_ready: grokhub_agent::route::local::available(),
    };
    connection_from(
        grokhub_agent::route::connectivity::state().link(),
        grokhub_agent::route::local::switched_on(),
        &facts,
    )
}

pub fn connection_from(net: NetLink, local_on: bool, facts: &SendFacts) -> TileView {
    TileView::ready(tt::connection_text(net, local_on, facts))
        .button("Privacy", TileAction::settings("privacy"))
        .on_tap(TileAction::settings("privacy"))
}

fn usage_view(config_dir: &Path, now: u64) -> TileView {
    use grokhub_agent::route::budget;
    let (reg, _) = grokhub_agent::route::live::snapshot(config_dir);
    let week = budget::load_week(config_dir, &reg, now);
    let records = budget::read_records(config_dir);
    let cache = grokhub_agent::cache_watch::share_since(
        &records,
        now.saturating_sub(grokhub_agent::cache_watch::WINDOW_MS),
    )
    .map(|s| s.pct);
    usage_from(week.spent_usd, cache, grokhub_agent::cache_watch::FLOOR_PCT)
}

pub fn usage_from(week_usd: f64, cache_pct: Option<u8>, floor: u8) -> TileView {
    let cache = cache_pct.map(|pct| {
        if pct < floor {
            format!("Prompt cache {pct}% (low)")
        } else {
            format!("Prompt cache {pct}%")
        }
    });
    let text = TileText {
        big: format!("${week_usd:.2}"),
        caption: "AI spend this week".into(),
        detail: cache,
        tone: if week_usd > 0.0 {
            Tone::Normal
        } else {
            Tone::Quiet
        },
    };
    TileView::ready(text)
        .button("Budget", TileAction::settings("defaults"))
        .on_tap(TileAction::settings("defaults"))
}

pub fn local_ai_view(cfg: &local_ai::LocalAiConfig, installed: bool, ready: bool) -> TileView {
    if !installed {
        return TileView::empty(
            "Set up an on-device model to keep chats going offline",
            "Set up",
            TileAction::settings("localAi"),
        );
    }
    let model = if cfg.local_model.selected.trim().is_empty() {
        "Picked for this PC".to_string()
    } else {
        let s = cfg.local_model.selected.trim();
        Path::new(s)
            .file_name()
            .map_or_else(|| s.to_string(), |f| f.to_string_lossy().into_owned())
    };
    let (big, tone) = if ready {
        ("Ready", Tone::Normal)
    } else {
        ("Paused", Tone::Warn)
    };
    let mut text = TileText::new(big, model, tone);
    if cfg.pause_for_games {
        text = text.detail("Steps aside while you play");
    }
    TileView::ready(text)
        .button("Local AI", TileAction::settings("localAi"))
        .on_tap(TileAction::settings("localAi"))
}

fn open_cards(
    page: &workboard::Page,
) -> impl Iterator<Item = (&workboard::Column, &workboard::Card)> {
    page.columns
        .iter()
        .filter(|c| c.lane != workboard::Lane::Done)
        .flat_map(|c| c.cards.iter().map(move |card| (c, card)))
}

fn card_tap(card: &workboard::Card) -> TileAction {
    match &card.chat {
        Some(id) => TileAction::OpenChat { id: id.clone() },
        None => TileAction::nav("workboard"),
    }
}

pub fn workboard_view(page: &workboard::Page) -> TileView {
    let open: Vec<_> = open_cards(page).collect();
    let done = page
        .columns
        .iter()
        .filter(|c| c.lane == workboard::Lane::Done)
        .map(|c| c.cards.len())
        .sum::<usize>();
    if open.is_empty() {
        return TileView::empty(
            "No cards on the board. Ask for something and it lands here",
            "Open Workboard",
            TileAction::nav("workboard"),
        );
    }
    let doing = open
        .iter()
        .filter(|(c, _)| c.lane == workboard::Lane::Doing)
        .count();
    let rows = open
        .iter()
        .take(ROWS)
        .map(|(col, card)| {
            let tone = match col.lane {
                workboard::Lane::Blocked => Tone::Warn,
                workboard::Lane::FollowUp if card.fresh => Tone::Warn,
                _ => Tone::Normal,
            };
            TileRow::new(card.title.clone(), col.label.clone(), tone).on_tap(card_tap(card))
        })
        .collect();
    let total = open.len() + done;
    let pct = (done * 100).checked_div(total).unwrap_or(0) as u8;
    TileView::ready(TileText::new(
        format!("{} open", open.len()),
        format!("{doing} in progress · {done} done"),
        Tone::Normal,
    ))
    .rows(rows)
    .progress(pct)
    .button("Open Workboard", TileAction::nav("workboard"))
    .on_tap(TileAction::nav("workboard"))
}

/// What waits on you: blocked cards and follow-up reports you haven't opened.
pub fn decisions_view(page: &workboard::Page) -> TileView {
    let waiting: Vec<_> = open_cards(page)
        .filter(|(col, card)| {
            col.lane == workboard::Lane::Blocked
                || (col.lane == workboard::Lane::FollowUp && card.fresh)
                || card.can_resume
        })
        .collect();
    if waiting.is_empty() {
        return TileView::ready(TileText::new(
            "All clear",
            "Nothing waits on you",
            Tone::Quiet,
        ))
        .on_tap(TileAction::nav("workboard"));
    }
    let rows = waiting
        .iter()
        .take(ROWS)
        .map(|(col, card)| {
            let why = match col.lane {
                workboard::Lane::Blocked => "Blocked",
                workboard::Lane::FollowUp => "New report",
                _ => "Paused",
            };
            TileRow::new(card.title.clone(), why, Tone::Warn).on_tap(card_tap(card))
        })
        .collect();
    let n = waiting.len();
    TileView::ready(TileText::new(
        n.to_string(),
        if n == 1 {
            "waits on you"
        } else {
            "wait on you"
        },
        Tone::Warn,
    ))
    .rows(rows)
    .primary("Review", TileAction::nav("workboard"))
    .on_tap(TileAction::nav("workboard"))
}

pub fn journal_view(page: &journal::Page) -> TileView {
    if page.active.is_empty() {
        return TileView::empty(
            "Note a question or an experiment to keep it in view",
            "Open Journal",
            TileAction::nav("journal"),
        );
    }
    let rows = page
        .active
        .iter()
        .take(ROWS)
        .map(|q| {
            let tap = match &q.mission {
                Some(id) => TileAction::OpenChat { id: id.clone() },
                None => TileAction::nav("journal"),
            };
            TileRow::new(q.question.clone(), q.status.clone(), Tone::Normal).on_tap(tap)
        })
        .collect();
    TileView::ready(TileText::new(
        page.active.len().to_string(),
        format!("open · {} decided", page.history.len()),
        Tone::Normal,
    ))
    .rows(rows)
    .button("Open Journal", TileAction::nav("journal"))
    .on_tap(TileAction::nav("journal"))
}

pub fn playbooks_view(page: &playbooks::Page) -> TileView {
    if page.rows.is_empty() {
        return TileView::empty(
            "No playbooks yet",
            "Playbooks",
            TileAction::settings("playbooks"),
        );
    }
    let rows = page
        .rows
        .iter()
        .take(ROWS)
        .map(|r| {
            TileRow::new(r.name.clone(), r.steps.clone(), Tone::Normal)
                .on_tap(TileAction::settings("playbooks"))
        })
        .collect();
    let caption = match page.problems.len() {
        0 => "ready to run".to_string(),
        n => format!("ready · {n} need a fix"),
    };
    TileView::ready(TileText::new(
        page.rows.len().to_string(),
        caption,
        if page.problems.is_empty() {
            Tone::Normal
        } else {
            Tone::Warn
        },
    ))
    .rows(rows)
    .button("Playbooks", TileAction::settings("playbooks"))
    .on_tap(TileAction::settings("playbooks"))
}

pub fn daily_drop_view(page: &pulse::Page) -> TileView {
    let Some(idea) = page.ideas.iter().flat_map(|g| g.ideas.iter()).next() else {
        return TileView::empty(
            "Today's pick lands here once Pulse has ideas",
            "Open Pulse",
            TileAction::nav("pulse"),
        );
    };
    TileView::ready(TileText::new(
        idea.title.clone(),
        "Today's pick",
        Tone::Normal,
    ))
    .primary("Let's do it", TileAction::ask(&idea.action))
    .button("Open Pulse", TileAction::nav("pulse"))
    .on_tap(TileAction::nav("pulse"))
}

#[cfg(test)]
#[path = "home_tiles_tests.rs"]
mod tests;
