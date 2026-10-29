//! New local models for Pulse. Opt-in. Once a week the cabin scores the
//! curated catalog (`local_picker`) against this PC and, when a bigger model
//! than the one it runs now is Recommended here, posts a card that opens the
//! model picker on it, where Download (size shown) is the approval. It also
//! reads the newest GGUF repos of the official publishers on Hugging Face
//! (public API, no key) and posts the ones that would fit with headroom as a
//! link: they are not in the hash-pinned catalog, so they never download from
//! here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::local_picker::{score, BenchCache, Bucket, Memory, Profile};
use crate::local_setup::{local_models, size_label, LocalModel};

pub const FILE_NAME: &str = "pulse-local-models.json";
pub const SOURCE_NAME: &str = "Local models";
/// One check a week.
pub const CHECK_EVERY_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Publishers whose GGUF repos are read: straight quants of official weights.
pub const PUBLISHERS: [&str; 2] = ["ggml-org", "lmstudio-community"];
/// Repos read per publisher, newest first.
pub const REPOS_PER_PUBLISHER: usize = 20;
/// Release links one check posts before the feedback score moves it.
pub const LINKS_PER_CHECK: i32 = 2;
/// Most release links one check can post.
pub const LINKS_MAX: i32 = 4;
/// A release must fit in this share of the memory budget.
pub const HEADROOM_PERCENT: u64 = 85;
/// MiB per billion parameters at Q4_K_M (about 4.85 bits a weight).
const Q4_MB_PER_B: f64 = 606.0;
/// KV cache and runtime overhead on top of the weights.
const RUN_OVERHEAD_MB: u64 = 1_536;
/// Repos that are not a chat model, or not the official weights.
const SKIP_WORDS: [&str; 10] = [
    "abliterated",
    "uncensored",
    "embed",
    "rerank",
    "lora",
    "whisper",
    "tts",
    "draft",
    "mmproj",
    "merge",
];

/// The weekly check's memory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracker {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub last_check_ms: u64,
    /// The first check only records what is already out, so turning this on
    /// doesn't post a backlog.
    #[serde(default)]
    pub seeded: bool,
    /// Hugging Face repos already seen.
    #[serde(default)]
    pub seen: BTreeSet<String>,
    /// Catalog models already suggested.
    #[serde(default)]
    pub suggested: BTreeSet<String>,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> Tracker {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, tracker: &Tracker) -> Result<(), String> {
    let text = serde_json::to_string_pretty(tracker).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// The publisher's newest GGUF repos.
pub fn releases_url(publisher: &str) -> String {
    format!(
        "https://huggingface.co/api/models?author={publisher}&search=GGUF&sort=createdAt&direction=-1&limit={REPOS_PER_PUBLISHER}"
    )
}

pub fn repo_url(repo: &str) -> String {
    format!("https://huggingface.co/{repo}")
}

/// The card's action: opens Settings → Cabin defaults on that catalog model.
pub fn picker_link(id: &str) -> String {
    format!("grokhub://local-model/{id}")
}

/// The catalog id in a [`picker_link`].
pub fn picker_link_id(href: &str) -> Option<&str> {
    let id = href.trim().strip_prefix("grokhub://local-model/")?;
    crate::local_setup::model_by_id(id).map(|m| m.id)
}

/// Repo ids from a `/api/models` reply, newest first.
pub fn parse_releases(body: &str) -> Vec<String> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| it.get("id").and_then(|v| v.as_str()))
        .filter(|id| id.contains('/') && id.to_ascii_lowercase().contains("gguf"))
        .map(str::to_string)
        .collect()
}

/// Billions of parameters from a repo name: the largest `<n>B` in it, so a
/// mixture of experts (`30B-A3B`) counts its full weights.
pub fn params_b(repo: &str) -> Option<f64> {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    name.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
        .filter_map(|part| {
            let num = part.strip_suffix('B').or_else(|| part.strip_suffix('b'))?;
            let num = num.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            num.parse::<f64>().ok().filter(|n| *n > 0.0 && *n < 2_000.0)
        })
        .fold(None, |best: Option<f64>, n| {
            Some(best.map_or(n, |b| b.max(n)))
        })
}

/// Memory a release needs at Q4_K_M, in MiB.
pub fn est_run_mb(params_b: f64) -> u64 {
    (params_b * Q4_MB_PER_B).round() as u64 + RUN_OVERHEAD_MB
}

fn memory_words(mem: Memory) -> &'static str {
    match mem {
        Memory::Gpu => "GPU memory",
        Memory::Unified => "unified memory",
        Memory::System => "spare RAM",
    }
}

fn tok_s(m: &LocalModel, mem: Memory) -> u32 {
    match mem {
        Memory::Gpu => m.tok_s.gpu,
        Memory::Unified => m.tok_s.unified,
        Memory::System => m.tok_s.cpu,
    }
}

/// A Pulse suggestion.
#[derive(Clone, Debug, PartialEq)]
pub enum Suggestion {
    /// A catalog model that beats the one installed and is Recommended here.
    Upgrade {
        model: &'static LocalModel,
        current: &'static LocalModel,
        reason: String,
        tok_s: u32,
    },
    /// A new release that would fit; a link only.
    Release {
        repo: String,
        run_mb: u64,
        budget_mb: u64,
        mem: Memory,
        /// Speed of the catalog model nearest its size, and that model.
        like: Option<(&'static str, u32)>,
    },
}

/// The best catalog model Recommended on this PC that is bigger than the one
/// it runs (the catalog runs biggest first). Optional models are never pushed.
pub fn upgrade(p: &Profile, installed_id: &str, bench: &BenchCache) -> Option<Suggestion> {
    let catalog = local_models();
    let at = catalog.iter().position(|m| m.id == installed_id)?;
    let current = &catalog[at];
    let mem = p.budget().0;
    score(p, catalog, bench)
        .into_iter()
        .find(|pick| {
            pick.bucket == Bucket::Recommended
                && !pick.model.optional
                && catalog
                    .iter()
                    .position(|m| m.id == pick.model.id)
                    .is_some_and(|i| i < at)
        })
        .map(|pick| Suggestion::Upgrade {
            model: pick.model,
            current,
            reason: pick.reason,
            tok_s: tok_s(pick.model, mem),
        })
}

/// New repos that fit with headroom and run bigger than the installed model.
/// Returns the suggestions and every repo id read, for `seen`.
pub fn releases(p: &Profile, installed_run_mb: u64, repos: &[String]) -> Vec<Suggestion> {
    let (mem, budget) = p.budget();
    let catalog_repos: BTreeSet<&str> = local_models().iter().map(|m| m.repo).collect();
    repos
        .iter()
        .filter(|repo| !catalog_repos.contains(repo.as_str()))
        .filter(|repo| {
            let low = repo.to_ascii_lowercase();
            !SKIP_WORDS.iter().any(|w| low.contains(w))
        })
        .filter_map(|repo| {
            let run_mb = est_run_mb(params_b(repo)?);
            if run_mb * 100 > budget * HEADROOM_PERCENT || run_mb <= installed_run_mb {
                return None;
            }
            let like = local_models()
                .iter()
                .filter(|m| !m.is_drafter && tok_s(m, mem) > 0)
                .min_by_key(|m| m.run_mb.abs_diff(run_mb))
                .map(|m| (m.name, tok_s(m, mem)));
            Some(Suggestion::Release {
                repo: repo.clone(),
                run_mb,
                budget_mb: budget,
                mem,
                like,
            })
        })
        .collect()
}

/// How many release links one check may post, moved by feedback.
pub fn links_allowed(source_bias: i32) -> usize {
    usize::try_from((LINKS_PER_CHECK + source_bias).clamp(0, LINKS_MAX)).unwrap_or(0)
}

impl Suggestion {
    /// Stable per model, so one never posts twice.
    pub fn key(&self) -> String {
        match self {
            Suggestion::Upgrade { model, .. } => format!("catalog:{}", model.id),
            Suggestion::Release { repo, .. } => format!("hf:{repo}"),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Suggestion::Upgrade { model, .. } => format!("{} fits this PC", model.name),
            Suggestion::Release { repo, .. } => {
                format!(
                    "New on Hugging Face: {}",
                    repo.rsplit('/').next().unwrap_or(repo)
                )
            }
        }
    }

    pub fn body(&self) -> String {
        match self {
            Suggestion::Upgrade {
                model,
                current,
                reason,
                ..
            } => format!(
                "A bigger model than {} for local tasks. {} download. {reason}",
                current.name,
                size_label(model.download_mb)
            ),
            Suggestion::Release {
                run_mb,
                budget_mb,
                mem,
                like,
                ..
            } => {
                let speed = like
                    .map(|(name, t)| format!(", about {t} tok/s here like {name}"))
                    .unwrap_or_default();
                format!(
                    "Needs about {} at Q4, fits your {} of {}{speed}. Not in {product}'s checked catalog yet, so this only links it.",
                    size_label(*run_mb),
                    size_label(*budget_mb),
                    memory_words(*mem), product = crate::brand::product_name()
                )
            }
        }
    }

    pub fn card(&self, now_ms: u64) -> crate::UpdateCard {
        let source_id = format!("local-model:{}", self.key());
        let mut card =
            crate::update_feed::digest_card(&source_id, &self.title(), &self.body(), now_ms);
        card.pulse.source_name = Some(SOURCE_NAME.to_string());
        match self {
            Suggestion::Upgrade { model, .. } => {
                card.action = Some(crate::UpdateAction::DeepLink {
                    href: picker_link(model.id),
                });
                card.pulse.because = Some("Your local model, on this PC's hardware".into());
            }
            Suggestion::Release { repo, .. } => {
                card.citations = vec![repo_url(repo)];
                card.pulse.because = Some("A new open model that fits this PC".into());
            }
        }
        card
    }
}

/// A catalog suggestion card: it gets the button that opens the picker.
pub fn is_upgrade_card(card: &crate::UpdateCard) -> bool {
    card.source_id.starts_with("local-model:catalog:")
}

impl Tracker {
    pub fn due(&self, now_ms: u64) -> bool {
        self.on && now_ms.saturating_sub(self.last_check_ms) >= CHECK_EVERY_MS
    }

    /// Fold one check in and return what to post: the catalog upgrade (once
    /// per model) then up to `links` new releases. Every repo read is
    /// remembered; the first check posts no releases.
    pub fn check(
        &mut self,
        upgrade: Option<Suggestion>,
        releases: Vec<Suggestion>,
        read: &[String],
        links: usize,
        now_ms: u64,
    ) -> Vec<Suggestion> {
        self.last_check_ms = now_ms;
        let mut out = Vec::new();
        if let Some(up) = upgrade {
            if self.suggested.insert(up.key()) {
                out.push(up);
            }
        }
        if self.seeded {
            out.extend(
                releases
                    .into_iter()
                    .filter(|s| matches!(s, Suggestion::Release { repo, .. } if !self.seen.contains(repo)))
                    .take(links),
            );
        }
        self.seen.extend(read.iter().cloned());
        self.seeded = true;
        out
    }

    pub fn turn_off(&mut self) {
        *self = Tracker::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_picker::CpuSimd;

    fn gpu(vram_mb: u64) -> Profile {
        Profile {
            gpus_vram_mb: vec![vram_mb],
            unified: false,
            ram_mb: 32_768,
            free_disk_mb: 200_000,
            simd: CpuSimd::Avx2,
            llama_build: Some(99_999),
        }
    }

    fn repos(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_24_gb_gpu_running_the_12b_is_offered_the_31b() {
        let up = upgrade(&gpu(24_576), "gemma-4-12b-it-q4_k_m", &BenchCache::new()).unwrap();
        assert_eq!(up.title(), "Gemma 4 31B fits this PC");
        assert_eq!(
            up.body(),
            "A bigger model than Gemma 4 12B for local tasks. 17.4 GB download. Fits in 24 GB of GPU memory, about 35 tok/s."
        );
        let card = up.card(5);
        assert!(is_upgrade_card(&card));
        assert_eq!(
            card.action,
            Some(crate::UpdateAction::DeepLink {
                href: "grokhub://local-model/gemma-4-31b-it-q4_k_m".into()
            })
        );
        assert_eq!(
            picker_link_id("grokhub://local-model/gemma-4-31b-it-q4_k_m"),
            Some("gemma-4-31b-it-q4_k_m")
        );
        assert_eq!(picker_link_id("grokhub://local-model/not-a-model"), None);
    }

    #[test]
    fn no_upgrade_when_nothing_bigger_fits_or_nothing_is_installed() {
        // 8 GB: the 12B and up need more GPU memory, the E4B is already running.
        assert_eq!(
            upgrade(&gpu(8_192), "gemma-4-e4b-it-q4_k_m", &BenchCache::new()),
            None
        );
        // Already on the biggest.
        assert_eq!(
            upgrade(&gpu(24_576), "gemma-4-31b-it-q4_k_m", &BenchCache::new()),
            None
        );
        assert_eq!(upgrade(&gpu(24_576), "unknown", &BenchCache::new()), None);
    }

    #[test]
    fn param_counts_read_from_repo_names() {
        assert_eq!(params_b("ggml-org/Qwen3-30B-A3B-GGUF"), Some(30.0));
        assert_eq!(
            params_b("lmstudio-community/Mistral-Small-3.2-24B-GGUF"),
            Some(24.0)
        );
        assert_eq!(params_b("ggml-org/gpt-oss-20b-GGUF"), Some(20.0));
        assert_eq!(params_b("ggml-org/SmolLM3-1.7B-GGUF"), Some(1.7));
        assert_eq!(params_b("ggml-org/some-model-GGUF"), None);
        assert_eq!(est_run_mb(14.0), 10_020);
    }

    #[test]
    fn releases_that_fit_with_headroom_and_beat_the_current_one_only() {
        let read = repos(&[
            "ggml-org/Qwen3-14B-GGUF",
            "ggml-org/Qwen3-32B-GGUF",
            "ggml-org/Qwen3-4B-GGUF",
            "lmstudio-community/Llama-3.3-8B-abliterated-GGUF",
            "lmstudio-community/gemma-4-31B-it-GGUF",
            "ggml-org/embeddinggemma-300M-GGUF",
            "ggml-org/no-size-GGUF",
        ]);
        // 16 GB GPU running the E4B (5.9 GB).
        let got = releases(&gpu(16_384), 5_900, &read);
        let names: Vec<String> = got.iter().map(Suggestion::title).collect();
        assert_eq!(names, vec!["New on Hugging Face: Qwen3-14B-GGUF"]);
        assert_eq!(
            got[0].body(),
            "Needs about 9.8 GB at Q4, fits your 16 GB of GPU memory, about 35 tok/s here like Gemma 4 12B. Not in Loadout's checked catalog yet, so this only links it."
        );
        let card = got[0].card(9);
        assert_eq!(
            card.citations,
            vec!["https://huggingface.co/ggml-org/Qwen3-14B-GGUF"]
        );
        assert_eq!(card.action, None, "a release never downloads from Pulse");
        assert!(!is_upgrade_card(&card));
    }

    #[test]
    fn the_releases_reply_reads_gguf_repo_ids_in_order() {
        let body = r#"[{"id":"ggml-org/Qwen3-14B-GGUF","createdAt":"2026-10-20T00:00:00.000Z"},{"id":"ggml-org/whisper.cpp"},{"id":"ggml-org/Qwen3-4B-GGUF"}]"#;
        assert_eq!(
            parse_releases(body),
            repos(&["ggml-org/Qwen3-14B-GGUF", "ggml-org/Qwen3-4B-GGUF"])
        );
        assert!(parse_releases("{\"error\":\"rate limited\"}").is_empty());
    }

    #[test]
    fn the_first_check_only_records_releases_and_each_posts_once() {
        let p = gpu(16_384);
        let read = repos(&["ggml-org/Qwen3-14B-GGUF"]);
        let mut t = Tracker {
            on: true,
            ..Default::default()
        };
        let first = t.check(None, releases(&p, 5_900, &read), &read, 2, 10);
        assert!(first.is_empty(), "a backlog never posts");
        assert!(t.seeded);
        let newer = repos(&["ggml-org/Qwen3-14B-GGUF", "ggml-org/Phi-5-12B-GGUF"]);
        let second = t.check(None, releases(&p, 5_900, &newer), &newer, 2, 20);
        let names: Vec<String> = second.iter().map(Suggestion::title).collect();
        assert_eq!(names, vec!["New on Hugging Face: Phi-5-12B-GGUF"]);
        let up = || upgrade(&gpu(24_576), "gemma-4-12b-it-q4_k_m", &BenchCache::new());
        assert_eq!(t.check(up(), Vec::new(), &[], 2, 30).len(), 1);
        assert!(t.check(up(), Vec::new(), &[], 2, 40).is_empty());
        assert!(!t.due(40));
        assert!(t.due(40 + CHECK_EVERY_MS));
    }

    #[test]
    fn the_feedback_score_moves_how_many_links_post() {
        assert_eq!(links_allowed(0), 2);
        assert_eq!(links_allowed(-3), 0);
        assert_eq!(links_allowed(7), 4);
    }
}
