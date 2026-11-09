//! Achievement nudges (`grokhub_core::steam_achievements`) and wishlist sale
//! alerts (`grokhub_core::steam_wishlist`) on the shell, so Home's
//! Achievements and Deals tiles turn on in place and fill without the
//! desktop app. Same files, same SteamID seal and same checks as the
//! desktop app's Pulse sources: the SteamID is the account signed in to
//! Steam on this PC, sealed with the private-data key, and it only ever
//! leaves in the requests to Steam. The checks run off the main thread on a
//! slow clock: the wishlist once a day, achievements once a week, each only
//! while switched on. Cards land in the same Pulse store the feed reads.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_agent::harness::LearnedVault;
use grokhub_core::amr::Sealer as _;
use grokhub_core::pulse_sources::{self as ps, Fetched, SourceKind, SourceReq};
use grokhub_core::steam_achievements as sa;
use grokhub_core::steam_wishlist as sw;

const TICK: Duration = Duration::from_secs(10 * 60);
/// After a wishlist check Steam didn't answer, the next try waits this long
/// (achievements keep their own backoff in the tracker).
const WISH_RETRY_MS: u64 = 60 * 60 * 1000;

static RUNNING: AtomicBool = AtomicBool::new(false);
static WISH_FAILED_MS: Mutex<u64> = Mutex::new(0);
/// The last thing each check said, for its tile: (module, sentence).
static NOTES: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());

/// What a check last said for `module` ("achievements" or "deals").
pub fn note(module: &str) -> Option<String> {
    let notes = NOTES.lock().ok()?;
    notes
        .iter()
        .find(|(m, _)| *m == module)
        .map(|(_, n)| n.clone())
        .filter(|n| !n.is_empty())
}

fn set_note(module: &'static str, text: String) {
    if let Ok(mut notes) = NOTES.lock() {
        notes.retain(|(m, _)| *m != module);
        notes.push((module, text));
    }
}

pub fn open_steam_id(dir: &Path) -> Option<u64> {
    let sealed = std::fs::read_to_string(dir.join(sw::ID_FILE)).ok()?;
    LearnedVault::new(dir)
        .open(sw::ID_AAD, sealed.trim())
        .ok()?
        .parse()
        .ok()
}

fn seal_steam_id(dir: &Path, id: u64) -> Result<(), String> {
    let sealed = LearnedVault::new(dir).seal(sw::ID_AAD, &id.to_string())?;
    let path = dir.join(sw::ID_FILE);
    let tmp = dir.join(format!("{}.shell-{}.tmp", sw::ID_FILE, std::process::id()));
    std::fs::write(&tmp, sealed.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// One keyless GET through the Pulse fetcher (public pages only).
fn get(url: &str) -> Result<String, String> {
    let req = SourceReq {
        kind: SourceKind::Rss,
        url: url.to_string(),
        name: sw::SOURCE_NAME.to_string(),
        topic: None,
    };
    match crate::pulse_feed::net().source(&req, None) {
        Fetched::Body { body, .. } => Ok(body),
        Fetched::NotModified => Err("no reply".into()),
        Fetched::Failed(why) => Err(why),
    }
}

/// How the person rated this source's posts (Not interested lowers it).
fn bias(dir: &Path, source: &str) -> i32 {
    ps::load_cache(&dir.join(ps::CACHE_FILE))
        .source_bias
        .get(source)
        .copied()
        .unwrap_or(0)
}

/// Runs the checks on a slow clock, starting a minute after launch.
pub fn start(config_dir: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            tick(&config_dir, grokhub_core::now_ms());
            std::thread::sleep(TICK);
        }
    });
}

/// Runs whichever check is on and due. One run at a time.
pub fn tick(dir: &Path, now: u64) {
    if !crate::capabilities::current().pulse || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let post = |card| crate::pulse::add_post(dir, card).unwrap_or(false);
    wishlist(dir, now, &mut get, &post);
    achievements(dir, now, &mut get, &post);
    RUNNING.store(false, Ordering::SeqCst);
}

fn wishlist(
    dir: &Path,
    now: u64,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
    post: &dyn Fn(grokhub_core::UpdateCard) -> bool,
) {
    let list = sw::load(dir);
    let failed_at = WISH_FAILED_MS.lock().map(|t| *t).unwrap_or(0);
    if !list.due(now) || now.saturating_sub(failed_at) < WISH_RETRY_MS {
        return;
    }
    let Some(id) = open_steam_id(dir) else {
        return;
    };
    let allowed = sw::alerts_allowed(bias(dir, sw::SOURCE_NAME));
    let run = sw::run_check(list, id, allowed, now, get);
    if let (Some(_), Ok(mut t)) = (&run.error, WISH_FAILED_MS.lock()) {
        *t = now;
    }
    set_note("deals", finish_wishlist(dir, run, post));
}

/// Folds a wishlist check in and posts its sales. Settings may have changed
/// while it ran; the person's choice wins.
pub fn finish_wishlist(
    dir: &Path,
    run: sw::Run,
    post: &dyn Fn(grokhub_core::UpdateCard) -> bool,
) -> String {
    let saved = sw::load(dir);
    if let Some(why) = run.error {
        return format!("Wishlist check failed: {why}. Trying again in an hour.");
    }
    if !saved.on {
        return String::new();
    }
    let mut list = run.list;
    list.threshold = saved.threshold;
    let at = list.last_check_ms;
    let added = run.alerts.iter().filter(|a| post(a.card(at))).count();
    let note = if list.private {
        PRIVATE.to_string()
    } else {
        sw::checked_note(&list, added)
    };
    let _ = sw::save(dir, &list);
    note
}

/// What a private profile (or an empty wishlist) needs, in plain words.
pub const PRIVATE: &str =
    "Steam isn't sharing this. In Steam, set your profile's Game details to Public.";

fn achievements(
    dir: &Path,
    now: u64,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
    post: &dyn Fn(grokhub_core::UpdateCard) -> bool,
) {
    let mut tracker = sa::load(dir);
    if !tracker.due(now) {
        return;
    }
    let Some(id) = open_steam_id(dir) else {
        return;
    };
    // Counted as a check either way; a failure shortens the wait.
    tracker.last_check_ms = now;
    let _ = sa::save(dir, &tracker);
    let root = grokhub_core::steam::detect_root();
    let games = sa::games_to_check(dir, root.as_deref());
    let run = sa::run_check(&games, id, get);
    set_note("achievements", finish_achievements(dir, run, now, post));
}

/// Folds an achievements check in and posts its nudges.
pub fn finish_achievements(
    dir: &Path,
    run: sa::Run,
    now: u64,
    post: &dyn Fn(grokhub_core::UpdateCard) -> bool,
) -> String {
    let mut tracker = sa::load(dir);
    if !tracker.on {
        return String::new();
    }
    if let Some(why) = run.error {
        tracker.failed(now);
        let _ = sa::save(dir, &tracker);
        let hours = tracker.wait_ms() / (60 * 60 * 1000);
        let when = if hours == 1 {
            "an hour".to_string()
        } else {
            format!("{hours} hours")
        };
        return format!("Achievement check failed: {why}. Trying again in {when}.");
    }
    tracker.counts.extend(run.counts);
    let allowed = sa::nudges_allowed(bias(dir, sa::SOURCE_NAME));
    let picks = tracker.check(run.picks, run.private, allowed, now);
    let added = picks.iter().filter(|n| post(n.card(now))).count();
    let _ = sa::save(dir, &tracker);
    grokhub_core::objectives::refresh_from_games(dir, now);
    if run.private {
        PRIVATE.to_string()
    } else {
        sa::checked_note(false, added)
    }
}

/// The account a one-tap turn-on uses, or why there isn't one.
fn account(dir: &Path) -> Result<u64, String> {
    if let Some(id) = open_steam_id(dir) {
        return Ok(id);
    }
    let root = grokhub_core::steam::detect_root()
        .ok_or("Steam isn't installed on this PC, so there's nothing to check yet.")?;
    let local = sw::local_accounts(&root);
    let id = sw::pick_account(&local, sw::most_recent_account(&root)).ok_or(if local.is_empty() {
        "Sign in to Steam on this PC once, then tap Turn on again."
    } else {
        "More than one Steam account signs in here. Sign in to the one you play on, then tap Turn on again."
    })?;
    seal_steam_id(dir, id)?;
    Ok(id)
}

/// Home's Turn on (or Turn off) for Achievements and Deals: uses the Steam
/// account signed in on this PC, switches the source, and checks right away.
pub fn turn(dir: &Path, module: &str, on: bool) -> Result<String, String> {
    if !on {
        match module {
            "achievements" => {
                let mut t = sa::load(dir);
                t.turn_off();
                sa::save(dir, &t)?;
            }
            "deals" => {
                let mut l = sw::load(dir);
                l.turn_off();
                sw::save(dir, &l)?;
            }
            _ => return Err("That module has no setup here.".into()),
        }
        set_note(
            if module == "deals" {
                "deals"
            } else {
                "achievements"
            },
            String::new(),
        );
        return Ok("Off. Nothing more is checked.".into());
    }
    let line = match module {
        "achievements" => "On. Checking your games now; ones close to 100% show here.",
        "deals" => "On. Checking your wishlist now; sales show here.",
        _ => return Err("That module has no setup here.".into()),
    };
    if let Err(why) = account(dir) {
        set_note(
            if module == "deals" {
                "deals"
            } else {
                "achievements"
            },
            why.clone(),
        );
        return Err(why);
    }
    match module {
        "achievements" => {
            let mut t = sa::load(dir);
            t.on = true;
            t.last_check_ms = 0;
            t.failures = 0;
            sa::save(dir, &t)?;
            set_note("achievements", "Checking your games…".into());
        }
        _ => {
            let mut l = sw::load(dir);
            l.on = true;
            l.last_check_ms = 0;
            sw::save(dir, &l)?;
            if let Ok(mut t) = WISH_FAILED_MS.lock() {
                *t = 0;
            }
            set_note("deals", "Checking your wishlist…".into());
        }
    }
    let dir = dir.to_path_buf();
    std::thread::spawn(move || tick(&dir, grokhub_core::now_ms()));
    Ok(line.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("steam-checks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn replies(url: &str) -> Result<String, String> {
        if url.contains("GetWishlist") {
            Ok(r#"{"response":{"items":[{"appid":11,"priority":0}]}}"#.into())
        } else if url.contains("price_overview") {
            Ok(r#"{"11":{"success":true,"data":{"price_overview":{"initial":2000,"final":1000,"discount_percent":50,"initial_formatted":"$20.00","final_formatted":"$10.00"}}}}"#.into())
        } else if url.contains("filters=basic") {
            Ok(r#"{"11":{"success":true,"data":{"name":"Fake Quest"}}}"#.into())
        } else {
            Err(format!("unexpected {url}"))
        }
    }

    #[test]
    fn a_wishlist_sale_posts_once_while_on_and_says_so() {
        let d = dir("wish");
        sw::save(
            &d,
            &sw::Wishlist {
                on: true,
                ..Default::default()
            },
        )
        .unwrap();
        let posted = RefCell::new(Vec::new());
        let post = |c: grokhub_core::UpdateCard| {
            posted.borrow_mut().push(c.title);
            true
        };
        let run = sw::run_check(sw::load(&d), 76_561_197_960_278_073, 3, 9, &mut replies);
        assert_eq!(
            finish_wishlist(&d, run, &post),
            "Checked 1 wishlisted games. 1 on sale."
        );
        assert_eq!(*posted.borrow(), vec!["Fake Quest is 50% off".to_string()]);
        // Switched off while a check ran: nothing posts.
        let run = sw::run_check(sw::load(&d), 76_561_197_960_278_073, 3, 10, &mut replies);
        let mut off = sw::load(&d);
        off.turn_off();
        sw::save(&d, &off).unwrap();
        posted.borrow_mut().clear();
        assert_eq!(finish_wishlist(&d, run, &post), "");
        assert!(posted.borrow().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_achievement_check_backs_off_and_names_no_steam_id() {
        let d = dir("ach");
        sa::save(
            &d,
            &sa::Tracker {
                on: true,
                ..Default::default()
            },
        )
        .unwrap();
        let run = sa::run_check(&[(1, "Game".into())], 76_561_197_960_278_073, &mut |_| {
            Err("HTTP 500".into())
        });
        let note = finish_achievements(&d, run, 5, &|_| true);
        assert_eq!(
            note,
            "Achievement check failed: Steam's stats pages didn't answer. Trying again in an hour."
        );
        assert!(!note.contains("76561197960278073"));
        assert_eq!(sa::load(&d).failures, 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn turning_off_forgets_the_source_and_never_needs_steam() {
        let d = dir("off");
        sw::save(
            &d,
            &sw::Wishlist {
                on: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            turn(&d, "deals", false),
            Ok("Off. Nothing more is checked.".into())
        );
        assert!(!sw::load(&d).on);
        assert!(turn(&d, "frame_rate", true).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
