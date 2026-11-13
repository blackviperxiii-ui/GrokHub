//! What the Loadout Factorio mod wrote (`factorio_mod`), read back from
//! `script-output/loadout/state.json` in Factorio's user folder. The file is
//! data: it is size-capped, parsed as JSON and never run. A broken, partly
//! written or too-big file reads as nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::update_feed::{digest_card, UpdateCard};

/// Biggest state file read.
const MAX_FILE: u64 = 256 * 1024;

/// The mod's file under Factorio's user folder.
pub fn state_path(user_dir: &Path) -> PathBuf {
    user_dir.join("script-output/loadout/state.json")
}

/// The research in progress.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Research {
    pub name: String,
    #[serde(default)]
    pub progress: f64,
}

/// One summary the mod wrote.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct FactorioState {
    #[serde(default)]
    pub tick: u64,
    #[serde(default)]
    pub played_ticks: u64,
    #[serde(default)]
    pub player: Option<String>,
    #[serde(default)]
    pub researched: u32,
    #[serde(default)]
    pub research: Option<Research>,
    #[serde(default)]
    pub rockets: u64,
    #[serde(default)]
    pub deaths: u32,
    /// Output per minute of a few plates and circuits.
    #[serde(default)]
    pub items: BTreeMap<String, f64>,
}

/// Parse the mod's JSON. `None` for anything else.
pub fn parse(text: &str) -> Option<FactorioState> {
    #[derive(Deserialize)]
    struct Versioned {
        v: u32,
    }
    let v: Versioned = serde_json::from_str(text).ok()?;
    if v.v != 1 {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// Read the state file, size-capped.
pub fn read(user_dir: &Path) -> Option<FactorioState> {
    let path = state_path(user_dir);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    parse(&std::fs::read_to_string(path).ok()?)
}

/// `electronic-circuit` → `electronic circuit`.
fn item_name(id: &str) -> String {
    id.replace('-', " ")
}

/// `7h 5m`, from game ticks at 60 a second.
fn played(ticks: u64) -> String {
    let min = ticks / 3_600;
    match min / 60 {
        0 => format!("{min}m"),
        h => format!("{h}h {}m", min % 60),
    }
}

impl FactorioState {
    /// `42 technologies researched, researching logistics 2 (40%). 1 rocket launched. No deaths. 7h 5m played. Per minute: 120 iron plate, 30 electronic circuit.`
    pub fn state_line(&self) -> String {
        let mut parts = vec![format!(
            "{} {} researched",
            self.researched,
            if self.researched == 1 {
                "technology"
            } else {
                "technologies"
            }
        )];
        if let Some(r) = &self.research {
            let pct = (r.progress.clamp(0.0, 1.0) * 100.0).round();
            parts[0].push_str(&format!(", researching {} ({pct}%)", item_name(&r.name)));
        }
        if self.rockets > 0 {
            let rockets = if self.rockets == 1 {
                "rocket"
            } else {
                "rockets"
            };
            parts.push(format!("{} {rockets} launched", self.rockets));
        }
        parts.push(match self.deaths {
            0 => "No deaths".into(),
            1 => "1 death".into(),
            n => format!("{n} deaths"),
        });
        if self.played_ticks > 0 {
            parts.push(format!("{} played", played(self.played_ticks)));
        }
        let rates: Vec<String> = self
            .items
            .iter()
            .filter(|(_, n)| n.is_finite() && **n > 0.0)
            .map(|(id, n)| format!("{} {}", n.round(), item_name(id)))
            .collect();
        if !rates.is_empty() {
            parts.push(format!("Per minute: {}", rates.join(", ")));
        }
        parts.join(". ") + "."
    }
}

/// What one play session did: how the state stood when the game started
/// (`None` when there was no file yet) and when it closed.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorioSession {
    pub before: Option<FactorioState>,
    pub after: FactorioState,
}

impl FactorioSession {
    /// The session, or `None` when the mod wrote nothing new.
    pub fn between(before: Option<FactorioState>, after: Option<FactorioState>) -> Option<Self> {
        let after = after?;
        if before.as_ref() == Some(&after) {
            return None;
        }
        Some(Self { before, after })
    }

    fn gain<T: Copy + std::ops::Sub<Output = T> + PartialOrd + Default>(
        &self,
        f: impl Fn(&FactorioState) -> T,
    ) -> T {
        let after = f(&self.after);
        match &self.before {
            // A different save can be behind the one before.
            Some(b) if f(b) <= after => after - f(b),
            Some(_) => T::default(),
            None => after,
        }
    }

    /// `3 technologies researched, 1 rocket launched, 1 death.`
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let research = self.gain(|s| s.researched);
        if research > 0 {
            let t = if research == 1 {
                "technology"
            } else {
                "technologies"
            };
            parts.push(format!("{research} {t} researched"));
        }
        let rockets = self.gain(|s| s.rockets);
        if rockets > 0 {
            let r = if rockets == 1 { "rocket" } else { "rockets" };
            parts.push(format!("{rockets} {r} launched"));
        }
        let deaths = self.gain(|s| s.deaths);
        parts.push(match deaths {
            0 => "no deaths".into(),
            1 => "1 death".into(),
            n => format!("{n} deaths"),
        });
        let mut line = parts.join(", ");
        if let Some(first) = line.get(..1) {
            line = first.to_uppercase() + &line[1..];
        }
        if let Some(r) = &self.after.research {
            line.push_str(&format!(". Now researching {}", item_name(&r.name)));
        }
        line + "."
    }
}

/// The session as a Pulse card, posted when the game closes. The player's
/// name stays out of it.
pub fn session_card(s: &FactorioSession, at_ms: u64) -> UpdateCard {
    let rockets = s.gain(|x| x.rockets);
    let title = match rockets {
        0 => "Factorio session".to_string(),
        1 => "Factorio session: a rocket launched".to_string(),
        n => format!("Factorio session: {n} rockets launched"),
    };
    digest_card("source:game-watch:factorio", &title, &s.summary(), at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = r#"{"v":1,"tick":1530000,"played_ticks":1530000,"player":"Testengineer","researched":42,"research":{"name":"logistics-2","progress":0.4},"rockets":1,"deaths":0,"items":{"electronic-circuit":30.2,"iron-plate":120}}"#;

    #[test]
    fn the_mods_file_parses_and_reads_plainly() {
        let s = parse(STATE).unwrap();
        assert_eq!(s.researched, 42);
        assert_eq!(s.player.as_deref(), Some("Testengineer"));
        assert_eq!(
            s.state_line(),
            "42 technologies researched, researching logistics 2 (40%). 1 rocket launched. No deaths. 7h 5m played. Per minute: 30 electronic circuit, 120 iron plate."
        );
    }

    #[test]
    fn broken_foreign_or_too_big_files_read_as_nothing() {
        assert_eq!(parse(&STATE[..STATE.len() / 2]), None);
        assert_eq!(parse(r#"{"v":2,"researched":1}"#), None);
        assert_eq!(parse(r#"{"researched":1}"#), None);
        assert_eq!(parse("[]"), None);
        let dir =
            std::env::temp_dir().join(format!("loadout-factorio-state-{}", std::process::id()));
        let path = state_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, STATE).unwrap();
        assert_eq!(read(&dir).map(|s| s.rockets), Some(1));
        std::fs::write(&path, " ".repeat(MAX_FILE as usize + 1)).unwrap();
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_session_counts_what_changed_since_launch() {
        let before = parse(STATE).unwrap();
        assert_eq!(
            FactorioSession::between(Some(before.clone()), Some(before.clone())),
            None
        );
        assert_eq!(FactorioSession::between(Some(before.clone()), None), None);
        let mut after = before.clone();
        after.researched = 45;
        after.rockets = 3;
        after.deaths = 1;
        after.tick += 100;
        let s = FactorioSession::between(Some(before), Some(after.clone())).unwrap();
        assert_eq!(
            s.summary(),
            "3 technologies researched, 2 rockets launched, 1 death. Now researching logistics 2."
        );
        let card = session_card(&s, 1);
        assert_eq!(card.title, "Factorio session: 2 rockets launched");
        assert!(!card.body.as_deref().unwrap_or("").contains("Testengineer"));

        // Loading an earlier save counts no negative progress.
        let mut earlier = after.clone();
        earlier.researched = 10;
        earlier.rockets = 0;
        earlier.research = None;
        let back = FactorioSession::between(Some(after), Some(earlier)).unwrap();
        assert_eq!(back.summary(), "No deaths.");
    }
}
