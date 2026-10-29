//! Free Pulse sources: the background fetch, the Feed posts it makes, and
//! Settings → Pulse → Sources. Every post names its item and source and links to it.
//! Everything is fetched and ranked locally; only a run's top picks go to the fast
//! model, in one call, for short summaries, and a run with nothing new calls no model.
//! The cache, last run, already-posted links and today's model calls live in
//! `{config}/pulse-sources.json`.

use super::pulse_ui::SourceAct;
use super::*;
use grokhub_agent::harness::DataClass;
use grokhub_core::interests::InterestSignal;
use grokhub_core::pulse::LedgerReason;
use grokhub_core::pulse_sources::{
    self as ps, Fetched, Ranked, SavedFind, SourceItem, SourceKind, SourceReq,
};
use std::io::Read as _;

/// The note when no interest or source is set up to check.
pub(super) const NOTHING_TO_CHECK: &str = "Nothing to check yet. Add an interest or a source.";
/// Largest reply read from one source.
const BODY_CAP: u64 = 2 * 1024 * 1024;
const FETCH_TIMEOUT_SECS: u64 = 8;
const REDIRECTS_MAX: usize = 3;

#[derive(Default)]
pub(super) struct SourcesState {
    pub rx: Option<mpsc::Receiver<SourceRun>>,
    /// A run's picks waiting on the fast model's summaries.
    pub summary_rx: Option<mpsc::Receiver<Summarized>>,
    /// The check after launch has been considered.
    pub opened: bool,
    /// Today's summary calls as (UTC day, count), read from the cache file once.
    pub budget: Option<(u64, u32)>,
    /// Saved Finds, read from disk the first time anything needs them.
    pub saved: Option<Vec<SavedFind>>,
    /// The Add box for each list, by `SourceKind::key`.
    pub drafts: std::collections::BTreeMap<&'static str, String>,
    /// What the last run did, for Settings and the Feed banner.
    pub note: String,
    /// From the cache file the first time it is needed.
    pub last_run_ms: Option<u64>,
    /// More like this started a check; Feed shows Looking… until it finishes.
    pub looking_more: bool,
    /// The Steam library and its Pulse prefs.
    pub steam: super::steam_ui::SteamState,
    /// Know me's saved tags and its running scan.
    pub know_me: super::knowme_ui::KnowMeState,
    /// Learned reactions per card category, read from disk when first needed.
    pub reactions: Option<grokhub_core::reactions::Reactions>,
    /// The learned daily rhythm, read from disk when first needed.
    pub rhythm: Option<grokhub_core::rhythm::Rhythm>,
    /// Steam wishlist sale alerts and their daily check.
    pub wish: super::wishlist_ui::WishState,
    /// Achievement nudges and their weekly check.
    pub ach: super::achievements_ui::AchState,
    /// Upcoming DLC and sequels, and release-day reminders.
    pub radar: super::release_radar_ui::RadarState,
    /// New local model suggestions and their weekly check.
    pub models: super::local_models_ui::ModelNewsState,
}

/// A run, its picks, and the fast model's reply.
pub(super) type Summarized = (SourceRun, Vec<Ranked>, Result<String, String>);

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

pub(super) fn saved_finds_path() -> std::path::PathBuf {
    crate::config::config_dir().join(ps::SAVED_FILE)
}

/// Saved Finds listed in Settings before "and N more".
const SAVED_SHOWN: usize = 20;

fn user_agent() -> String {
    format!(
        "GrokHub/{} (+https://github.com/blackviperxiii-ui/GrokHub)",
        env!("CARGO_PKG_VERSION")
    )
}

fn source_agent() -> ureq::Agent {
    // Redirects are followed by hand so every hop is checked as a public URL.
    // Dial resolver blocks DNS→private/CGNAT even when the hostname looked public.
    // No env proxy: HTTP(S)_PROXY would dial the proxy and skip public_only_resolve
    // on the target (same as web_fetch).
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .resolver(grokhub_agent::public_only_resolve)
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
pub(super) fn fetch_source(req: &SourceReq, have: Option<&ps::CacheEntry>) -> Fetched {
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
                "application/json, application/atom+xml, application/rss+xml, application/xml;q=0.9, text/html;q=0.8, */*;q=0.5",
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
        if resp
            .into_reader()
            .take(BODY_CAP)
            .read_to_end(&mut buf)
            .is_err()
        {
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

const BUDGET_USED: &str = "Today's summary budget is used, so they show the sources' own text.";

fn plural(n: usize) -> &'static str {
    if n == 1 {
        "post"
    } else {
        "posts"
    }
}

/// "just now", "12 min ago", "3 h ago", for the Settings note.
pub(super) fn ago_label(ms: u64, now: u64) -> String {
    let mins = now.saturating_sub(ms) / 60_000;
    match mins {
        0 => "just now".into(),
        1..=59 => format!("{mins} min ago"),
        _ => format!("{} h ago", mins / 60),
    }
}

impl Cabin {
    /// Ready + searchable interest topics (evidence-gated graph, then #727 junk filter).
    fn source_topics(&mut self) -> Vec<String> {
        self.interests_now().search_topics()
    }

    fn sources_last_run(&mut self) -> u64 {
        if let Some(at) = self.sources.last_run_ms {
            return at;
        }
        let at = ps::load_cache(&sources_cache_path()).last_run_ms;
        self.sources.last_run_ms = Some(at);
        at
    }

    /// Housekeeping: run the sources on the system cadence (every two hours) and once
    /// when the app opens, never in quiet hours.
    pub(super) fn follow_source_fetch(&mut self) {
        let just_opened = !std::mem::replace(&mut self.sources.opened, true);
        if self.sources_busy() || !self.cfg.feed_pulse.digest_on || self.quiet_now() {
            return;
        }
        // The system cadence (base 120 minutes); the cabin keeps no
        // engagement clock, so it never speeds up or slows down.
        let every = u64::from(grokhub_core::pulse_writer::cadence_mins(0, now_ms())) * 60_000;
        if !ps::check_due(self.sources_last_run(), now_ms(), every, just_opened) {
            return;
        }
        self.kick_source_fetch();
    }

    fn summary_calls_today(&mut self, now: u64) -> u32 {
        let (summary_day, summary_calls) = *self.sources.budget.get_or_insert_with(|| {
            let cache = ps::load_cache(&sources_cache_path());
            (cache.summary_day, cache.summary_calls)
        });
        ps::SourceCache {
            summary_day,
            summary_calls,
            ..Default::default()
        }
        .calls_today(now)
    }

    pub(super) fn sources_busy(&self) -> bool {
        self.sources.rx.is_some() || self.sources.summary_rx.is_some()
    }

    /// Fetch every enabled source now, in the background.
    pub(super) fn kick_source_fetch(&mut self) {
        if self.sources_busy() {
            return;
        }
        let now = now_ms();
        self.kick_know_me(false);
        self.kick_wishlist(now);
        self.kick_release_radar(now);
        if !self.interests_now().seeded {
            let user_md = crate::config::read_memory("USER.md");
            let memory_md = crate::config::read_memory("MEMORY.md");
            self.seed_interests(&user_md, &memory_md);
        }
        self.interests_observe_recent();
        let topics = self.source_topics();
        let probes = self
            .interests_now()
            .probe_queries(grokhub_core::interests::PROBE_MAX);
        let path = sources_cache_path();
        let mut cache = ps::load_cache(&path);
        let mut reqs = ps::requests_for(&self.cfg.pulse_sources, &topics, cache.runs, now);
        reqs.extend(self.steam_requests(now));
        // Graph probes + More like this asks run on this check.
        let mut related = cache.related.clone();
        for p in probes {
            if !related
                .iter()
                .any(|r| r.query.eq_ignore_ascii_case(&p.query))
            {
                related.push(p);
            }
        }
        if !related.is_empty() {
            let related_reqs = ps::related_requests(&self.cfg.pulse_sources, &related, now);
            // Only drain related when at least one search kind is on. Clearing
            // first dropped More like this asks when HN/Reddit/Web were all off.
            if !related_reqs.is_empty() {
                reqs.extend(related_reqs);
                cache.related.clear();
                if let Err(err) = ps::save_cache(&path, &cache) {
                    self.status = err;
                }
            }
        }
        // Counted as a run either way, so an empty setup does not retry every tick.
        self.sources.last_run_ms = Some(now);
        if reqs.is_empty() {
            self.sources.note = NOTHING_TO_CHECK.into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.sources.rx = Some(rx);
        self.sources.note = format!("Checking {} sources…", reqs.len());
        std::thread::spawn(move || {
            let path = sources_cache_path();
            let mut cache = ps::load_cache(&path);
            let got = ps::gather(&reqs, &mut cache, now, &mut |req, have| {
                fetch_source(req, have)
            });
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
        self.poll_source_summaries();
        self.poll_know_me();
        self.poll_wishlist();
        self.tick_achievements(now_ms());
        self.poll_release_radar();
        self.tick_local_models(now_ms());
        let Some(rx) = self.sources.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(run) => {
                self.sources.looking_more = false;
                self.post_source_run(run);
            }
            Err(mpsc::TryRecvError::Empty) => self.sources.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sources.looking_more = false;
                self.sources.note = "The source check stopped before it finished.".into();
            }
        }
    }

    fn poll_source_summaries(&mut self) {
        let Some(rx) = self.sources.summary_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((run, picks, reply)) => {
                self.sources.looking_more = false;
                self.finish_source_run(run, picks, Some(reply));
            }
            Err(mpsc::TryRecvError::Empty) => self.sources.summary_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sources.looking_more = false;
                self.sources.note = "The summary step stopped before it finished.".into();
            }
        }
    }

    /// Rank a run locally and pick at most five new items. Nothing new: no model call.
    /// Otherwise the picks go to the fast model in one call when Grok is connected and
    /// today's budget has room, else they post with the sources' own text. Returns how
    /// many posted now.
    pub(super) fn post_source_run(&mut self, run: SourceRun) -> usize {
        let cache = ps::load_cache(&sources_cache_path());
        let ranked = ps::rank_with(
            &run.items,
            self.interests_now(),
            &cache.source_bias,
            &cache.source_posts,
            run.at_ms,
        );
        let posted_urls: Vec<String> = self
            .updates
            .iter()
            .flat_map(|c| c.citations.iter().cloned())
            .collect();
        // Live posts only: a dismissed story may come back from a different outlet.
        let posted_titles: Vec<String> = self
            .updates
            .iter()
            .filter(|c| c.status != grokhub_core::UpdateStatus::Dismissed)
            .map(|c| c.title.clone())
            .collect();
        let picks = ps::pick_new(&ranked, &cache, &posted_urls, &posted_titles);
        let Some(messages) = ps::summary_messages(&picks) else {
            return self.finish_source_run(run, picks, None);
        };
        if !self.llm_ready() {
            return self.finish_source_run(run, picks, None);
        }
        let path = sources_cache_path();
        let mut cache = cache;
        let spent = cache.take_summary_call(run.at_ms);
        self.sources.budget = Some((cache.summary_day, cache.summary_calls));
        if !spent {
            return self.finish_source_run(run, picks, Some(Err(BUDGET_USED.into())));
        }
        if let Err(err) = ps::save_cache(&path, &cache) {
            self.status = err;
        }
        let key = self.bearer();
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        self.sources.summary_rx = Some(rx);
        self.sources.note = format!("Summarizing {} new {}…", picks.len(), plural(picks.len()));
        std::thread::spawn(move || {
            let reply = grok_chat(
                &key,
                &model,
                &messages,
                None,
                Some(grokhub_core::BACKGROUND_EFFORT),
            );
            let _ = tx.send((run, picks, reply));
        });
        0
    }

    /// Post the picks, with the model's summaries where it gave them, and remember
    /// them so a dismissed post never comes back. Returns how many posted.
    pub(super) fn finish_source_run(
        &mut self,
        run: SourceRun,
        mut picks: Vec<Ranked>,
        reply: Option<Result<String, String>>,
    ) -> usize {
        let mut summary_note = String::new();
        match reply {
            Some(Ok(text)) => {
                if ps::apply_summaries(&mut picks, &text) == 0 {
                    summary_note = " Grok's summaries didn't come back usable, so they show the sources' own text.".into();
                }
            }
            Some(Err(err)) if err == BUDGET_USED => {
                summary_note = format!(" {BUDGET_USED}");
            }
            Some(Err(err)) => {
                summary_note =
                    format!(" Summaries failed ({err}), so they show the sources' own text.");
            }
            None => {}
        }
        let path = sources_cache_path();
        let mut cache = ps::load_cache(&path);
        let mut added = 0;
        for item in &picks {
            if grokhub_core::post_update(&mut self.updates, ps::item_card(item, run.at_ms)) {
                added += 1;
            }
        }
        ps::mark_seen(&mut cache, &picks);
        cache.mark_source_posts(&picks);
        cache.runs = cache.runs.saturating_add(1);
        cache.last_run_ms = run.at_ms;
        if let Err(err) = ps::save_cache(&path, &cache) {
            self.status = err;
        }
        if added > 0 {
            self.persist_updates();
        }
        let mut note = format!(
            "Checked {} sources ({} from the network).",
            run.asked, run.fetched
        );
        if added == 0 {
            note.push(' ');
            note.push_str(grokhub_core::update_feed::HONEST_EMPTY);
        } else {
            note.push_str(&format!(" {added} new {}.", plural(added)));
            note.push_str(&summary_note);
        }
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

    pub(super) fn saved_now(&mut self) -> &mut Vec<SavedFind> {
        self.sources
            .saved
            .get_or_insert_with(|| ps::load_saved(&saved_finds_path()))
    }

    fn persist_saved(&mut self) {
        let list = self.saved_now().clone();
        if let Err(err) = ps::save_saved(&saved_finds_path(), &list) {
            self.status = err;
        }
    }

    /// The interest a post was for moves too when its title doesn't name it.
    /// Topic weights often already moved via `pulse_note`; edges always get the anchor.
    fn post_topic_feedback(&mut self, card: &grokhub_core::UpdateCard, signal: InterestSignal) {
        let anchor = card.pulse.because.as_deref();
        if let Some(topic) = anchor {
            if !grokhub_core::interests::names_topic(&card.title, topic) {
                self.interests_feedback_anchored(topic, Some(topic), signal);
                return;
            }
        }
        self.interests_edge_feedback(&card.title, anchor, signal);
    }

    fn nudge_post_source(&mut self, card: &grokhub_core::UpdateCard, step: i32, related: bool) {
        let path = sources_cache_path();
        let mut cache = ps::load_cache(&path);
        cache.nudge_source(card.pulse.source_name.as_deref().unwrap_or(""), step);
        if related {
            cache.queue_related(
                &ps::related_query(&card.title),
                card.pulse.because.as_deref(),
            );
        }
        if let Err(err) = ps::save_cache(&path, &cache) {
            self.status = err;
        }
    }

    /// After enough Save / More like this hits, follow the place (subreddit, repo, channel).
    fn learn_place_from_card(&mut self, card: &grokhub_core::UpdateCard) -> bool {
        let Some((kind, entry)) = ps::place_from_card(card) else {
            return false;
        };
        let path = sources_cache_path();
        let mut cache = ps::load_cache(&path);
        let ready = cache.note_place(kind, &entry);
        if let Err(err) = ps::save_cache(&path, &cache) {
            self.status = err;
            return false;
        }
        if !ready || !self.cfg.pulse_sources.on(kind) {
            return false;
        }
        match self.cfg.pulse_sources.add(kind, &entry) {
            Ok(added) => {
                self.persist_cfg();
                self.status = format!(
                    "Following {} — learned from what you liked.",
                    match kind {
                        ps::SourceKind::Reddit => format!("r/{added}"),
                        ps::SourceKind::GithubReleases => format!("{added} on GitHub"),
                        ps::SourceKind::YouTube => format!("YouTube {added}"),
                        ps::SourceKind::Rss => added,
                        ps::SourceKind::HackerNews | ps::SourceKind::WebSearch => added,
                    }
                );
                true
            }
            Err(err) if err.contains("already on the list") => false,
            Err(err) => {
                self.status = err;
                false
            }
        }
    }

    /// Save keeps the post in Saved Finds and raises its interest. Not interested
    /// quietly removes it and lowers its interest and source. More like this raises
    /// both and starts a source check now for the post's own words (HN / Reddit /
    /// Web search when those kinds are on). Save and More like this also count
    /// toward following a new place after two hits.
    pub(super) fn source_post_act(&mut self, id: &str, did: SourceAct) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        let day = Self::local_day();
        match did {
            SourceAct::Save => {
                let Some(find) = ps::saved_from_card(&card, now_ms()) else {
                    return;
                };
                if ps::save_find(self.saved_now(), find) {
                    self.persist_saved();
                }
                self.status = "Saved to Saved Finds in Settings → Pulse.".into();
                if card.reaction == Some(grokhub_core::CardReaction::Up) {
                    let _ = self.learn_place_from_card(&card);
                    return;
                }
                if let Some(c) = self.updates.iter_mut().find(|c| c.id == id) {
                    c.reaction = Some(grokhub_core::CardReaction::Up);
                }
                self.persist_updates();
                self.pulse_note(&card.title, LedgerReason::Liked, &day);
                self.post_topic_feedback(&card, InterestSignal::Saved);
                let _ = self.learn_place_from_card(&card);
            }
            SourceAct::NotInterested => {
                self.pulse_note(&card.title, LedgerReason::NotThis, &day);
                self.post_topic_feedback(&card, InterestSignal::NotInterested);
                self.nudge_post_source(&card, -1, false);
                self.pulse_remove(&card);
                self.heartbeat_card_dismissed();
            }
            SourceAct::MoreLike => {
                let text = format!(
                    "{} {}",
                    card.title,
                    card.pulse.because.as_deref().unwrap_or("")
                );
                self.interests_feedback_anchored(
                    &text,
                    card.pulse.because.as_deref(),
                    InterestSignal::MoreLikeThis,
                );
                self.nudge_post_source(&card, 1, true);
                let followed = self.learn_place_from_card(&card);
                let was_busy = self.sources_busy();
                let search_on = self.cfg.pulse_sources.hacker_news
                    || self.cfg.pulse_sources.reddit
                    || self.cfg.pulse_sources.web_search;
                self.kick_source_fetch();
                if self.sources.rx.is_some() && !was_busy {
                    self.sources.looking_more = true;
                    if !followed {
                        self.status = "Looking for more like this…".into();
                    }
                } else if was_busy {
                    self.sources.looking_more = true;
                    if !followed {
                        self.status = "More like this queued — check already running.".into();
                    }
                } else if !followed && !search_on {
                    self.status =
                        "Turn on Hacker News, Reddit, or Web search to look for more.".into();
                } else if !followed {
                    self.status = "More like this queued for the next check.".into();
                }
            }
        }
    }

    /// Settings → Pulse → Saved Finds: newest first, each opens its link.
    fn ui_saved_finds(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Saved Finds")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.add_space(4.0);
        let list = self.saved_now().clone();
        if list.is_empty() {
            crate::cards::settings_note(
                ui,
                "Nothing saved yet. Save on a Feed post keeps it here.",
            );
            return;
        }
        let mut open: Option<String> = None;
        let mut remove: Option<String> = None;
        for find in list.iter().take(SAVED_SHOWN) {
            ui.horizontal(|ui| {
                let link = ui.add(
                    egui::Label::new(
                        RichText::new(&find.title)
                            .size(crate::theme::FONT_BODY)
                            .color(crate::theme::link()),
                    )
                    .sense(egui::Sense::click())
                    .truncate(),
                );
                if link.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if link.clicked() {
                    open = Some(find.url.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Remove") {
                        remove = Some(find.url.clone());
                    }
                    ui.label(
                        RichText::new(&find.source_name)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                });
            });
        }
        if list.len() > SAVED_SHOWN {
            crate::cards::settings_note(
                ui,
                &format!(
                    "And {} more in {}.",
                    list.len() - SAVED_SHOWN,
                    ps::SAVED_FILE
                ),
            );
        }
        if let Some(url) = open {
            self.follow_update_action(Some(grokhub_core::UpdateAction::DeepLink { href: url }));
        }
        if let Some(url) = remove {
            self.saved_now().retain(|f| f.url != url);
            self.persist_saved();
        }
    }

    pub(super) fn ui_pulse_sources(&mut self, ui: &mut egui::Ui) {
        self.poll_source_fetch();
        if self.sources_busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
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
            "Free sources only, no keys. Hacker News, Reddit and Web search (DuckDuckGo Lite) search your top three interests; the lists below are followed as they are. Each post names its source and links to it.",
        );
        ui.add_space(8.0);
        let mut changed = false;
        for kind in SourceKind::ALL {
            let mut on = self.cfg.pulse_sources.on(kind);
            if ui
                .checkbox(
                    &mut on,
                    RichText::new(kind.label()).size(crate::theme::FONT_BODY),
                )
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
        self.ui_steam_source(ui);
        self.ui_wishlist_source(ui);
        self.ui_achievements_source(ui);
        self.ui_release_radar_source(ui);
        self.ui_local_models_source(ui);
        // The paid search extension point, shown off and not clickable.
        let mut paid = ps::PAID_SEARCH_ENABLED;
        ui.add_enabled(
            ps::PAID_SEARCH_ENABLED,
            egui::Checkbox::new(
                &mut paid,
                RichText::new(ps::PAID_SEARCH_LABEL).size(crate::theme::FONT_BODY),
            ),
        );
        ui.indent("pulse-paid-search", |ui| {
            ui.label(
                RichText::new(ps::PAID_SEARCH_NOTE)
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        });
        ui.add_space(6.0);
        ui.add_space(4.0);
        crate::cards::help_text(
            ui,
            &format!("Checks run every two hours, once when {} opens, and never in quiet hours. Sources are fetched and ranked on this computer; only each check's top five new posts go to Grok's fast model, in one call, for short summaries.", grokhub_core::brand::product_name()),
        );
        let today = self.summary_calls_today(now_ms());
        ui.label(
            RichText::new(format!(
                "Model calls today: {today} of {}",
                ps::SUMMARY_CALLS_PER_DAY
            ))
            .size(crate::theme::FONT_TIP)
            .color(crate::theme::muted()),
        );
        if changed {
            self.persist_cfg();
        }
        ui.add_space(4.0);
        if !self.sources_busy() && crate::cards::white_pill(ui, "Check now") {
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
        ui.add_space(16.0);
        self.ui_saved_finds(ui);
    }

    /// The followed list under a source, with Remove and an Add box. True when it changed.
    fn ui_source_list(&mut self, ui: &mut egui::Ui, kind: SourceKind) -> bool {
        let hint = match kind {
            SourceKind::HackerNews | SourceKind::WebSearch => return false,
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
                    ui.label(
                        RichText::new(entry)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::fg()),
                    );
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

    fn cabin() -> (
        std::sync::MutexGuard<'static, ()>,
        crate::config::TestConfigDir,
        Cabin,
    ) {
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
            engagement: 0,
        }
    }

    #[test]
    fn a_run_posts_named_linked_feed_cards_once() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("Rust");
        let now = now_ms();
        let run = |items: Vec<SourceItem>| SourceRun {
            items,
            errors: vec!["r/rust: HTTP 429".into()],
            fetched: 3,
            asked: 4,
            at_ms: now,
        };
        let added = cabin.post_source_run(run(vec![
            item(
                "Rust 1.99 released",
                "https://blog.rust-lang.org/1.99",
                now - 60_000,
            ),
            item(
                "Rust 1.99 released (dup)",
                "https://www.blog.rust-lang.org/1.99/?utm_source=hn",
                now - 120_000,
            ),
        ]));
        assert_eq!(added, 1);
        assert_eq!(
            cabin.sources.note,
            "Checked 4 sources (3 from the network). 1 new post. Failed: r/rust: HTTP 429."
        );
        let card = cabin
            .updates
            .iter()
            .find(|c| c.title == "Rust 1.99 released")
            .cloned()
            .expect("posted");
        assert_eq!(
            card.citations,
            vec!["https://blog.rust-lang.org/1.99".to_string()]
        );
        assert_eq!(card.pulse.source_name.as_deref(), Some("Hacker News"));
        assert_eq!(card.pulse.because.as_deref(), Some("Rust"));
        assert_eq!(super::super::pulse_ui::why_line("Rust"), "Why: Rust");
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
        assert_eq!(
            cabin.post_source_run(run(vec![item(
                "Rust 1.99 released",
                "https://blog.rust-lang.org/1.99",
                now - 60_000
            )])),
            0
        );
        assert!(ps::load_cache(&sources_cache_path())
            .seen
            .contains(&"blog.rust-lang.org/1.99".to_string()));
    }

    #[test]
    fn muted_topics_never_post_and_an_empty_setup_says_so() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("crypto");
        cabin.interests_now().set_muted("crypto", true);
        let now = now_ms();
        let added = cabin.post_source_run(SourceRun {
            items: vec![item(
                "Crypto exchange collapses",
                "https://news.test/c",
                now,
            )],
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
        assert_eq!(
            cabin.sources.note,
            "Nothing to check yet. Add an interest or a source."
        );
    }

    fn one_item_run(items: Vec<SourceItem>, now: u64) -> SourceRun {
        SourceRun {
            items,
            errors: Vec::new(),
            fetched: 1,
            asked: 1,
            at_ms: now,
        }
    }

    #[test]
    fn nothing_new_calls_no_model_and_says_so() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.secrets.api_key = "sk-abcdefghijklmnopqrstuv".into();
        assert!(cabin.llm_ready());
        cabin.add_interest("crypto");
        cabin.interests_now().set_muted("crypto", true);
        let now = now_ms();
        let added = cabin.post_source_run(one_item_run(
            vec![item(
                "Crypto exchange collapses",
                "https://news.test/c",
                now,
            )],
            now,
        ));
        assert_eq!(added, 0);
        assert!(cabin.sources.summary_rx.is_none());
        assert_eq!(cabin.summary_calls_today(now), 0);
        assert_eq!(
            cabin.sources.note,
            "Checked 1 sources (1 from the network). I looked and did not find a source worth your time."
        );
    }

    #[test]
    fn a_spent_budget_posts_the_sources_own_text_without_a_model_call() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.secrets.api_key = "sk-abcdefghijklmnopqrstuv".into();
        cabin.add_interest("Rust");
        let now = now_ms();
        let mut cache = ps::SourceCache::default();
        for _ in 0..ps::SUMMARY_CALLS_PER_DAY {
            assert!(cache.take_summary_call(now));
        }
        ps::save_cache(&sources_cache_path(), &cache).unwrap();
        let added = cabin.post_source_run(one_item_run(
            vec![item(
                "Rust 1.99 released",
                "https://blog.rust-lang.org/1.99",
                now,
            )],
            now,
        ));
        assert_eq!(added, 1);
        assert!(cabin.sources.summary_rx.is_none());
        assert_eq!(cabin.summary_calls_today(now), 16);
        assert_eq!(
            cabin.sources.note,
            "Checked 1 sources (1 from the network). 1 new post. Today's summary budget is used, so they show the sources' own text."
        );
        let card = cabin
            .updates
            .iter()
            .find(|c| c.title == "Rust 1.99 released")
            .expect("posted");
        assert_eq!(
            card.body.as_deref(),
            Some("812 points, 301 comments on Hacker News.")
        );
    }

    #[test]
    fn model_summaries_replace_the_source_text_on_the_picks_they_name() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("Rust");
        let now = now_ms();
        let run = one_item_run(Vec::new(), now);
        let picks = ps::rank(
            &[
                item("Rust 1.99 released", "https://blog.rust-lang.org/1.99", now),
                item("Rust in the kernel", "https://lwn.test/rust", now),
            ],
            cabin.interests_now(),
            now,
        );
        let reply =
            r#"Here you go: [{"n":1,"summary":"Rust 1.99 is out. It adds async closures."}]"#;
        assert_eq!(
            cabin.finish_source_run(run, picks, Some(Ok(reply.into()))),
            2
        );
        let body = |title: &str| {
            cabin
                .updates
                .iter()
                .find(|c| c.title == title)
                .and_then(|c| c.body.clone())
                .expect("posted")
        };
        assert_eq!(
            body("Rust 1.99 released"),
            "Rust 1.99 is out. It adds async closures."
        );
        assert_eq!(
            body("Rust in the kernel"),
            "812 points, 301 comments on Hacker News."
        );
        assert_eq!(
            cabin.sources.note,
            "Checked 1 sources (1 from the network). 2 new posts."
        );
    }

    #[test]
    fn the_background_check_runs_on_open_then_on_its_clock() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.cfg.feed_pulse.digest_on = true;
        cabin.cfg.quiet_start.clear();
        cabin.cfg.quiet_end.clear();
        cabin.interests.profile = Some(Default::default());
        cabin.cfg.pulse_sources = ps::SourceSettings::default();
        // Last check 20 minutes ago: the first tick after launch runs it anyway.
        let now = now_ms();
        cabin.sources.last_run_ms = Some(now - 20 * 60_000);
        cabin.follow_source_fetch();
        assert!(cabin.sources.last_run_ms.unwrap() >= now, "ran on open");
        assert_eq!(
            cabin.sources.note,
            "Nothing to check yet. Add an interest or a source."
        );
        // Not again until two hours pass.
        cabin.sources.note.clear();
        cabin.sources.last_run_ms = Some(now - 20 * 60_000);
        cabin.follow_source_fetch();
        assert_eq!(cabin.sources.note, "");
        cabin.sources.last_run_ms = Some(now - 91 * 60_000);
        cabin.follow_source_fetch();
        assert_eq!(cabin.sources.note, "");
        cabin.sources.last_run_ms = Some(now - 121 * 60_000);
        cabin.follow_source_fetch();
        assert_eq!(
            cabin.sources.note,
            "Nothing to check yet. Add an interest or a source."
        );
        // Never in quiet hours, however overdue.
        cabin.sources.note.clear();
        cabin.sources.last_run_ms = Some(now - 5 * 60 * 60_000);
        cabin.cfg.quiet_start = "00:00".into();
        cabin.cfg.quiet_end = "23:59".into();
        if cabin.quiet_now() {
            cabin.follow_source_fetch();
            assert_eq!(cabin.sources.note, "");
        }
    }

    #[test]
    fn save_not_interested_and_more_like_this_move_the_profile_sources_and_saved_finds() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("Rust");
        let now = now_ms();
        let mut kernel = item("Rust in the kernel", "https://lwn.test/rust", now);
        kernel.source_name = "r/rust".into();
        kernel.kind = SourceKind::Reddit;
        let mut book = item("Rust async book", "https://book.test/async", now);
        book.source_name = "blog.test".into();
        book.kind = SourceKind::Rss;
        let added = cabin.post_source_run(one_item_run(
            vec![
                item("Rust 1.99 released", "https://blog.rust-lang.org/1.99", now),
                kernel,
                book,
            ],
            now,
        ));
        assert_eq!(added, 3);
        let id = |cabin: &Cabin, title: &str| {
            cabin
                .updates
                .iter()
                .find(|c| c.title == title)
                .map(|c| c.id.clone())
                .expect("posted")
        };
        let weight = |cabin: &mut Cabin| {
            cabin
                .interests_now()
                .find("Rust")
                .map(|t| t.weight)
                .expect("topic")
        };
        assert_eq!(weight(&mut cabin), 70);

        let saved = id(&cabin, "Rust 1.99 released");
        cabin.source_post_act(&saved, SourceAct::Save);
        cabin.source_post_act(&saved, SourceAct::Save);
        assert_eq!(cabin.status, "Saved to Saved Finds in Settings → Pulse.");
        let on_disk = ps::load_saved(&saved_finds_path());
        let rows: Vec<(&str, &str, &str, Option<&str>)> = on_disk
            .iter()
            .map(|f| {
                (
                    f.title.as_str(),
                    f.url.as_str(),
                    f.source_name.as_str(),
                    f.because.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![(
                "Rust 1.99 released",
                "https://blog.rust-lang.org/1.99",
                "Hacker News",
                Some("Rust")
            )]
        );
        let card = cabin
            .updates
            .iter()
            .find(|c| c.id == saved)
            .expect("still up");
        assert_eq!(card.reaction, Some(grokhub_core::CardReaction::Up));
        assert_eq!(weight(&mut cabin), 80);

        cabin.status.clear();
        let gone = id(&cabin, "Rust in the kernel");
        cabin.source_post_act(&gone, SourceAct::NotInterested);
        assert!(
            !cabin
                .updates
                .iter()
                .any(|c| c.id == gone && c.status != grokhub_core::UpdateStatus::Dismissed),
            "Not interested takes the post down"
        );
        assert_eq!(cabin.status, "", "Not interested is quiet");
        assert_eq!(weight(&mut cabin), 65);
        let cache = ps::load_cache(&sources_cache_path());
        assert_eq!(cache.source_bias.get("r/rust"), Some(&-1));

        let more = id(&cabin, "Rust async book");
        cabin.source_post_act(&more, SourceAct::MoreLike);
        assert_eq!(cabin.status, "Looking for more like this…");
        assert!(cabin.sources.looking_more, "Feed button flips to Looking…");
        assert!(
            cabin.sources.rx.is_some(),
            "More like this starts a source check now"
        );
        assert!(
            cabin.sources.note.starts_with("Checking "),
            "Feed banner shows the check: {}",
            cabin.sources.note
        );
        assert_eq!(weight(&mut cabin), 77);
        let cache = ps::load_cache(&sources_cache_path());
        assert_eq!(cache.source_bias.get("blog.test"), Some(&1));
        assert!(
            cache.related.is_empty(),
            "queued related search is drained into this check"
        );
        // Drop the in-flight fetch so the cabin teardown stays quiet.
        cabin.sources.rx = None;
        cabin.sources.looking_more = false;
    }

    #[test]
    fn two_saves_from_a_subreddit_follow_it() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("Rust");
        cabin.cfg.pulse_sources.subreddits.clear();
        let now = now_ms();
        let mut a = item(
            "Async in the kernel",
            "https://www.reddit.com/r/rust/comments/a/x",
            now,
        );
        a.kind = SourceKind::Reddit;
        a.source_name = "r/rust".into();
        let mut b = item(
            "Egui 0.40",
            "https://www.reddit.com/r/rust/comments/b/y",
            now - 1,
        );
        b.kind = SourceKind::Reddit;
        b.source_name = "r/rust".into();
        assert_eq!(cabin.post_source_run(one_item_run(vec![a, b], now)), 2);
        let first = cabin
            .updates
            .iter()
            .find(|c| c.title == "Async in the kernel")
            .map(|c| c.id.clone())
            .expect("first");
        let second = cabin
            .updates
            .iter()
            .find(|c| c.title == "Egui 0.40")
            .map(|c| c.id.clone())
            .expect("second");
        cabin.source_post_act(&first, SourceAct::Save);
        assert_eq!(cabin.cfg.pulse_sources.subreddits, Vec::<String>::new());
        assert_eq!(cabin.status, "Saved to Saved Finds in Settings → Pulse.");
        cabin.source_post_act(&second, SourceAct::Save);
        assert_eq!(cabin.cfg.pulse_sources.subreddits, vec!["rust".to_string()]);
        assert_eq!(
            cabin.status,
            "Following r/rust — learned from what you liked."
        );
    }

    #[test]
    fn relative_redirects_stay_on_the_same_host() {
        assert_eq!(
            redirect_target("https://a.test/feed", "/feed.xml"),
            "https://a.test/feed.xml"
        );
        assert_eq!(
            redirect_target("http://a.test/x/y", "rss"),
            "http://a.test/rss"
        );
        assert_eq!(
            redirect_target("http://a.test/x", "https://b.test/rss"),
            "https://b.test/rss"
        );
    }

    #[test]
    fn source_agent_dials_through_the_public_only_resolver() {
        let src = include_str!("pulse_sources_ui.rs");
        let arm = src
            .split("fn source_agent()")
            .nth(1)
            .and_then(|rest| rest.split("fn redirect_target(").next())
            .expect("source_agent");
        assert!(
            arm.contains(".resolver(grokhub_agent::public_only_resolve)"),
            "Pulse source fetches must refuse DNS→private: {arm}"
        );
        assert!(
            arm.contains(".redirects(0)"),
            "manual redirect checks need redirects(0): {arm}"
        );
        assert!(
            !arm.contains("try_proxy_from_env"),
            "env proxy would skip public_only_resolve on the target: {arm}"
        );
    }

    #[test]
    fn more_like_this_keeps_related_when_search_kinds_are_off() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.updates.clear();
        cabin.add_interest("Rust");
        cabin.cfg.pulse_sources.hacker_news = false;
        cabin.cfg.pulse_sources.reddit = false;
        cabin.cfg.pulse_sources.web_search = false;
        let now = now_ms();
        let added = cabin.post_source_run(one_item_run(
            vec![item("Rust async book", "https://book.test/async", now)],
            now,
        ));
        assert_eq!(added, 1);
        let more = cabin
            .updates
            .iter()
            .find(|c| c.title == "Rust async book")
            .map(|c| c.id.clone())
            .expect("posted");
        cabin.source_post_act(&more, SourceAct::MoreLike);
        assert_eq!(
            cabin.status,
            "Turn on Hacker News, Reddit, or Web search to look for more."
        );
        assert!(
            !cabin.sources.looking_more,
            "no Looking… when nothing can run"
        );
        assert!(cabin.sources.rx.is_none(), "no in-flight check");
        let cache = ps::load_cache(&sources_cache_path());
        assert_eq!(
            cache.related.len(),
            1,
            "related ask must stay queued until a search kind is on: {:?}",
            cache.related
        );
        assert!(
            !cache.related[0].query.trim().is_empty(),
            "queued query must be non-empty: {:?}",
            cache.related[0]
        );
    }

    #[test]
    fn more_like_this_kicks_a_check_and_shows_looking() {
        let act = include_str!("pulse_sources_ui.rs")
            .split("SourceAct::MoreLike => {")
            .nth(1)
            .and_then(|rest| rest.split("\n            }\n        }\n    }").next())
            .expect("MoreLike arm");
        assert!(
            act.contains("self.kick_source_fetch()"),
            "More like this must start a check now: {act}"
        );
        assert!(
            act.contains("Looking for more like this…"),
            "status must name the look: {act}"
        );
        assert!(
            act.contains("looking_more = true"),
            "Feed button needs looking_more: {act}"
        );
        assert!(
            act.contains("Turn on Hacker News, Reddit, or Web search"),
            "off-search path must say what to turn on: {act}"
        );
        let kick = include_str!("pulse_sources_ui.rs")
            .split("fn kick_source_fetch(")
            .nth(1)
            .and_then(|rest| rest.split("\n    pub(super) fn ").next())
            .expect("kick_source_fetch");
        assert!(
            kick.contains("if !related_reqs.is_empty()") && kick.contains("cache.related.clear()"),
            "related queue clears only after search reqs exist: {kick}"
        );
        let ui = include_str!("pulse_ui.rs");
        assert!(ui.contains("\"Looking…\""), "button label while looking");
        let feed = ui
            .split("fn ui_pulse_feed(")
            .nth(1)
            .and_then(|rest| rest.split("/// One post:").next())
            .expect("ui_pulse_feed");
        assert!(
            feed.contains("self.sources.note"),
            "Feed banner shows the check note: {feed}"
        );
    }

    #[test]
    fn ago_labels() {
        assert_eq!(ago_label(1_000, 30_000), "just now");
        assert_eq!(ago_label(0, 12 * 60_000), "12 min ago");
        assert_eq!(ago_label(0, 185 * 60_000), "3 h ago");
    }
}
