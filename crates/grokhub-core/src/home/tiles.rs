//! Tiles both UIs build the same way from core facts: the Steam library,
//! drives and the game guard, and the modules still waiting on their own
//! data. The desktop app and the shell feed these and draw the result.

use super::text;
use super::view::{TileAction, TileRow, TileText, TileView, Tone};
use super::{FREE_STORAGE_ASK, HOME_CHIPS, TUNE_UP_ASK};
use crate::disk::DiskUse;
use crate::game_guard::GameGuard;

/// Rows a list tile builds; each size shows as many as fit.
const ROWS: usize = 8;

/// One Steam game as the tiles need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayedGame {
    pub app_id: u32,
    pub name: String,
    pub installed: bool,
    /// Unix seconds, 0 when never played.
    pub last_played: u64,
    pub playtime_min: u64,
}

fn hours(min: u64) -> String {
    match min {
        0 => "never played".into(),
        m if m < 60 => format!("{m} min"),
        m => format!("{:.1} h", m as f64 / 60.0),
    }
}

pub fn now_playing_view(games: &[PlayedGame], now: u64) -> TileView {
    let mut played: Vec<&PlayedGame> = games.iter().filter(|g| g.last_played > 0).collect();
    played.sort_by_key(|g| std::cmp::Reverse(g.last_played));
    let Some(top) = played.first() else {
        return TileView::empty(
            "Play a Steam game and it shows here",
            "Open Library",
            TileAction::nav("library"),
        );
    };
    let ago = crate::pulse::ago_label(top.last_played * 1000, now);
    let rows = played
        .iter()
        .take(ROWS)
        .map(|g| {
            TileRow::new(g.name.clone(), hours(g.playtime_min), Tone::Normal)
                .on_tap(TileAction::Play { app_id: g.app_id })
        })
        .collect();
    TileView::ready(TileText::new(
        top.name.clone(),
        format!("Last played {ago} · {} total", hours(top.playtime_min)),
        Tone::Normal,
    ))
    .rows(rows)
    .button("Library", TileAction::nav("library"))
    .on_tap(TileAction::nav("library"))
}

pub fn game_library_view(games: &[PlayedGame]) -> TileView {
    let installed = games.iter().filter(|g| g.installed).count();
    if games.is_empty() {
        return TileView::empty(
            "No Steam games found yet",
            "Open Library",
            TileAction::nav("library"),
        );
    }
    let rows = games
        .iter()
        .take(ROWS)
        .map(|g| {
            TileRow::new(
                g.name.clone(),
                if g.installed { "Installed" } else { "Updating" },
                if g.installed {
                    Tone::Normal
                } else {
                    Tone::Warn
                },
            )
        })
        .collect();
    TileView::ready(TileText::new(
        format!("{} games", games.len()),
        format!("{installed} installed · Steam"),
        Tone::Normal,
    ))
    .rows(rows)
    .button("Open Library", TileAction::nav("library"))
    .on_tap(TileAction::nav("library"))
}

/// One installed game for today: one you've played but not lately, picked
/// by the day so it holds still until tomorrow.
pub fn what_to_play_view(games: &[PlayedGame], now: u64, can_play: bool) -> TileView {
    let day = now / 86_400_000;
    let recent = (now / 1000).saturating_sub(3 * 86_400);
    let mut pool: Vec<&PlayedGame> = games
        .iter()
        .filter(|g| g.installed && g.last_played > 0 && g.last_played < recent)
        .collect();
    if pool.is_empty() {
        pool = games.iter().filter(|g| g.installed).collect();
    }
    pool.sort_by_key(|g| g.app_id);
    let Some(pick) = (!pool.is_empty()).then(|| pool[(day as usize) % pool.len()]) else {
        return TileView::empty(
            "Install a Steam game and today's pick shows here",
            "Open Library",
            TileAction::nav("library"),
        );
    };
    let caption = match pick.last_played {
        0 => "Never played · installed".to_string(),
        at => format!(
            "Last played {} · {}",
            crate::pulse::ago_label(at * 1000, now),
            hours(pick.playtime_min)
        ),
    };
    let v = TileView::ready(TileText::new(pick.name.clone(), caption, Tone::Normal));
    if can_play {
        v.primary(
            "Play",
            TileAction::Play {
                app_id: pick.app_id,
            },
        )
        .on_tap(TileAction::nav("library"))
    } else {
        v.on_tap(TileAction::nav("library"))
    }
}

/// The state of this PC in a few lines: drives, the game guard.
pub fn systems_brief_view(drives: &[DiskUse], guard: GameGuard) -> TileView {
    let storage = text::storage_text(drives, None);
    let game = text::game_mode_text(guard, None, false);
    let mut rows = vec![
        TileRow::new("Storage", storage.big.clone(), storage.tone),
        TileRow::new("Games", game.big.clone(), Tone::Normal),
    ];
    rows.extend(text::storage_rows(drives).into_iter().skip(1));
    let tone = storage.tone;
    let big = match tone {
        Tone::Alert => "Needs a look",
        Tone::Warn => "Mostly fine",
        _ => "All good",
    };
    TileView::ready(TileText::new(big, "This PC right now", tone))
        .rows(rows)
        .primary("Tune-up", TileAction::ask(TUNE_UP_ASK))
}

/// One tip from what this PC shows: a full drive first, then play state.
pub fn coach_tip_view(drives: &[DiskUse], guard: GameGuard) -> TileView {
    let fullest = text::storage_text(drives, None);
    if matches!(fullest.tone, Tone::Alert | Tone::Warn) {
        return TileView::ready(
            TileText::new("Free some space", fullest.caption, Tone::Warn)
                .detail("Games stutter while a full drive swaps and patches"),
        )
        .primary("Free up storage", TileAction::ask(FREE_STORAGE_ASK));
    }
    if let Some(ac) = guard.anticheat {
        return TileView::ready(TileText::new(
            "Hands off",
            format!(
                "{ac} is running, so {} stays out of the way",
                crate::brand::product_name()
            ),
            Tone::Quiet,
        ));
    }
    TileView::ready(
        TileText::new(
            "Warm up first",
            "Ten minutes of aim or drills before ranked",
            Tone::Normal,
        )
        .detail(format!(
            "Tips get sharper as {} learns your games",
            crate::brand::product_name()
        )),
    )
    .button("Fix my stutter", TileAction::ask(HOME_CHIPS[0]))
}

/// Modules whose own data comes next; until then they show the library
/// they'll draw from and one way in.
pub fn soon_view(module: &str, games: usize) -> TileView {
    let sentence = match module {
        "clips" => "Your latest clips and captures show here",
        "achievements" => "New unlocks for your games show here",
        "patch_notes" => "The latest patch for your games shows here",
        _ => "Sales on your wishlist show here",
    };
    let mut v = TileView::empty(sentence, "Open Library", TileAction::nav("library"));
    v.text.big = format!("{games} games");
    v
}

/// Modules with nothing to read yet: what they'll show and one way in.
pub fn waiting_view(module: &str) -> TileView {
    match module {
        "squad" => TileView::empty(
            "Friends online show here once Steam friends are linked",
            "Find a duo",
            TileAction::ask(HOME_CHIPS[1]),
        ),
        "tonight" => TileView::empty(
            "Your evening's sessions and events show here once a calendar is linked",
            "Plan my week",
            TileAction::ask(HOME_CHIPS[3]),
        ),
        "audio_check" => TileView::empty(
            "Your mic and output device show here",
            "Voice settings",
            TileAction::settings("general"),
        ),
        "stream_check" => TileView::empty(
            "Mic, scene and upload readiness show here before you go live",
            "Connectors",
            TileAction::nav("connectors"),
        ),
        "session_timer" => TileView::empty(
            "Track a play session and get a nudge to take a break",
            "Objectives",
            TileAction::nav("objectives"),
        ),
        _ => TileView::empty(
            "A daily line from your companion shows here",
            "Personality",
            TileAction::settings("personality"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::TileState;

    const NOW: u64 = 1_800_000_000_000;

    fn game(
        app_id: u32,
        name: &str,
        installed: bool,
        last_played: u64,
        playtime_min: u64,
    ) -> PlayedGame {
        PlayedGame {
            app_id,
            name: name.into(),
            installed,
            last_played,
            playtime_min,
        }
    }

    #[test]
    fn now_playing_leads_with_the_last_game_played() {
        let secs = NOW / 1000;
        let games = [
            game(10, "Old", true, secs - 30 * 86_400, 600),
            game(20, "Fresh", true, secs - 3_600, 90),
            game(30, "Never", true, 0, 0),
        ];
        let v = now_playing_view(&games, NOW);
        assert_eq!(v.text.big, "Fresh");
        assert_eq!(v.text.caption, "Last played 1h ago · 1.5 h total");
        assert_eq!(v.rows.len(), 2, "a never-played game isn't now playing");
        assert_eq!(v.rows[0].tap, Some(TileAction::Play { app_id: 20 }));
        assert_eq!(now_playing_view(&[], NOW).state, TileState::Empty);
    }

    #[test]
    fn what_to_play_holds_one_pick_all_day_and_plays_only_on_the_pc() {
        let secs = NOW / 1000;
        let games = [
            game(1, "A", true, secs - 10 * 86_400, 60),
            game(2, "B", true, secs - 20 * 86_400, 60),
            game(3, "Today", true, secs - 60, 60),
            game(4, "Updating", false, secs - 30 * 86_400, 60),
        ];
        let a = what_to_play_view(&games, NOW, true);
        let b = what_to_play_view(&games, NOW + 3_600_000, true);
        assert_eq!(a.text.big, b.text.big, "the pick holds through the day");
        assert!(["A", "B"].contains(&a.text.big.as_str()), "{}", a.text.big);
        assert!(matches!(a.buttons[0].action, TileAction::Play { .. }));
        assert!(what_to_play_view(&games, NOW, false).buttons.is_empty());
    }

    #[test]
    fn coach_tip_puts_a_full_drive_first() {
        let full = [DiskUse {
            label: "C:".into(),
            total: 1_000,
            free: 20,
        }];
        let v = coach_tip_view(&full, GameGuard::default());
        assert_eq!(v.text.big, "Free some space");
        let ok = [DiskUse {
            label: "C:".into(),
            total: 1_000,
            free: 600,
        }];
        let ac = GameGuard {
            anticheat: Some("Vanguard"),
            gaming: true,
            idle_anticheat: None,
        };
        assert_eq!(coach_tip_view(&ok, ac).text.big, "Hands off");
    }

    #[test]
    fn waiting_tiles_say_what_they_will_show_and_offer_one_way_in() {
        for m in [
            "squad",
            "tonight",
            "audio_check",
            "stream_check",
            "session_timer",
            "hype_line",
        ] {
            let v = waiting_view(m);
            assert_eq!(v.state, TileState::Empty, "{m}");
            assert_eq!(v.buttons.len(), 1, "{m}");
        }
        assert_eq!(soon_view("deals", 12).text.big, "12 games");
    }
}
