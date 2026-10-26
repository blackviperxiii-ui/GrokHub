//! One medium-effort sleep call after the deterministic dream.
//! Lessons go through [`crate::harness::lesson_checks`] and the change ledger.
//! USER and SOUL lines are proposals. Nothing here writes those files.

use std::path::Path;
use std::time::Duration;

use grokhub_core::amr::{
    allows_lesson_write, approval_samples, calibrate_reflect_min, memory_rows, morning_text,
    reflect_threshold_log, stage_core_line, unsafe_to_learn, CoreWriter, DreamOpts, DreamPhase,
    DreamReport, DreamStep, LineWrite, ModelDreamOpts, ModelDreamReport, MorningNote, Noted,
    StagedCore,
};
use grokhub_core::{automation_done_card, skill_dir_name, UpdateCard};

use crate::client::{CancelToken, ContentPart, InputItem, ModelClient, ResponsesRequest};
use crate::harness::{lesson_checks, record_skill_change, Origin};

const LESSON_SKILL: &str = "dream-lessons";

/// Deterministic dream, then at most one model call when `model_opts.enabled`.
pub struct SleepDreamOut {
    pub note: MorningNote,
    pub card: UpdateCard,
    pub calls: u32,
    pub output_token_cap: u32,
    pub report: ModelDreamReport,
    pub phase: DreamPhase,
}

#[derive(serde::Deserialize, Default)]
struct ModelReply {
    #[serde(default)]
    lessons: Vec<LessonItem>,
    #[serde(default)]
    insights: Vec<InsightItem>,
}

#[derive(serde::Deserialize)]
struct LessonItem {
    text: String,
    #[serde(default)]
    source: String,
}

#[derive(serde::Deserialize)]
struct InsightItem {
    block: String,
    text: String,
    #[serde(default)]
    source: String,
}

pub fn sleep_dream(
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    config_dir: &Path,
    store: &grokhub_core::amr::AmrStore,
    now_ms: u64,
    dream: &DreamOpts,
    model_opts: &ModelDreamOpts,
) -> Result<SleepDreamOut, String> {
    let dream_report = store
        .dream_once(now_ms, dream)
        .map_err(|err| err.to_string())?;
    let mut phase = DreamPhase::after_deterministic(&dream_report);
    let mut report = ModelDreamReport {
        output_token_cap: model_opts.output_token_cap,
        ..ModelDreamReport::default()
    };
    if model_opts.enabled {
        phase = phase
            .advance(DreamStep::RunModel)
            .map_err(|err| err.to_string())?;
        match one_call(
            client,
            cancel,
            model,
            store,
            &dream_report,
            model_opts.output_token_cap,
        ) {
            Ok(text) => {
                report.calls = 1;
                let capped = cap_text(&text, model_opts.output_token_cap);
                match parse_reply(&capped) {
                    Ok(reply) => {
                        apply_lessons(config_dir, &reply.lessons, &mut report);
                        apply_insights(store, now_ms, &dream_report, &reply.insights, &mut report);
                        phase = phase
                            .advance(DreamStep::ModelParsed {
                                had_items: !reply.lessons.is_empty() || !reply.insights.is_empty(),
                            })
                            .map_err(|err| err.to_string())?;
                    }
                    Err(()) => {
                        report.refused.push("model reply was not usable".into());
                        phase = phase
                            .advance(DreamStep::ModelParsed { had_items: false })
                            .map_err(|err| err.to_string())?;
                    }
                }
            }
            Err(()) => {
                report.calls = 1;
                report.refused.push("model pass failed".into());
                phase = phase
                    .advance(DreamStep::ModelParsed { had_items: false })
                    .map_err(|err| err.to_string())?;
            }
        }
    } else {
        phase = phase
            .advance(DreamStep::SkipModel)
            .map_err(|err| err.to_string())?;
    }
    let threshold = calibrate_reflect_min(&approval_samples(store));
    let log_line = reflect_threshold_log(threshold);
    let text = morning_text(&report.proposed, report.lessons_applied > 0, &log_line);
    phase = phase
        .advance(DreamStep::Summarize { text: text.clone() })
        .map_err(|err| err.to_string())?;
    let card = automation_done_card("dream-model", "Memory dream", &text, now_ms);
    Ok(SleepDreamOut {
        note: MorningNote { text },
        card,
        calls: report.calls,
        output_token_cap: model_opts.output_token_cap,
        report,
        phase,
    })
}

fn one_call(
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    store: &grokhub_core::amr::AmrStore,
    report: &DreamReport,
    cap: u32,
) -> Result<String, ()> {
    let req = ResponsesRequest {
        model: model.to_string(),
        // The live `background:dream` row starts at low and ceilings at medium.
        effort: Some("medium".into()),
        input: vec![
            InputItem::Message {
                role: "system".into(),
                content: vec![ContentPart::InputText(system_prompt(cap))],
            },
            InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText(user_prompt(store, report))],
            },
        ],
        conversation_id: "background:dream".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: Some(Duration::from_secs(120)),
    };
    crate::route::live::stream_routed(client, &req, cancel, &mut |_| {}, "background:dream")
        .map(|turn| turn.text)
        .map_err(|_| ())
}

fn system_prompt(cap: u32) -> String {
    format!(
        "You are the sleep-time memory pass. Reply with one JSON object and no other text. \
A lesson is {{\"text\",\"source\"}}. An insight is {{\"block\":\"user\" or \"soul\",\"text\",\"source\"}}. \
Each text is one line the user already showed you. Do not invent facts. \
Do not agree with a wrong claim. Do not drop a stored fact. Stay under {cap} output tokens."
    )
}

fn user_prompt(store: &grokhub_core::amr::AmrStore, report: &DreamReport) -> String {
    let (rows, _) = memory_rows(store);
    let mut lines = String::new();
    for row in rows.into_iter().filter(|row| !row.proposed).take(24) {
        let text: String = row.text.chars().take(120).collect();
        lines.push_str(&text);
        lines.push('\n');
    }
    format!(
        "Deterministic dream {}: looked at {}, merged {}, retired {}.\nActive lines:\n{}Propose lessons and USER or SOUL insights as JSON.",
        report.date,
        report.looked_at,
        report.merges.len(),
        report.retired.len(),
        lines
    )
}

fn cap_text(text: &str, tokens: u32) -> String {
    let max = crate::tokens::estimate_chars(u64::from(tokens)) as usize;
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn strip_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let without = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();
    without.strip_suffix("```").unwrap_or(without).trim()
}

fn parse_reply(text: &str) -> Result<ModelReply, ()> {
    serde_json::from_str(strip_fence(text)).map_err(|_| ())
}

fn short_refusal(err: &str) -> String {
    if err.contains("sycophancy") {
        "lesson refused: sycophancy".into()
    } else if err.contains("longmem") {
        "lesson refused: longmem".into()
    } else if err.contains("unsafe") {
        "lesson refused: unsafe".into()
    } else {
        "lesson refused".into()
    }
}

fn apply_lessons(config_dir: &Path, lessons: &[LessonItem], report: &mut ModelDreamReport) {
    if !allows_lesson_write(true, CoreWriter::DreamPass) {
        return;
    }
    for lesson in lessons {
        let text = lesson.text.lines().next().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        match apply_lesson(config_dir, text, lesson.source.trim()) {
            Ok(true) => report.lessons_applied += 1,
            Ok(false) => {}
            Err(why) => report.lessons_refused.push(short_refusal(&why)),
        }
    }
}

fn apply_lesson(config_dir: &Path, line: &str, source: &str) -> Result<bool, String> {
    let note = source.lines().next().unwrap_or("").trim();
    let stored = if note.is_empty() {
        line.to_string()
    } else {
        format!("{line} ({note})")
    };
    if grokhub_core::amr::unsafe_to_learn(&stored).is_some() {
        return Err("lesson refused: unsafe".into());
    }
    let path = config_dir
        .join("skills")
        .join(skill_dir_name(LESSON_SKILL))
        .join("SKILL.md");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if !old.is_empty() && old.contains(&stored) {
        return Ok(false);
    }
    let body = if old.trim().is_empty() {
        format!("---\nname: dream-lessons\n---\n\n# dream-lessons\n\n{stored}\n")
    } else if old.ends_with('\n') {
        format!("{old}{stored}\n")
    } else {
        format!("{old}\n{stored}\n")
    };
    lesson_checks(&old, &body)?;
    let skills = config_dir.join("skills");
    let bytes = body.into_bytes();
    record_skill_change(
        config_dir,
        &skills,
        LESSON_SKILL,
        Origin::SelfManage,
        "dream lesson",
        move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            std::fs::write(&path, &bytes).map_err(|err| err.to_string())
        },
    )?;
    Ok(true)
}

fn apply_insights(
    store: &grokhub_core::amr::AmrStore,
    now_ms: u64,
    dream: &DreamReport,
    insights: &[InsightItem],
    report: &mut ModelDreamReport,
) {
    let date = if dream.date.is_empty() {
        "night"
    } else {
        dream.date.as_str()
    };
    for insight in insights {
        let block = match insight.block.trim() {
            "user" => "user",
            "soul" => "soul",
            _ => {
                report.refused.push("insight skipped".into());
                continue;
            }
        };
        let text = insight.text.lines().next().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        if unsafe_to_learn(text).is_some() {
            report.refused.push("insight refused: unsafe".into());
            continue;
        }
        let bit = insight.source.trim();
        let bit = if bit.is_empty() { "model" } else { bit };
        let source = format!("dream:{date}:{block}:{bit}");
        let write = LineWrite {
            text,
            source: &source,
            tags: Vec::new(),
            confidence: 0.4,
            revive: false,
        };
        match stage_core_line(store, &write, now_ms, true, CoreWriter::DreamPass) {
            Ok(StagedCore::Proposed(Noted::New(_) | Noted::Known(_))) => {
                report.proposed.push(text.to_string());
            }
            Ok(StagedCore::Proposed(Noted::Skipped(_))) | Ok(StagedCore::Remembered(_)) => {
                report.refused.push("insight skipped".into());
            }
            Err(_) => report.refused.push("insight skipped".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    use grokhub_core::amr::DreamOpts as CoreDreamOpts;
    use grokhub_core::amr::{
        dream_report_path, memory_rows, remember_line, AmrStore, EdgeRel, LineWrite, SourceLink,
        MODEL_DREAM_OUTPUT_TOKEN_CAP,
    };

    use crate::client::{ClientError, StreamEvent, TurnOutput};
    use crate::identity::identity_blocks;
    use crate::memory::remember;
    use crate::perm::ConfigGuard;

    const NOW: u64 = 1_792_972_800_000;
    const HAPPY: &str = r#"{"lessons":[{"text":"Keep the harbor code 4419 and speak plainly.","source":"chat:harbor#4"}],"insights":[{"block":"user","text":"prefers quiet mornings","source":"chat:harbor#4"},{"block":"soul","text":"speaks in short lines","source":"chat:harbor#4"}]}"#;

    struct Script {
        calls: AtomicU32,
        text: String,
        seen: Mutex<Seen>,
    }

    #[derive(Default)]
    struct Seen {
        effort: Option<String>,
        conversation_id: String,
    }

    impl Script {
        fn new(text: &str) -> Self {
            Self {
                calls: AtomicU32::new(0),
                text: text.to_string(),
                seen: Mutex::new(Seen::default()),
            }
        }
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut seen) = self.seen.lock() {
                seen.effort = req.effort.clone();
                seen.conversation_id = req.conversation_id.clone();
            }
            Ok(TurnOutput {
                text: self.text.clone(),
                ..TurnOutput::default()
            })
        }
    }

    fn pair(tag: &str) -> (PathBuf, PathBuf, ConfigGuard) {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("gh-sleep-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cfg = root.join("cfg");
        let ws = root.join("ws");
        fs::create_dir_all(&cfg).unwrap();
        fs::create_dir_all(&ws).unwrap();
        let guard = ConfigGuard::set(&cfg);
        (cfg, ws, guard)
    }

    fn line<'a>(text: &'a str) -> LineWrite<'a> {
        LineWrite {
            text,
            source: "user",
            tags: Vec::new(),
            confidence: 0.9,
            revive: false,
        }
    }

    fn opts() -> CoreDreamOpts {
        CoreDreamOpts {
            date: Some("2026-10-26".into()),
            ..CoreDreamOpts::default()
        }
    }

    #[test]
    fn flag_off_runs_the_deterministic_dream_and_skips_the_model() {
        let (cfg, _ws, _guard) = pair("off");
        fs::write(cfg.join("app.json"), "{\"memory_backend\":\"amr\"}\n").unwrap();
        let store = AmrStore::at(cfg.join("amr"));
        store.init().unwrap();
        let script = Script::new(HAPPY);
        let out = sleep_dream(
            &script,
            &CancelToken::new(),
            "grok-4.7",
            &cfg,
            &store,
            NOW,
            &opts(),
            &ModelDreamOpts::default(),
        )
        .unwrap();
        assert_eq!(script.calls.load(Ordering::SeqCst), 0);
        assert_eq!(out.calls, 0);
        assert_eq!(out.phase.name(), "morning_summary");
        assert!(dream_report_path(&store, "2026-10-26").is_file());
        assert!(!cfg
            .join("skills")
            .join("dream-lessons")
            .join("SKILL.md")
            .exists());
    }

    #[test]
    fn enabled_dream_proposes_lessons_and_leaves_identity_files() {
        let (cfg, ws, _guard) = pair("on");
        fs::write(
            cfg.join("app.json"),
            "{\"memory_backend\":\"amr\",\"dream_model\":true}\n",
        )
        .unwrap();
        fs::create_dir_all(cfg.join("memory")).unwrap();
        let user = "USER_MARK from the file\n";
        let soul = "SOUL_MARK speaks plainly\n";
        fs::write(cfg.join("memory").join("USER.md"), user).unwrap();
        fs::write(cfg.join("memory").join("SOUL.md"), soul).unwrap();
        let store = AmrStore::at(cfg.join("amr"));
        store.init().unwrap();
        let old = remember_line(&store, &line("keeps a paper notebook"), NOW).unwrap();
        let new = remember_line(&store, &line("keeps a slate board"), NOW + 1).unwrap();
        store.link(new.id(), old.id(), EdgeRel::Supersedes).unwrap();

        let status = remember(&ws, "prefers harbor ink").unwrap();
        assert_eq!(
            status,
            "Proposed for approval. Nothing was added to core memory."
        );
        assert!(!ws.join("MEMORY.md").exists());

        let script = Script::new(HAPPY);
        let out = sleep_dream(
            &script,
            &CancelToken::new(),
            "grok-4.7",
            &cfg,
            &store,
            NOW,
            &opts(),
            &ModelDreamOpts {
                enabled: true,
                output_token_cap: MODEL_DREAM_OUTPUT_TOKEN_CAP,
            },
        )
        .unwrap();
        assert_eq!(script.calls.load(Ordering::SeqCst), 1);
        assert_eq!(out.calls, 1);
        assert_eq!(out.output_token_cap, 4096);
        let seen = script.seen.lock().unwrap();
        assert_eq!(seen.effort.as_deref(), Some("low"));
        assert_eq!(seen.conversation_id, "background:dream");
        drop(seen);
        assert_eq!(out.phase.name(), "morning_summary");
        assert!(out.note.text.contains("reflect_min_confidence"));
        assert!(out.note.text.contains("applied dream-lessons"));
        assert_eq!(out.card.title, "Memory dream");
        assert_eq!(out.card.source_id, "dream-model");
        let body = out.card.body.expect("morning body");
        assert!(body.contains("reflect_min_confidence"), "{body}");
        assert!(body.contains("applied dream-lessons"), "{body}");

        let block = identity_blocks(&cfg);
        assert!(block.contains("SOUL_MARK"), "{block}");
        assert!(block.contains("keeps a slate board"), "{block}");
        assert!(!block.contains("paper notebook"), "{block}");
        assert!(!block.contains("quiet mornings"), "{block}");
        assert!(!block.contains("short lines"), "{block}");
        assert!(!block.contains("harbor ink"), "{block}");
        assert_eq!(
            fs::read_to_string(cfg.join("memory").join("USER.md")).unwrap(),
            user
        );
        assert_eq!(
            fs::read_to_string(cfg.join("memory").join("SOUL.md")).unwrap(),
            soul
        );

        let lesson =
            fs::read_to_string(cfg.join("skills").join("dream-lessons").join("SKILL.md")).unwrap();
        assert!(lesson.contains("4419"), "{lesson}");
        let (rows, _) = memory_rows(&store);
        let quiet = rows
            .iter()
            .find(|row| row.text == "prefers quiet mornings")
            .expect("proposed user line");
        assert!(quiet.proposed);
        match &quiet.source {
            SourceLink::Other(source) => {
                assert!(source.contains("dream:2026-10-26:user:"), "{source}");
            }
            other => panic!("provenance was {other:?}"),
        }
        assert!(rows
            .iter()
            .any(|row| row.text == "speaks in short lines" && row.proposed));
        assert!(rows
            .iter()
            .any(|row| row.text == "prefers harbor ink" && row.proposed));
    }

    #[test]
    fn an_over_cap_reply_is_not_parsed_and_proposes_nothing() {
        let (cfg, _ws, _guard) = pair("cap");
        let store = AmrStore::at(cfg.join("amr"));
        store.init().unwrap();
        let junk = format!("{{{}", "x".repeat(20_000));
        let script = Script::new(&junk);
        let out = sleep_dream(
            &script,
            &CancelToken::new(),
            "grok-4.7",
            &cfg,
            &store,
            NOW,
            &opts(),
            &ModelDreamOpts {
                enabled: true,
                output_token_cap: MODEL_DREAM_OUTPUT_TOKEN_CAP,
            },
        )
        .unwrap();
        assert_eq!(script.calls.load(Ordering::SeqCst), 1);
        assert_eq!(out.calls, 1);
        assert!(out.report.proposed.is_empty());
        assert!(
            out.report
                .refused
                .iter()
                .any(|line| line.contains("model reply was not usable")),
            "{:?}",
            out.report.refused
        );
        assert_eq!(out.phase.name(), "morning_summary");
        assert!(!cfg
            .join("skills")
            .join("dream-lessons")
            .join("SKILL.md")
            .exists());
    }
}
