//! The slash commands that act on a saved chat or on the project list:
//! `/rename`, `/pin`, `/delete`, `/sessions`, `/context`, `/usage`, `/rewind`
//! and `/project`. `slash.rs` reads the line; this runs it. The destructive
//! ones answer with a question first and change nothing until the same line
//! comes back with `yes`. The memory, learning, skill and ledger commands
//! run here too: a read answers with a notice, a change asks first.

use std::path::Path;

use grokhub_agent::harness::ChangeKind;
use grokhub_agent::perm::ConfigGuard;

use crate::slash::{Effect, Op};
use crate::{history, projects};

/// The cabin's words for `/rewind` with nothing after it.
pub const REWIND_NA: &str = "N/A on native threads: the session is append-only.";
const NO_CHAT: &str = "Send a message first. This chat isn't saved yet.";

/// What a command needs to know about where it was typed.
pub struct Ctx<'a> {
    pub config_dir: &'a Path,
    /// The chat the line was typed in, when the page says which.
    pub conversation_id: Option<&'a str>,
    /// The folder the chat works in now.
    pub bound: &'a str,
    /// Which chats a paired browser may see; `None` on this computer.
    pub owned: Option<&'a dyn Fn(&str) -> bool>,
}

/// An effect, and the folder the chat works in afterwards when it changed
/// (`Some("")` is unbound).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub effect: Effect,
    pub workspace: Option<String>,
}

fn notice(text: impl Into<String>) -> Result<Applied, String> {
    Ok(Applied {
        effect: Effect::Notice { text: text.into() },
        workspace: None,
    })
}

fn confirm(prompt: String, yes: &str) -> Result<Applied, String> {
    Ok(Applied {
        effect: Effect::Confirm {
            prompt,
            yes: yes.into(),
        },
        workspace: None,
    })
}

fn chat<'a>(cx: &Ctx<'a>) -> Result<&'a str, String> {
    let id = cx.conversation_id.ok_or(NO_CHAT)?;
    if cx.owned.is_some_and(|owned| !owned(id)) {
        return Err(crate::remote::NOT_YOURS.into());
    }
    Ok(id)
}

fn saved(cx: &Ctx, id: &str) -> Result<grokhub_agent::SessionInfo, String> {
    if !id.starts_with(history::PREFIX) {
        return Err(history::NOT_SHELL.into());
    }
    let _config = ConfigGuard::set(cx.config_dir);
    grokhub_agent::load_session(id).map_err(|_| NO_CHAT.to_string())
}

pub fn run(op: Op, cx: &Ctx) -> Result<Applied, String> {
    let work = crate::agent::workspace_for(cx.bound);
    match op {
        Op::Rename(title) => {
            let id = chat(cx)?;
            saved(cx, id)?;
            history::rename(cx.config_dir, id, &title)?;
            notice(format!("Renamed to {}", title.trim()))
        }
        Op::Pin => {
            let id = chat(cx)?;
            let info = saved(cx, id)?;
            let pinned = history::toggle_pin(cx.config_dir, id)?;
            notice(format!(
                "{} {}",
                if pinned { "Pinned" } else { "Unpinned" },
                info.title
            ))
        }
        Op::Delete { yes } => {
            let id = chat(cx)?;
            let info = saved(cx, id)?;
            if !yes {
                return confirm(
                    format!(
                        "Delete {}? The chat and its files are removed for good.",
                        info.title
                    ),
                    "/delete yes",
                );
            }
            history::delete(cx.config_dir, id)?;
            Ok(Applied {
                effect: Effect::Deleted {
                    id: id.to_string(),
                    title: info.title,
                },
                workspace: None,
            })
        }
        Op::Sessions => match cx.owned {
            None => Ok(Applied {
                effect: Effect::Open { view: "history" },
                workspace: None,
            }),
            Some(owned) => {
                let rows: Vec<String> = history::list(cx.config_dir)
                    .into_iter()
                    .filter(|r| owned(&r.id))
                    .take(10)
                    .map(|r| format!("{}{}", if r.pinned { "📌 " } else { "" }, r.title))
                    .collect();
                notice(if rows.is_empty() {
                    "No saved chats from this browser yet.".to_string()
                } else {
                    rows.join("\n")
                })
            }
        },
        Op::Context => {
            let info = saved(cx, chat(cx)?)?;
            let pairs = grokhub_agent::transcript_pairs(&info);
            let turns = grokhub_core::visible_turn_count(&pairs);
            let tokens = grokhub_core::estimate_messages(&pairs);
            notice(format!(
                "{turns} {} · {tokens} tokens · {}%",
                if turns == 1 { "turn" } else { "turns" },
                grokhub_core::context_percent(tokens, grokhub_core::CONTEXT_BUDGET_TOKENS)
            ))
        }
        Op::Usage => {
            let info = saved(cx, chat(cx)?)?;
            let u = &info.usage;
            let line = grokhub_agent::usage_label(
                u.input_tokens,
                u.output_tokens,
                u.reasoning_tokens,
                u.cost_in_usd_ticks,
                &info.meter,
            );
            notice(if line.is_empty() {
                "No usage recorded for this chat yet.".to_string()
            } else {
                line
            })
        }
        Op::RewindNone => notice(REWIND_NA),
        Op::RewindList => {
            let id = chat(cx)?;
            let _config = ConfigGuard::set(cx.config_dir);
            notice(grokhub_agent::format_steps(id))
        }
        Op::RewindStep(n) => {
            let id = chat(cx)?;
            let _config = ConfigGuard::set(cx.config_dir);
            notice(grokhub_agent::restore_step(id, n)?)
        }
        Op::RewindFiles { yes } => {
            let id = chat(cx)?;
            let _config = ConfigGuard::set(cx.config_dir);
            let steps = grokhub_agent::list_steps(id);
            let last = steps
                .last()
                .ok_or("No agent file checkpoints yet. Edit a file in this chat first.")?;
            if !yes {
                return confirm(
                    format!(
                        "Put back {} as they were before step {}? Today's versions are saved first, so /rewind step can undo it.",
                        last.files.join(", "),
                        last.step
                    ),
                    "/rewind files yes",
                );
            }
            notice(grokhub_agent::restore_step(id, last.step)?)
        }
        Op::Recall(query) => {
            let _config = ConfigGuard::set(cx.config_dir);
            notice(crate::memory::recall(&work, &query)?)
        }
        Op::Forget { topic, yes } => {
            let _config = ConfigGuard::set(cx.config_dir);
            let tail = topic
                .as_deref()
                .map(|t| format!(" {t}"))
                .unwrap_or_default();
            if !yes {
                return match crate::memory::forget_plan(topic.as_deref())? {
                    crate::memory::Plan::Nothing(text) => notice(text),
                    crate::memory::Plan::Ask(prompt) => {
                        confirm(prompt, &format!("/forget --yes{tail}"))
                    }
                };
            }
            notice(crate::memory::forget(topic.as_deref())?)
        }
        Op::MemoryPrune { yes } => {
            let _config = ConfigGuard::set(cx.config_dir);
            if !yes {
                return match crate::memory::prune_plan()? {
                    crate::memory::Plan::Nothing(text) => notice(text),
                    crate::memory::Plan::Ask(prompt) => confirm(prompt, "/memory prune yes"),
                };
            }
            notice(crate::memory::prune()?)
        }
        Op::MemoryDream => {
            let _config = ConfigGuard::set(cx.config_dir);
            notice(crate::memory::dream_report(cx.config_dir))
        }
        Op::Dream => {
            let _config = ConfigGuard::set(cx.config_dir);
            notice(crate::memory::dream_now(cx.config_dir)?)
        }
        Op::Learned(rest) => {
            let _config = ConfigGuard::set(cx.config_dir);
            notice(grokhub_core::learn::view::run(
                &rest,
                grokhub_core::now_ms(),
            ))
        }
        Op::Skill(name) => {
            let _config = ConfigGuard::set(cx.config_dir);
            let home = grokhub_core::user_home();
            let page = crate::skills::load(cx.config_dir, &work, home.as_deref());
            let want = name.trim().trim_start_matches('/');
            let row = page.rows.iter().find(|r| {
                r.name.eq_ignore_ascii_case(want)
                    || r.trigger
                        .trim()
                        .trim_start_matches('/')
                        .eq_ignore_ascii_case(want)
            });
            match row {
                Some(r) if r.on => Ok(Applied {
                    effect: Effect::Mode {
                        mode: "chat",
                        text: Some(grokhub_core::skill_use_in_chat_prompt(&r.trigger, &r.name)),
                    },
                    workspace: None,
                }),
                Some(r) => Err(format!("{} is switched off. Turn it on in Skills.", r.name)),
                None => Err(format!("No skill {}", name.trim())),
            }
        }
        Op::SkillChanges => notice(crate::changes::report(
            cx.config_dir,
            ChangeKind::Skill,
            grokhub_core::now_ms(),
        )),
        Op::ConnectionChanges => notice(crate::changes::report(
            cx.config_dir,
            ChangeKind::Connection,
            grokhub_core::now_ms(),
        )),
        Op::AutomationChanges => notice(crate::changes::report(
            cx.config_dir,
            ChangeKind::Automation,
            grokhub_core::now_ms(),
        )),
        Op::SkillUndo(name) => notice(crate::changes::undo(
            cx.config_dir,
            ChangeKind::Skill,
            &name,
            false,
        )?),
        Op::SkillRestore(name) => {
            notice(crate::changes::restore_skill(cx.config_dir, &name, false)?)
        }
        Op::Privacy => notice(privacy_text(cx.config_dir)),
        Op::Brief => notice(brief_text(cx.config_dir)),
        Op::Health => notice(health_text(cx.config_dir)),
        Op::Project(op) => {
            let done = projects::run(cx.config_dir, cx.bound, op)?;
            if let Some(prompt) = done.ask {
                return confirm(prompt, "/project delete yes");
            }
            Ok(Applied {
                effect: Effect::Notice { text: done.text },
                workspace: done.workspace,
            })
        }
    }
}

/// `/privacy`: the grants that are on and what left this computer in the
/// last week, by destination. No content: the ledger and the log hold none.
fn privacy_text(config_dir: &Path) -> String {
    use grokhub_agent::harness as hx;
    let now = grokhub_core::now_ms();
    let ledger = hx::ConsentLedger::load(config_dir);
    let mut out = vec![
        "/privacy — what leaves this computer".to_string(),
        String::new(),
        format!(
            "Allowed by default: {} (chats and memory in model prompts), and chat text elsewhere, logged below. Sending your memory anywhere else waits for your OK: a hard card, or a grant in Settings → Permissions.",
            grokhub_core::DEFAULT_CONNECTOR_HOSTS.join(", ")
        ),
        String::new(),
        "Grants".to_string(),
    ];
    let mut any = false;
    for g in ledger.active() {
        any = true;
        let what = if g.destination.is_empty() {
            &g.source
        } else {
            &g.destination
        };
        out.push(format!(
            "- {what}: on since {}",
            grokhub_core::pulse::ago_label(g.granted_at, now)
        ));
    }
    if !any {
        out.push("- None on. Every grant is off.".to_string());
    }
    out.push(String::new());
    out.push("Sent in the last 7 days (no content stored)".to_string());
    let read = hx::read_egress_report(config_dir);
    let hosts = crate::privacy::week_hosts(&read.lines, now);
    if read.locked.is_some() {
        out.push("- The off-device log is sealed and your keyring is locked.".to_string());
    } else if hosts.is_empty() {
        out.push("- Nothing logged yet.".to_string());
    }
    for h in hosts {
        out.push(format!(
            "- {} · {} {}",
            h.host,
            h.calls,
            if h.calls == 1 { "time" } else { "times" }
        ));
    }
    out.join("\n")
}

/// `/brief`: the newest weekly systems briefing the cabin saved.
fn brief_text(config_dir: &Path) -> String {
    #[derive(serde::Deserialize, Default)]
    struct Saved {
        latest_md: Option<String>,
    }
    std::fs::read_to_string(config_dir.join("systems_brief.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
        .and_then(|s| s.latest_md)
        .filter(|md| !md.trim().is_empty())
        .unwrap_or_else(|| "No briefing yet.".into())
}

/// `/health`: failing automations and the last dream. The update line points
/// at Settings, since this app updates itself there.
fn health_text(config_dir: &Path) -> String {
    let automations = crate::automations::load(config_dir).unwrap_or_default();
    let failed: Vec<String> = automations
        .iter()
        .filter_map(|a| {
            grokhub_core::automation_health_line(a)
                .map(|line| format!("Automation {}: {line}", a.name))
        })
        .collect();
    let input = grokhub_core::health::HealthInput {
        cabin_update: None,
        failed,
        checked: automations.len(),
        last_dream: grokhub_core::amr::latest_dream(&config_dir.join("amr")),
    };
    grokhub_core::health::health_block(&input).replace(
        "Updates: none pending.",
        "Updates: Settings → Updates checks for a new version.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slash::{self, Action};

    struct Home {
        config: std::path::PathBuf,
        ws: std::path::PathBuf,
        _guard: ConfigGuard,
    }

    fn home(name: &str) -> Home {
        let root = std::env::temp_dir().join(format!(
            "grokhub-shell-slash2-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (config, ws) = (root.join("cfg"), root.join("ws"));
        std::fs::create_dir_all(&config).expect("cfg");
        std::fs::create_dir_all(&ws).expect("ws");
        Home {
            _guard: ConfigGuard::set(&config),
            config,
            ws,
        }
    }

    fn go(h: &Home, line: &str) -> Result<Effect, String> {
        let action = slash::parse(line, false)?;
        match action {
            Action::Op(op) => {
                let bound = h.ws.display().to_string();
                run(
                    op,
                    &Ctx {
                        config_dir: &h.config,
                        conversation_id: None,
                        bound: &bound,
                        owned: None,
                    },
                )
                .map(|a| a.effect)
            }
            other => slash::apply(other, &h.ws),
        }
    }

    fn text(effect: Result<Effect, String>) -> String {
        match effect {
            Ok(Effect::Notice { text }) => text,
            other => panic!("not a notice: {other:?}"),
        }
    }

    #[test]
    fn remember_then_recall_finds_the_line() {
        let h = home("recall");
        text(go(&h, "/remember the ferry leaves at nine"));
        let found = text(go(&h, "/recall ferry"));
        assert!(found.starts_with("Recalled for ferry\n- "), "{found}");
        assert!(found.contains("the ferry leaves at nine"), "{found}");
        assert_eq!(
            text(go(&h, "/recall zeppelin")),
            "Nothing in memory matches zeppelin."
        );
        assert_eq!(
            go(&h, "/recall"),
            Err("Type what to recall after /recall.".to_string())
        );
    }

    #[test]
    fn forget_asks_first_and_changes_nothing_without_a_yes() {
        let h = home("forget");
        crate::memory::add(&h.ws, "the harbor light is green", true).expect("add");
        crate::memory::add(&h.ws, "tea at four", true).expect("add");
        let file = grokhub_agent::global_memory_path();

        let asked = go(&h, "/forget harbor").expect("asks");
        let Effect::Confirm { prompt, yes } = asked else {
            panic!("expected a question");
        };
        assert!(prompt.starts_with("Forget harbor? These go:\n"), "{prompt}");
        assert!(
            prompt.contains("MEMORY.md: the harbor light is green"),
            "{prompt}"
        );
        assert!(!prompt.contains("tea at four"), "{prompt}");
        assert_eq!(yes, "/forget --yes harbor");
        let kept = std::fs::read_to_string(&file).expect("read");
        assert!(kept.contains("the harbor light is green"), "{kept}");

        let done = text(go(&h, &yes));
        assert!(done.starts_with("Forgot harbor"), "{done}");
        let after = std::fs::read_to_string(&file).expect("read");
        assert!(!after.contains("harbor"), "{after}");
        assert!(after.contains("tea at four"), "{after}");
        assert_eq!(
            text(go(&h, "/forget harbor")),
            "Nothing in memory mentions harbor."
        );
    }

    #[test]
    fn a_bare_forget_empties_memory_md_only_after_a_yes() {
        let h = home("forget-all");
        crate::memory::add(&h.ws, "one", true).expect("add");
        crate::memory::add(&h.ws, "two", true).expect("add");
        let asked = go(&h, "/forget").expect("asks");
        assert_eq!(
            asked,
            Effect::Confirm {
                prompt: "Empty MEMORY.md? All 2 lines go. Learned lines stay; /forget <topic> removes those.".into(),
                yes: "/forget --yes".into(),
            }
        );
        assert_eq!(text(go(&h, "/forget --yes")), "Forgot MEMORY.md");
        assert_eq!(
            std::fs::read_to_string(grokhub_agent::global_memory_path()).expect("read"),
            ""
        );
        assert_eq!(text(go(&h, "/forget")), "MEMORY.md is already empty.");
    }

    #[test]
    fn prune_with_nothing_old_says_what_it_kept() {
        let h = home("prune");
        assert_eq!(
            text(go(&h, "/memory prune")),
            "Memory retention: retired 0 unsure notes not updated in 90 days, kept 0."
        );
        assert_eq!(
            text(go(&h, "/memory prune yes")),
            "Memory retention: retired 0 unsure notes not updated in 90 days, kept 0."
        );
    }

    #[test]
    fn prune_asks_with_the_notes_it_would_retire_and_keeps_them_until_yes() {
        let h = home("prune-old");
        let store = grokhub_agent::amr_store();
        store.init().expect("init");
        let old = grokhub_core::now_ms() - 200 * 86_400_000;
        let id = grokhub_core::amr::add_fact(
            &store,
            "maybe likes jazz",
            grokhub_core::amr::PersonalGroup::Likes,
            old,
        )
        .expect("fact");
        let _ = id;
        let before = grokhub_core::amr::memory_rows(&store).0.len();
        match go(&h, "/memory prune").expect("plan") {
            Effect::Confirm { prompt, yes } => {
                assert!(prompt.starts_with("Retire "), "{prompt}");
                assert_eq!(yes, "/memory prune yes");
            }
            Effect::Notice { text } => {
                // A fact the user wrote is sure, so retention keeps it.
                assert!(text.contains("retired 0"), "{text}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(grokhub_core::amr::memory_rows(&store).0.len(), before);
    }

    #[test]
    fn memory_dream_reads_the_newest_report_or_says_none() {
        let h = home("dream");
        assert!(
            text(go(&h, "/memory dream")).starts_with("No dream yet. "),
            "none yet"
        );
        let report = text(go(&h, "/dream"));
        assert!(report.starts_with("Memory dream "), "{report}");
        assert!(!text(go(&h, "/memory dream")).starts_with("No dream yet"));
    }

    #[test]
    fn the_ledger_reports_read_from_the_config_folder() {
        let h = home("ledger");
        assert!(text(go(&h, "/skills changes")).contains("- Nothing yet. When "));
        assert!(text(go(&h, "/connections changes")).starts_with("/connections changes — what "));
        assert!(text(go(&h, "/automations changes")).starts_with("/automations changes — what "));
        assert_eq!(
            go(&h, "/skills undo ghost"),
            Err("No change to undo on ghost".to_string())
        );
    }

    #[test]
    fn brief_and_health_and_privacy_answer_on_a_fresh_profile() {
        let h = home("misc");
        assert_eq!(text(go(&h, "/brief")), "No briefing yet.");
        let health = text(go(&h, "/health"));
        assert!(health.starts_with("Health\n\nUpdates: "), "{health}");
        assert!(health.contains("Services: all 0 ok."), "{health}");
        let privacy = text(go(&h, "/privacy"));
        assert!(
            privacy.starts_with("/privacy — what leaves this computer"),
            "{privacy}"
        );
        assert!(
            privacy.contains("- None on. Every grant is off."),
            "{privacy}"
        );
        std::fs::write(
            h.config.join("systems_brief.json"),
            "{\"latest_md\":\"Week in review\"}",
        )
        .expect("write");
        assert_eq!(text(go(&h, "/brief")), "Week in review");
    }

    #[test]
    fn a_skill_runs_by_name_and_an_unknown_one_errors() {
        let h = home("skill");
        let folder = h.config.join("skills").join("tidy-desk");
        std::fs::create_dir_all(&folder).expect("dir");
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: tidy-desk\ndescription: Clean the desk\n---\n\n# tidy-desk\n\n## Steps\nwipe it\n",
        )
        .expect("skill");
        match go(&h, "/skill tidy-desk") {
            Ok(Effect::Mode { mode, text }) => {
                assert_eq!(mode, "chat");
                assert!(text.is_some_and(|t| t.contains("tidy-desk")));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(go(&h, "/skill nope"), Err("No skill nope".to_string()));
    }
}
