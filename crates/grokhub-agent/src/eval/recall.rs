//! LongMemEval-style recall eval on local, synthetic fixtures.
//!
//! Four categories modelled on LongMemEval's: information extraction (one
//! stored fact answers the question), multi-session (a later turn of a chat
//! needs a fact from another session), knowledge update (a newer fact on the
//! same topic replaces an older one) and abstention (nothing stored answers
//! the question, so nothing should be recalled for it). No model is called:
//! a case passes when the recall block the agent would inject carries what
//! the answer needs and nothing stale.
//!
//! Two arms. **Legacy** keeps `"memory_backend": "legacy"` and every fact in
//! `MEMORY.md`. **AMR** is the default: facts are AMR nodes, and a newer fact
//! supersedes the older one on its topic. Each arm also carries the
//! SOUL/USER block, so the per-turn token figures include it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grokhub_core::amr::{detect_correction, note_chat, ChatLine};

use crate::client::{ContentPart, InputItem};
use crate::perm::ConfigGuard;
use crate::tokens::estimate_tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    InfoExtraction,
    MultiSession,
    KnowledgeUpdate,
    Abstention,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::InfoExtraction,
        Category::MultiSession,
        Category::KnowledgeUpdate,
        Category::Abstention,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::InfoExtraction => "information extraction",
            Category::MultiSession => "multi-session",
            Category::KnowledgeUpdate => "knowledge update",
            Category::Abstention => "abstention",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    Legacy,
    Amr,
}

struct Case {
    category: Category,
    /// What the user said, in order. Each inner list is one session; a line
    /// starting "Actually," corrects an earlier one.
    sessions: &'static [&'static [&'static str]],
    /// An earlier message in the same chat, for multi-session cases.
    opener: Option<&'static str>,
    question: &'static str,
    /// Text the injected block must carry.
    expect: Option<&'static str>,
    /// Text it must not carry (a replaced fact).
    stale: Option<&'static str>,
}

const CASES: &[Case] = &[
    Case {
        category: Category::InfoExtraction,
        sessions: &[&["Preferred editor: helix with the gruvbox theme"]],
        opener: None,
        question: "which editor do I like to use for quick fixes",
        expect: Some("helix"),
        stale: None,
    },
    Case {
        category: Category::InfoExtraction,
        sessions: &[&[
            "Dog's name: Pepper, a border collie",
            "Gym days: Tuesday and Thursday mornings",
        ]],
        opener: None,
        question: "remind me what my dog is called again",
        expect: Some("Pepper"),
        stale: None,
    },
    Case {
        category: Category::MultiSession,
        sessions: &[
            &["Backup drive: the blue external disk labelled ARCHIVE"],
            &["Router admin page lives at the usual gateway address"],
        ],
        opener: Some("my router keeps dropping the connection tonight"),
        question: "also, which backup drive should the nightly job copy to",
        expect: Some("ARCHIVE"),
        stale: None,
    },
    Case {
        category: Category::MultiSession,
        sessions: &[
            &["Sister's birthday: March 14"],
            &["Bakery order: lemon cake from the corner shop"],
        ],
        opener: Some("help me plan a bakery order for the weekend"),
        question: "and when is my sister's birthday coming up",
        expect: Some("March 14"),
        stale: None,
    },
    Case {
        category: Category::KnowledgeUpdate,
        sessions: &[
            &["Home city: Lisbon"],
            &["Actually, home city: Porto since the move in June"],
        ],
        opener: None,
        question: "what is the weather like in my home city",
        expect: Some("Porto"),
        stale: Some("Lisbon"),
    },
    Case {
        category: Category::KnowledgeUpdate,
        sessions: &[
            &["Work laptop: ThinkPad X1 running Fedora"],
            &["Actually, work laptop: Framework 13 running Arch"],
        ],
        opener: None,
        question: "which distro is on my work laptop right now",
        expect: Some("Framework 13"),
        stale: Some("ThinkPad"),
    },
    Case {
        category: Category::Abstention,
        sessions: &[&["Coffee order: oat flat white"]],
        opener: None,
        question: "what was the name of my first teacher at school",
        expect: None,
        stale: None,
    },
    Case {
        category: Category::Abstention,
        sessions: &[&["Bike: a green gravel bike, size medium"]],
        opener: None,
        question: "which airline did I fly with last spring",
        expect: None,
        stale: None,
    },
];

const SOUL: &str = "# SOUL\nCalm, direct, and honest. Says when the user is wrong.\n";
const USER: &str = "# USER\n- Prefers short answers\n- Uses metric units\n";

/// One case's outcome in one arm.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseRun {
    pub category: Category,
    pub passed: bool,
    /// SOUL/USER block plus injected recall, in estimated tokens.
    pub added_tokens: u64,
}

/// Every case in one arm.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmRun {
    pub arm: Arm,
    pub runs: Vec<CaseRun>,
}

impl ArmRun {
    /// Passed and total for one category.
    pub fn score(&self, category: Category) -> (usize, usize) {
        let runs: Vec<&CaseRun> = self
            .runs
            .iter()
            .filter(|r| r.category == category)
            .collect();
        (runs.iter().filter(|r| r.passed).count(), runs.len())
    }

    /// Added tokens per turn at percentile `pct` (nearest rank).
    pub fn added_tokens_pct(&self, pct: usize) -> u64 {
        let mut all: Vec<u64> = self.runs.iter().map(|r| r.added_tokens).collect();
        all.sort_unstable();
        if all.is_empty() {
            return 0;
        }
        let rank = (pct * all.len()).div_ceil(100).max(1);
        all[rank - 1]
    }
}

/// Run every case in `arm` under `scratch`, one fresh config dir per case.
pub fn run_arm(arm: Arm, scratch: &Path) -> ArmRun {
    let runs = CASES
        .iter()
        .enumerate()
        .map(|(i, case)| run_case(arm, case, &scratch.join(format!("{arm:?}-{i}"))))
        .collect();
    ArmRun { arm, runs }
}

fn run_case(arm: Arm, case: &Case, root: &Path) -> CaseRun {
    let _ = fs::remove_dir_all(root);
    let cfg = root.join("cfg");
    let ws = root.join("ws");
    fs::create_dir_all(cfg.join("memory")).expect("eval config dir");
    fs::create_dir_all(&ws).expect("eval workspace");
    let _guard = ConfigGuard::set(&cfg);
    crate::harness::use_key_store_for(&cfg, Arc::new(crate::harness::MemoryKeyStore::new()));
    fs::write(cfg.join("memory/SOUL.md"), SOUL).expect("SOUL.md");
    fs::write(cfg.join("memory/USER.md"), USER).expect("USER.md");
    store_facts(arm, case, &cfg);

    let identity = crate::identity::identity_block(&cfg);
    let block = match case.opener {
        None => crate::memory::first_turn_injection(&ws, case.question),
        Some(opener) => {
            let mut history = Vec::new();
            if let Some(first) = crate::memory::first_turn_injection(&ws, opener) {
                history.push(system(first));
            }
            history.push(user(opener));
            crate::memory::followup_injection(&ws, case.question, &history)
        }
    };
    let text = block.clone().unwrap_or_default();
    let passed = match (case.expect, case.stale) {
        (None, _) => block.is_none(),
        (Some(want), stale) => text.contains(want) && stale.is_none_or(|s| !text.contains(s)),
    };
    let added_tokens = estimate_tokens(&identity) + estimate_tokens(&text);
    let _ = fs::remove_dir_all(root);
    CaseRun {
        category: case.category,
        passed,
        added_tokens,
    }
}

fn store_facts(arm: Arm, case: &Case, cfg: &Path) {
    let said = case.sessions.iter().flat_map(|s| s.iter().copied());
    match arm {
        Arm::Legacy => {
            // Legacy keeps every line it was told, corrections appended.
            fs::write(cfg.join("app.json"), r#"{"memory_backend":"legacy"}"#).expect("app.json");
            let body: Vec<String> = said
                .map(|line| detect_correction(line).map_or(line.to_string(), |c| c.fixed))
                .collect();
            fs::write(cfg.join("memory/MEMORY.md"), body.join("\n\n") + "\n").expect("MEMORY.md");
        }
        Arm::Amr => {
            // The chat learner: a correction supersedes the line it fixes.
            let store = crate::memory::amr_store();
            for (turn, text) in said.enumerate() {
                let line = ChatLine {
                    thread: "eval",
                    turn: turn as u32,
                    text,
                };
                let noted = note_chat(&store, &line, 1_000 + turn as u64).expect("note");
                assert!(noted.id().is_some(), "{text}: {noted:?}");
            }
        }
    }
}

fn system(text: String) -> InputItem {
    InputItem::Message {
        role: "system".into(),
        content: vec![ContentPart::InputText(text)],
    }
}

fn user(text: &str) -> InputItem {
    InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

/// Markdown table: per-category score for both arms, then added tokens.
pub fn render(legacy: &ArmRun, amr: &ArmRun) -> String {
    let mut out = String::from("| Category | Legacy | AMR |\n|---|---|---|\n");
    for category in Category::ALL {
        let (lp, ln) = legacy.score(category);
        let (ap, an) = amr.score(category);
        out.push_str(&format!(
            "| {} | {lp}/{ln} | {ap}/{an} |\n",
            category.label()
        ));
    }
    out.push_str(&format!(
        "| added tokens p50 / p95 | {} / {} | {} / {} |\n",
        legacy.added_tokens_pct(50),
        legacy.added_tokens_pct(95),
        amr.added_tokens_pct(50),
        amr.added_tokens_pct(95)
    ));
    out
}

/// Scratch root for one run.
pub fn scratch_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("gh-recall-eval-{tag}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amr_default_does_not_regress_knowledge_update_or_abstention() {
        let root = scratch_root("gate");
        let legacy = run_arm(Arm::Legacy, &root);
        let amr = run_arm(Arm::Amr, &root);
        let table = render(&legacy, &amr);
        eprintln!("{table}");
        for category in [Category::KnowledgeUpdate, Category::Abstention] {
            assert!(
                amr.score(category).0 >= legacy.score(category).0,
                "{} regressed:\n{table}",
                category.label()
            );
        }
        let scores = |run: &ArmRun| Category::ALL.map(|c| run.score(c).0);
        // Legacy keeps both sides of an update and misses the drive question
        // a turn later; AMR misses "dog" because its recall skips words under
        // four letters.
        assert_eq!(scores(&legacy), [2, 1, 0, 2], "{table}");
        assert_eq!(scores(&amr), [1, 2, 2, 2], "{table}");
        assert!(amr.added_tokens_pct(95) < 1_500, "{table}");
        let _ = fs::remove_dir_all(&root);
    }
}
