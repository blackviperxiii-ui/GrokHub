//! The local model's self-check: why this computer's model is or isn't ready,
//! read from the files and the engine's own state, never guessed. It reads the
//! models folders (the chat model, its drafter, unfinished downloads, files
//! the engine can't see), the pick, the engine, the governor and the last
//! start, and says what's wrong in plain words and what the app does about it.
//!
//! Three places use it: the `local_ai_status` tool, the note a chat turn gets
//! when it asks about the local model ([`question`], [`note`]), and the line an
//! 18+ turn gets when the model can't take it ([`super::adult::no_local_notice`]),
//! so that reply needs no model at all.

use std::path::{Path, PathBuf};

use grokhub_core::local_draft as ld;
use grokhub_core::local_governor::Level;

use super::{local, local_gov, local_sidecar, local_spec};

/// What stands between the chat and this computer's model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Loaded, or starts on the next message.
    Ready,
    /// Starting now.
    Loading,
    /// Local AI is switched off.
    Off,
    /// No `llama-server`.
    NoEngine,
    /// No chat model the engine can load.
    NoModel,
    /// A game, low battery or heat paused it.
    Paused(String),
    /// It kept crashing, so the crash breaker holds it.
    Breaker(String),
    /// The files are there, but the last start failed.
    StartFailed(String),
}

/// The self-check's findings: the verdict and every fact it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub verdict: Verdict,
    /// One fact a line: folders, files, the pick, the drafter, the engine.
    pub facts: Vec<String>,
}

impl Report {
    /// One or two plain sentences: what's wrong and what the app does now.
    pub fn summary(&self) -> String {
        match &self.verdict {
            Verdict::Ready => "This computer's model is ready.".into(),
            Verdict::Loading => "This computer's model is starting now.".into(),
            Verdict::Off => "Local AI is switched off, so the model on disk isn't used.".into(),
            Verdict::NoEngine => {
                "The local engine (llama-server) isn't installed yet, so no model can run. Setting up Local AI downloads it.".into()
            }
            Verdict::NoModel => {
                "No chat model the engine can load is in the models folder.".into()
            }
            Verdict::Paused(why) => format!("The model is paused because {why}. It comes back by itself."),
            Verdict::Breaker(why) => format!("{}.", capital(why)),
            Verdict::StartFailed(why) => format!(
                "The model is on disk, but it didn't start: {why}. It tries again on the next message."
            ),
        }
    }

    /// The summary, then every fact.
    pub fn text(&self) -> String {
        let mut out = vec![self.summary(), "What the check read:".into()];
        out.extend(self.facts.iter().map(|f| format!("- {f}")));
        out.join("\n")
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn size(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    if bytes >= 100_000_000 {
        format!("{:.1} GB", bytes as f64 / GB)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

/// Files in `dir` whose name ends in `ext` (case-insensitive), sorted.
fn files_with(dir: &Path, ext: &str) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, u64)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_ascii_lowercase();
            let md = e.metadata().ok()?;
            (md.is_file() && name.ends_with(ext)).then_some((p, md.len()))
        })
        .collect();
    out.sort();
    out
}

/// Folders the chat model may sit in, as the engine looks: the shared models
/// root, then the config folder's own.
fn model_dirs(config_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ld::models_root().into_iter().collect();
    let own = config_dir.join("models");
    if !dirs.contains(&own) {
        dirs.push(own);
    }
    dirs
}

/// Folders the models folder keeps for other kinds; not a misplaced model.
const KIND_DIRS: &[&str] = &[ld::DRAFTERS_DIR, "image", "voice", "other"];

/// GGUFs one folder down that the engine never looks at.
fn tucked_away(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        let skip = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| KIND_DIRS.iter().any(|k| k.eq_ignore_ascii_case(n)));
        if p.is_dir() && !skip {
            out.extend(
                files_with(&p, ".gguf")
                    .into_iter()
                    .filter(|(f, _)| !local_sidecar::helper_gguf(f)),
            );
        }
    }
    out
}

fn name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// The raw pick in `local_ai.json`, when one is set.
fn raw_pick(config_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(config_dir.join("local_ai.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let pick = v.get("localModel")?.get("selected")?.as_str()?.trim();
    (!pick.is_empty()).then(|| pick.to_string())
}

/// What the models folders hold, one line each.
fn folder_facts(config_dir: &Path, facts: &mut Vec<String>) {
    for dir in model_dirs(config_dir) {
        if !dir.is_dir() {
            continue;
        }
        let ggufs = files_with(&dir, ".gguf");
        let (helpers, chat): (Vec<_>, Vec<_>) = ggufs
            .into_iter()
            .partition(|(p, _)| local_sidecar::helper_gguf(p));
        let list = |v: &[(PathBuf, u64)]| {
            v.iter()
                .map(|(p, b)| format!("{} ({})", name(p), size(*b)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        facts.push(if chat.is_empty() {
            format!("{}: no chat model.", dir.display())
        } else {
            format!("{}: {}.", dir.display(), list(&chat))
        });
        if !helpers.is_empty() {
            facts.push(format!(
                "Helper files there, never loaded as the chat model: {}.",
                list(&helpers)
            ));
        }
        let parts = files_with(&dir, ".part");
        if !parts.is_empty() {
            facts.push(format!(
                "Unfinished downloads (they resume, and don't count until done): {}.",
                list(&parts)
            ));
        }
        let away = tucked_away(&dir);
        if !away.is_empty() {
            facts.push(format!(
                "In a subfolder the engine doesn't look in: {}. Pick it under Local AI, or move it into {}.",
                away.iter()
                    .map(|(p, b)| format!("{} ({})", p.display(), size(*b)))
                    .collect::<Vec<_>>()
                    .join(", "),
                dir.display()
            ));
        }
        let drafters = files_with(&dir.join(ld::DRAFTERS_DIR), ".gguf");
        if !drafters.is_empty() {
            facts.push(format!("Draft models: {}.", list(&drafters)));
        }
        if let Some(rec) = grokhub_core::model_download::installed(&dir) {
            facts.push(format!("Last finished download there: {}.", rec.file));
        } else if dir
            .join(grokhub_core::model_download::INSTALLED_FILE)
            .is_file()
        {
            facts.push(
                "The download record there names a file that's gone or a different size.".into(),
            );
        }
    }
}

/// Read everything and decide. Reads files and in-memory state only: it never
/// starts the model or touches the network.
pub fn diagnose(config_dir: &Path) -> Report {
    let mut facts = Vec::new();
    let on = local::switched_on();
    facts.push(format!(
        "Local AI switch: {}.",
        if on { "on" } else { "off" }
    ));
    let binary = local_sidecar::find_binary(config_dir);
    facts.push(match &binary {
        Some(b) => format!("Engine: {}.", b.display()),
        None => "Engine: llama-server not found in the app's bin folder or on PATH.".into(),
    });
    folder_facts(config_dir, &mut facts);
    let pick = raw_pick(config_dir);
    let picked = local_sidecar::picked_model(config_dir);
    if let Some(p) = &pick {
        facts.push(match &picked {
            Some(f) => format!("Picked model: {}.", f.display()),
            None => format!("Picked model {p} isn't on disk, so the engine looks for another."),
        });
    }
    let model = local_sidecar::find_model(config_dir);
    facts.push(match &model {
        Some(m) => format!("The engine loads: {}.", m.display()),
        None => "The engine finds no chat model to load.".into(),
    });
    if let Some((drafter, decision)) = local_spec::draft_status() {
        facts.push(match decision {
            ld::Decision::On => format!("Draft model {drafter}: on."),
            ld::Decision::Off(why) => format!("Draft model {drafter}: off, because {why}."),
            ld::Decision::NeedsTest => {
                format!("Draft model {drafter}: waiting for its speed test.")
            }
        });
    }
    let start_error = local::start_failure();
    if let Some(e) = &start_error {
        facts.push(format!("Last start: failed ({e})."));
    }
    let verdict = if !on {
        Verdict::Off
    } else if binary.is_none() {
        Verdict::NoEngine
    } else if model.is_none() {
        Verdict::NoModel
    } else if local_gov::level() == Level::Unload {
        Verdict::Paused(local_gov::reason().to_string())
    } else if let Err(why) = local_gov::may_start(local_sidecar::unix_ms()) {
        Verdict::Breaker(why)
    } else if let Some(e) = start_error {
        Verdict::StartFailed(e)
    } else {
        match local_sidecar::runtime(config_dir) {
            local_sidecar::Runtime::Loading => Verdict::Loading,
            local_sidecar::Runtime::Error(e) => Verdict::StartFailed(e),
            _ => Verdict::Ready,
        }
    };
    Report { verdict, facts }
}

/// The turn asks why this computer's model isn't ready, or to check it.
pub fn question(text: &str) -> bool {
    let t = super::ask::own_words(text).to_lowercase();
    const LOCAL: &[&str] = &[
        "local model",
        "local ai",
        "local llm",
        "on-device model",
        "on device model",
        "draft model",
        "drafter",
        "model folder",
        "models folder",
        "model directory",
        "models directory",
    ];
    const ASK: &[&str] = &[
        "ready", "working", "work?", "detect", "pick", "find", "found", "load", "diagnose",
        "check", "why", "broke", "fix", "see it", "recogni", "running", "start",
    ];
    LOCAL.iter().any(|w| t.contains(w)) && ASK.iter().any(|w| t.contains(w))
}

pub const NOTE_OPENS: &str = "[Local check] ";

/// The note a turn asking about the local model carries: the check's result,
/// and that the answer comes from it.
pub fn note(report: &Report) -> String {
    format!(
        "{NOTE_OPENS}The user asked about this computer's model. The app just read its model folders and engine:\n{}\nTell the user what it found and what the app is doing about it, plainly and in your own voice. This check is the look at their files: never say you can't reach their files, and never ask them to check folders, paths or file names themselves. If a step is truly theirs (a switch only they should flip), name it once. Don't mention this note.",
        report.text()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_about_the_local_model_get_the_check_and_others_dont() {
        for ask in [
            "why does the app keep saying that no local model is ready when one is downloaded along with draft model",
            "check the model directory for me then diagnose why its not picking it up",
            "is my local ai working?",
            "why won't the drafter load",
        ] {
            assert!(question(ask), "{ask}");
        }
        for ask in [
            "keep going",
            "whats a good budget gaming mouse",
            "check my wifi",
            "load my last save in elden ring",
        ] {
            assert!(!question(ask), "{ask}");
        }
    }

    #[test]
    fn a_failed_start_says_the_files_are_there_and_that_it_retries() {
        let r = Report {
            verdict: Verdict::StartFailed("the draft model's tokenizer doesn't match".into()),
            facts: vec!["Engine: /bin/llama-server.".into()],
        };
        assert_eq!(
            r.summary(),
            "The model is on disk, but it didn't start: the draft model's tokenizer doesn't match. It tries again on the next message."
        );
        assert_eq!(
            r.text(),
            "The model is on disk, but it didn't start: the draft model's tokenizer doesn't match. It tries again on the next message.\nWhat the check read:\n- Engine: /bin/llama-server."
        );
        let n = note(&r);
        assert!(n.starts_with(NOTE_OPENS), "{n}");
        assert!(n.contains("never ask them to check folders"), "{n}");
    }

    #[test]
    fn the_folder_read_finds_unfinished_downloads_drafters_and_tucked_away_models() {
        let dir = std::env::temp_dir().join(format!("gh-doctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(models.join(ld::DRAFTERS_DIR)).unwrap();
        std::fs::create_dir_all(models.join("gemma")).unwrap();
        std::fs::write(models.join("big.gguf.part"), vec![0u8; 10]).unwrap();
        std::fs::write(models.join(ld::DRAFTERS_DIR).join("small.gguf"), b"d").unwrap();
        std::fs::write(models.join("gemma").join("gemma-26b.gguf"), b"m").unwrap();
        std::fs::write(models.join("vision-mmproj-f16.gguf"), b"p").unwrap();
        let mut facts = Vec::new();
        folder_facts(&dir, &mut facts);
        let all = facts.join("\n");
        assert!(
            all.contains(&format!("{}: no chat model.", models.display())),
            "{all}"
        );
        assert!(all.contains("Unfinished downloads"), "{all}");
        assert!(all.contains("big.gguf.part (0 MB)"), "{all}");
        assert!(all.contains("Draft models: small.gguf (0 MB)."), "{all}");
        assert!(all.contains("gemma-26b.gguf"), "{all}");
        assert!(
            all.contains("subfolder the engine doesn't look in"),
            "{all}"
        );
        assert!(
            all.contains("never loaded as the chat model: vision-mmproj-f16.gguf"),
            "{all}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
