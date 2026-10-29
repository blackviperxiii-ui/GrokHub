//! New local model suggestions (`grokhub_core::local_model_news`), opt-in
//! under Settings → Pulse → Sources. Once a week, off the UI thread: read
//! this PC's hardware and the installed model, score the catalog, and read
//! the official publishers' newest GGUF repos on Hugging Face. A catalog
//! upgrade card opens the model picker on that model, where Download is the
//! approval; a release card only links Hugging Face.

use grokhub_core::local_model_news::{self as lm, Suggestion, Tracker};
use grokhub_core::local_picker::{BenchCache, CpuSimd, Profile};
use grokhub_core::pulse_sources::{Fetched, SourceKind, SourceReq};

use super::setup_wizard::models_dir;
use super::*;

/// Deck shots (`scripts/deck-shots.ps1`) set this to run the check on a
/// fixed 24 GB GPU, the 12B installed and a canned Hugging Face reply.
const DECK_ENV: &str = "GROKHUB_DECK_LOCAL_MODELS";

#[derive(Default)]
pub(super) struct ModelNewsState {
    tracker: Option<Tracker>,
    rx: Option<mpsc::Receiver<NewsRun>>,
    pub note: String,
}

struct NewsRun {
    upgrade: Option<Suggestion>,
    releases: Vec<Suggestion>,
    read: Vec<String>,
    /// No local model is installed: nothing to beat yet.
    no_model: bool,
}

/// What the check reads off this PC: the hardware and the installed model.
struct Machine {
    profile: Profile,
    installed: Option<&'static grokhub_core::local_setup::LocalModel>,
}

/// The whole check for one machine, given a GET for the releases.
fn run_check(machine: &Machine, get: &mut dyn FnMut(&str) -> Result<String, String>) -> NewsRun {
    let Some(current) = machine.installed else {
        return NewsRun {
            upgrade: None,
            releases: Vec::new(),
            read: Vec::new(),
            no_model: true,
        };
    };
    let upgrade = lm::upgrade(&machine.profile, current.id, &BenchCache::new());
    let mut read = Vec::new();
    for publisher in lm::PUBLISHERS {
        if let Ok(body) = get(&lm::releases_url(publisher)) {
            read.extend(lm::parse_releases(&body));
        }
    }
    let releases = lm::releases(&machine.profile, current.run_mb, &read);
    NewsRun {
        upgrade,
        releases,
        read,
        no_model: false,
    }
}

/// One keyless GET through the source fetcher (public-only, egress-guarded).
fn get(url: &str) -> Result<String, String> {
    let req = SourceReq {
        kind: SourceKind::Rss,
        url: url.to_string(),
        name: lm::SOURCE_NAME.to_string(),
        topic: None,
    };
    match super::pulse_sources_ui::fetch_source(&req, None) {
        Fetched::Body { body, .. } => Ok(body),
        Fetched::NotModified => Err("no reply".into()),
        Fetched::Failed(why) => Err(why),
    }
}

fn deck_check() -> bool {
    super::loadout_home::deck_switch_is(DECK_ENV, "1")
}

fn deck_machine() -> Machine {
    Machine {
        profile: Profile {
            gpus_vram_mb: vec![24_576],
            unified: false,
            ram_mb: 32_768,
            free_disk_mb: 200_000,
            simd: CpuSimd::Avx2,
            llama_build: Some(99_999),
        },
        installed: grokhub_core::local_setup::model_by_id("gemma-4-12b-it-q4_k_m"),
    }
}

fn deck_reply(url: &str) -> Result<String, String> {
    if url.contains("author=ggml-org") {
        Ok(
            r#"[{"id":"ggml-org/Qwen3-14B-GGUF"},{"id":"ggml-org/embeddinggemma-300M-GGUF"}]"#
                .into(),
        )
    } else {
        Ok("[]".into())
    }
}

fn this_machine() -> Machine {
    let dir = models_dir();
    let hw = super::setup_wizard::detect_hardware(&dir);
    Machine {
        profile: Profile::from_hardware(&hw, CpuSimd::detect()),
        installed: grokhub_core::model_download::installed(&dir)
            .and_then(|rec| grokhub_core::local_setup::model_by_id(&rec.id)),
    }
}

impl Cabin {
    fn news_now(&mut self) -> &mut Tracker {
        self.sources
            .models
            .tracker
            .get_or_insert_with(|| lm::load(&crate::config::config_dir()))
    }

    fn save_news(&mut self) {
        let tracker = self.news_now().clone();
        if let Err(err) = lm::save(&crate::config::config_dir(), &tracker) {
            self.status = err;
        }
    }

    /// Every frame with the source poll: finish a check, or start the weekly one.
    pub(super) fn tick_local_models(&mut self, now: u64) {
        if let Some(rx) = self.sources.models.rx.take() {
            match rx.try_recv() {
                Ok(run) => self.finish_local_models(run, now),
                Err(mpsc::TryRecvError::Empty) => self.sources.models.rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.sources.models.note =
                        "The local model check stopped before it finished.".into();
                }
            }
            return;
        }
        if !self.news_now().due(now) {
            return;
        }
        // Counted as a check either way, so a failure waits a week like a success.
        self.news_now().last_check_ms = now;
        self.save_news();
        let (tx, rx) = mpsc::channel();
        self.sources.models.rx = Some(rx);
        self.sources.models.note = "Checking for new local models…".into();
        std::thread::spawn(move || {
            let run = if deck_check() {
                run_check(&deck_machine(), &mut deck_reply)
            } else {
                run_check(&this_machine(), &mut get)
            };
            let _ = tx.send(run);
        });
    }

    fn finish_local_models(&mut self, run: NewsRun, now: u64) {
        // Switched off while it ran: the user's choice wins.
        if !self.news_now().on {
            return;
        }
        if run.no_model {
            self.sources.models.note =
                "No local model yet. Set one up under Cabin defaults first.".into();
            self.save_news();
            return;
        }
        let bias =
            grokhub_core::pulse_sources::load_cache(&super::pulse_sources_ui::sources_cache_path())
                .source_bias
                .get(lm::SOURCE_NAME)
                .copied()
                .unwrap_or(0);
        let picks = self.news_now().check(
            run.upgrade,
            run.releases,
            &run.read,
            lm::links_allowed(bias),
            now,
        );
        let mut added = 0;
        for pick in &picks {
            if grokhub_core::post_update(&mut self.updates, pick.card(now)) {
                added += 1;
            }
        }
        if added > 0 {
            self.persist_updates();
        }
        self.sources.models.note = if added == 0 {
            "Checked. Nothing new that fits this PC better.".into()
        } else {
            format!("Checked. {added} new for this PC.")
        };
        self.save_news();
    }

    /// A catalog upgrade card's link: Settings → Cabin defaults with the
    /// picker on that model. False for any other link.
    pub(super) fn open_local_model_link(&mut self, href: &str) -> bool {
        let Some(id) = lm::picker_link_id(href) else {
            return false;
        };
        self.setup.choice = Some(id.to_string());
        self.set_nav_id("settings");
        self.settings_sec = SettingsSec::Defaults;
        true
    }

    fn set_news_on(&mut self, on: bool) {
        if on {
            self.news_now().on = true;
        } else {
            self.news_now().turn_off();
            self.sources.models.note.clear();
        }
        self.save_news();
    }

    /// Settings → Pulse → Sources: the new local models toggle.
    pub(super) fn ui_local_models_source(&mut self, ui: &mut egui::Ui) {
        let mut on = self.news_now().on;
        if ui
            .checkbox(
                &mut on,
                RichText::new("Suggest new local models that fit this PC")
                    .size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.set_news_on(on);
        }
        if !on {
            ui.add_space(6.0);
            return;
        }
        ui.indent("pulse-local-models", |ui| {
            crate::cards::help_text(
                ui,
                &format!("Once a week, from this PC's memory and the official model publishers on Hugging Face. Only models in {}'s checked catalog download, from Settings.", grokhub_core::brand::product_name()),
            );
            if crate::cards::ghost_pill(ui, "Check now") {
                self.news_now().last_check_ms = 0;
                self.tick_local_models(now_ms());
            }
            if !self.sources.models.note.is_empty() {
                crate::cards::help_text(ui, &self.sources.models.note.clone());
            }
        });
        ui.add_space(6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_offers_the_bigger_catalog_model_and_reads_both_publishers() {
        let mut asked = Vec::new();
        let run = run_check(&deck_machine(), &mut |url| {
            asked.push(url.to_string());
            deck_reply(url)
        });
        assert!(!run.no_model);
        assert_eq!(
            run.upgrade.as_ref().map(Suggestion::title).as_deref(),
            Some("Gemma 4 31B fits this PC")
        );
        // The 14B (9.8 GB) runs bigger than the installed 12B (8.2 GB).
        let names: Vec<String> = run.releases.iter().map(Suggestion::title).collect();
        assert_eq!(names, vec!["New on Hugging Face: Qwen3-14B-GGUF"]);
        assert_eq!(asked.len(), 2);
        assert!(asked[0].contains("author=ggml-org"));
        assert!(asked[1].contains("author=lmstudio-community"));
    }

    #[test]
    fn no_installed_model_reads_nothing_and_says_so() {
        let _g = crate::config::hold_test_config();
        let mut machine = deck_machine();
        machine.installed = None;
        let run = run_check(&machine, &mut |url| panic!("no request expected: {url}"));
        assert!(run.no_model);
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.models.tracker = Some(Tracker {
            on: true,
            ..Default::default()
        });
        let before = cabin.updates.len();
        cabin.finish_local_models(run, 5);
        assert_eq!(cabin.updates.len(), before);
        assert_eq!(
            cabin.sources.models.note,
            "No local model yet. Set one up under Cabin defaults first."
        );
    }

    #[test]
    fn the_upgrade_posts_once_and_releases_wait_for_the_second_check() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.models.tracker = Some(Tracker {
            on: true,
            ..Default::default()
        });
        let before = cabin.updates.len();
        cabin.finish_local_models(run_check(&deck_machine(), &mut deck_reply), 5);
        assert_eq!(
            cabin.updates.len(),
            before + 1,
            "first check: the upgrade only"
        );
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "Gemma 4 31B fits this PC"));
        assert_eq!(cabin.sources.models.note, "Checked. 1 new for this PC.");
        let mut later = run_check(&deck_machine(), &mut deck_reply);
        later.read = vec!["ggml-org/Qwen3-14B-GGUF".into()];
        // Seen on the first check, so it never posts.
        cabin.finish_local_models(later, 6);
        assert_eq!(cabin.updates.len(), before + 1);
        assert_eq!(
            cabin.sources.models.note,
            "Checked. Nothing new that fits this PC better."
        );
        let saved = lm::load(&crate::config::config_dir());
        assert!(saved.seeded);
        assert!(saved.suggested.contains("catalog:gemma-4-31b-it-q4_k_m"));
    }

    #[test]
    fn the_upgrade_link_opens_the_picker_on_that_model() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        assert!(cabin.open_local_model_link("grokhub://local-model/gemma-4-31b-it-q4_k_m"));
        assert_eq!(cabin.nav, Nav::Settings);
        assert_eq!(cabin.settings_sec, SettingsSec::Defaults);
        assert_eq!(cabin.setup.choice.as_deref(), Some("gemma-4-31b-it-q4_k_m"));
        assert!(!cabin.open_local_model_link("https://huggingface.co/ggml-org/Qwen3-14B-GGUF"));
        assert!(!cabin.open_local_model_link("grokhub://local-model/not-a-model"));
    }
}
