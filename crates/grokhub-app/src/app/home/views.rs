//! Home modules the desktop app draws from the shared tile model
//! (`grokhub_core::home::TileView`): every module without a painter of its
//! own in `paint.rs`. Facts come from what the cabin already holds in
//! memory (chats, the board, the journal, automations, playbooks, the Steam
//! library read on its own thread); the few checks that touch disk are
//! cached for the module's refresh time. Words shared with the shell come
//! from `grokhub_core::home::{text, tiles}`.

use grokhub_core::home::tiles::{self, PlayedGame};
use grokhub_core::home::{TileAction, TileRow, TileText, TileView, Tone};
use grokhub_core::workboard::BoardStatus;
use grokhub_core::JournalStatus;

use super::super::*;
use super::live::live_facts;

/// Rows a list tile builds; each size shows as many as fit.
const ROWS: usize = 8;

/// The tile for `module`, or `None` when it has a painter of its own.
pub(super) fn desktop_view(cabin: &mut Cabin, module: &str) -> Option<TileView> {
    let now = now_ms();
    let v = match module {
        "recent_chats" => recent_chats_view(&cabin.threads, now),
        "local_ai" => {
            let f = live_facts();
            local_ai_view(cabin.cfg.local_model, f.local_installed, f.local_ready)
        }
        "personality" => personality_view(&cabin.cfg.personality),
        "playbooks" => {
            if !cabin.playbooks.loaded {
                cabin.reload_playbooks();
            }
            playbooks_view(
                cabin.playbooks.all.iter().map(|l| l.playbook.name.clone()),
                cabin.playbooks.problems.len(),
            )
        }
        "automations" => automations_view(&cabin.automations, now),
        "journal" => journal_view(&cabin.journal),
        "decisions" => decisions_view(&cabin.board),
        "daily_drop" => {
            let (lines, _) =
                super::super::loadout_home::pulse_lines(&cabin.updates, &cabin.cfg.feed_pulse, now);
            daily_drop_view(lines.first().map(|(title, _)| title.as_str()))
        }
        "systems_brief" => {
            tiles::systems_brief_view(&live_facts().drives, grokhub_desktop::game_guard::peek())
        }
        "coach_tip" => {
            tiles::coach_tip_view(&live_facts().drives, grokhub_desktop::game_guard::peek())
        }
        "clips" => {
            let facts = live_facts();
            tiles::clips_view(&facts.clips, facts.steam_found, now)
        }
        "achievements" | "deals" | "patch_notes" => {
            let on = match module {
                "achievements" => cabin.ach_now().on,
                "deals" => cabin.wish_now().on,
                _ => cabin.cfg.game_companion,
            };
            tiles::feed_view(module, on, &tiles::feed_items(&cabin.updates, module), now)
        }
        "now_playing" | "game_library" | "what_to_play" => {
            let running = cabin.game_running();
            let (found, games) = cabin.steam_library(now);
            let games = played(games);
            if cabin.sources.steam.reading() {
                TileView::ready(TileText::new("—", "Reading your library…", Tone::Quiet))
            } else if !found {
                tiles::no_steam_view("Install Steam to see your games here")
            } else {
                match module {
                    "now_playing" => tiles::now_playing_view(&games, now, running),
                    "what_to_play" => tiles::what_to_play_view(&games, now, true, Some(rerolls())),
                    _ => tiles::game_library_view(&games),
                }
            }
        }
        "session_timer" => {
            let running = cabin.game_running();
            let game = running.or(grokhub_desktop::game_guard::peek().gaming.then_some(0));
            let clock = note_session(game, now);
            let name = running.and_then(|id| {
                cabin
                    .steam_library(now)
                    .1
                    .iter()
                    .find(|g| g.app_id == id)
                    .map(|g| g.name.clone())
            });
            tiles::session_timer_view(name.as_deref(), clock.started_ms, now)
        }
        "hype_line" => {
            let id = match cabin.cfg.personality.trim() {
                "" => grokhub_core::personality::DEFAULT_ID,
                id => id,
            };
            let name = grokhub_core::personality::builtins()
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| p.name);
            tiles::hype_line_view(name.as_deref(), now)
        }
        "audio_check" => tiles::audio_view(&live_facts().audio),
        "stream_check" => {
            let f = live_facts();
            let log = grokhub_core::net_log::load(&crate::config::config_dir());
            tiles::stream_view(&tiles::StreamFacts {
                app: f.stream_app,
                audio: f.audio,
                online: log.on.then(|| !log.down()),
            })
        }
        "squad" => tiles::squad_view(live_facts().steam_friends),
        "tonight" => tiles::tonight_view(&tonight_due(&cabin.automations), now),
        _ => return None,
    };
    Some(v.at(now))
}

/// `(name, next run)` for each automation that is on and scheduled.
pub(super) fn tonight_due(autos: &[grokhub_core::automation::Automation]) -> Vec<(String, u64)> {
    autos
        .iter()
        .filter(|a| a.enabled)
        .filter_map(|a| a.next_run.map(|t| (a.name.clone(), t)))
        .collect()
}

/// How many times What To Play was asked for another pick this run.
static REROLLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn rerolls() -> usize {
    REROLLS.load(std::sync::atomic::Ordering::Relaxed)
}

/// What To Play's Another: move to the next pick.
pub(super) fn reroll() {
    REROLLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

static SESSION: std::sync::Mutex<tiles::SessionClock> =
    std::sync::Mutex::new(tiles::SessionClock {
        game: None,
        started_ms: None,
    });

/// One play sample for the Session Timer: the running game, or 0 for a game
/// seen by process name only. The game watch feeds it too, so the clock
/// runs while Home is out of sight.
pub(in crate::app) fn note_session(game: Option<u32>, now: u64) -> tiles::SessionClock {
    let Ok(mut clock) = SESSION.lock() else {
        return tiles::SessionClock::default();
    };
    clock.note(game, now);
    *clock
}

/// The clip at `index` in the Clips tile's list, as last read.
pub(super) fn clip_path(index: usize) -> Option<std::path::PathBuf> {
    live_facts().clips.get(index).map(|c| c.path.clone())
}

/// The library reads installed games only.
fn played(games: &[grokhub_core::steam::SteamGame]) -> Vec<PlayedGame> {
    games
        .iter()
        .map(|g| PlayedGame {
            app_id: g.app_id,
            name: g.name.clone(),
            installed: true,
            last_played: g.last_played,
            playtime_min: g.playtime_min,
        })
        .collect()
}

pub(super) fn recent_chats_view(threads: &[ChatThread], now: u64) -> TileView {
    let mut chats: Vec<&ChatThread> = threads
        .iter()
        .filter(|t| {
            !t.scratch
                && t.archived_at.is_none()
                && t.deleted_at.is_none()
                && !t.messages.is_empty()
        })
        .collect();
    chats.sort_by_key(|t| std::cmp::Reverse(t.last_active_ms));
    let Some(top) = chats.first() else {
        return TileView::empty(
            "Your chats show here once you start one",
            "Start a chat",
            TileAction::nav("chat"),
        );
    };
    let rows = chats
        .iter()
        .take(ROWS)
        .map(|t| {
            TileRow::new(
                chat_title(t),
                grokhub_core::pulse::ago_label(t.last_active_ms, now),
                Tone::Normal,
            )
            .on_tap(TileAction::OpenChat { id: t.id.clone() })
        })
        .collect();
    TileView::ready(TileText::new(
        chat_title(top),
        format!(
            "Last chat · {}",
            grokhub_core::pulse::ago_label(top.last_active_ms, now)
        ),
        Tone::Normal,
    ))
    .rows(rows)
    .button("History", TileAction::nav("history"))
    .on_tap(TileAction::OpenChat { id: top.id.clone() })
}

fn chat_title(t: &ChatThread) -> String {
    let title = t.title.trim();
    if title.is_empty() {
        "New chat".to_string()
    } else {
        title.to_string()
    }
}

pub(super) fn local_ai_view(switched_on: bool, installed: bool, ready: bool) -> TileView {
    if !installed {
        return TileView::empty(
            "Set up an on-device model to keep chats going offline",
            "Set up",
            TileAction::settings("localAi"),
        );
    }
    if !switched_on {
        return TileView::off(
            "local_ai",
            "The on-device model is installed but switched off",
        );
    }
    let (big, tone) = if ready {
        ("Ready", Tone::Normal)
    } else {
        ("Paused", Tone::Warn)
    };
    TileView::ready(TileText::new(big, "On-device model", tone))
        .button("Local AI", TileAction::settings("localAi"))
        .on_tap(TileAction::settings("localAi"))
}

pub(super) fn personality_view(id: &str) -> TileView {
    let id = if id.trim().is_empty() {
        grokhub_core::personality::DEFAULT_ID
    } else {
        id.trim()
    };
    // Built-ins resolve in memory; a custom voice shows its id until the
    // Personality page loads it.
    let p = grokhub_core::personality::builtins()
        .into_iter()
        .find(|p| p.id == id);
    let (name, about) = match &p {
        Some(p) => (p.name.clone(), p.description.clone()),
        None => (id.to_string(), "Your own voice".to_string()),
    };
    let mut text = TileText::new(name, "Your companion's voice", Tone::Normal);
    if !about.trim().is_empty() {
        text = text.detail(about);
    }
    TileView::ready(text)
        .button("Change", TileAction::settings("personality"))
        .on_tap(TileAction::settings("personality"))
}

pub(super) fn playbooks_view(names: impl Iterator<Item = String>, problems: usize) -> TileView {
    let names: Vec<String> = names.collect();
    if names.is_empty() {
        return TileView::empty(
            "No playbooks yet",
            "Playbooks",
            TileAction::settings("playbooks"),
        );
    }
    let rows = names
        .iter()
        .take(ROWS)
        .map(|n| {
            TileRow::new(n.clone(), "Ready", Tone::Normal).on_tap(TileAction::settings("playbooks"))
        })
        .collect();
    let (caption, tone) = match problems {
        0 => ("ready to run".to_string(), Tone::Normal),
        n => (format!("ready · {n} need a fix"), Tone::Warn),
    };
    TileView::ready(TileText::new(names.len().to_string(), caption, tone))
        .rows(rows)
        .button("Playbooks", TileAction::settings("playbooks"))
        .on_tap(TileAction::settings("playbooks"))
}

/// "in 3h" / "in 12m" for a time ahead of now.
fn until_label(at: u64, now: u64) -> String {
    let min = at.saturating_sub(now) / 60_000;
    match min {
        0 => "now".into(),
        m if m < 60 => format!("in {m}m"),
        m if m < 48 * 60 => format!("in {}h", m / 60),
        m => format!("in {}d", m / (24 * 60)),
    }
}

pub(super) fn automations_view(
    autos: &[grokhub_core::automation::Automation],
    now: u64,
) -> TileView {
    if autos.is_empty() {
        return TileView::empty(
            "Schedule a check or a report and it runs on its own",
            "Open Automations",
            TileAction::nav("automations"),
        );
    }
    let on: Vec<_> = autos.iter().filter(|a| a.enabled).collect();
    let next = on
        .iter()
        .filter_map(|a| a.next_run)
        .filter(|t| *t >= now)
        .min();
    let mut sorted: Vec<_> = autos.iter().collect();
    sorted.sort_by_key(|a| (!a.enabled, a.next_run.unwrap_or(u64::MAX)));
    let rows = sorted
        .iter()
        .take(ROWS)
        .map(|a| {
            let (value, tone) = match (a.enabled, a.next_run) {
                (false, _) => ("Off".to_string(), Tone::Quiet),
                (true, Some(t)) if t >= now => {
                    (format!("Next {}", until_label(t, now)), Tone::Normal)
                }
                (true, _) => ("Waiting".to_string(), Tone::Normal),
            };
            TileRow::new(a.name.clone(), value, tone).on_tap(TileAction::nav("automations"))
        })
        .collect();
    let caption = match next {
        Some(t) => format!("on · next {}", until_label(t, now)),
        None => "on".to_string(),
    };
    TileView::ready(TileText::new(
        on.len().to_string(),
        caption,
        if on.is_empty() {
            Tone::Quiet
        } else {
            Tone::Normal
        },
    ))
    .rows(rows)
    .button("Open Automations", TileAction::nav("automations"))
    .on_tap(TileAction::nav("automations"))
}

fn journal_status(s: JournalStatus) -> &'static str {
    match s {
        JournalStatus::Open => "Open",
        JournalStatus::Investigating => "Looking into it",
        JournalStatus::ReadyToDecide => "Ready to decide",
        JournalStatus::Decided => "Decided",
        JournalStatus::Deferred => "Later",
        JournalStatus::Abandoned => "Dropped",
    }
}

pub(super) fn journal_view(entries: &[grokhub_core::JournalEntry]) -> TileView {
    let active: Vec<_> = entries
        .iter()
        .filter(|e| {
            matches!(
                e.status,
                JournalStatus::Open | JournalStatus::Investigating | JournalStatus::ReadyToDecide
            )
        })
        .collect();
    if active.is_empty() {
        return TileView::empty(
            "Note a question or an experiment to keep it in view",
            "Open Journal",
            TileAction::nav("journal"),
        );
    }
    let decided = entries
        .iter()
        .filter(|e| e.status == JournalStatus::Decided)
        .count();
    let rows = active
        .iter()
        .take(ROWS)
        .map(|e| {
            let tap = match &e.mission_thread {
                Some(id) => TileAction::OpenChat { id: id.clone() },
                None => TileAction::nav("journal"),
            };
            let tone = if e.status == JournalStatus::ReadyToDecide {
                Tone::Warn
            } else {
                Tone::Normal
            };
            TileRow::new(e.question.clone(), journal_status(e.status), tone).on_tap(tap)
        })
        .collect();
    TileView::ready(TileText::new(
        active.len().to_string(),
        format!("open · {decided} decided"),
        Tone::Normal,
    ))
    .rows(rows)
    .button("Open Journal", TileAction::nav("journal"))
    .on_tap(TileAction::nav("journal"))
}

/// What waits on you: blocked cards and reports a run left to read.
pub(super) fn decisions_view(board: &[grokhub_core::workboard::BoardCard]) -> TileView {
    let waiting: Vec<_> = board
        .iter()
        .filter(|c| matches!(c.status, BoardStatus::Blocked | BoardStatus::FollowUp))
        .collect();
    if waiting.is_empty() {
        return TileView::ready(TileText::new(
            "All clear",
            "Nothing waits on you",
            Tone::Quiet,
        ))
        .button("Open Workboard", TileAction::nav("workboard"))
        .on_tap(TileAction::nav("workboard"));
    }
    let rows = waiting
        .iter()
        .take(ROWS)
        .map(|c| {
            let (label, tone) = match c.status {
                BoardStatus::Blocked => ("Blocked", Tone::Warn),
                _ => ("Report to read", Tone::Normal),
            };
            let tap = match &c.thread_id {
                Some(id) => TileAction::OpenChat { id: id.clone() },
                None => TileAction::nav("workboard"),
            };
            TileRow::new(c.title.clone(), label, tone).on_tap(tap)
        })
        .collect();
    TileView::ready(TileText::new(
        waiting.len().to_string(),
        "waiting on you",
        Tone::Warn,
    ))
    .rows(rows)
    .primary("Review", TileAction::nav("workboard"))
    .on_tap(TileAction::nav("workboard"))
}

pub(super) fn daily_drop_view(top: Option<&str>) -> TileView {
    let Some(title) = top else {
        return TileView::empty(
            "Today's pick lands here once Pulse has ideas",
            "Open Pulse",
            TileAction::nav("pulse"),
        );
    };
    TileView::ready(TileText::new(title, "Today's pick", Tone::Normal))
        .primary("Open Pulse", TileAction::nav("pulse"))
        .on_tap(TileAction::nav("pulse"))
}

/// The Settings page a tile's section name opens. The names are the
/// shell's, so one tile model drives both apps.
pub(super) fn settings_sec(section: &str) -> SettingsSec {
    match section {
        "localAi" | "ai" | "models" => SettingsSec::AiModels,
        "privacy" => SettingsSec::Privacy,
        "personality" => SettingsSec::Personality,
        "playbooks" => SettingsSec::Playbooks,
        "defaults" => SettingsSec::Defaults,
        "about" | "update" => SettingsSec::Update,
        "connectors" => SettingsSec::Connectors,
        "pulse" => SettingsSec::Pulse,
        "automations" => SettingsSec::Automations,
        "pc" => SettingsSec::Behavior,
        _ => SettingsSec::Account,
    }
}

/// The section an off feature's Turn on opens.
pub(super) fn feature_sec(feature: &str) -> SettingsSec {
    match feature {
        "local_ai" => SettingsSec::AiModels,
        "net_log" => SettingsSec::Privacy,
        "steam_achievements" | "steam_wishlist" => SettingsSec::Pulse,
        _ => SettingsSec::Behavior,
    }
}
