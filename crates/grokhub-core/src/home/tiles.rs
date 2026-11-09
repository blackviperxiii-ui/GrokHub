//! Tiles both UIs build the same way from core facts: the Steam library,
//! drives and the game guard, and the modules still waiting on their own
//! data. The desktop app and the shell feed these and draw the result.

use super::text;
use super::view::{TileAction, TileRow, TileText, TileView, Tone};
use super::{FREE_STORAGE_ASK, HOME_CHIPS, TUNE_UP_ASK};
use crate::audio_check::{AudioDevice, AudioFacts};
use crate::clips::Clip;
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

/// The game running now when the app can tell (`running`, a Steam app id),
/// else the last one played.
pub fn now_playing_view(games: &[PlayedGame], now: u64, running: Option<u32>) -> TileView {
    let mut played: Vec<&PlayedGame> = games
        .iter()
        .filter(|g| g.last_played > 0 || Some(g.app_id) == running)
        .collect();
    played.sort_by_key(|g| (Some(g.app_id) != running, std::cmp::Reverse(g.last_played)));
    if let Some(live) = played.first().filter(|g| Some(g.app_id) == running) {
        let rows = played
            .iter()
            .skip(1)
            .take(ROWS)
            .map(|g| TileRow::new(g.name.clone(), hours(g.playtime_min), Tone::Normal))
            .collect();
        return TileView::ready(TileText::new(
            live.name.clone(),
            match live.playtime_min {
                0 => "Playing now".to_string(),
                m => format!("Playing now · {} total", hours(m)),
            },
            Tone::Normal,
        ))
        .tag("LIVE")
        .rows(rows)
        .on_tap(TileAction::nav("library"));
    }
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
/// by the day so it holds still until tomorrow. `skip` moves along the list
/// when the player asks for another; `None` where there's no way to ask.
pub fn what_to_play_view(
    games: &[PlayedGame],
    now: u64,
    can_play: bool,
    skip: Option<usize>,
) -> TileView {
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
    let at = (day as usize).wrapping_add(skip.unwrap_or(0));
    let Some(pick) = (!pool.is_empty()).then(|| pool[at % pool.len()]) else {
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
    let mut v = TileView::ready(TileText::new(pick.name.clone(), caption, Tone::Normal));
    if skip.is_some() && pool.len() > 1 {
        v = v.button("Another", TileAction::Reroll);
    }
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

/// Steam's own download page, for a tile that needs Steam on a PC without it.
pub const STEAM_DOWNLOAD: &str = "https://store.steampowered.com/about/";

/// An empty tile on a PC without Steam: one tap to Steam's download page.
pub fn no_steam_view(text: &str) -> TileView {
    TileView::empty(
        text,
        "Get Steam",
        TileAction::OpenUrl {
            url: STEAM_DOWNLOAD.into(),
        },
    )
}

/// The newest clips and captures on this PC; a tap opens one. With none
/// yet, Steam's Settings is where Game Recording turns on (`steam` is
/// whether Steam is on this PC).
pub fn clips_view(clips: &[Clip], steam: bool, now: u64) -> TileView {
    let Some(top) = clips.first() else {
        if !steam {
            return no_steam_view(
                "Save a clip with your capture tool, or Steam's Game Recording, and it shows here",
            );
        }
        return TileView::empty(
            "Turn on Game Recording in Steam's Settings and your clips show here",
            "Steam settings",
            TileAction::SteamSettings,
        );
    };
    let rows = clips
        .iter()
        .take(ROWS)
        .enumerate()
        .map(|(index, c)| {
            TileRow::new(
                c.name.clone(),
                crate::pulse::ago_label(c.modified_ms, now),
                Tone::Normal,
            )
            .on_tap(TileAction::OpenClip { index })
        })
        .collect();
    let week = clips
        .iter()
        .filter(|c| c.modified_ms + 7 * 86_400_000 >= now)
        .count();
    TileView::ready(TileText::new(
        top.name.clone(),
        format!(
            "Newest clip · {} · {week} this week",
            crate::pulse::ago_label(top.modified_ms, now)
        ),
        Tone::Normal,
    ))
    .rows(rows)
    .primary("Watch", TileAction::OpenClip { index: 0 })
}

fn pick(list: &[AudioDevice]) -> Option<&AudioDevice> {
    list.iter().find(|d| d.is_default).or(list.first())
}

fn device_row(kind: &str, d: &AudioDevice) -> TileRow {
    let (value, tone) = match (d.muted, d.volume_pct) {
        (true, _) => ("Muted".to_string(), Tone::Alert),
        (false, Some(0)) => ("Volume 0%".to_string(), Tone::Warn),
        (false, Some(v)) => (format!("{} · {v}%", d.label()), Tone::Normal),
        (false, None) => (d.label().to_string(), Tone::Normal),
    };
    TileRow::new(kind, value, tone)
}

/// The mic and speakers in use and anything that would keep a squad from
/// hearing you. Read only; the full check (a short recording) is a button
/// away.
pub fn audio_view(f: &AudioFacts) -> TileView {
    if f.is_empty() {
        return waiting_view();
    }
    if f.outputs.is_empty() && f.inputs.is_empty() {
        let bad: Vec<&str> = f
            .devices
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(n, _)| n.as_str())
            .collect();
        let rows = f
            .devices
            .iter()
            .take(ROWS)
            .map(|(n, ok)| {
                let (v, t) = if *ok {
                    ("Working", Tone::Normal)
                } else {
                    ("Problem", Tone::Alert)
                };
                TileRow::new(n.clone(), v, t)
            })
            .collect();
        let text = match bad.first() {
            Some(name) => TileText::new(
                "Needs a look",
                format!("{name} reports a problem"),
                Tone::Alert,
            ),
            None => TileText::new(
                "Sound ready",
                match f.devices.len() {
                    1 => "1 sound device working".to_string(),
                    n => format!("{n} sound devices working"),
                },
                Tone::Normal,
            ),
        };
        return TileView::ready(text)
            .rows(rows)
            .button("Voice settings", TileAction::settings("general"));
    }
    let mic = pick(&f.inputs);
    let out = pick(&f.outputs);
    let text = match mic {
        None => TileText::new(
            "No mic found",
            "Plug one in to talk to your squad",
            Tone::Warn,
        ),
        Some(m) if m.muted => TileText::new("Mic muted", m.label().to_string(), Tone::Alert),
        Some(m) if m.volume_pct == Some(0) => {
            TileText::new("Mic at 0%", m.label().to_string(), Tone::Warn)
        }
        Some(m) => TileText::new("Mic ready", m.label().to_string(), Tone::Normal),
    };
    let tone = match out {
        Some(o) if o.muted => Tone::Alert,
        _ => text.tone,
    };
    let text = TileText { tone, ..text };
    let mut rows = Vec::new();
    if let Some(m) = mic {
        rows.push(device_row("Mic", m));
    }
    if let Some(o) = out {
        rows.push(device_row("Speakers", o));
    }
    TileView::ready(text)
        .rows(rows)
        .button("Voice settings", TileAction::settings("general"))
}

/// The streaming app open on this PC, from its process name.
pub fn stream_app(process_names: &[String]) -> Option<&'static str> {
    process_names.iter().find_map(|n| {
        let n = n.to_ascii_lowercase();
        let n = n.strip_suffix(".exe").unwrap_or(&n);
        match n {
            "obs" | "obs64" | "obs32" | "obs-studio" => Some("OBS Studio"),
            "streamlabs obs" | "streamlabs desktop" => Some("Streamlabs"),
            "xsplit.core" | "xsplit" => Some("XSplit"),
            "twitchstudio" | "twitch studio" => Some("Twitch Studio"),
            _ => None,
        }
    })
}

/// What the stream check reads: all of it already on this PC.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamFacts {
    pub app: Option<&'static str>,
    pub audio: AudioFacts,
    /// `Some(false)` when the internet log saw the link drop; `None` while
    /// the log is off.
    pub online: Option<bool>,
}

/// A go-live checklist: streaming app open, mic live, internet up.
pub fn stream_view(f: &StreamFacts) -> TileView {
    let app = match f.app {
        Some(name) => TileRow::new("Streaming app", format!("{name} open"), Tone::Normal),
        None => TileRow::new("Streaming app", "Not open", Tone::Warn),
    };
    let mic = pick(&f.audio.inputs);
    let mic_row = match (mic, f.audio.devices.iter().any(|(_, ok)| !ok)) {
        (Some(m), _) if m.muted => TileRow::new("Mic", "Muted", Tone::Alert),
        (Some(m), _) => TileRow::new("Mic", m.label().to_string(), Tone::Normal),
        (None, true) => TileRow::new("Mic", "A sound device reports a problem", Tone::Alert),
        (None, false) if !f.audio.devices.is_empty() => {
            TileRow::new("Mic", "Sound devices working", Tone::Normal)
        }
        (None, false) => TileRow::new("Mic", "Not found", Tone::Warn),
    };
    let net = match f.online {
        Some(true) => TileRow::new("Internet", "Up", Tone::Normal),
        Some(false) => TileRow::new("Internet", "Down", Tone::Alert),
        None => TileRow::new("Internet", "Not watched", Tone::Quiet),
    };
    let rows = vec![app, mic_row, net];
    let issues = rows
        .iter()
        .filter(|r| matches!(r.tone, Tone::Warn | Tone::Alert))
        .count();
    let worst = if rows.iter().any(|r| r.tone == Tone::Alert) {
        Tone::Alert
    } else if issues > 0 {
        Tone::Warn
    } else {
        Tone::Normal
    };
    let text = match issues {
        0 => TileText::new(
            "Ready to go live",
            "Streaming app, mic and internet look good",
            worst,
        ),
        1 => TileText::new("1 thing to check", "Before you go live", worst),
        n => TileText::new(format!("{n} things to check"), "Before you go live", worst),
    };
    let mut v = TileView::ready(text).rows(rows);
    if f.online.is_none() {
        v = v.button("Watch internet", TileAction::turn_on("net_log"));
    }
    v.button("Voice settings", TileAction::settings("general"))
}

/// Minutes of play before the Session Timer suggests a break.
pub const BREAK_MIN: u64 = 90;

/// When the current play session started. Fed one sample at a time with
/// the running game (a Steam app id, or 0 for a game the guard sees by
/// process name only); a change of game starts the clock again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionClock {
    pub game: Option<u32>,
    pub started_ms: Option<u64>,
}

impl SessionClock {
    pub fn note(&mut self, game: Option<u32>, now: u64) {
        if game != self.game {
            self.game = game;
            self.started_ms = game.map(|_| now);
        }
    }
}

fn span(min: u64) -> String {
    match min {
        0 => "Just started".into(),
        m if m < 60 => format!("{m} min"),
        m if m % 60 == 0 => format!("{} h", m / 60),
        m => format!("{} h {} min", m / 60, m % 60),
    }
}

/// How long this session has run, and a nudge to stretch past
/// [`BREAK_MIN`]. `name` is the game when the app knows it.
pub fn session_timer_view(name: Option<&str>, started: Option<u64>, now: u64) -> TileView {
    let Some(at) = started else {
        return TileView::empty(
            "Start a game and this times your session on its own",
            "Open Library",
            TileAction::nav("library"),
        );
    };
    let min = now.saturating_sub(at) / 60_000;
    let game = name.unwrap_or("Game running");
    let pct = (min.min(BREAK_MIN) * 100 / BREAK_MIN) as u8;
    let text = if min >= BREAK_MIN {
        TileText::new(span(min), format!("{game} · break time"), Tone::Warn)
            .detail("Stand up, drink some water, rest your eyes for a minute")
    } else {
        TileText::new(span(min), format!("{game} · this session"), Tone::Normal).detail(format!(
            "Break nudge in {}",
            span(BREAK_MIN - min).to_lowercase()
        ))
    };
    TileView::ready(text).progress(pct).tag("LIVE")
}

/// Short lines the Hype Line tile turns over once a day. Free to show: no
/// model is asked.
const HYPE: [&str; 12] = [
    "Warm hands, cool head. Let's go.",
    "One more game? Make it the good one.",
    "Aim small, miss small.",
    "Today's the day that clutch finally lands.",
    "Hydrate, then dominate.",
    "Every pro was a bronze once.",
    "Play the objective. Collect the dubs.",
    "Comms clear, crosshair steady.",
    "Losses are just free lessons.",
    "Fresh day, fresh MMR.",
    "Breathe before the big fight.",
    "You vs. yesterday's you. Win that one.",
];

/// Today's line, held all day, in the voice of the active personality.
pub fn hype_line_view(personality: Option<&str>, now: u64) -> TileView {
    let day = (now / 86_400_000) as usize;
    let caption = match personality {
        Some(name) if !name.trim().is_empty() => format!("Today's line · {name}"),
        _ => "Today's line".to_string(),
    };
    TileView::ready(TileText::new(HYPE[day % HYPE.len()], caption, Tone::Normal))
        .button("Personality", TileAction::settings("personality"))
}

/// One Pulse card a feed tile lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItem {
    pub title: String,
    pub at_ms: u64,
    /// The card's first source link, when it has one.
    pub link: Option<String>,
}

/// The Pulse cards a feed tile lists: those its source posted, not
/// dismissed. Game companion cards are told by their id.
pub fn feed_items(cards: &[crate::UpdateCard], module: &str) -> Vec<FeedItem> {
    let ours = |c: &crate::UpdateCard| match module {
        "achievements" => {
            c.pulse.source_name.as_deref() == Some(crate::steam_achievements::SOURCE_NAME)
        }
        "deals" => c.pulse.source_name.as_deref() == Some(crate::steam_wishlist::SOURCE_NAME),
        _ => c.source_id.starts_with("source:game-companion:"),
    };
    cards
        .iter()
        .filter(|c| ours(c) && c.status != crate::UpdateStatus::Dismissed)
        .map(|c| FeedItem {
            title: c.title.clone(),
            at_ms: c.created_at,
            link: c.citations.first().cloned(),
        })
        .collect()
}

/// Achievements, Deals and Patch Notes: the cards their opt-in Pulse
/// source posted (games close to 100%, wishlist sales, what changed since
/// you last played). `on` is whether that source is switched on.
pub fn feed_view(module: &str, on: bool, items: &[FeedItem], now: u64) -> TileView {
    let (feature, off, none, every) = match module {
        "achievements" => (
            "steam_achievements",
            "Turn on achievement nudges and games close to 100% show here",
            "No game is close to 100% right now",
            "Checked weekly from your Steam profile",
        ),
        "deals" => (
            "steam_wishlist",
            "Turn on wishlist alerts and sales on your wishlist show here",
            "No wishlist sales right now",
            "Checked daily from your Steam wishlist",
        ),
        _ => (
            "game_companion",
            "Turn on the game companion and what changed since you last played shows here",
            "Launch a game and what changed since last time shows here",
            "Read when you start a game",
        ),
    };
    if !on {
        return TileView::off(feature, off);
    }
    let mut items: Vec<&FeedItem> = items.iter().collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.at_ms));
    let Some(top) = items.first() else {
        return TileView::ready(TileText::new("All caught up", none, Tone::Quiet).detail(every))
            .button("Open Pulse", TileAction::nav("pulse"));
    };
    let open = |i: &FeedItem| match &i.link {
        Some(url) if url.starts_with("https://") => TileAction::OpenUrl { url: url.clone() },
        _ => TileAction::nav("pulse"),
    };
    let rows = items
        .iter()
        .take(ROWS)
        .map(|i| {
            TileRow::new(
                i.title.clone(),
                crate::pulse::ago_label(i.at_ms, now),
                Tone::Normal,
            )
            .on_tap(open(i))
        })
        .collect();
    let caption = match items.len() {
        1 => format!("{} · {every}", crate::pulse::ago_label(top.at_ms, now)),
        n => format!("{} · {n} in Pulse", crate::pulse::ago_label(top.at_ms, now)),
    };
    TileView::ready(TileText::new(top.title.clone(), caption, Tone::Normal))
        .rows(rows)
        .primary("Open", open(top))
        .button("Open Pulse", TileAction::nav("pulse"))
}

/// Modules with nothing to read yet: what they'll show and one way in.
/// How far ahead the Tonight tile looks.
pub const TONIGHT_HOURS: u64 = 12;

fn until(at: u64, now: u64) -> String {
    match at.saturating_sub(now) / 60_000 {
        0 => "now".into(),
        m if m < 60 => format!("in {m} min"),
        m => format!("in {} h", m / 60),
    }
}

/// The automations due in the next [`TONIGHT_HOURS`], soonest first, from
/// `(name, next run in unix ms)` pairs of the ones switched on.
pub fn tonight_view(due: &[(String, u64)], now: u64) -> TileView {
    let end = now + TONIGHT_HOURS * 3_600_000;
    let mut soon: Vec<&(String, u64)> = due
        .iter()
        .filter(|(_, at)| *at >= now && *at <= end)
        .collect();
    soon.sort_by_key(|(name, at)| (*at, name.clone()));
    let Some((first, first_at)) = soon.first() else {
        return TileView::empty(
            "Nothing scheduled for the next 12 hours",
            "Plan my week",
            TileAction::ask(HOME_CHIPS[3]),
        )
        .on_tap(TileAction::nav("automations"));
    };
    let rows = soon
        .iter()
        .take(ROWS)
        .map(|(name, at)| {
            TileRow::new(name.clone(), until(*at, now), Tone::Normal)
                .on_tap(TileAction::nav("automations"))
        })
        .collect();
    let big = match soon.len() {
        1 => "1 tonight".to_string(),
        n => format!("{n} tonight"),
    };
    TileView::ready(TileText::new(
        big,
        format!("Next: {first} {}", until(*first_at, now)),
        Tone::Normal,
    ))
    .rows(rows)
    .primary("Plan my week", TileAction::ask(HOME_CHIPS[3]))
    .button("Open Automations", TileAction::nav("automations"))
    .on_tap(TileAction::nav("automations"))
}

/// The Squad tile from the Steam friends saved on this PC (`None` when
/// Steam isn't here). Who is online lives in Steam itself, so the tile
/// opens Steam's friends list rather than guess.
pub fn squad_view(friends: Option<usize>) -> TileView {
    match friends {
        None => no_steam_view("Your Steam friends show here once Steam is installed"),
        Some(0) => TileView::empty(
            "No Steam friends saved on this PC yet",
            "Friends list",
            TileAction::SteamFriends,
        ),
        Some(n) => TileView::ready(TileText::new(
            if n == 1 {
                "1 friend".to_string()
            } else {
                format!("{n} friends")
            },
            "On Steam · open the list to see who's on",
            Tone::Normal,
        ))
        .primary("Friends list", TileAction::SteamFriends)
        .button("Find a duo", TileAction::ask(HOME_CHIPS[1]))
        .on_tap(TileAction::SteamFriends),
    }
}

/// The Audio tile before the first read of this PC's devices.
pub fn waiting_view() -> TileView {
    TileView::empty(
        "Your mic and output device show here",
        "Voice settings",
        TileAction::settings("general"),
    )
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
        let v = now_playing_view(&games, NOW, None);
        assert_eq!(v.text.big, "Fresh");
        assert_eq!(v.text.caption, "Last played 1h ago · 1.5 h total");
        assert_eq!(v.rows.len(), 2, "a never-played game isn't now playing");
        assert_eq!(v.rows[0].tap, Some(TileAction::Play { app_id: 20 }));
        assert_eq!(now_playing_view(&[], NOW, None).state, TileState::Empty);
        let live = now_playing_view(&games, NOW, Some(30));
        assert_eq!(live.text.big, "Never", "the running game leads");
        assert_eq!(live.text.caption, "Playing now");
        assert_eq!(live.tag.as_deref(), Some("LIVE"));
        assert_eq!(live.rows[0].label, "Fresh");
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
        let a = what_to_play_view(&games, NOW, true, None);
        let b = what_to_play_view(&games, NOW + 3_600_000, true, None);
        assert_eq!(a.text.big, b.text.big, "the pick holds through the day");
        assert!(["A", "B"].contains(&a.text.big.as_str()), "{}", a.text.big);
        assert!(matches!(a.buttons[0].action, TileAction::Play { .. }));
        assert!(what_to_play_view(&games, NOW, false, None)
            .buttons
            .is_empty());
        let next = what_to_play_view(&games, NOW, true, Some(1));
        assert_ne!(next.text.big, a.text.big, "Another moves to the other pick");
        assert!(next.buttons.iter().any(|b| b.action == TileAction::Reroll));
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
    fn the_waiting_audio_tile_offers_one_way_in() {
        let v = waiting_view();
        assert_eq!(v.state, TileState::Empty);
        assert_eq!(v.buttons.len(), 1);
    }

    #[test]
    fn tonight_lists_the_next_12_hours_soonest_first() {
        let hour = 3_600_000;
        let due = vec![
            ("Patch notes digest".to_string(), NOW + 3 * hour),
            ("Raid reminder".to_string(), NOW + 30 * 60_000),
            ("Weekly report".to_string(), NOW + 30 * hour),
            ("Missed".to_string(), NOW - hour),
        ];
        let v = tonight_view(&due, NOW);
        assert_eq!(v.text.big, "2 tonight");
        assert_eq!(v.text.caption, "Next: Raid reminder in 30 min");
        let names: Vec<&str> = v.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(names, ["Raid reminder", "Patch notes digest"]);
        assert_eq!(v.rows[1].value, "in 3 h");
        assert!(v.buttons[0].primary);
        let empty = tonight_view(&due[2..], NOW);
        assert_eq!(empty.state, TileState::Empty);
        assert_eq!(empty.buttons.len(), 1);
        assert_eq!(empty.buttons[0].action, TileAction::ask(HOME_CHIPS[3]));
        assert_eq!(empty.tap, Some(TileAction::nav("automations")));
    }

    #[test]
    fn squad_opens_steam_friends_and_never_guesses_who_is_online() {
        let v = squad_view(Some(14));
        assert_eq!(v.text.big, "14 friends");
        assert_eq!(v.tap, Some(TileAction::SteamFriends));
        assert_eq!(v.buttons[0].action, TileAction::SteamFriends);
        assert_eq!(squad_view(Some(1)).text.big, "1 friend");
        assert_eq!(squad_view(None).state, TileState::Empty);
        assert_eq!(squad_view(None).buttons.len(), 1);
        // No Steam: the one button gets it, not a chat ask.
        assert_eq!(
            squad_view(None).buttons[0].action,
            TileAction::OpenUrl {
                url: STEAM_DOWNLOAD.into()
            }
        );
        assert_eq!(squad_view(Some(0)).buttons.len(), 1);
        assert_eq!(
            squad_view(Some(0)).buttons[0].action,
            TileAction::SteamFriends
        );
    }

    #[test]
    fn the_session_clock_starts_with_a_game_and_nudges_a_break() {
        let mut clock = SessionClock::default();
        clock.note(None, NOW);
        assert_eq!(clock.started_ms, None);
        clock.note(Some(570), NOW);
        clock.note(Some(570), NOW + 60_000);
        assert_eq!(clock.started_ms, Some(NOW), "the same game keeps its start");
        let v = session_timer_view(Some("Dota 2"), clock.started_ms, NOW + 45 * 60_000);
        assert_eq!(v.text.big, "45 min");
        assert_eq!(v.text.caption, "Dota 2 · this session");
        assert_eq!(v.text.detail.as_deref(), Some("Break nudge in 45 min"));
        assert_eq!(v.progress, Some(50));
        let late = session_timer_view(None, Some(NOW), NOW + 125 * 60_000);
        assert_eq!(late.text.big, "2 h 5 min");
        assert_eq!(late.text.tone, Tone::Warn);
        clock.note(None, NOW + 2);
        assert_eq!(clock.started_ms, None, "stopping clears the clock");
        assert_eq!(session_timer_view(None, None, NOW).state, TileState::Empty);
    }

    #[test]
    fn the_hype_line_holds_all_day_and_names_the_voice() {
        let a = hype_line_view(Some("Coach"), NOW);
        let b = hype_line_view(Some("Coach"), NOW + 3_600_000);
        assert_eq!(a.text.big, b.text.big);
        assert_eq!(a.text.caption, "Today's line · Coach");
        assert_ne!(hype_line_view(None, NOW + 86_400_000).text.big, a.text.big);
    }

    #[test]
    fn feed_tiles_are_off_until_their_source_is_on_then_list_its_cards() {
        let off = feed_view("deals", false, &[], NOW);
        assert_eq!(off.state, TileState::Off);
        assert_eq!(
            off.buttons[0].action,
            TileAction::TurnOn {
                feature: "steam_wishlist".into()
            }
        );
        let quiet = feed_view("achievements", true, &[], NOW);
        assert_eq!(quiet.text.big, "All caught up");
        let items = [
            FeedItem {
                title: "Hades: 2 to 100%".into(),
                at_ms: NOW - 3_600_000,
                link: Some("https://steamcommunity.com/app/1145360/guides/".into()),
            },
            FeedItem {
                title: "Older".into(),
                at_ms: NOW - 86_400_000 * 2,
                link: Some("javascript:alert(1)".into()),
            },
        ];
        let v = feed_view("achievements", true, &items, NOW);
        assert_eq!(v.text.big, "Hades: 2 to 100%");
        assert_eq!(v.text.caption, "1h ago · 2 in Pulse");
        assert!(matches!(v.buttons[0].action, TileAction::OpenUrl { .. }));
        assert_eq!(
            v.rows[1].tap,
            Some(TileAction::nav("pulse")),
            "only https links open"
        );
    }

    #[test]
    fn the_audio_tile_puts_a_muted_mic_first() {
        let dev = |name: &str, muted: bool, def: bool| AudioDevice {
            name: name.into(),
            description: name.into(),
            muted,
            volume_pct: Some(70),
            is_default: def,
        };
        let f = AudioFacts {
            outputs: vec![dev("Headset", false, true)],
            inputs: vec![
                dev("Webcam mic", false, false),
                dev("Headset mic", true, true),
            ],
            devices: Vec::new(),
        };
        let v = audio_view(&f);
        assert_eq!(v.text.big, "Mic muted");
        assert_eq!(v.text.caption, "Headset mic");
        assert_eq!(v.rows[0].value, "Muted");
        assert_eq!(v.rows[1].value, "Headset · 70%");
        let win = AudioFacts {
            devices: vec![("Realtek".into(), true), ("USB DAC".into(), false)],
            ..AudioFacts::default()
        };
        assert_eq!(audio_view(&win).text.caption, "USB DAC reports a problem");
        assert_eq!(audio_view(&AudioFacts::default()).state, TileState::Empty);
    }

    #[test]
    fn the_stream_check_counts_what_stands_between_you_and_live() {
        assert_eq!(stream_app(&["OBS64.EXE".into()]), Some("OBS Studio"));
        assert_eq!(stream_app(&["chrome.exe".into()]), None);
        let mic = AudioDevice {
            name: "mic".into(),
            description: "Headset mic".into(),
            muted: false,
            volume_pct: Some(80),
            is_default: true,
        };
        let ready = StreamFacts {
            app: Some("OBS Studio"),
            audio: AudioFacts {
                inputs: vec![mic.clone()],
                ..AudioFacts::default()
            },
            online: Some(true),
        };
        assert_eq!(stream_view(&ready).text.big, "Ready to go live");
        let muted = StreamFacts {
            app: None,
            audio: AudioFacts {
                inputs: vec![AudioDevice { muted: true, ..mic }],
                ..AudioFacts::default()
            },
            online: None,
        };
        let v = stream_view(&muted);
        assert_eq!(v.text.big, "2 things to check");
        assert_eq!(v.text.tone, Tone::Alert);
        assert_eq!(v.rows[1].value, "Muted");
        assert_eq!(v.buttons[0].label, "Watch internet");
    }

    #[test]
    fn clips_lead_with_the_newest_and_open_by_place_in_the_list() {
        let clip = |name: &str, ago_h: u64| Clip {
            name: name.into(),
            path: std::path::PathBuf::from(format!("/fake/{name}.mp4")),
            modified_ms: NOW - ago_h * 3_600_000,
        };
        let v = clips_view(&[clip("Ace", 2), clip("Old one", 24 * 30)], true, NOW);
        assert_eq!(v.text.big, "Ace");
        assert_eq!(v.text.caption, "Newest clip · 2h ago · 1 this week");
        assert_eq!(v.rows[1].tap, Some(TileAction::OpenClip { index: 1 }));
        assert_eq!(v.buttons[0].action, TileAction::OpenClip { index: 0 });
        // None yet: Steam's Settings turns Game Recording on; no Steam gets it.
        let empty = clips_view(&[], true, NOW);
        assert_eq!(empty.state, TileState::Empty);
        assert_eq!(empty.buttons[0].action, TileAction::SteamSettings);
        assert_eq!(
            clips_view(&[], false, NOW).buttons[0].action,
            TileAction::OpenUrl {
                url: STEAM_DOWNLOAD.into()
            }
        );
    }
}
