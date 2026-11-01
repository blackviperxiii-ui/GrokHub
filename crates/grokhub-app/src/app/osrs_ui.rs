//! Old School RuneScape (`grokhub_core::osrs_hiscores`).
//!
//! Under the game watcher in Settings: a display-name field and an account
//! type picker, then what Jagex's official hiscores show and the XP gained
//! over the last day. Reads at most once an hour and on Refresh, off the UI
//! thread and through the egress guard (so Offline only stops it). No key,
//! no plugin, nothing touches the game.

use std::sync::mpsc;

use super::*;
use grokhub_core::osrs_hiscores::{self as osrs, AccountType, OsrsSaved, OsrsStats};

#[derive(Default)]
pub(super) struct OsrsUi {
    saved: Option<OsrsSaved>,
    loaded: bool,
    name_draft: String,
    kind_draft: AccountType,
    pending: Option<mpsc::Receiver<Result<OsrsStats, String>>>,
    next_ms: u64,
    note: String,
}

fn api_agent() -> grokhub_core::http::Agent {
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: std::time::Duration::from_secs(8),
        overall: Some(std::time::Duration::from_secs(20)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

/// One read of the hiscores for a player.
fn fetch_stats(name: &str, kind: AccountType) -> Result<OsrsStats, String> {
    let url = osrs::url(name, kind);
    if cfg!(test) || !url.starts_with(osrs::HOST) {
        return Err("Not sent.".into());
    }
    crate::xai::egress_ok(&url, &[]).map_err(|_| {
        "Offline only or the network guard held the hiscores refresh back.".to_string()
    })?;
    let resp = api_agent().get(&url).call().map_err(|e| match e {
        grokhub_core::http::Error::Status { code: 404, .. } => format!(
            "Jagex's {} hiscores have no player named {name}. If you changed your name, enter the new one.",
            kind.label().to_lowercase()
        ),
        grokhub_core::http::Error::Status { code, .. } => {
            format!("The hiscores answered {code}; Loadout tries again later.")
        }
        _ => "Couldn't reach Jagex's hiscores.".into(),
    })?;
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), osrs::MAX_BODY),
        &mut text,
    )
    .map_err(|_| "The hiscores answer didn't arrive whole.".to_string())?;
    osrs::parse(&text, now_ms()).ok_or_else(|| "The hiscores sent something unexpected.".into())
}

impl Cabin {
    pub(super) fn tick_osrs(&mut self) {
        if !self.focus.osrs.loaded {
            self.focus.osrs.loaded = true;
            if !cfg!(test) {
                self.focus.osrs.saved = osrs::load(&crate::config::config_dir());
            }
        }
        if let Some(rx) = &self.focus.osrs.pending {
            match rx.try_recv() {
                Ok(done) => {
                    self.focus.osrs.pending = None;
                    self.finish_osrs(done);
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => self.focus.osrs.pending = None,
            }
        }
        if self.cfg.game_watch
            && self.focus.osrs.saved.is_some()
            && now_ms() >= self.focus.osrs.next_ms
        {
            self.start_osrs_refresh();
        }
    }

    fn start_osrs_refresh(&mut self) {
        let o = &mut self.focus.osrs;
        if o.pending.is_some() {
            return;
        }
        let Some(saved) = &o.saved else {
            return;
        };
        o.next_ms = now_ms() + osrs::REFRESH_MS;
        let (name, kind) = (saved.name.clone(), saved.kind);
        let (tx, rx) = mpsc::channel();
        o.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(fetch_stats(&name, kind));
        });
    }

    fn finish_osrs(&mut self, done: Result<OsrsStats, String>) {
        let o = &mut self.focus.osrs;
        let Some(saved) = o.saved.as_mut() else {
            return;
        };
        match done {
            Ok(stats) => {
                saved.push(stats);
                o.note.clear();
                if !cfg!(test) {
                    let _ = osrs::save(&crate::config::config_dir(), saved);
                }
            }
            Err(e) => o.note = e,
        }
    }

    /// The Old School RuneScape rows under the game watcher.
    pub(super) fn ui_osrs(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.game_watch {
            return;
        }
        let o = &self.focus.osrs;
        let line = osrs_status(o.saved.as_ref(), now_ms());
        let gains = o
            .saved
            .as_ref()
            .and_then(|s| osrs::gains_line(&s.snapshots));
        let note = o.note.clone();
        let busy = o.pending.is_some();
        let has_player = o.saved.is_some();
        let (mut save, mut refresh, mut forget) = (false, false, false);
        ui.indent("osrs-hiscores", |ui| {
            crate::cards::help_text(ui, &line);
            if let Some(g) = &gains {
                crate::cards::help_text(ui, g);
            }
            if has_player {
                ui.horizontal(|ui| {
                    if busy {
                        crate::cards::help_text(ui, "Refreshing...");
                    } else {
                        refresh = crate::cards::ghost_pill(ui, "Refresh hiscores");
                    }
                    forget = crate::cards::ghost_pill(ui, "Forget player");
                });
            } else {
                crate::cards::settings_field(
                    ui,
                    "Old School RuneScape name",
                    "Your display name on Jagex's public hiscores. No password or key.",
                    &mut self.focus.osrs.name_draft,
                    false,
                );
                egui::ComboBox::from_id_salt("osrs-kind")
                    .selected_text(self.focus.osrs.kind_draft.label())
                    .show_ui(ui, |ui| {
                        for k in AccountType::ALL {
                            ui.selectable_value(&mut self.focus.osrs.kind_draft, k, k.label());
                        }
                    });
                save = crate::cards::settings_action(
                    ui,
                    "Old School RuneScape name",
                    "Read from the hiscores at most once an hour.",
                    "Save name",
                );
            }
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
        });
        if save {
            self.save_osrs_name();
        }
        if refresh {
            self.focus.osrs.next_ms = 0;
            self.start_osrs_refresh();
        }
        if forget {
            self.forget_osrs();
        }
    }

    fn save_osrs_name(&mut self) {
        let o = &mut self.focus.osrs;
        let name = o.name_draft.trim().to_string();
        if !osrs::valid_name(&name) {
            o.note = "Old School names are up to 12 letters, numbers, spaces, - or _.".into();
            return;
        }
        o.saved = Some(OsrsSaved {
            name,
            kind: o.kind_draft,
            snapshots: Vec::new(),
        });
        o.name_draft.clear();
        o.next_ms = 0;
        o.note.clear();
    }

    fn forget_osrs(&mut self) {
        let o = &mut self.focus.osrs;
        o.saved = None;
        o.note = "Player forgotten.".into();
        if !cfg!(test) {
            let _ = std::fs::remove_file(osrs::saved_path(&crate::config::config_dir()));
        }
    }
}

/// `Old School RuneScape (Zezima, ironman): ... Updated 20 minutes ago.`
fn osrs_status(saved: Option<&OsrsSaved>, now: u64) -> String {
    let Some(s) = saved else {
        return "Old School RuneScape: add your display name to read Jagex's official hiscores."
            .into();
    };
    let who = match s.kind {
        AccountType::Normal => s.name.clone(),
        k => format!("{}, {}", s.name, k.label().to_lowercase()),
    };
    match s.latest() {
        None => format!("{} ({who}): nothing read yet.", osrs::NAME),
        Some(l) => {
            let mins = now.saturating_sub(l.fetched_ms) / 60_000;
            let age = match mins {
                0 => "just now".to_string(),
                1 => "a minute ago".to_string(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            format!("{} ({who}): {} Updated {age}.", osrs::NAME, l.stats_line())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LITE: &str = r#"{"skills":[{"id":0,"name":"Overall","rank":10,"level":500,"xp":100000},{"id":1,"name":"Attack","rank":5,"level":40,"xp":40000}],"activities":[]}"#;

    #[test]
    fn the_status_line_names_the_player_and_table() {
        assert!(osrs_status(None, 0).contains("add your display name"));
        let mut saved = OsrsSaved {
            name: "Zezima".into(),
            kind: AccountType::Ironman,
            snapshots: Vec::new(),
        };
        assert_eq!(
            osrs_status(Some(&saved), 0),
            "Old School RuneScape (Zezima, ironman): nothing read yet."
        );
        saved.push(osrs::parse(LITE, 0).unwrap());
        assert_eq!(
            osrs_status(Some(&saved), 61_000),
            "Old School RuneScape (Zezima, ironman): Total level 500, 100,000 XP, rank 10. Highest: Attack 40. Updated a minute ago."
        );
    }

    #[test]
    fn names_are_checked_and_reads_are_kept() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.osrs.name_draft = "far too long a name".into();
        cabin.save_osrs_name();
        assert!(cabin.focus.osrs.saved.is_none());
        cabin.focus.osrs.name_draft = "Zezima".into();
        cabin.save_osrs_name();
        assert_eq!(cabin.focus.osrs.saved.as_ref().unwrap().name, "Zezima");
        cabin.finish_osrs(Err(
            "Jagex's normal hiscores have no player named Zezima.".into()
        ));
        assert!(cabin.focus.osrs.note.contains("no player"));
        cabin.finish_osrs(Ok(osrs::parse(LITE, 5).unwrap()));
        assert_eq!(cabin.focus.osrs.saved.as_ref().unwrap().snapshots.len(), 1);
        assert!(cabin.focus.osrs.note.is_empty());
        cabin.forget_osrs();
        assert!(cabin.focus.osrs.saved.is_none());
    }

    #[test]
    fn nothing_is_sent_from_tests() {
        assert_eq!(
            fetch_stats("Zezima", AccountType::Normal),
            Err("Not sent.".into())
        );
    }
}
