//! The Updates & Drivers page: everything waiting to update on this PC in one
//! list, grouped by kind, with one offer per group. Pure: the shell reads the
//! sources (read-only, from lists already on disk) and this module turns
//! their output into the page and its chips.

use serde::{Deserialize, Serialize};

/// One thing that can update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateItem {
    pub name: String,
    /// The installed version, when the source says.
    pub from: Option<String>,
    /// The version waiting.
    pub to: Option<String>,
}

impl UpdateItem {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            from: None,
            to: None,
        }
    }
}

/// What one source found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Found {
    /// Waiting updates (empty means up to date).
    Items { items: Vec<UpdateItem> },
    /// The source isn't on this PC.
    NotHere,
    /// The source is here but its list couldn't be read.
    CantCheck { why: String },
}

impl Found {
    pub fn items(items: Vec<UpdateItem>) -> Self {
        Found::Items { items }
    }

    pub fn count(&self) -> usize {
        match self {
            Found::Items { items } => items.len(),
            _ => 0,
        }
    }
}

/// The kinds of update the page groups by, in the order it shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Loadout,
    Steam,
    System,
    Apps,
    Windows,
    Drivers,
}

/// What a chip on the page does. The shell runs it; nothing runs on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Offer {
    /// The app's own signed updater.
    UpdateLoadout,
    /// Steam's Downloads page, where its game updates queue.
    OpenSteamDownloads,
    /// The distro's package upgrade, behind the OS password prompt.
    UpdateSystem,
    /// winget's own upgrade, in a window the user can watch and answer.
    UpdateApps,
    /// Windows Update's own page in Settings.
    OpenWindowsUpdate,
    /// The driver update both checks agreed on, way back first, behind the
    /// OS password prompt.
    UpdateDrivers,
    /// The old driver packages back, behind the OS password prompt.
    RollBackDrivers,
    /// Windows Update's Optional updates, where Windows lists drivers.
    OpenOptionalUpdates,
}

/// How a driver update can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WayBack {
    /// A system snapshot by this tool (snapper, timeshift) is taken first.
    Snapshot(String),
    /// The old version stays available, so the page can put it back.
    OldPackages,
}

/// What the driver check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drivers {
    UpToDate,
    /// Checked twice (update list, then installed and repo versions read
    /// again), with a way back ready.
    Ready {
        items: Vec<UpdateItem>,
        way_back: WayBack,
    },
    /// Listed, but it won't update; `why` in plain words.
    Held {
        items: Vec<UpdateItem>,
        why: String,
    },
    /// The last update the page made is installed.
    Updated {
        items: Vec<UpdateItem>,
        way_back: WayBack,
    },
    /// The page updated, but the re-check doesn't see the new version.
    NotLanded {
        items: Vec<UpdateItem>,
    },
    /// Windows: the driver updates Windows Update lists.
    Windows {
        items: Vec<UpdateItem>,
    },
}

/// One group on the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Group {
    pub kind: Kind,
    pub title: String,
    /// One plain line: how many wait, up to date, or why it couldn't check.
    pub line: String,
    pub items: Vec<UpdateItem>,
    /// The one offer for this group, if there is something to do.
    pub offer: Option<OfferChip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfferChip {
    pub offer: Offer,
    pub label: String,
    /// Why the chip can't run right now (a game is running), shown beside it.
    pub held: Option<String>,
}

/// Everything the shell read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sources {
    pub version: String,
    /// The newer version the app's own updater found.
    pub loadout_newer: Option<String>,
    /// The app updates itself here (AppImage, Windows installer).
    pub loadout_updatable: bool,
    pub steam: Option<Vec<String>>,
    pub system: Option<Found>,
    pub apps: Option<Found>,
    pub windows: Option<Found>,
    pub gpu_driver: Option<String>,
    /// A game is running: nothing heavy starts now.
    pub drivers: Option<Drivers>,
    pub gaming: bool,
}

/// The page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdatesPage {
    /// "3 waiting" or "Up to date".
    pub headline: String,
    pub groups: Vec<Group>,
    /// "GPU driver 550.120", shown under the groups.
    pub driver: Option<String>,
}

const GAME_HOLD: &str = "Waits until your game closes";

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn found_line(found: &Found, what: (&str, &str)) -> String {
    match found {
        Found::Items { items } if items.is_empty() => "Up to date".into(),
        Found::Items { items } => format!("{} waiting", plural(items.len(), what.0, what.1)),
        Found::NotHere => "Not on this PC".into(),
        Found::CantCheck { why } => format!("Couldn't check: {why}"),
    }
}

fn chip(offer: Offer, label: &str, held: bool) -> Option<OfferChip> {
    Some(OfferChip {
        offer,
        label: label.into(),
        held: held.then(|| GAME_HOLD.to_string()),
    })
}

/// Builds the page from what the shell read. A source that wasn't read
/// (`None`) is left off; a source that isn't on this PC is left off too,
/// so the page only lists what this PC has.
pub fn page(s: &Sources) -> UpdatesPage {
    let mut groups = Vec::new();
    let product = crate::brand::product_name();
    groups.push(Group {
        kind: Kind::Loadout,
        title: product.to_string(),
        line: match &s.loadout_newer {
            Some(v) => format!("{v} is ready (you have {})", s.version),
            None => format!("{} is the newest", s.version),
        },
        items: Vec::new(),
        offer: s
            .loadout_newer
            .as_ref()
            .filter(|_| s.loadout_updatable)
            .and_then(|_| chip(Offer::UpdateLoadout, "Update", false)),
    });
    if let Some(games) = &s.steam {
        groups.push(Group {
            kind: Kind::Steam,
            title: "Steam games".into(),
            line: if games.is_empty() {
                "Up to date".into()
            } else {
                format!("{} waiting", plural(games.len(), "game", "games"))
            },
            items: games.iter().map(UpdateItem::named).collect(),
            offer: (!games.is_empty())
                .then_some(())
                .and_then(|_| chip(Offer::OpenSteamDownloads, "Open Steam downloads", false)),
        });
    }
    let sources = [
        (
            Kind::System,
            "System",
            ("package", "packages"),
            &s.system,
            Offer::UpdateSystem,
            "Update system",
            true,
        ),
        (
            Kind::Apps,
            "Apps",
            ("app", "apps"),
            &s.apps,
            Offer::UpdateApps,
            "Update apps",
            true,
        ),
        (
            Kind::Windows,
            "Windows Update",
            ("update", "updates"),
            &s.windows,
            Offer::OpenWindowsUpdate,
            "Open Windows Update",
            false,
        ),
    ];
    for (kind, title, what, found, offer, label, heavy) in sources {
        let Some(found) = found else { continue };
        if *found == Found::NotHere {
            continue;
        }
        let items = match found {
            Found::Items { items } => items.clone(),
            _ => Vec::new(),
        };
        // Windows Update's page is worth opening even when it can't be read.
        let worth = !items.is_empty() || (kind == Kind::Windows && found != &Found::items(vec![]));
        groups.push(Group {
            kind,
            title: title.into(),
            line: found_line(found, what),
            items,
            offer: worth
                .then_some(())
                .and_then(|_| chip(offer, label, heavy && s.gaming)),
        });
    }
    if let Some(d) = &s.drivers {
        groups.push(drivers_group(d, s.gaming));
    }
    let waiting = usize::from(s.loadout_newer.is_some())
        + s.steam.as_ref().map_or(0, Vec::len)
        + [&s.system, &s.apps, &s.windows]
            .iter()
            .map(|f| f.as_ref().map_or(0, Found::count))
            .sum::<usize>()
        + match &s.drivers {
            Some(Drivers::Ready { items, .. } | Drivers::Windows { items }) => items.len(),
            _ => 0,
        };
    UpdatesPage {
        headline: if waiting == 0 {
            "Up to date".into()
        } else {
            format!("{waiting} waiting")
        },
        groups,
        driver: s
            .gpu_driver
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(|d| format!("GPU driver {d}")),
    }
}

fn drivers_group(d: &Drivers, gaming: bool) -> Group {
    let n = |items: &[UpdateItem]| plural(items.len(), "driver update", "driver updates");
    let (line, items, offer) = match d {
        Drivers::UpToDate => ("Up to date".to_string(), Vec::new(), None),
        Drivers::Ready { items, way_back } => (
            format!(
                "{} checked twice. {}",
                n(items),
                match way_back {
                    WayBack::Snapshot(tool) => format!("A {tool} snapshot is taken first."),
                    WayBack::OldPackages => "The old version stays, so it can go back.".into(),
                }
            ),
            items.clone(),
            chip(Offer::UpdateDrivers, "Update drivers", gaming),
        ),
        Drivers::Held { items, why } => (why.clone(), items.clone(), None),
        Drivers::Updated { items, way_back } => match way_back {
            WayBack::OldPackages => (
                "Updated. Restart to load it. Put old driver back if a game misbehaves.".into(),
                items.clone(),
                chip(Offer::RollBackDrivers, "Put old driver back", gaming),
            ),
            WayBack::Snapshot(tool) => (
                format!("Updated. Restart to load it. To go back, restore the before-driver-update snapshot in {tool}."),
                items.clone(),
                None,
            ),
        },
        Drivers::NotLanded { items } => (
            "The update ran, but the new version isn't installed. Nothing else changed.".into(),
            items.clone(),
            None,
        ),
        Drivers::Windows { items } if items.is_empty() => ("Up to date".into(), Vec::new(), None),
        Drivers::Windows { items } => (
            format!("{} in Windows Update", n(items)),
            items.clone(),
            chip(Offer::OpenOptionalUpdates, "Open optional updates", false),
        ),
    };
    Group {
        kind: Kind::Drivers,
        title: "Drivers".into(),
        line,
        items,
        offer,
    }
}

/// The package list a `PendingUpdates` probe printed, as items. `program` is
/// the probe's program (`apt`, `pacman`, `dnf`, `zypper`).
pub fn parse_linux(program: &str, stdout: &str) -> Vec<UpdateItem> {
    let lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    match program {
        // `name/suite version arch [upgradable from: old]`
        "apt" => lines
            .filter(|l| l.contains("[upgradable from:"))
            .filter_map(|l| {
                let mut words = l.split_whitespace();
                let name = words.next()?.split('/').next()?.to_string();
                let to = words.next().map(str::to_string);
                let from = l
                    .split("[upgradable from:")
                    .nth(1)
                    .map(|r| r.trim().trim_end_matches(']').trim().to_string());
                Some(UpdateItem { name, from, to })
            })
            .collect(),
        // `name old -> new`, or `name new` from checkupdates' short form.
        "pacman" => lines
            .filter_map(|l| {
                let words: Vec<&str> = l.split_whitespace().collect();
                match words.as_slice() {
                    [name, from, "->", to] => Some(UpdateItem {
                        name: (*name).into(),
                        from: Some((*from).into()),
                        to: Some((*to).into()),
                    }),
                    [name, to] => Some(UpdateItem {
                        name: (*name).into(),
                        from: None,
                        to: Some((*to).into()),
                    }),
                    _ => None,
                }
            })
            .collect(),
        // `v | repo | name | old | new | arch`
        "zypper" => lines
            .filter(|l| l.starts_with("v "))
            .filter_map(|l| {
                let cols: Vec<&str> = l.split('|').map(str::trim).collect();
                Some(UpdateItem {
                    name: cols.get(2)?.to_string(),
                    from: cols.get(3).map(|s| s.to_string()),
                    to: cols.get(4).map(|s| s.to_string()),
                })
            })
            .collect(),
        // dnf check-update: `name.arch  version  repo`.
        _ => lines
            .filter_map(|l| {
                let words: Vec<&str> = l.split_whitespace().collect();
                match words.as_slice() {
                    [name_arch, to, _repo] if name_arch.contains('.') => Some(UpdateItem {
                        name: name_arch
                            .rsplit_once('.')
                            .map_or(*name_arch, |(n, _)| n)
                            .into(),
                        from: None,
                        to: Some((*to).into()),
                    }),
                    _ => None,
                }
            })
            .collect(),
    }
}

/// winget's `upgrade` table: a header with `Name Id Version Available`, a
/// dashed rule, then one row per app, then "N upgrades available.". Columns
/// are cut at the header's offsets, so names with spaces stay whole.
pub fn parse_winget(stdout: &str) -> Vec<UpdateItem> {
    let lines: Vec<&str> = stdout.lines().collect();
    let Some(head_at) = lines
        .iter()
        .position(|l| l.contains("Id") && l.contains("Version") && l.contains("Available"))
    else {
        return Vec::new();
    };
    let head = lines[head_at];
    // Offsets in chars: winget pads with spaces, and names may be non-ASCII.
    let at = |word: &str| head.find(word).map(|b| head[..b].chars().count());
    let (Some(id), Some(ver), Some(avail)) = (at("Id"), at("Version"), at("Available")) else {
        return Vec::new();
    };
    let source = at("Source");
    let cut = |row: &[char], from: usize, to: Option<usize>| -> String {
        let end = to.unwrap_or(row.len()).min(row.len());
        if from >= end {
            return String::new();
        }
        row[from..end].iter().collect::<String>().trim().to_string()
    };
    lines[head_at + 1..]
        .iter()
        .filter(|l| !l.trim_start().starts_with('-') && !l.trim().is_empty())
        .take_while(|l| !l.contains("upgrades available") && !l.contains("upgrade available"))
        .filter_map(|l| {
            let row: Vec<char> = l.chars().collect();
            let name = cut(&row, 0, Some(id));
            let from = cut(&row, ver, Some(avail));
            let to = cut(&row, avail, source);
            (!name.is_empty() && !to.is_empty()).then(|| UpdateItem {
                name,
                from: (!from.is_empty()).then_some(from),
                to: Some(to),
            })
        })
        .collect()
}

/// The page's Windows Update read: one `title` per line (not installed, not
/// hidden; software here, drivers in [`WINDOWS_DRIVER_TITLES`]).
pub fn parse_windows_update(stdout: &str) -> Vec<UpdateItem> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(UpdateItem::named)
        .collect()
}

/// The read-only Windows Update lists for the page: titles only, no install.
/// They run as a standard user; Windows Update's own page installs.
pub const WINDOWS_UPDATE_TITLES: &str = "foreach ($u in (New-Object -ComObject Microsoft.Update.Session).CreateUpdateSearcher().Search('IsInstalled=0 and IsHidden=0 and Type=''Software''').Updates) { $u.Title }";
pub const WINDOWS_DRIVER_TITLES: &str = "foreach ($u in (New-Object -ComObject Microsoft.Update.Session).CreateUpdateSearcher().Search('IsInstalled=0 and IsHidden=0 and Type=''Driver''').Updates) { $u.Title }";

/// The distro's own full upgrade for `program` (the `PendingUpdates` probe's
/// program), run as root behind the OS prompt. A snapper or timeshift
/// snapshot is tried first so there is a way back; a failed snapshot doesn't
/// stop the update (the user asked for it), but a failed update stops there.
/// apt keeps the user's own config files on conflicts.
pub fn system_upgrade_script(program: &str) -> Option<String> {
    let upgrade = match program {
        "apt" => "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold upgrade",
        "pacman" => "pacman -Syu --noconfirm",
        "dnf" => "dnf -y upgrade",
        "zypper" => "zypper --non-interactive update",
        _ => return None,
    };
    Some(format!(
        "if command -v snapper >/dev/null 2>&1; then snapper create -d 'Before system update' || true; \
elif command -v timeshift >/dev/null 2>&1; then timeshift --create --scripted --comments 'Before system update' || true; fi\n{upgrade}"
    ))
}

/// winget's own upgrade of everything it lists, in a console window the user
/// watches; winget asks for any agreement or admin rights itself.
pub const WINGET_UPGRADE_ALL: &str = "Start-Process winget -ArgumentList 'upgrade','--all'";

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
