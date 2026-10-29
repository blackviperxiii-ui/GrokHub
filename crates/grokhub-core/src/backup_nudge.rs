//! Gentle backup and unpushed-work reminders. Opt-in. Signals are metadata
//! only: git ahead counts and the age of the oldest unpushed commit (from
//! `code_resume`), the newest modified time under folders the user marked
//! important, and when a detected backup tool last ran (from its snapshot
//! folders' names and times). Nothing is copied or uploaded, and no file is
//! opened.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chips::{ChipKind, QuickChip};
use crate::code_resume::RepoStatus;

pub const FILE_NAME: &str = "backup-nudge.json";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Unpushed this long earns a reminder.
pub const UNPUSHED_DAYS: u64 = 3;
/// A folder changed since a backup this old earns a reminder.
pub const FOLDER_DAYS: u64 = 7;
/// No system snapshot for this long earns a reminder.
pub const SNAPSHOT_DAYS: u64 = 14;
pub const SNOOZE_DAYS: u64 = 3;
/// Files looked at (by modified time) under one important folder.
const WALK_MAX: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupTool {
    Timeshift,
    Snapper,
    FileHistory,
}

impl BackupTool {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Timeshift => "Timeshift",
            Self::Snapper => "Snapper",
            Self::FileHistory => "File History",
        }
    }

    /// The shell line that opens the tool.
    pub fn open_line(&self) -> &'static str {
        match self {
            Self::Timeshift => "timeshift-launcher",
            Self::Snapper => "snapper list",
            Self::FileHistory => "control /name Microsoft.FileHistory",
        }
    }
}

/// A backup tool and when it last made a snapshot (Unix ms; 0 when never).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRun {
    pub tool: BackupTool,
    pub last_ms: u64,
}

fn newest_child_ms(dir: &Path) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| modified_ms(&e.path()))
        .max()
}

fn modified_ms(path: &Path) -> Option<u64> {
    let at = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(at.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// Timeshift and Snapper under `root` (`/` on Linux), File History under
/// `local_app_data` on Windows. Snapshot folders count by their modified time.
pub fn detect_tools(root: &Path, local_app_data: Option<&Path>) -> Vec<ToolRun> {
    let mut out = Vec::new();
    for dir in [
        root.join("timeshift/snapshots"),
        root.join("run/timeshift/backup/timeshift/snapshots"),
    ] {
        if dir.is_dir() {
            out.push(ToolRun {
                tool: BackupTool::Timeshift,
                last_ms: newest_child_ms(&dir).unwrap_or(0),
            });
            break;
        }
    }
    let snapper = root.join(".snapshots");
    if snapper.is_dir() {
        out.push(ToolRun {
            tool: BackupTool::Snapper,
            last_ms: newest_child_ms(&snapper).unwrap_or(0),
        });
    }
    if let Some(local) = local_app_data {
        let config = local.join("Microsoft/Windows/FileHistory/Configuration");
        if config.is_dir() {
            out.push(ToolRun {
                tool: BackupTool::FileHistory,
                last_ms: newest_child_ms(&config).unwrap_or(0),
            });
        }
    }
    out
}

/// The newest modified time of any file under `dir`, by metadata only.
pub fn newest_change_ms(dir: &Path) -> u64 {
    let mut newest = 0;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > WALK_MAX {
                return newest;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    stack.push(entry.path());
                }
            } else if kind.is_file() {
                newest = newest.max(modified_ms(&entry.path()).unwrap_or(0));
            }
        }
    }
    newest
}

/// Snooze and once-a-day state, in `{config}/backup-nudge.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeState {
    #[serde(default)]
    pub snoozed_until_ms: u64,
    #[serde(default)]
    pub last_shown_ms: u64,
    /// The reminder on the Pulse post, so its one-taps come back after a
    /// restart. Snooze clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder: Option<Reminder>,
}

impl NudgeState {
    pub fn may_show(&self, now_ms: u64) -> bool {
        now_ms >= self.snoozed_until_ms && now_ms.saturating_sub(self.last_shown_ms) >= DAY_MS
    }

    pub fn snooze(&mut self, now_ms: u64) {
        self.snoozed_until_ms = now_ms + SNOOZE_DAYS * DAY_MS;
    }
}

pub fn load(config_dir: &Path) -> NudgeState {
    std::fs::read_to_string(config_dir.join(FILE_NAME))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, state: &NudgeState) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&config_dir.join(FILE_NAME), &text)
}

/// One reminder: what is at risk, and the one-taps that help.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    pub lines: Vec<String>,
    /// The repo to push: (path, branch).
    pub push: Option<(PathBuf, String)>,
    pub tool: Option<BackupTool>,
}

fn days(ms: u64) -> u64 {
    ms / DAY_MS
}

fn plural(n: u64, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// What deserves a reminder now. `folders` are (path, newest change ms).
pub fn reminder(
    repos: &[RepoStatus],
    folders: &[(PathBuf, u64)],
    tools: &[ToolRun],
    now_ms: u64,
) -> Option<Reminder> {
    let mut out = Reminder::default();
    let mut stale: Vec<&RepoStatus> = repos
        .iter()
        .filter(|r| {
            r.ahead > 0
                && r.oldest_unpushed_s > 0
                && now_ms.saturating_sub(r.oldest_unpushed_s * 1000) >= UNPUSHED_DAYS * DAY_MS
        })
        .collect();
    stale.sort_by_key(|r| r.oldest_unpushed_s);
    for r in &stale {
        out.lines.push(format!(
            "{} has {} on {} not pushed for {}.",
            r.name,
            plural(u64::from(r.ahead), "commit"),
            r.branch,
            plural(
                days(now_ms.saturating_sub(r.oldest_unpushed_s * 1000)),
                "day"
            )
        ));
    }
    out.push = stale.first().map(|r| (r.path.clone(), r.branch.clone()));
    let best = tools.iter().max_by_key(|t| t.last_ms);
    let last_backup = best.map_or(0, |t| t.last_ms);
    for (path, changed) in folders {
        let behind =
            *changed > last_backup && now_ms.saturating_sub(last_backup) >= FOLDER_DAYS * DAY_MS;
        if behind {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            out.lines.push(match best {
                Some(t) if t.last_ms > 0 => format!(
                    "{name} changed since {}'s last snapshot {} ago.",
                    t.tool.name(),
                    plural(days(now_ms - t.last_ms), "day")
                ),
                Some(t) => format!("{name} changed and {} has no snapshots yet.", t.tool.name()),
                None => format!("{name} changed and no backup tool was found."),
            });
        }
    }
    if let Some(t) = best {
        if now_ms.saturating_sub(t.last_ms) >= SNAPSHOT_DAYS * DAY_MS
            && !out.lines.iter().any(|l| l.contains(t.tool.name()))
        {
            out.lines.push(if t.last_ms == 0 {
                format!("{} has no snapshots yet.", t.tool.name())
            } else {
                format!(
                    "No {} snapshot for {}.",
                    t.tool.name(),
                    plural(days(now_ms - t.last_ms), "day")
                )
            });
        }
        out.tool = Some(t.tool.clone());
    }
    (!out.lines.is_empty()).then_some(out)
}

pub const SNOOZE_ID: &str = "backup:snooze";
/// `source_id` prefix of the reminder's Pulse post.
pub const CARD_SOURCE: &str = "backup-nudge:";

impl Reminder {
    pub fn title(&self) -> String {
        if self.lines.len() == 1 {
            "Some work isn't backed up".into()
        } else {
            format!("{} things aren't backed up", self.lines.len())
        }
    }

    pub fn body(&self) -> String {
        self.lines.join(" ")
    }

    pub fn card(&self, now_ms: u64) -> crate::UpdateCard {
        let day = now_ms / DAY_MS;
        let mut card = crate::update_feed::digest_card(
            &format!("{CARD_SOURCE}{day}"),
            &self.title(),
            &self.body(),
            now_ms,
        );
        card.pulse.because = Some("You turned on backup reminders".into());
        card
    }

    /// Push (through the agent, which asks first), open the backup tool,
    /// and snooze.
    pub fn chips(&self) -> Vec<QuickChip> {
        let hint = self.body();
        let chip = |id: String, label: String, value: String, kind: ChipKind| QuickChip {
            id,
            label,
            value,
            kind,
            score: 1.0,
            hint: hint.clone(),
            primary: false,
        };
        let mut out = Vec::new();
        if let Some((path, branch)) = &self.push {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            out.push(chip(
                format!("backup:push:{name}"),
                format!("Push {name}"),
                format!(
                    "Push branch {branch} in {} to its upstream. Show me what will be pushed and ask before pushing.",
                    path.to_string_lossy()
                ),
                ChipKind::Chat,
            ));
        }
        if let Some(tool) = &self.tool {
            out.push(chip(
                format!("backup:open:{}", tool.name()),
                format!("Open {}", tool.name()),
                tool.open_line().into(),
                ChipKind::Shell,
            ));
        }
        out.push(chip(
            SNOOZE_ID.into(),
            format!("Snooze {SNOOZE_DAYS} days"),
            "Snooze backup reminders".into(),
            ChipKind::Chat,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn repo(name: &str, ahead: u32, unpushed_days_ago: u64) -> RepoStatus {
        RepoStatus {
            path: PathBuf::from(format!("/home/user/code/{name}")),
            name: name.into(),
            branch: "feat-x".into(),
            ahead,
            has_upstream: true,
            oldest_unpushed_s: (NOW - unpushed_days_ago * DAY_MS) / 1000,
            ..Default::default()
        }
    }

    fn tmp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-backup-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn old_unpushed_work_reminds_and_fresh_work_does_not() {
        assert_eq!(reminder(&[repo("my-app", 2, 1)], &[], &[], NOW), None);
        let got = reminder(&[repo("my-app", 2, 4), repo("tools", 0, 9)], &[], &[], NOW).unwrap();
        assert_eq!(
            got.lines,
            vec!["my-app has 2 commits on feat-x not pushed for 4 days.".to_string()]
        );
        assert_eq!(
            got.push,
            Some((PathBuf::from("/home/user/code/my-app"), "feat-x".into()))
        );
        let chips = got.chips();
        let labels: Vec<&str> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Push my-app", "Snooze 3 days"]);
        assert!(chips[0].value.contains("ask before pushing"));
        assert_eq!(
            chips[0].kind,
            ChipKind::Chat,
            "push goes through the agent's approval"
        );
    }

    #[test]
    fn folders_and_snapshots_compare_by_time_only() {
        let tools = [ToolRun {
            tool: BackupTool::Timeshift,
            last_ms: NOW - 20 * DAY_MS,
        }];
        let folders = [
            (PathBuf::from("/home/user/Documents"), NOW - DAY_MS),
            (PathBuf::from("/home/user/Old"), NOW - 30 * DAY_MS),
        ];
        let got = reminder(&[], &folders, &tools, NOW).unwrap();
        assert_eq!(
            got.lines,
            vec!["Documents changed since Timeshift's last snapshot 20 days ago.".to_string()]
        );
        assert_eq!(got.tool, Some(BackupTool::Timeshift));
        assert_eq!(got.chips()[0].value, "timeshift-launcher");
        let fresh = [ToolRun {
            tool: BackupTool::Snapper,
            last_ms: NOW - DAY_MS,
        }];
        assert_eq!(reminder(&[], &folders, &fresh, NOW), None);
        let none = reminder(&[], &folders[..1], &[], NOW).unwrap();
        assert_eq!(
            none.lines,
            vec!["Documents changed and no backup tool was found.".to_string()]
        );
        let old_only = [ToolRun {
            tool: BackupTool::Snapper,
            last_ms: NOW - 15 * DAY_MS,
        }];
        assert_eq!(
            reminder(&[], &[], &old_only, NOW).unwrap().lines,
            vec!["No Snapper snapshot for 15 days.".to_string()]
        );
        let never = [ToolRun {
            tool: BackupTool::FileHistory,
            last_ms: 0,
        }];
        assert_eq!(
            reminder(&[], &folders[..1], &never, NOW).unwrap().lines,
            vec!["Documents changed and File History has no snapshots yet.".to_string()],
            "a tool with no snapshot is not 'no tool'"
        );
    }

    #[test]
    fn the_reminder_round_trips_through_the_state_file() {
        let dir = tmp("state");
        let mut state = NudgeState {
            last_shown_ms: NOW,
            ..Default::default()
        };
        state.reminder = reminder(&[repo("my-app", 2, 4)], &[], &[], NOW);
        state.reminder.as_mut().unwrap().tool = Some(BackupTool::Snapper);
        save(&dir, &state).unwrap();
        assert_eq!(load(&dir), state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snooze_and_once_a_day() {
        let mut state = NudgeState::default();
        assert!(state.may_show(NOW));
        state.last_shown_ms = NOW;
        assert!(!state.may_show(NOW + DAY_MS - 1));
        assert!(state.may_show(NOW + DAY_MS));
        state.snooze(NOW);
        assert!(!state.may_show(NOW + 2 * DAY_MS));
        assert!(state.may_show(NOW + SNOOZE_DAYS * DAY_MS));
    }

    #[test]
    fn tools_and_folder_changes_come_from_metadata() {
        let root = tmp("tools");
        std::fs::create_dir_all(root.join("timeshift/snapshots/2026-10-01_00-00-01")).unwrap();
        std::fs::create_dir_all(root.join(".snapshots/12")).unwrap();
        let local = root.join("LocalAppData");
        std::fs::create_dir_all(local.join("Microsoft/Windows/FileHistory/Configuration")).unwrap();
        let tools = detect_tools(&root, Some(&local));
        let names: Vec<&str> = tools.iter().map(|t| t.tool.name()).collect();
        assert_eq!(names, vec!["Timeshift", "Snapper", "File History"]);
        assert!(tools[0].last_ms > 0);
        assert_eq!(tools[2].last_ms, 0, "no runs yet");
        assert!(detect_tools(&root.join("nothing"), None).is_empty());
        let docs = root.join("Documents");
        std::fs::create_dir_all(docs.join("deep/er")).unwrap();
        assert_eq!(newest_change_ms(&docs), 0);
        std::fs::write(docs.join("deep/er/plan.txt"), "private words").unwrap();
        assert!(newest_change_ms(&docs) > 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
