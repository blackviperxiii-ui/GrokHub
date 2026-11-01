//! Static Home module catalog. Adding a module = one file + one line here
//! (paint lives under `modules/` once wired; metadata ships for all 36 now).

use super::layout::ModuleSize;

/// Catalog group for Edit Home category tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    SystemPerformance,
    Gaming,
    AiUsage,
    SocialStreaming,
    ProductivityFun,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::SystemPerformance,
        Category::Gaming,
        Category::AiUsage,
        Category::SocialStreaming,
        Category::ProductivityFun,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::SystemPerformance => "System & Performance",
            Category::Gaming => "Gaming",
            Category::AiUsage => "AI & Usage",
            Category::SocialStreaming => "Social & Streaming",
            Category::ProductivityFun => "Productivity & Fun",
        }
    }
}

/// How complete the module's backing data is on beta today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataStatus {
    /// Backed by code on beta.
    Exists,
    /// Some fields exist; others still need wiring.
    Partial,
    /// Needs new data; show a clean "coming soon" until wired.
    New,
}

impl DataStatus {
    pub fn label(self) -> &'static str {
        match self {
            DataStatus::Exists => "ready",
            DataStatus::Partial => "partial",
            DataStatus::New => "coming soon",
        }
    }
}

/// One catalog entry. Paint is keyed by `id` once module files land.
#[derive(Debug, Clone, Copy)]
pub struct ModuleEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub sizes: &'static [ModuleSize],
    pub data: DataStatus,
    /// One-line purpose for empty / coming-soon chrome.
    pub purpose: &'static str,
}

impl ModuleEntry {
    pub fn supports(self, size: ModuleSize) -> bool {
        self.sizes.contains(&size)
    }
}

macro_rules! entry {
    ($id:literal, $name:literal, $cat:ident, $data:ident, $purpose:literal, [$($sz:ident),+ $(,)?]) => {
        ModuleEntry {
            id: $id,
            name: $name,
            category: Category::$cat,
            sizes: &[$(ModuleSize::$sz),+],
            data: DataStatus::$data,
            purpose: $purpose,
        }
    };
}

/// All 36 modules. Order matches the SPEC catalog.
pub const REGISTRY: &[ModuleEntry] = &[
    // System & Performance
    entry!(
        "pc_loadout",
        "PC Loadout",
        SystemPerformance,
        Partial,
        "Health at a glance: score, temps, storage, RAM, startup apps",
        [Large, Medium, Small]
    ),
    entry!(
        "speeds",
        "Speeds",
        SystemPerformance,
        Partial,
        "Down/up Mbps, ping, jitter, game-server ping",
        [Large, Medium, Small]
    ),
    entry!(
        "frame_rate",
        "Frame Rate",
        SystemPerformance,
        Partial,
        "Says how to see FPS without Loadout hooking into games",
        [Medium, Small]
    ),
    entry!(
        "storage",
        "Storage",
        SystemPerformance,
        Exists,
        "Free per drive, big caches",
        [Medium, Small]
    ),
    entry!(
        "updates",
        "Updates & Drivers",
        SystemPerformance,
        Partial,
        "Pending OS / GPU / game updates",
        [Medium, Small]
    ),
    entry!(
        "game_mode",
        "Game Mode",
        SystemPerformance,
        Exists,
        "Governor state: game detected, quiet, power, GPU busy",
        [Medium, Small]
    ),
    entry!(
        "audio_check",
        "Audio Check",
        SystemPerformance,
        Exists,
        "Output/mic device, level, Audio check",
        [Medium, Small]
    ),
    entry!(
        "systems_brief",
        "Systems Brief",
        SystemPerformance,
        Exists,
        "Weekly state-of-systems text + update tiles",
        [Large, Medium]
    ),
    entry!(
        "connection",
        "Connection",
        SystemPerformance,
        Exists,
        "Local/cloud, bytes sent, local model ready",
        [Medium, Small]
    ),
    // Gaming
    entry!(
        "up_next",
        "Up Next",
        Gaming,
        Partial,
        "Next games to play as cover cards",
        [Large, Medium, Small]
    ),
    entry!(
        "now_playing",
        "Games · Now Playing",
        Gaming,
        Partial,
        "Current/recent games, session and weekly time",
        [Large, Medium, Small]
    ),
    entry!(
        "game_library",
        "Game Library",
        Gaming,
        Partial,
        "Grid of installed games, store counts",
        [Large, Medium, Small]
    ),
    entry!(
        "objectives",
        "Objectives",
        Gaming,
        New,
        "Today's objectives with progress, streak and coach",
        [Large, Medium, Small]
    ),
    entry!(
        "streak",
        "Streak & XP",
        Gaming,
        New,
        "Level, XP bar, 7-day streak",
        [Medium, Small]
    ),
    entry!(
        "achievements",
        "Achievements",
        Gaming,
        New,
        "Progress, new unlocks",
        [Medium, Small]
    ),
    entry!(
        "coach_tip",
        "Coach Tip",
        Gaming,
        Partial,
        "One stat-backed tip",
        [Medium, Small]
    ),
    entry!(
        "patch_notes",
        "Patch Notes",
        Gaming,
        Partial,
        "Latest patch for your games",
        [Medium, Small]
    ),
    entry!(
        "deals",
        "Deals",
        Gaming,
        New,
        "Wishlist discounts",
        [Medium, Small]
    ),
    entry!(
        "what_to_play",
        "What To Play",
        Gaming,
        Partial,
        "Random pick from library fitted to time",
        [Medium, Small]
    ),
    // AI & Usage
    entry!(
        "usage",
        "Usage & Budget",
        AiUsage,
        Exists,
        "Today's percent of cap, local vs cloud share",
        [Medium, Small]
    ),
    entry!(
        "local_ai",
        "Local AI",
        AiUsage,
        Exists,
        "On-device model state, VRAM",
        [Medium, Small]
    ),
    entry!(
        "personality",
        "Personality",
        AiUsage,
        Exists,
        "Active preset, quick switch chips",
        [Medium, Small]
    ),
    entry!(
        "playbooks",
        "Playbooks",
        AiUsage,
        Exists,
        "Top playbooks, last run",
        [Medium, Small]
    ),
    entry!(
        "recent_chats",
        "Recent Chats",
        AiUsage,
        Exists,
        "Last three threads",
        [Medium, Small]
    ),
    entry!(
        "automations",
        "Automations",
        AiUsage,
        Exists,
        "Active automations, next run",
        [Medium, Small]
    ),
    entry!(
        "decisions",
        "Decisions",
        AiUsage,
        Exists,
        "Pending approvals",
        [Medium, Small]
    ),
    // Social & Streaming
    entry!(
        "squad",
        "Squad",
        SocialStreaming,
        New,
        "Friends online or in game, invite",
        [Medium, Small]
    ),
    entry!(
        "clips",
        "Clips & Captures",
        SocialStreaming,
        Partial,
        "Latest clips, share and record",
        [Large, Medium, Small]
    ),
    entry!(
        "stream_check",
        "Stream Check",
        SocialStreaming,
        New,
        "Go-live readiness: mic, scene, upload",
        [Medium, Small]
    ),
    entry!(
        "pulse",
        "Pulse · For You",
        SocialStreaming,
        Exists,
        "Ranked feed posts",
        [Large, Medium, Small]
    ),
    entry!(
        "daily_drop",
        "Daily Drop",
        SocialStreaming,
        Exists,
        "Today's pick",
        [Medium, Small]
    ),
    // Productivity & Fun
    entry!(
        "workboard",
        "Workboard",
        ProductivityFun,
        Exists,
        "Active cards with status",
        [Large, Medium, Small]
    ),
    entry!(
        "journal",
        "Journal",
        ProductivityFun,
        Exists,
        "Experiments and decisions backlog",
        [Medium, Small]
    ),
    entry!(
        "tonight",
        "Tonight",
        ProductivityFun,
        New,
        "Upcoming sessions and events",
        [Medium, Small]
    ),
    entry!(
        "session_timer",
        "Session Timer",
        ProductivityFun,
        New,
        "Play-time timer and break nudge",
        [Medium, Small]
    ),
    entry!(
        "hype_line",
        "Hype Line",
        ProductivityFun,
        Partial,
        "Daily line from the active personality",
        [Medium, Small]
    ),
];

pub fn lookup(id: &str) -> Option<&'static ModuleEntry> {
    REGISTRY.iter().find(|m| m.id == id)
}

pub fn modules_for_category(cat: Category) -> impl Iterator<Item = &'static ModuleEntry> {
    REGISTRY.iter().filter(move |m| m.category == cat)
}

pub fn modules_fitting(size: ModuleSize) -> impl Iterator<Item = &'static ModuleEntry> {
    REGISTRY.iter().filter(move |m| m.supports(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_exactly_thirty_six_modules() {
        assert_eq!(REGISTRY.len(), 36);
        let mut ids: Vec<_> = REGISTRY.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 36, "duplicate module ids");
    }

    #[test]
    fn every_module_declares_at_least_one_size() {
        for m in REGISTRY {
            assert!(!m.sizes.is_empty(), "{} has no sizes", m.id);
            assert!(
                m.sizes.contains(&ModuleSize::Small)
                    || m.sizes.contains(&ModuleSize::Medium)
                    || m.sizes.contains(&ModuleSize::Large),
                "{} sizes empty of L/M/S",
                m.id
            );
        }
    }

    #[test]
    fn default_slot_modules_are_in_the_catalog() {
        for id in [
            "objectives",
            "up_next",
            "pulse",
            "pc_loadout",
            "speeds",
            "usage",
            "streak",
            "clips",
            "squad",
            "stream_check",
            "connection",
            "daily_drop",
            "frame_rate",
            "storage",
            "game_mode",
            "updates",
        ] {
            let m = lookup(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(!m.name.is_empty());
        }
    }

    #[test]
    fn objectives_supports_large_and_pc_loadout_supports_small() {
        let objectives = lookup("objectives").expect("objectives");
        assert!(objectives.supports(ModuleSize::Large));
        assert!(objectives.supports(ModuleSize::Medium));
        assert!(objectives.supports(ModuleSize::Small));
        let pc = lookup("pc_loadout").expect("pc_loadout");
        assert!(pc.supports(ModuleSize::Small));
        let frame = lookup("frame_rate").expect("frame_rate");
        assert!(!frame.supports(ModuleSize::Large));
        assert!(frame.supports(ModuleSize::Medium));
    }

    #[test]
    fn category_tabs_cover_all_modules() {
        let mut n = 0;
        for cat in Category::ALL {
            n += modules_for_category(cat).count();
        }
        assert_eq!(n, 36);
        assert_eq!(Category::Gaming.label(), "Gaming");
        assert_eq!(DataStatus::New.label(), "coming soon");
    }

    #[test]
    fn systems_brief_has_no_small_look() {
        let brief = lookup("systems_brief").expect("systems_brief");
        assert!(!brief.supports(ModuleSize::Small));
        assert!(brief.supports(ModuleSize::Large));
    }

    #[test]
    fn coming_soon_modules_keep_a_purpose_line() {
        let deals = lookup("deals").expect("deals");
        assert_eq!(deals.data, DataStatus::New);
        assert_eq!(deals.data.label(), "coming soon");
        assert!(deals.purpose.contains("discounts"));
        // Speeds reads the internet log now; upload and game-server ping are still to come.
        assert_eq!(lookup("speeds").map(|m| m.data), Some(DataStatus::Partial));
        let fitting: Vec<_> = modules_fitting(ModuleSize::Large).map(|m| m.id).collect();
        assert!(fitting.contains(&"objectives"));
        assert!(!fitting.contains(&"frame_rate"));
    }
}
