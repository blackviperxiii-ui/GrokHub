//! Native plugins in Labs. Install, trust, and enable run off the UI thread.
//! A bundle contributes nothing until it is trusted for its current contents and enabled.
//! Fetching a marketplace index never installs a bundle.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use grokhub_agent::plugins::{MarketEntry, PluginInfo};

struct PaintState {
    rows: Vec<PluginInfo>,
    market: Vec<MarketEntry>,
    note: String,
    source: String,
    market_url: String,
    workspace: PathBuf,
    busy: bool,
    seeded: bool,
}

fn paint_state() -> &'static Mutex<PaintState> {
    static STATE: OnceLock<Mutex<PaintState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(PaintState {
            rows: Vec::new(),
            market: Vec::new(),
            note: String::new(),
            source: String::new(),
            market_url: String::new(),
            workspace: PathBuf::new(),
            busy: false,
            seeded: false,
        })
    })
}

enum Job {
    Install { workspace: PathBuf, source: String },
    Trust { workspace: PathBuf, name: String },
    Enable { workspace: PathBuf, name: String },
    Disable { workspace: PathBuf, name: String },
    Remove { workspace: PathBuf, name: String },
    Fetch { workspace: PathBuf, url: String },
    InstallEntry { workspace: PathBuf, source: String },
}

pub fn paint(ui: &mut eframe::egui::Ui, workspace: &Path) {
    ui.add_space(12.0);
    crate::cards::section_label(ui, "Plugins");
    crate::cards::help_text(
        ui,
        "Bundles stay off until you trust this version and enable them. Trust covers the hook commands, MCP server commands, skills, and agents listed here. Fetching a marketplace list does not install anything.",
    );
    ui.add_space(6.0);
    let workspace = workspace.to_path_buf();
    // Plugin discovery takes its own lock. Do not hold the paint lock across it.
    let reload = {
        let held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        !held.seeded || held.workspace != workspace
    };
    if reload {
        let rows = grokhub_agent::plugins::list_plugins(&workspace);
        let stored_url = grokhub_agent::plugins::marketplace_url();
        let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        if !held.seeded || held.workspace != workspace {
            held.rows = rows;
            if held.market_url.is_empty() {
                held.market_url = stored_url;
            }
            held.workspace = workspace.clone();
            held.seeded = true;
        }
    }
    let (busy, note, rows, market) = {
        let held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        (
            held.busy,
            held.note.clone(),
            held.rows.clone(),
            held.market.clone(),
        )
    };
    let mut source = {
        let held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        held.source.clone()
    };
    let mut market_url = {
        let held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        held.market_url.clone()
    };
    if !note.is_empty() {
        crate::cards::settings_note(ui, &note);
    }
    crate::cards::settings_field(
        ui,
        "Install",
        "https, ssh, or file URL, or an absolute path. Nothing from the bundle runs.",
        &mut source,
        false,
    );
    if crate::cards::settings_action(
        ui,
        "Install a bundle",
        "It stays off and untrusted.",
        "Install",
    ) && !busy
    {
        spawn(Job::Install {
            workspace: workspace.clone(),
            source: source.trim().to_string(),
        });
    }
    if rows.is_empty() {
        crate::cards::settings_note(ui, "No plugins");
    }
    for row in &rows {
        paint_row(ui, &workspace, row, busy);
    }
    ui.add_space(8.0);
    crate::cards::settings_field(
        ui,
        "Marketplace",
        "https index URL. Fetch lists entries. Install is a separate action.",
        &mut market_url,
        false,
    );
    if crate::cards::settings_action(
        ui,
        "Marketplace index",
        "Fetched only when you ask.",
        "Fetch",
    ) && !busy
    {
        spawn(Job::Fetch {
            workspace: workspace.clone(),
            url: market_url.trim().to_string(),
        });
    }
    if market.is_empty() {
        crate::cards::settings_note(ui, "No marketplace list yet");
    }
    for entry in &market {
        let hint = format!("{} · {}", entry.version, entry.source);
        if crate::cards::settings_action(ui, &entry.name, &hint, "Install") && !busy {
            spawn(Job::InstallEntry {
                workspace: workspace.clone(),
                source: entry.source.clone(),
            });
        }
    }
    {
        let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        held.source = source;
        held.market_url = market_url;
    }
    if busy {
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }
}

fn paint_row(ui: &mut eframe::egui::Ui, workspace: &Path, row: &PluginInfo, busy: bool) {
    let title = format!("{} {}", row.name, row.version);
    let hint = clip_line(&row.summary, 500);
    let action = if !row.trusted {
        "Trust"
    } else if row.enabled {
        "Disable"
    } else {
        "Enable"
    };
    if crate::cards::settings_action(ui, &title, &hint, action) && !busy {
        let name = row.name.clone();
        let workspace = workspace.to_path_buf();
        let job = if !row.trusted {
            Job::Trust { workspace, name }
        } else if row.enabled {
            Job::Disable { workspace, name }
        } else {
            Job::Enable { workspace, name }
        };
        spawn(job);
    }
    if row.installed
        && crate::cards::settings_action(ui, &row.name, "Remove the installed copy.", "Remove")
        && !busy
    {
        spawn(Job::Remove {
            workspace: workspace.to_path_buf(),
            name: row.name.clone(),
        });
    }
}

fn clip_line(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " · ");
    if flat.chars().count() <= max {
        flat
    } else {
        flat.chars().take(max).collect()
    }
}

fn spawn(job: Job) {
    {
        let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        if held.busy {
            return;
        }
        held.busy = true;
        held.note.clear();
    }
    std::thread::spawn(move || {
        let (workspace, note, market) = match job {
            Job::Install { workspace, source } => {
                let note = match grokhub_agent::plugins::install_source(&source) {
                    Ok(info) => {
                        format!("Installed {}. It stays off until you trust it.", info.name)
                    }
                    Err(err) => err,
                };
                (workspace, note, None)
            }
            Job::Trust { workspace, name } => {
                let note = match grokhub_agent::plugins::trust_plugin(&workspace, &name) {
                    Ok(()) => format!("Trusted {name}. It stays off until you enable it."),
                    Err(err) => err,
                };
                (workspace, note, None)
            }
            Job::Enable { workspace, name } => {
                let note = match grokhub_agent::plugins::enable_plugin(&workspace, &name) {
                    Ok(()) => format!("Enabled {name}"),
                    Err(err) => err,
                };
                (workspace, note, None)
            }
            Job::Disable { workspace, name } => {
                let note = match grokhub_agent::plugins::disable_plugin(&name) {
                    Ok(()) => format!("Disabled {name}"),
                    Err(err) => err,
                };
                (workspace, note, None)
            }
            Job::Remove { workspace, name } => {
                let note = match grokhub_agent::plugins::remove_plugin(&workspace, &name) {
                    Ok(()) => format!("Removed {name}"),
                    Err(err) => err,
                };
                (workspace, note, None)
            }
            Job::Fetch { workspace, url } => {
                if url.is_empty() {
                    let note = match grokhub_agent::plugins::set_marketplace_url("") {
                        Ok(()) => "Marketplace URL cleared.".to_string(),
                        Err(err) => err,
                    };
                    (workspace, note, Some(Vec::new()))
                } else {
                    match grokhub_agent::plugins::set_marketplace_url(&url) {
                        Ok(()) => match grokhub_agent::plugins::fetch_marketplace(&url) {
                            Ok(entries) => (
                                workspace,
                                format!("{} entries. Nothing was installed.", entries.len()),
                                Some(entries),
                            ),
                            Err(err) => (workspace, err, None),
                        },
                        Err(err) => (workspace, err, None),
                    }
                }
            }
            Job::InstallEntry { workspace, source } => {
                let note = match grokhub_agent::plugins::install_source(&source) {
                    Ok(info) => {
                        format!("Installed {}. It stays off until you trust it.", info.name)
                    }
                    Err(err) => err,
                };
                (workspace, note, None)
            }
        };
        let rows = grokhub_agent::plugins::list_plugins(&workspace);
        let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        held.rows = rows;
        if let Some(market) = market {
            held.market = market;
        }
        held.note = note;
        held.busy = false;
        held.seeded = true;
        held.workspace = workspace;
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_plugins_settings_are_behind_the_labs_toggle() {
        let settings = include_str!("app/settings.rs");
        let start = settings.find("SettingsSec::Labs => {").expect("labs body");
        let arm = settings[start..]
            .split("SettingsSec::")
            .nth(1)
            .expect("labs arm");
        assert!(arm.contains("self.cfg.native_engine"), "{arm}");
        let guard = arm
            .find("if self.cfg.native_engine")
            .expect("native engine guard");
        let paint = arm.find("native_plugins::paint").expect("plugins paint");
        assert!(paint > guard, "{arm}");
        let chat = include_str!("app/chat_ui.rs");
        assert!(!chat.contains("native_plugins"));
    }
}
