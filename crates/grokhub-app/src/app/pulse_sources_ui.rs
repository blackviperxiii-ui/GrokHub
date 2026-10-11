//! Free Pulse sources: the background fetch, the Feed posts it makes, and
//! Settings → Pulse → Sources. Every post names its item and source and links to it;
//! no model writes any of it. The cache, last run and already-posted links live in
//! `{config}/pulse-sources.json`.

use super::*;
use grokhub_agent::harness::DataClass;
use grokhub_core::pulse_sources::{self as ps, Fetched, SourceItem, SourceKind, SourceReq};
use std::io::Read as _;

/// Largest reply read from one source.
const BODY_CAP: u64 = 2 * 1024 * 1024;
const FETCH_TIMEOUT_SECS: u64 = 8;
const REDIRECTS_MAX: usize = 3;

#[derive(Default)]
pub(super) struct SourcesState {
    pub rx: Option<mpsc::Receiver<SourceRun>>,
    /// The Add box for each list, by `SourceKind::key`.
    pub drafts: std::collections::BTreeMap<&'static str, String>,
    /// What the last run did, for Settings.
    pub note: String,
    /// From the cache file the first time it is needed.
    pub last_run_ms: Option<u64>,
}

pub(super) struct SourceRun {
    pub items: Vec<SourceItem>,
    pub errors: Vec<String>,
    pub fetched: usize,
    pub asked: usize,
    pub at_ms: u64,
}

pub(super) fn sources_cache_path() -> std::path::PathBuf {
    crate::config::config_dir().join(ps::CACHE_FILE)
}

fn user_agent() -> String {
    format!(
        "GrokHub/{} (+https://github.com/blackviperxiii-ui/GrokHub)",
        env!("CARGO_PKG_VERSION")
    )
}

fn source_agent() -> ureq::Agent {
    // Redirects are followed by hand so every hop is checked as a public URL.
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .redirects(0)
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
}

fn redirect_target(from: &str, location: &str) -> String {
    if location.starts_with("https://") || location.starts_with("http://") {
        return location.to_string();
    }
    let (scheme, rest) = from.split_once("://").unwrap_or(("https", from));
    let host = rest.split('/').next().unwrap_or("");
    if location.starts_with('/') {
        format!("{scheme}://{host}{location}")
    } else {
        format!("{scheme}://{host}/{location}")
    }
}

/// One source over HTTP. A topic search carries the person's topic, so it goes as
/// chat text; a followed feed URL carries nothing of theirs.
fn fetch_source(req: &SourceReq, have: Option<&ps::CacheEntry>) -> Fetched {
    let data: &[DataClass] = if req.topic.is_some() {
        &[DataClass::Chat]
    } else {
        &[]
    };
    let mut url = req.url.clone();
    for hop in 0..=REDIRECTS_MAX {
        if !grokhub_core::public_http_url(&url) {
            return Fetched::Failed("not a public address".into());
        }
        if let Err(why) = crate::xai::egress_ok(&url, data) {
            return Fetched::Failed(why);
        }
        let mut call = source_agent()
            .get(&url)
            .set("user-agent", &user_agent())
            .set(
                "accept",
                "application/json, application/atom+xml, application/rss+xml, application/xml;q=0.9, */*;q=0.5",
            );
        if hop == 0 {
            if let Some(entry) = have {
                if let Some(etag) = entry.etag.as_deref() {
                    call = call.set("if-none-match", etag);
                }
                if let Some(when) = entry.last_modified.as_deref() {
                    call = call.set("if-modified-since", when);
                }
            }
        }
        let resp = match call.call() {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, _)) => return Fetched::Failed(format!("HTTP {code}")),
            Err(ureq::Error::Transport(t)) => {
                let why = match t.kind() {
                    ureq::ErrorKind::Dns => "could not find the host",
                    ureq::ErrorKind::ConnectionFailed => "could not connect",
                    ureq::ErrorKind::Io => "timed out or dropped",
                    _ => "network error",
                };
                return Fetched::Failed(why.into());
            }
        };
        let status = resp.status();
        if status == 304 {
            return Fetched::NotModified;
        }
        if (300..400).contains(&status) {
            match resp.header("location") {
                Some(loc) => {
                    url = redirect_target(&url, loc);
                    continue;
                }
                None => return Fetched::Failed(format!("HTTP {status}")),
            }
        }
        let etag = resp.header("etag").map(str::to_string);
        let last_modified = resp.header("last-modified").map(str::to_string);
        let mut buf = Vec::new();
        if resp.into_reader().take(BODY_CAP).read_to_end(&mut buf).is_err() {
            return Fetched::Failed("timed out or dropped".into());
        }
        return Fetched::Body {
            body: String::from_utf8_lossy(&buf).into_owned(),
            etag,
            last_modified,
        };
    }
    Fetched::Failed("too many redirects".into())
}

/// "just now", "12 min ago", "3 h ago", for the Settings note.
fn ago_label(ms: u64, now: u64) -> String {
    let mins = now.saturating_sub(ms) / 60_000;
    match mins {
        0 => "just now".into(),
        1..=59 => format!("{mins} min ago"),
        _ => format!("{} h ago", mins / 60),
    }
}

impl Cabin {
    /// Interest topics to search, strongest first, and the muted ones.
    fn source_topics(&mut self) -> (Vec<String>, Vec<String>) {
        let profile = self.interests_now();
        let live = profile.ranked().into_iter().map(|t| t.topic.clone()).collect();
        let muted = profile
            .topics
            .iter()
            .filter(|t| t.muted)
            .map(|t| t.topic.clone())
            .collect();
        (live, muted)
    }

    fn sources_last_run(&mut self) -> u64 {
        if let Some(at) = self.sources.last_run_ms {
            return at;
        }
        let at = ps::load_cache(&sources_cache_path()).last_run_ms;
        self.sources.last_run_ms = Some(at);
        at
    }

    /// Housekeeping: run the sources on the feed clock, never in quiet hours.
    pub(super) fn follow_source_fetch(&mut self) {
        let pulse = &self.cfg.feed_pulse;
        if self.sources.rx.is_some() || !pulse.digest_on || self.quiet_now() {
            return;
        }
        let every = pulse.digest_ms;
        let now = now_ms();
        if now.saturating_sub(self.sources_last_run()) < every {
            return;
        }
        self.kick_source_fetch();
    }

    /// Fetch every enabled source now, in the background.
    pub(super) fn kick_source_fetch(&mut self) {
        if self.sources.rx.is_some() {
            return;
        }
        let now = now_ms();
        if !self.interests_now().seeded {
            let user_md = crate::config::read_memory("USER.md");
            let memory_md = crate::config::read_memory("MEMORY.md");
            self.seed_interests(&user_md, &memory_md);
        }
        let (topics, _) = self.source_topics();
        let reqs = ps::requests(&self.cfg.pulse_sources, &topics, now);
        // Counted as a run either way, so an empty setup does not retry every tick.
        self.sources.last_run_ms = Some(now);
        if reqs.is_empty() {
            self.sources.note = "Nothing to check yet. Add an interest or a source.".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.sources.rx = Some(rx);
        self.sources.note = format!("Checking {} sources…", reqs.len());
        std::thread::spawn(move || {
            let path = sources_cache_path();
            let mut cache = ps::load_cache(&path);
            let got = ps::gather(&reqs, &mut cache, now, &mut |req, have| fetch_source(req, have));
            cache.last_run_ms = now;
            let _ = ps::save_cache(&path, &cache);
            let _ = tx.send(SourceRun {
                items: got.items,
                errors: got.errors,
                fetched: got.fetched,
                asked: reqs.len(),
                at_ms: now,
            });
        });
    }

    pub(super) fn poll_source_fetch(&mut self) {
        let Some(rx) = self.sources.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(run) => {
                self.post_source_run(run);
            }
            Err(mpsc::TryRecvError::Empty) => self.sources.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sources.note = "The source check stopped before it finished.".into();
            }
        }
    }

    /// New items become Feed posts; everything posted is remembered so a dismissed
    /// post never comes back. Returns how many posted.
    pub(super) fn post_source_run(&mut self, run: SourceRun) -> usize {
        let (_, muted) = self.source_topics();
        let path = sources_cache_path();
        let mut cache = ps::load_cache(&path);
        let posted: Vec<String> = self
            .updates
            .iter()
            .flat_map(|c| c.citations.iter().cloned())
            .collect();
        let fresh = ps::pick_new(&run.items, &cache, &muted, &posted, run.at_ms);
        let mut added = 0;
        for item in &fresh {
            if grokhub_core::post_update(&mut self.updates, ps::item_card(item, run.at_ms)) {
                added += 1;
            }
        }
        ps::mark_seen(&mut cache, &fresh);
        cache.last_run_ms = run.at_ms;
        if let Err(err) = ps::save_cache(&path, &cache) {
            self.status = err;
        }
        if added > 0 {
            self.persist_updates();
        }
        let mut note = format!(
            "Checked {} sources ({} from the network), {} new {}.",
            run.asked,
            run.fetched,
            added,
            if added == 1 { "post" } else { "posts" }
        );
        if let Some(first) = run.errors.first() {
            let more = run.errors.len() - 1;
            note.push_str(&format!(" Failed: {first}"));
            if more > 0 {
                note.push_str(&format!(" and {more} more"));
            }
            note.push('.');
        }
        self.sources.note = note;
        self.sources.last_run_ms = Some(run.at_ms);
        added
    }

    pub(super) fn ui_pulse_sources(&mut self, ui: &mut egui::Ui) {
        self.poll_source_fetch();
        if self.sources.rx.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        ui.label(
            RichText::new("Sources")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.add_space(2.0);
        crate::cards::help_text(
            ui,
            "Free sources only, no keys. Hacker News and Reddit search your top three interests; the lists below are followed as they are. Each post names its source and links to it.",
        );
        ui.add_space(8.0);
        let mut changed = false;
        for kind in SourceKind::ALL {
            let mut on = self.cfg.pulse_sources.on(kind);
            if ui
                .checkbox(&mut on, RichText::new(kind.label()).size(crate::theme::FONT_BODY))
                .changed()
            {
                self.cfg.pulse_sources.set_on(kind, on);
                changed = true;
            }
            if on {
                changed |= self.ui_source_list(ui, kind);
            }
            ui.add_space(6.0);
        }
        if changed {
            self.persist_cfg();
        }
        ui.add_space(4.0);
        if self.sources.rx.is_none() && crate::cards::white_pill(ui, "Check now") {
            self.kick_source_fetch();
        }
        ui.add_space(4.0);
        {
            let note = if self.sources.note.is_empty() {
                match self.sources_last_run() {
                    0 => "Not checked yet.".to_string(),
                    at => format!("Last checked {}.", ago_label(at, now_ms())),
                }
            } else {
                self.sources.note.clone()
            };
            ui.add(
                egui::Label::new(
                    RichText::new(note)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                )
                .wrap(),
            );
        }
    }

    /// The followed list under a source, with Remove and an Add box. True when it changed.
    fn ui_source_list(&mut self, ui: &mut egui::Ui, kind: SourceKind) -> bool {
        let hint = match kind {
            SourceKind::HackerNews => return false,
            SourceKind::Reddit => "Subreddit, like rust or r/rust",
            SourceKind::GithubReleases => "Repo, like rust-lang/rust",
            SourceKind::YouTube => "Channel id (UC…) or channel URL",
            SourceKind::Rss => "Feed URL, like https://example.com/feed.xml",
        };
        let entries = self
            .cfg
            .pulse_sources
            .list_mut(kind)
            .map(|l| l.clone())
            .unwrap_or_default();
        let mut remove: Option<String> = None;
        let mut changed = false;
        ui.indent(("pulse-source", kind.key()), |ui| {
            for entry in &entries {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(entry).size(crate::theme::FONT_TIP).color(crate::theme::fg()));
                    if crate::cards::ghost_pill(ui, "Remove") {
                        remove = Some(entry.clone());
                    }
                });
            }
            let mut add = false;
            let draft = self.sources.drafts.entry(kind.key()).or_default();
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(draft)
                        .id(egui::Id::new(("pulse-source-add", kind.key())))
                        .hint_text(hint)
                        .desired_width(300.0),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    add = true;
                }
                if crate::cards::ghost_pill(ui, "Add") {
                    add = true;
                }
            });
            if add {
                let text = draft.clone();
                match self.cfg.pulse_sources.add(kind, &text) {
                    Ok(entry) => {
                        self.sources.drafts.insert(kind.key(), String::new());
                        self.status = format!("Pulse will follow {entry}.");
                        changed = true;
                    }
                    Err(why) => self.status = why,
                }
            }
        });
        if let Some(entry) = remove {
            if let Some(list) = self.cfg.pulse_sources.list_mut(kind) {
                list.retain(|e| *e != entry);
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cabin() -> (std::sync::MutexGuard<'static, ()>, crate::config::TestConfigDir, Cabin) {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root("pulse-sources-ui");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pin = crate::config::TestConfigDir::set(root);
        let cabin = Cabin::quiet_for_test();
        (lock, pin, cabin)
    }

    fn item(title: &str, url: &str, at: u64) -> SourceItem {
        SourceItem {
            title: title.into(),
            url: url.into(),
            kind: SourceKind::HackerNews,
            source_name: "Hacker News".into(),
            published_ms: at,
            summary: "812 points, 301 comments on Hacker News.".into(),
            image: None,
            topic: Some("Rust".into()),
        }
    }

    #[test]
    fn a_run_posts_named_linked_feed_cards_once() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        let now = now_ms();
        let run = |items: Vec<SourceItem>| SourceRun {
            items,
            errors: vec!["r/rust: HTTP 429".into()],
            fetched: 3,
            asked: 4,
            at_ms: now,
        };
        let added = cabin.post_source_run(run(vec![
            item("Rust 1.99 released", "https://blog.rust-lang.org/1.99", now - 60_000),
            item("Rust 1.99 released (dup)", "https://www.blog.rust-lang.org/1.99/?utm_source=hn", now - 120_000),
        ]));
        assert_eq!(added, 1);
        assert_eq!(
            cabin.sources.note,
            "Checked 4 sources (3 from the network), 1 new post. Failed: r/rust: HTTP 429."
        );
        let card = cabin
            .updates
            .iter()
            .find(|c| c.title == "Rust 1.99 released")
            .cloned()
            .expect("posted");
        assert_eq!(card.citations, vec!["https://blog.rust-lang.org/1.99".to_string()]);
        assert_eq!(card.pulse.source_name.as_deref(), Some("Hacker News"));
        // It shows on Pulse → Feed.
        let inputs = grokhub_core::pulse::PulseInputs {
            now_ms: now,
            ..Default::default()
        };
        let feed = grokhub_core::pulse::feed_posts(&cabin.updates, &inputs);
        assert!(feed.iter().any(|c| c.id == card.id), "{feed:?}");
        // Dismissed, it never comes back on the next run.
        let id = card.id.clone();
        cabin.dismiss_feed_card(&id);
        cabin.updates.retain(|c| c.id != id);
        assert_eq!(cabin.post_source_run(run(vec![item("Rust 1.99 released", "https://blog.rust-lang.org/1.99", now - 60_000)])), 0);
        assert!(ps::load_cache(&sources_cache_path()).seen.contains(&"blog.rust-lang.org/1.99".to_string()));
    }

    #[test]
    fn muted_topics_never_post_and_an_empty_setup_says_so() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("crypto");
        cabin.interests_now().set_muted("crypto", true);
        let now = now_ms();
        let added = cabin.post_source_run(SourceRun {
            items: vec![item("Crypto exchange collapses", "https://news.test/c", now)],
            errors: Vec::new(),
            fetched: 1,
            asked: 1,
            at_ms: now,
        });
        assert_eq!(added, 0);
        cabin.cfg.pulse_sources = ps::SourceSettings::default();
        cabin.interests.profile = Some(Default::default());
        cabin.kick_source_fetch();
        assert!(cabin.sources.rx.is_none());
        assert_eq!(cabin.sources.note, "Nothing to check yet. Add an interest or a source.");
    }

    #[test]
    fn relative_redirects_stay_on_the_same_host() {
        assert_eq!(redirect_target("https://a.test/feed", "/feed.xml"), "https://a.test/feed.xml");
        assert_eq!(redirect_target("http://a.test/x/y", "rss"), "http://a.test/rss");
        assert_eq!(redirect_target("http://a.test/x", "https://b.test/rss"), "https://b.test/rss");
    }

    #[test]
    fn ago_labels() {
        assert_eq!(ago_label(1_000, 30_000), "just now");
        assert_eq!(ago_label(0, 12 * 60_000), "12 min ago");
        assert_eq!(ago_label(0, 185 * 60_000), "3 h ago");
    }
}
