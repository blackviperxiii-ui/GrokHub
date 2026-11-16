//! The Fleet page's machines: this computer's card, read when asked, and the
//! last card each paired computer sent to this computer's hub
//! (`POST /v1/fleet`). Nothing here leaves this computer.

use std::sync::Mutex;

use grokhub_core::fleet::MachineCard;
use serde::{Deserialize, Serialize};

/// A card and how the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    #[serde(flatten)]
    pub card: MachineCard,
    /// The card is under three minutes old; older ones show dimmed.
    pub online: bool,
    pub this_computer: bool,
}

/// This computer's card at `now`. CPU use is since the last call, so the
/// first one has none.
pub fn local_card(id: &str, name: &str, now: u64) -> MachineCard {
    MachineCard {
        id: id.to_string(),
        name: name.to_string(),
        cpu_pct: cpu_pct(),
        ram_mb: grokhub_agent::device::ram_mb(),
        free_ram_mb: grokhub_agent::device::free_ram_now(),
        app_version: crate::shell_update::version(),
        at_ms: now,
    }
}

/// The `/proc/stat` totals at the last call.
static CPU_PREV: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn cpu_pct() -> Option<u8> {
    let now = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|t| grokhub_core::pc_health::parse_proc_stat_cpu(&t))?;
    let mut prev = CPU_PREV.lock().unwrap_or_else(|e| e.into_inner());
    let pct = prev.and_then(|p| grokhub_core::pc_health::cpu_util_between(p, now));
    *prev = Some(now);
    pct
}

/// Not read on Windows and macOS yet; the page shows a dash.
#[cfg(not(target_os = "linux"))]
fn cpu_pct() -> Option<u8> {
    let _ = &CPU_PREV;
    None
}

/// This computer first, then the paired computers' cards: online ones before
/// asleep ones, each group by name. A held card with this computer's id is
/// left out, so a stale self-report never shows twice.
pub fn machines(local: MachineCard, held: Vec<MachineCard>, now: u64) -> Vec<Machine> {
    let mut others: Vec<Machine> = held
        .into_iter()
        .filter(|card| card.id != local.id)
        .map(|card| Machine {
            online: card.online(now),
            card,
            this_computer: false,
        })
        .collect();
    others.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.card.name.to_lowercase().cmp(&b.card.name.to_lowercase()))
    });
    let mine = Machine {
        online: true,
        card: local,
        this_computer: true,
    };
    std::iter::once(mine).chain(others).collect()
}

/// The subscription rows the app can read on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// From Claude Code's local transcripts; `None` when it isn't installed.
    pub claude: Option<grokhub_core::claude_usage::ClaudeUsage>,
    /// From the rate-limit headers on the last signed-in Grok reply; `None`
    /// before one this run.
    pub grok: Option<GrokRoom>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokRoom {
    pub requests_left: Option<u64>,
    pub requests_limit: Option<u64>,
    pub tokens_left: Option<u64>,
    pub tokens_limit: Option<u64>,
    pub resets_at_ms: Option<u64>,
    pub seen_ms: u64,
}

impl From<grokhub_agent::plan_room::PlanRoom> for GrokRoom {
    fn from(room: grokhub_agent::plan_room::PlanRoom) -> Self {
        Self {
            requests_left: room.requests_left,
            requests_limit: room.requests_limit,
            tokens_left: room.tokens_left,
            tokens_limit: room.tokens_limit,
            resets_at_ms: room
                .reset_secs
                .map(|secs| room.seen_ms.saturating_add(secs.saturating_mul(1000))),
            seen_ms: room.seen_ms,
        }
    }
}

/// Claude Code's folder on this computer: `CLAUDE_CONFIG_DIR`, or `.claude`
/// in the home folder.
pub fn claude_dir() -> Option<std::path::PathBuf> {
    let env = std::env::var("CLAUDE_CONFIG_DIR").ok();
    grokhub_core::paths::user_home()
        .map(|home| grokhub_core::claude_usage::claude_dir(env.as_deref(), &home))
}

/// The rows at `now`, with Claude Code's folder at `claude_dir`.
pub fn usage(claude_dir: Option<&std::path::Path>, now: u64) -> Usage {
    use grokhub_core::claude_usage;
    let claude = claude_dir
        .filter(|dir| dir.join("projects").is_dir())
        .map(|dir| {
            claude_usage::usage(
                claude_usage::read_turns(dir, now),
                now,
                claude_usage::read_from(now),
            )
        });
    Usage {
        claude,
        grok: grokhub_agent::plan_room::latest().map(GrokRoom::from),
    }
}

/// Subscriptions with no usage the app can read: the user types what their
/// own dashboard shows, and the page keeps it here.
pub const MANUAL_ROWS: [&str; 2] = ["cursor", "antigravity"];
const ROWS_FILE: &str = "fleet-rows.json";

/// A row the user fills in: the short window and the longer period (a week or
/// a month), each as a percent used and when it resets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualRow {
    pub id: String,
    pub window_pct: Option<u8>,
    pub window_resets_at_ms: Option<u64>,
    pub period_pct: Option<u8>,
    pub period_resets_at_ms: Option<u64>,
    /// When the user last changed it, so the page can say how old it is.
    pub updated_at: u64,
}

/// One row per [`MANUAL_ROWS`] id, in that order, empty until filled in.
pub fn manual_rows(config_dir: &std::path::Path) -> Vec<ManualRow> {
    let saved: Vec<ManualRow> = std::fs::read_to_string(config_dir.join(ROWS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    MANUAL_ROWS
        .iter()
        .map(|id| {
            saved
                .iter()
                .find(|row| row.id == *id)
                .cloned()
                .unwrap_or(ManualRow {
                    id: id.to_string(),
                    window_pct: None,
                    window_resets_at_ms: None,
                    period_pct: None,
                    period_resets_at_ms: None,
                    updated_at: 0,
                })
        })
        .collect()
}

/// Save `row` at `now`; percents over 100 are kept as 100.
pub fn set_manual_row(
    config_dir: &std::path::Path,
    row: ManualRow,
    now: u64,
) -> Result<Vec<ManualRow>, String> {
    if !MANUAL_ROWS.contains(&row.id.as_str()) {
        return Err(format!("There's no {} row to fill in.", row.id));
    }
    let row = ManualRow {
        window_pct: row.window_pct.map(|p| p.min(100)),
        period_pct: row.period_pct.map(|p| p.min(100)),
        updated_at: now,
        ..row
    };
    let rows: Vec<ManualRow> = manual_rows(config_dir)
        .into_iter()
        .map(|old| if old.id == row.id { row.clone() } else { old })
        .collect();
    let text = serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join(ROWS_FILE), text).map_err(|e| e.to_string())?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, name: &str, at_ms: u64) -> MachineCard {
        MachineCard {
            id: id.into(),
            name: name.into(),
            cpu_pct: Some(12),
            ram_mb: Some(16_000),
            free_ram_mb: Some(4_000),
            app_version: "3.6.0".into(),
            at_ms,
        }
    }

    #[test]
    fn this_computer_leads_then_online_machines_by_name_then_asleep_ones() {
        let now = 1_000_000;
        let held = vec![
            card("d-zed", "zed", now - 10_000),
            card("d-old", "Attic", now - 600_000),
            card("d-me", "Stale me", now - 900_000),
            card("d-box", "Box", now - 1_000),
        ];
        let shown = machines(card("d-me", "Desk", now), held, now);
        let shown: Vec<(&str, bool, bool)> = shown
            .iter()
            .map(|m| (m.card.id.as_str(), m.online, m.this_computer))
            .collect();
        assert_eq!(
            shown,
            vec![
                ("d-me", true, true),
                ("d-box", true, false),
                ("d-zed", true, false),
                ("d-old", false, false),
            ]
        );
    }

    #[test]
    fn a_machine_reads_as_one_flat_record_for_the_page() {
        let shown = machines(card("d-me", "Desk", 5), vec![], 5);
        assert_eq!(
            serde_json::to_value(&shown).unwrap(),
            serde_json::json!([{
                "id": "d-me", "name": "Desk", "cpuPct": 12, "ramMb": 16000, "freeRamMb": 4000,
                "appVersion": "3.6.0", "atMs": 5, "online": true, "thisComputer": true
            }])
        );
    }

    #[test]
    fn this_computers_card_has_its_version_and_cpu_from_the_second_reading() {
        let first = local_card("d-me", "Desk", 7);
        assert_eq!(
            (first.id.as_str(), first.name.as_str(), first.at_ms),
            ("d-me", "Desk", 7)
        );
        assert_eq!(first.app_version, crate::shell_update::version());
        assert!(!first.app_version.is_empty());
        #[cfg(target_os = "linux")]
        {
            assert!(first.ram_mb.is_some_and(|mb| mb > 0), "{first:?}");
            std::thread::sleep(std::time::Duration::from_millis(200));
            let second = local_card("d-me", "Desk", 8);
            assert!(second.cpu_pct.is_some_and(|p| p <= 100), "{second:?}");
        }
    }

    #[test]
    fn grok_room_says_when_the_window_resets() {
        let room = grokhub_agent::plan_room::PlanRoom {
            requests_left: Some(40),
            requests_limit: Some(50),
            tokens_left: None,
            tokens_limit: None,
            reset_secs: Some(90),
            seen_ms: 10_000,
        };
        assert_eq!(
            GrokRoom::from(room),
            GrokRoom {
                requests_left: Some(40),
                requests_limit: Some(50),
                tokens_left: None,
                tokens_limit: None,
                resets_at_ms: Some(100_000),
                seen_ms: 10_000,
            }
        );
    }

    #[test]
    fn claude_shows_only_when_its_projects_folder_is_there() {
        let home = std::env::temp_dir().join(format!("fleet-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let dir = home.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(usage(Some(&dir), 5).claude, None);
        assert_eq!(usage(None, 5).claude, None);
        let project = home.join(".claude/projects/my-app");
        std::fs::create_dir_all(&project).unwrap();
        let now = grokhub_core::now_ms();
        let at = grokhub_core::oauth::unix_ms_to_rfc3339(now - 60_000);
        std::fs::write(
            project.join("s.jsonl"),
            format!(
                r#"{{"type":"assistant","timestamp":"{at}","requestId":"r1","message":{{"id":"m1","usage":{{"input_tokens":2,"output_tokens":30}}}}}}"#
            ),
        )
        .unwrap();
        let claude = usage(Some(&dir), now).claude.expect("claude row");
        assert_eq!((claude.window.output, claude.week.output), (30, 30));
        assert!(claude.window_resets_at_ms.is_some_and(|end| end > now));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn rows_to_fill_in_start_empty_and_keep_what_was_saved() {
        let dir = std::env::temp_dir().join(format!("fleet-rows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rows = manual_rows(&dir);
        let ids: Vec<(&str, Option<u8>, u64)> = rows
            .iter()
            .map(|r| (r.id.as_str(), r.window_pct, r.updated_at))
            .collect();
        assert_eq!(ids, vec![("cursor", None, 0), ("antigravity", None, 0)]);

        let saved = set_manual_row(
            &dir,
            ManualRow {
                id: "cursor".into(),
                window_pct: None,
                window_resets_at_ms: None,
                period_pct: Some(140),
                period_resets_at_ms: Some(1_800_000_000_000),
                updated_at: 1,
            },
            99,
        )
        .unwrap();
        assert_eq!(
            saved[0],
            ManualRow {
                id: "cursor".into(),
                window_pct: None,
                window_resets_at_ms: None,
                period_pct: Some(100),
                period_resets_at_ms: Some(1_800_000_000_000),
                updated_at: 99,
            }
        );
        assert_eq!(manual_rows(&dir), saved);
        assert_eq!(saved[1].updated_at, 0);
        assert_eq!(
            set_manual_row(
                &dir,
                ManualRow {
                    id: "other".into(),
                    ..saved[0].clone()
                },
                5
            ),
            Err("There's no other row to fill in.".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
