//! New local model suggestions (`grokhub_core::local_model_news`), opt-in
//! under Settings → Pulse. Once a week, off the main thread: read this PC's
//! hardware and the local model it runs, score the catalog, and read
//! huihui-ai's newest abliterated GGUF repos on Hugging Face (public API, no
//! key). A catalog upgrade card opens Settings → Local AI on that model,
//! where Download is the approval; a release card only links Hugging Face.
//! Nothing about this PC is sent: the only request is the public repo list.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_core::local_model_news::{self as lm, Suggestion};
use grokhub_core::local_picker::{BenchCache, Profile};
use grokhub_core::local_setup::{local_models, LocalModel};
use grokhub_core::pulse_sources as ps;

const TICK: Duration = Duration::from_secs(10 * 60);

pub const CHECKING: &str = "Checking for new local models…";
pub const NO_MODEL: &str = "No local model yet. Set one up under Settings → Local AI first.";
pub const NOTHING_NEW: &str = "Checked. Nothing new that fits this PC better.";
pub const TURN_ON_FIRST: &str = "Turn local model suggestions on first.";
pub const LIST_UNREAD: &str = "Couldn't read Hugging Face's model list. Trying again next week.";

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Held across each load, change and save of the tracker file, so a switch
/// in Settings and a check finishing on the timer thread can't undo each
/// other. Never held across a request.
static TRACKER: Mutex<()> = Mutex::new(());

fn tracker_lock() -> std::sync::MutexGuard<'static, ()> {
    TRACKER.lock().unwrap_or_else(|e| e.into_inner())
}
/// What the last check said, for Settings.
static NOTE: Mutex<String> = Mutex::new(String::new());

fn note() -> String {
    NOTE.lock().map(|n| n.clone()).unwrap_or_default()
}

fn set_note(text: &str) {
    if let Ok(mut n) = NOTE.lock() {
        *n = text.to_string();
    }
}

/// What the check reads off this PC: the hardware and the local model.
pub(crate) struct Machine {
    pub profile: Profile,
    pub installed: Option<&'static LocalModel>,
}

/// One check's findings, before the tracker decides what to post.
pub(crate) struct NewsRun {
    upgrade: Option<Suggestion>,
    releases: Vec<Suggestion>,
    read: Vec<String>,
    /// No catalog chat model runs here: nothing to beat yet.
    no_model: bool,
}

/// The whole check for one machine, given a GET for the repo lists. With no
/// local model it asks for nothing.
pub(crate) fn run_check(
    m: &Machine,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
) -> NewsRun {
    let Some(current) = m.installed else {
        return NewsRun {
            upgrade: None,
            releases: Vec::new(),
            read: Vec::new(),
            no_model: true,
        };
    };
    let upgrade = lm::upgrade(&m.profile, current.id, &BenchCache::new());
    let mut read = Vec::new();
    for publisher in lm::PUBLISHERS {
        // A publisher that didn't answer is read again next week.
        if let Ok(body) = get(&lm::releases_url(publisher)) {
            read.extend(lm::parse_releases(&body));
        }
    }
    let releases = lm::releases(&m.profile, current.run_mb, &read);
    NewsRun {
        upgrade,
        releases,
        read,
        no_model: false,
    }
}

/// The catalog chat model the local engine runs, by its file name. A model
/// file the catalog doesn't list gives `None`: there is nothing to compare.
pub(crate) fn installed_model(config_dir: &Path) -> Option<&'static LocalModel> {
    let path = grokhub_agent::route::local_sidecar::find_model(config_dir)?;
    let name = path.file_name()?;
    local_models()
        .iter()
        .find(|m| m.is_chat() && Path::new(m.file).file_name() == Some(name))
}

fn this_machine(dir: &Path) -> Machine {
    Machine {
        profile: crate::local_ai::read_machine(dir).profile(),
        installed: installed_model(dir),
    }
}

/// One keyless GET through the Pulse fetcher (public pages, egress guard).
fn get(url: &str) -> Result<String, String> {
    crate::pulse_feed::net().page(url)
}

/// How the person rated these posts (Not interested lowers it).
fn bias(dir: &Path) -> i32 {
    ps::load_cache(&dir.join(ps::CACHE_FILE))
        .source_bias
        .get(lm::SOURCE_NAME)
        .copied()
        .unwrap_or(0)
}

/// Fold a check in: post the new cards and say what happened. Returns the
/// note, empty when the source was switched off while the check ran.
pub(crate) fn finish(
    dir: &Path,
    run: NewsRun,
    now: u64,
    post: &dyn Fn(grokhub_core::UpdateCard) -> bool,
) -> String {
    let _held = tracker_lock();
    // Read again: switched off while it ran, the user's choice wins.
    let mut t = lm::load(dir);
    if !t.on {
        return String::new();
    }
    if run.no_model {
        return NO_MODEL.to_string();
    }
    let unread = run.read.is_empty();
    let picks = t.check(
        run.upgrade,
        run.releases,
        &run.read,
        lm::links_allowed(bias(dir)),
        now,
    );
    let added = picks.iter().filter(|s| post(s.card(now))).count();
    if let Err(why) = lm::save(dir, &t) {
        return why;
    }
    match (added, unread) {
        (0, true) => LIST_UNREAD.to_string(),
        (0, false) => NOTHING_NEW.to_string(),
        (n, true) => format!("Checked. {n} new for this PC. {LIST_UNREAD}"),
        (n, false) => format!("Checked. {n} new for this PC."),
    }
}

/// Checks on a slow clock, starting a minute after launch.
pub fn start(config_dir: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            tick(&config_dir, grokhub_core::now_ms());
            std::thread::sleep(TICK);
        }
    });
}

/// Runs the weekly check when it is on and due, where Pulse and local models
/// both run. One run at a time.
pub fn tick(dir: &Path, now: u64) {
    let caps = crate::capabilities::current();
    if !caps.pulse || !caps.local_ai || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let post = |card| crate::pulse::add_post(dir, card).unwrap_or(false);
    if let Some(note) = check_if_due(dir, now, &this_machine, &mut get, &post) {
        set_note(&note);
    }
    RUNNING.store(false, Ordering::SeqCst);
}

/// [`tick`] with the PC and the network passed in. `None` when nothing ran.
pub(crate) fn check_if_due(
    dir: &Path,
    now: u64,
    machine: &dyn Fn(&Path) -> Machine,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
    post: &dyn Fn(grokhub_core::UpdateCard) -> bool,
) -> Option<String> {
    {
        let _held = tracker_lock();
        let mut t = lm::load(dir);
        if !t.due(now) {
            return None;
        }
        // Counted before any request, so a failure also waits a week.
        t.last_check_ms = now;
        if let Err(why) = lm::save(dir, &t) {
            return Some(why);
        }
    }
    set_note(CHECKING);
    let run = run_check(&machine(dir), get);
    Some(finish(dir, run, now, post))
}

/// Settings → Pulse's row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelNews {
    pub on: bool,
    pub note: String,
}

pub fn status(dir: &Path) -> LocalModelNews {
    let on = lm::load(dir).on;
    LocalModelNews {
        on,
        note: if on { note() } else { String::new() },
    }
}

/// Turn suggestions on, or off (which forgets what was seen, so turning them
/// on again starts fresh without a backlog).
pub fn set_on(dir: &Path, on: bool) -> Result<LocalModelNews, String> {
    {
        let _held = tracker_lock();
        let mut t = lm::load(dir);
        if on {
            t.on = true;
        } else {
            t.turn_off();
            set_note("");
        }
        lm::save(dir, &t)?;
    }
    Ok(status(dir))
}

/// Check now, off this thread.
pub fn check_now(dir: &Path) -> Result<LocalModelNews, String> {
    {
        let _held = tracker_lock();
        let mut t = lm::load(dir);
        if !t.on {
            return Err(TURN_ON_FIRST.into());
        }
        t.last_check_ms = 0;
        lm::save(dir, &t)?;
    }
    set_note(CHECKING);
    let dir_owned = dir.to_path_buf();
    std::thread::spawn(move || tick(&dir_owned, grokhub_core::now_ms()));
    Ok(status(dir))
}

#[cfg(test)]
#[path = "local_model_checks_tests.rs"]
mod tests;
