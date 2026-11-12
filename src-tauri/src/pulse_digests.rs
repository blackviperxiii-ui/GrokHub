//! The shell's Pulse digest and offer writer: parity with the cabin's
//! start-my-day brief, weekly systems brief, weekly recap, daily drop,
//! release radar, hobby learning paths, repeated-ask offers and one-time
//! helper offers.
//!
//! Every builder lives in `grokhub_core`; this module is the shell's glue.
//! It runs from the feed clock (one call in `pulse_feed::tick`), behind the
//! scheduler lease: while the cabin's lease is fresh the shell writes
//! nothing, and each digest reads the same state file the cabin writes, so
//! a slot the cabin already filled is skipped here. Settings are the
//! cabin's own `app.json` keys, read and patched through
//! `grokhub_core::app_json`, so both apps share one set of switches.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use grokhub_core::app_json;
use grokhub_core::ask_offers::{self as ao, AskOfferLedger, AskOfferSettings};
use grokhub_core::daily_drop::DailyDropSettings;
use grokhub_core::governor::{self, DailyDropRefreshCtx};
use grokhub_core::hobby_paths::{self as hp, HobbyPathSettings};
use grokhub_core::organs::LocalClock;
use grokhub_core::pulse_sources::{self as ps};
use grokhub_core::release_radar::{self as rr};
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::update_feed::{self as feed, UpdateCard, UpdateKind, UpdateStatus};
use grokhub_core::{day_brief as db, systems_brief as sb, week_recap as wr};
use serde::{Deserialize, Serialize};

use crate::pulse_feed;

/// One run's shared inputs: the config dir, the fixed instant, the local
/// clock built from the page's reported offset, and the local day key.
struct Ctx {
    config_dir: PathBuf,
    now: u64,
    clock: LocalClock,
    day: String,
    quiet: bool,
}

/// The cabin's digest switches, read from `app.json`.
#[derive(Debug, Clone, Default)]
struct Switches {
    daily_drop: DailyDropSettings,
    ask_offers: AskOfferSettings,
    hobby_paths: HobbyPathSettings,
    helper_dismissed: Vec<String>,
    downloads_tidy: bool,
    dream_hour: u32,
}

fn switches(config_dir: &Path) -> Switches {
    let map = app_json::read(config_dir).unwrap_or_default();
    let get = |key: &str| map.get(key).cloned();
    let mut out = Switches {
        daily_drop: get("dailyDrop")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        ask_offers: get("askOffers")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        hobby_paths: get("hobbyPaths")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        helper_dismissed: get("helperOffersDismissed")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        downloads_tidy: get("downloadsTidy")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        dream_hour: get("dreamHour")
            .and_then(|v| v.as_u64())
            .map(|h| h.min(23) as u32)
            .unwrap_or(3),
    };
    if map.get("dreamHour").is_none() {
        out.dream_hour = 3;
    }
    out
}

/// Local day key (`YYYY-MM-DD`) for a local-clock day count (days since the
/// Unix epoch, already offset). Civil-from-days algorithm.
fn day_key(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub(crate) fn local_day(now_ms: u64, offset_min: i32) -> String {
    let local = (now_ms / 60_000) as i64 + i64::from(offset_min);
    day_key(local.div_euclid(1440))
}

fn build_ctx(config_dir: &Path, now: u64) -> Ctx {
    let offset = crate::pulse_feed::state(config_dir)
        .utc_offset_min
        .unwrap_or(0);
    let clock = crate::scheduler::local_clock(now, offset);
    let day = local_day(now, offset);
    let behavior = crate::settings::load(config_dir).behavior;
    let quiet = !behavior.quiet_start.is_empty()
        && !behavior.quiet_end.is_empty()
        && grokhub_core::quiet_hours_active(
            &grokhub_core::pulse_writer::local_hm(now, offset),
            &behavior.quiet_start,
            &behavior.quiet_end,
        );
    Ctx {
        config_dir: config_dir.to_path_buf(),
        now,
        clock,
        day,
        quiet,
    }
}

fn tone(config_dir: &Path) -> grokhub_core::personality::Personality {
    let settings = crate::settings::load(config_dir);
    grokhub_core::personality::gated(
        grokhub_core::personality::lookup(
            &crate::personality::dir(config_dir),
            &settings.personality,
        ),
        settings.explicit_personalities,
    )
}

fn category_scores(config_dir: &Path) -> Vec<(String, i32, bool)> {
    let reactions =
        grokhub_core::reactions::load(&config_dir.join(grokhub_core::reactions::FILE_NAME));
    reactions
        .categories
        .iter()
        .map(|(k, v)| (k.clone(), v.score, v.quiet))
        .collect()
}

fn cards(ctx: &Ctx) -> Vec<UpdateCard> {
    crate::pulse::cards(&ctx.config_dir).unwrap_or_default()
}

fn post(ctx: &Ctx, card: UpdateCard) -> bool {
    crate::pulse::add_post(&ctx.config_dir, card).unwrap_or(false)
}

/// Post a digest card the way the cabin does: the prior live card for the
/// same source (or source prefix) goes away first, then the new one lands.
fn post_digest(ctx: &Ctx, card: UpdateCard, prefix: Option<&str>, exact: Option<&str>) -> bool {
    crate::pulse::post_digest(&ctx.config_dir, card, prefix, exact).unwrap_or(false)
}

/// One feed-clock pass over every digest and offer. Returns how many cards
/// were posted. The lease is the first gate: a fresh cabin lease means the
/// cabin is the active app and the shell stays out of the way.
pub fn run_due(config_dir: &Path, now: u64) -> usize {
    if !shell_may_fire(read_lease(config_dir).as_ref(), std::process::id(), now) {
        return 0;
    }
    let ctx = build_ctx(config_dir, now);
    let sw = switches(config_dir);
    let mut posted = 0;
    posted += day_brief(&ctx);
    posted += systems_brief(&ctx, &sw);
    posted += week_recap(&ctx);
    posted += daily_drop(&ctx, &sw);
    posted += release_radar(&ctx);
    posted += hobby_paths(&ctx, &sw);
    posted += ask_offers(&ctx, &sw);
    posted += helper_offers(&ctx, &sw);
    posted
}

// ---------------------------------------------------------------------------
// Shared digest state files. The cabin owns the formats; the shell reads
// and writes the same keys so either app can fill a slot and the other
// sees it as filled.

fn load_map(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_map(path: &Path, map: &serde_json::Map<String, serde_json::Value>) {
    if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Object(map.clone())) {
        let _ = std::fs::write(path, text);
    }
}

fn map_str(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    map.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn set_str(map: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: &str) {
    map.insert(
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    );
}

fn set_u64(map: &mut serde_json::Map<String, serde_json::Value>, key: &str, value: u64) {
    map.insert(key.to_string(), serde_json::json!(value));
}

// ---------------------------------------------------------------------------
// Start-my-day brief.

#[derive(Clone, Copy)]
enum Gather {
    Watchers,
    GameNews,
    Prints,
    Backups,
}

impl Gather {
    fn matches(self, card: &UpdateCard) -> bool {
        match self {
            Self::Watchers => grokhub_core::nudge_watchers::is_nudge_watcher_card(card),
            Self::GameNews => is_game_news_card(card),
            Self::Prints => {
                card.source_id.starts_with("print:") || card.source_id.starts_with("printer:")
            }
            Self::Backups => {
                card.source_id.starts_with("backup:") || card.source_id.starts_with("backup-nudge:")
            }
        }
    }

    fn default_category(self) -> &'static str {
        match self {
            Self::Watchers => db::DayBriefSection::Watcher.default_category(),
            Self::GameNews => db::DayBriefSection::GameNews.default_category(),
            Self::Prints => db::DayBriefSection::Print.default_category(),
            Self::Backups => db::DayBriefSection::Backup.default_category(),
        }
    }
}

fn is_game_news_card(card: &UpdateCard) -> bool {
    if card.source_id.starts_with("source:steam-wishlist:") {
        return true;
    }
    if let Some((cat, _)) = grokhub_core::reactions::card_category_tag(card) {
        if cat == "game news" {
            return true;
        }
    }
    card.pulse
        .source_name
        .as_deref()
        .is_some_and(|n| n.ends_with(" on Steam"))
}

fn gather_cards(all: &[UpdateCard], kind: Gather, now: u64) -> Vec<db::DayBriefSeed> {
    let mut out = Vec::new();
    for card in all {
        if !grokhub_core::pulse::pulse_visible(card, now) || !kind.matches(card) {
            continue;
        }
        let title = card.title.trim();
        if title.is_empty() {
            continue;
        }
        let body = card.body.clone().unwrap_or_default();
        let prompt = card
            .prompt
            .clone()
            .filter(|p| !p.trim().is_empty())
            .or_else(|| card.details.clone())
            .unwrap_or_else(|| format!("Look into: {title}"));
        let category = grokhub_core::reactions::card_category_tag(card)
            .map(|(c, _)| c)
            .unwrap_or_else(|| kind.default_category().to_string());
        out.push(db::DayBriefSeed::new(title, body, prompt, category).from_card(card.id.clone()));
    }
    out
}

fn day_brief(ctx: &Ctx) -> usize {
    let path = ctx.config_dir.join(db::FILE_NAME);
    let mut state = load_map(&path);
    let rhythm = grokhub_core::rhythm::load(&ctx.config_dir.join(grokhub_core::rhythm::FILE_NAME));
    if !db::day_brief_due(
        map_str(&state, "lastDay").as_deref(),
        &ctx.day,
        &ctx.clock,
        rhythm.usually_active(ctx.clock.hour),
        rhythm.brief_hour,
        rhythm.enough(),
    ) {
        return 0;
    }
    let all = cards(ctx);
    let input = db::DayBriefInputs {
        day_key: ctx.day.clone(),
        generated_at: ctx.now,
        resume: None,
        watchers: gather_cards(&all, Gather::Watchers, ctx.now),
        game_news: gather_cards(&all, Gather::GameNews, ctx.now),
        prints: gather_cards(&all, Gather::Prints, ctx.now),
        backups: gather_cards(&all, Gather::Backups, ctx.now),
        next_step: None,
        category_scores: category_scores(&ctx.config_dir),
        tone: db::BriefTone::from(&tone(&ctx.config_dir)),
    };
    let brief = db::build_brief(&input);
    let md = brief.render();
    set_str(&mut state, "lastDay", &ctx.day);
    set_str(&mut state, "latestDay", &brief.day_key);
    set_str(&mut state, "latestMd", &md);
    set_u64(&mut state, "latestAt", ctx.now);
    let taps: Vec<serde_json::Value> = brief
        .one_taps()
        .into_iter()
        .map(|(label, prompt, card)| {
            serde_json::json!({"label": label, "prompt": prompt, "card": card})
        })
        .collect();
    state.insert("latestTaps".to_string(), serde_json::Value::Array(taps));
    save_map(&path, &state);
    let mut card = feed::digest_card(
        &brief.source_id(),
        db::CARD_TITLE,
        &brief.card_body(),
        ctx.now,
    );
    card.details = Some(md);
    card.body = Some(brief.card_body());
    card.pulse.report = Some(brief.card_report());
    card.prompt = brief.primary_prompt();
    card.pulse.because = Some("Start my day brief".into());
    grokhub_core::hold_if_quiet(&mut card, ctx.quiet);
    post_digest(ctx, card, Some(db::CARD_SOURCE_PREFIX), None) as usize
}

// ---------------------------------------------------------------------------
// Weekly systems brief.

fn systems_brief(ctx: &Ctx, sw: &Switches) -> usize {
    let path = ctx.config_dir.join("systems_brief.json");
    let mut state = load_map(&path);
    if !sb::systems_brief_due(
        map_str(&state, "lastDay").as_deref(),
        &ctx.day,
        &ctx.clock,
        sw.dream_hour,
    ) {
        return 0;
    }
    let outcomes = grokhub_core::outcome::read_outcomes(&ctx.config_dir);
    let skill_stats = grokhub_core::self_review::skill_stats(&outcomes, ctx.now, sb::week_ms());
    let autos = crate::automations::load(&ctx.config_dir).unwrap_or_default();
    let automation_health: Vec<String> = autos
        .iter()
        .filter_map(grokhub_core::automation_health_line)
        .collect();
    let all = cards(ctx);
    let digest_titles: Vec<String> = all
        .iter()
        .filter(|c| c.kind == UpdateKind::Digest)
        .map(|c| c.title.clone())
        .collect();
    let open_decisions: Vec<String> = crate::journal::load(&ctx.config_dir)
        .map(|page| page.active.into_iter().map(|q| q.question).collect())
        .unwrap_or_default();
    let input = sb::BriefInputs {
        day: ctx.day.clone(),
        generated_at: ctx.now,
        skill_stats,
        failed_services: Vec::new(),
        services_checked: 0,
        cabin_update: None,
        automation_health,
        digest_titles,
        last_dream: None,
        progress_wins: Vec::new(),
        blocked: Vec::new(),
        open_decisions,
    };
    let brief = sb::build_brief(&input);
    let md = brief.render();
    set_str(&mut state, "lastDay", &ctx.day);
    set_str(&mut state, "latestDay", &brief.day);
    set_str(&mut state, "latestMd", &md);
    set_u64(&mut state, "latestAt", ctx.now);
    save_map(&path, &state);
    let mut card = feed::digest_card(
        &brief.source_id(),
        sb::CARD_TITLE,
        &brief.card_body(),
        ctx.now,
    );
    card.details = Some(md);
    card.body = Some(brief.card_body());
    card.pulse.report = Some(brief.card_report());
    grokhub_core::hold_if_quiet(&mut card, ctx.quiet);
    post_digest(ctx, card, None, Some(&brief.source_id())) as usize
}

// ---------------------------------------------------------------------------
// Weekly "your week" recap.

fn week_recap(ctx: &Ctx) -> usize {
    let path = ctx.config_dir.join(wr::FILE_NAME);
    let mut state = load_map(&path);
    let week_key = wr::week_key_from_day(&ctx.day, ctx.clock.weekday);
    let rhythm = grokhub_core::rhythm::load(&ctx.config_dir.join(grokhub_core::rhythm::FILE_NAME));
    if !wr::week_recap_due(
        map_str(&state, "lastWeek").as_deref(),
        &week_key,
        &ctx.clock,
        rhythm.usually_active(ctx.clock.hour),
        rhythm.brief_hour,
        rhythm.enough(),
    ) {
        return 0;
    }
    let games: Vec<wr::GamePlayed> = match grokhub_core::steam::detect_root() {
        Some(root) => {
            let library = grokhub_core::steam::read_library(&root);
            let rows: Vec<(String, u64, u64)> = library
                .iter()
                .map(|g| (g.name.clone(), g.last_played, g.playtime_min))
                .collect();
            wr::games_from_library(&rows, ctx.now)
        }
        None => Vec::new(),
    };
    let outcomes = grokhub_core::outcome::read_outcomes(&ctx.config_dir);
    let top_tasks = wr::top_tasks_from_outcomes(&outcomes, ctx.now);
    let scores = category_scores(&ctx.config_dir);
    let top_cat = wr::top_reaction_category(&scores);
    let suggestion = wr::suggest_next(
        top_cat.as_deref(),
        top_tasks.first().map(|t| t.label.as_str()),
    );
    let input = wr::WeekRecapInputs {
        week_key: week_key.clone(),
        generated_at: ctx.now,
        games,
        projects: Vec::new(),
        prints_made: None,
        disk_freed_bytes: None,
        top_tasks,
        suggestion,
        learned: grokhub_core::learn::hub::score_lines(ctx.now),
        tone: wr::RecapTone::from(&tone(&ctx.config_dir)),
    };
    let recap = wr::build_recap(&input);
    let md = recap.render();
    set_str(&mut state, "lastWeek", &week_key);
    set_str(&mut state, "latestWeek", &recap.week_key);
    set_str(&mut state, "latestMd", &md);
    set_u64(&mut state, "latestAt", ctx.now);
    save_map(&path, &state);
    let mut card = feed::digest_card(
        &recap.source_id(),
        wr::CARD_TITLE,
        &recap.card_body(),
        ctx.now,
    );
    card.details = Some(md);
    card.body = Some(recap.card_body());
    card.pulse.report = Some(recap.card_report());
    grokhub_core::hold_if_quiet(&mut card, ctx.quiet);
    post_digest(ctx, card, Some(wr::CARD_SOURCE_PREFIX), None) as usize
}

// ---------------------------------------------------------------------------
// Daily drop: the governor's overnight pick from ranked local source rows.
// The ranking reads the same `pulse-sources` cache the feed writer keeps.

fn daily_drop(ctx: &Ctx, sw: &Switches) -> usize {
    if !sw.daily_drop.active() {
        return 0;
    }
    let path = ctx.config_dir.join("daily_drop.json");
    let mut file = load_map(&path);
    let mut ledger: grokhub_core::daily_drop::DailyDropLedger = file
        .get("ledger")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let cache = ps::load_cache(&ctx.config_dir.join(ps::CACHE_FILE));
    let items: Vec<ps::SourceItem> = cache
        .entries
        .values()
        .flat_map(|e| e.items.iter().cloned())
        .collect();
    let profile =
        grokhub_core::interests::load(&ctx.config_dir.join(grokhub_core::interests::FILE_NAME));
    let ranked = ps::rank_with(
        &items,
        &profile,
        &cache.source_bias,
        &cache.source_posts,
        ctx.now,
    );
    let last_run_day = map_str(&file, "lastRunDay");
    let out = governor::daily_drop_refresh(DailyDropRefreshCtx {
        settings: &sw.daily_drop,
        ranked: &ranked,
        ledger: &mut ledger,
        last_run_day: last_run_day.as_deref(),
        today: &ctx.day,
        clock: &ctx.clock,
        night_hour: sw.dream_hour,
        now_ms: ctx.now,
    });
    let mut posted = 0;
    for card in out.cards {
        posted += post(ctx, card) as usize;
    }
    if out.mark_day.is_some() || posted > 0 {
        if let Ok(value) = serde_json::to_value(&ledger) {
            file.insert("ledger".to_string(), value);
        }
        if let Some(day) = &out.mark_day {
            set_str(&mut file, "lastRunDay", day);
        }
        save_map(&path, &file);
    }
    posted
}

// ---------------------------------------------------------------------------
// Release radar: a daily Steam store scan for DLC and sequels of owned and
// wishlisted games, plus the "out today" reminders the cabin posts.

fn release_radar(ctx: &Ctx) -> usize {
    let mut radar = rr::load(&ctx.config_dir);
    let mut posted = 0;
    // Reminders that came due post even between scans, like the cabin's.
    if !radar.reminders.is_empty() {
        let due = radar.take_due(ctx.now);
        if !due.is_empty() {
            let _ = rr::save(&ctx.config_dir, &radar);
            for (id, r) in &due {
                posted += post(ctx, rr::out_today_card(*id, r, ctx.now)) as usize;
            }
        }
    }
    if !radar.due(ctx.now) {
        return posted;
    }
    let mut library: Vec<(u32, String)> = Vec::new();
    let mut owned: BTreeSet<u32> = BTreeSet::new();
    if let Some(root) = grokhub_core::steam::detect_root() {
        let mut games = grokhub_core::steam::read_library(&root);
        games.sort_by_key(|g| std::cmp::Reverse(g.playtime_min));
        owned = games.iter().map(|g| g.app_id).collect();
        library = games.into_iter().map(|g| (g.app_id, g.name)).collect();
    }
    let wishlist = grokhub_core::steam_wishlist::load(&ctx.config_dir);
    let wished: Vec<(u32, String)> = wishlist
        .apps
        .iter()
        .filter(|(_, seen)| !seen.name.is_empty())
        .map(|(id, seen)| (*id, seen.name.clone()))
        .collect();
    let seeds = rr::interleave_seeds(library, wished);
    let (batch, next_seed) = radar.seed_batch(&seeds);
    radar.last_check_ms = ctx.now;
    let _ = rr::save(&ctx.config_dir, &radar);
    if batch.is_empty() {
        return posted;
    }
    let net = pulse_feed::net();
    let mut get = |url: &str| net.page(url);
    let found = match rr::scan(&batch, &owned, &mut get) {
        Ok(found) => found,
        Err(_) => return posted,
    };
    let mut radar = rr::load(&ctx.config_dir);
    if !radar.on {
        return posted;
    }
    radar.next_seed = next_seed;
    let cache = ps::load_cache(&ctx.config_dir.join(ps::CACHE_FILE));
    let bias = cache.source_bias.get(rr::SOURCE_NAME).copied().unwrap_or(0);
    let fresh = radar.fresh(found, rr::posts_allowed(bias));
    let _ = rr::save(&ctx.config_dir, &radar);
    for u in &fresh {
        posted += post(ctx, u.radar_card(ctx.now)) as usize;
    }
    posted
}

// ---------------------------------------------------------------------------
// Hobby learning paths (Know me opt-in).

fn hobby_paths(ctx: &Ctx, sw: &Switches) -> usize {
    if !sw.hobby_paths.any_on() {
        return 0;
    }
    let path = ctx.config_dir.join(hp::FILE_NAME);
    let mut ledger = hp::load_ledger(&path);
    let know_me = grokhub_core::know_me::load_tags(&ctx.config_dir);
    let out = hp::tick(hp::TickInput {
        settings: &sw.hobby_paths,
        ledger: &mut ledger,
        know_me: &know_me,
        focus_process: None,
        today: &ctx.day,
        now_ms: ctx.now,
    });
    let _ = hp::save_ledger(&path, &ledger);
    let mut posted = 0;
    for mut card in out.cards {
        grokhub_core::hold_if_quiet(&mut card, ctx.quiet);
        posted += post(ctx, card) as usize;
    }
    posted
}

// ---------------------------------------------------------------------------
// Repeated-ask automate offers.

fn load_ask_ledger(config_dir: &Path) -> AskOfferLedger {
    std::fs::read_to_string(config_dir.join(ao::LEDGER_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_ask_ledger(config_dir: &Path, ledger: &AskOfferLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let _ = std::fs::write(config_dir.join(ao::LEDGER_FILE), text);
    }
}

/// Log a typed user ask locally (intent + hour), the shell's half of the
/// cabin's `note_ask_offer`. Called next to `pulse_feed::note_chat`.
pub fn note_ask(config_dir: &Path, text: &str, now_ms: u64) {
    let sw = switches(config_dir);
    if !sw.ask_offers.enabled {
        return;
    }
    let offset = crate::pulse_feed::state(config_dir)
        .utc_offset_min
        .unwrap_or(0);
    let clock = crate::scheduler::local_clock(now_ms, offset);
    let mut ledger = load_ask_ledger(config_dir);
    ao::append_ask(&mut ledger, text, clock.hour, clock.weekday, now_ms);
    save_ask_ledger(config_dir, &ledger);
}

fn ask_offers(ctx: &Ctx, sw: &Switches) -> usize {
    let mut ledger = load_ask_ledger(&ctx.config_dir);
    if ctx.now.saturating_sub(ledger.last_tick_ms) < ao::TICK_EVERY_MS {
        return 0;
    }
    if !sw.ask_offers.enabled {
        ledger.last_tick_ms = ctx.now;
        save_ask_ledger(&ctx.config_dir, &ledger);
        return 0;
    }
    let autos = crate::automations::load(&ctx.config_dir).unwrap_or_default();
    let existing: Vec<String> = autos
        .iter()
        .flat_map(|a| [a.name.clone(), a.instructions.clone()])
        .collect();
    let live: Vec<String> = cards(ctx)
        .iter()
        .filter(|c| c.status != UpdateStatus::Dismissed)
        .map(|c| c.source_id.clone())
        .filter(|s| !s.is_empty())
        .collect();
    let out = ao::tick(ao::TickInput {
        settings: &sw.ask_offers,
        ledger: &mut ledger,
        existing_autos: &existing,
        live_sources: &live,
        today: &ctx.day,
        now_ms: ctx.now,
    });
    save_ask_ledger(&ctx.config_dir, &ledger);
    let mut posted = 0;
    for card in out.cards {
        posted += post(ctx, card) as usize;
    }
    posted
}

// ---------------------------------------------------------------------------
// One-time helper offers: helpers that are off by default get one Pulse
// card each, with the cabin's wording. Accepting flips the helper's own
// switch (see `apply_helper_offer` in pulse.rs); dismissing is remembered
// in `app.json`, like the cabin.

pub const HELPER_SOURCE_PREFIX: &str = "helper-offer:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperOffer {
    WindDown,
    Tidy,
}

impl HelperOffer {
    pub const ALL: [HelperOffer; 2] = [HelperOffer::WindDown, HelperOffer::Tidy];

    pub fn id(self) -> &'static str {
        match self {
            HelperOffer::WindDown => "wind_down",
            HelperOffer::Tidy => "downloads_tidy",
        }
    }

    pub fn from_source(source_id: &str) -> Option<Self> {
        let id = source_id.strip_prefix(HELPER_SOURCE_PREFIX)?;
        Self::ALL.into_iter().find(|o| o.id() == id)
    }

    fn ask(self) -> &'static str {
        match self {
            HelperOffer::WindDown => "Let the PC sleep when it sits idle?",
            HelperOffer::Tidy => "Suggest Downloads cleanups?",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            HelperOffer::WindDown => {
                "Pick how long the PC sits idle before it sleeps. Never during a game, a download or a task."
            }
            HelperOffer::Tidy => {
                "Once a week I look at Downloads and suggest old installers, big old archives and duplicates. Nothing moves until you approve it."
            }
        }
    }

    fn on(self, config_dir: &Path, sw: &Switches) -> bool {
        match self {
            HelperOffer::WindDown => grokhub_core::wind_down::load(config_dir).on,
            HelperOffer::Tidy => sw.downloads_tidy,
        }
    }
}

fn helper_offers(ctx: &Ctx, sw: &Switches) -> usize {
    let all = cards(ctx);
    let mut posted = 0;
    for offer in HelperOffer::ALL {
        if offer.on(&ctx.config_dir, sw) || sw.helper_dismissed.iter().any(|d| d == offer.id()) {
            continue;
        }
        let source = format!("{HELPER_SOURCE_PREFIX}{}", offer.id());
        if all.iter().any(|c| c.source_id == source) {
            continue;
        }
        let mut card = feed::automate_offer_card(&source, offer.ask(), ctx.now);
        card.body = Some(offer.hint().to_string());
        card.prompt = Some(offer.ask().to_string());
        card.why = Some("A helper that is off by default.".into());
        posted += post(ctx, card) as usize;
    }
    posted
}

// ---------------------------------------------------------------------------
// Pulse sources settings for the Tauri settings page: the cabin's rows,
// the cabin's defaults, the cabin's storage.

/// The digest and offer switches as the settings page paints them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesView {
    pub daily_drop: DailyDropSettings,
    pub ask_offers_enabled: bool,
    pub hobby_paths: HobbyPathSettings,
    pub release_radar_on: bool,
}

pub fn sources_view(config_dir: &Path) -> SourcesView {
    let sw = switches(config_dir);
    SourcesView {
        daily_drop: sw.daily_drop,
        ask_offers_enabled: sw.ask_offers.enabled,
        hobby_paths: sw.hobby_paths,
        release_radar_on: rr::load(config_dir).on,
    }
}

pub fn save_sources(config_dir: &Path, view: SourcesView) -> Result<SourcesView, String> {
    let current = switches(config_dir);
    let ask = AskOfferSettings {
        enabled: view.ask_offers_enabled,
        daily_max: current.ask_offers.daily_max,
    };
    app_json::patch(
        config_dir,
        &[
            (
                "dailyDrop",
                serde_json::to_value(&view.daily_drop).map_err(|e| e.to_string())?,
            ),
            (
                "askOffers",
                serde_json::to_value(&ask).map_err(|e| e.to_string())?,
            ),
            (
                "hobbyPaths",
                serde_json::to_value(&view.hobby_paths).map_err(|e| e.to_string())?,
            ),
        ],
    )?;
    let mut radar = rr::load(config_dir);
    if radar.on != view.release_radar_on {
        radar.on = view.release_radar_on;
        rr::save(config_dir, &radar)?;
    }
    Ok(sources_view(config_dir))
}

/// Accept on a helper offer card: flip the helper's own switch, the same
/// paths the cabin's answer uses.
pub fn apply_helper_accept(config_dir: &Path, offer: HelperOffer) -> Result<(), String> {
    match offer {
        HelperOffer::WindDown => {
            let mut tracker = grokhub_core::wind_down::load(config_dir);
            tracker.on = true;
            grokhub_core::wind_down::save(config_dir, &tracker)
        }
        HelperOffer::Tidy => app_json::patch(
            config_dir,
            &[("downloadsTidy", serde_json::Value::Bool(true))],
        ),
    }
}

/// Dismiss on a helper offer card: never offer that helper again.
pub fn note_helper_dismissed(config_dir: &Path, offer: HelperOffer) -> Result<(), String> {
    let mut dismissed = switches(config_dir).helper_dismissed;
    if dismissed.iter().any(|d| d == offer.id()) {
        return Ok(());
    }
    dismissed.push(offer.id().to_string());
    app_json::patch(
        config_dir,
        &[(
            "helperOffersDismissed",
            serde_json::to_value(&dismissed).map_err(|e| e.to_string())?,
        )],
    )
}

#[cfg(test)]
#[path = "pulse_digests_tests.rs"]
mod tests;
