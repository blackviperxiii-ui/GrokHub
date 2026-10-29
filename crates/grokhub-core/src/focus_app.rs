//! Context-aware help from the app the user was just in. Opt-in. The cabin
//! reads only the foreground app's process name, once, when it loses focus,
//! and keeps it in memory until it is back in front. Never window titles,
//! URLs, document names or screenshots, and never a running log.
//!
//! A known process name maps to a category; the category picks one chip for
//! the composer ("Because you were in Blender") and one line for the next
//! turn's context.

use crate::chips::{ChipKind, QuickChip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusCategory {
    Modelling,
    Printing,
    Coding,
    Gaming,
    Graphics,
}

impl FocusCategory {
    pub fn key(self) -> &'static str {
        match self {
            Self::Modelling => "modelling",
            Self::Printing => "printing",
            Self::Coding => "coding",
            Self::Gaming => "gaming",
            Self::Graphics => "graphics",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Modelling => "3D modelling",
            Self::Printing => "3D printing",
            Self::Coding => "coding",
            Self::Gaming => "a game",
            Self::Graphics => "image editing",
        }
    }

    /// The chip's label and the prompt it puts in the composer.
    fn ask(self, app: &str) -> (&'static str, String) {
        match self {
            Self::Modelling => (
                "Modelling tips",
                format!("I was just in {app}. Give me three quick modelling tips I can use right now."),
            ),
            Self::Printing => (
                "Print profile help",
                format!("I was just in {app}. Help me tune my print profile: what should I check first?"),
            ),
            Self::Coding => (
                "Next coding step",
                format!("I was just coding in {app}. What's a good next step for what I'm working on?"),
            ),
            Self::Gaming => (
                "Patch notes and builds",
                format!("I was just playing {app}. What changed in the latest patch, and any build tips?"),
            ),
            Self::Graphics => (
                "Editing tips",
                format!("I was just in {app}. Give me three quick tips for what I'm editing."),
            ),
        }
    }
}

/// A recognised app: its display name and category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusApp {
    pub name: String,
    pub category: FocusCategory,
}

/// Process names (lowercase, no `.exe`) to a display name and category.
const KNOWN: &[(&str, &str, FocusCategory)] = &[
    ("blender", "Blender", FocusCategory::Modelling),
    ("freecad", "FreeCAD", FocusCategory::Modelling),
    ("fusion360", "Fusion 360", FocusCategory::Modelling),
    ("openscad", "OpenSCAD", FocusCategory::Modelling),
    ("sketchup", "SketchUp", FocusCategory::Modelling),
    ("crealityprint", "Creality Print", FocusCategory::Printing),
    ("creality print", "Creality Print", FocusCategory::Printing),
    ("cura", "Cura", FocusCategory::Printing),
    ("ultimaker-cura", "Cura", FocusCategory::Printing),
    ("ultimaker cura", "Cura", FocusCategory::Printing),
    ("prusa-slicer", "PrusaSlicer", FocusCategory::Printing),
    ("prusaslicer", "PrusaSlicer", FocusCategory::Printing),
    ("orca-slicer", "OrcaSlicer", FocusCategory::Printing),
    ("orcaslicer", "OrcaSlicer", FocusCategory::Printing),
    ("bambu-studio", "Bambu Studio", FocusCategory::Printing),
    ("bambu studio", "Bambu Studio", FocusCategory::Printing),
    ("cursor", "Cursor", FocusCategory::Coding),
    ("code", "VS Code", FocusCategory::Coding),
    ("code-oss", "VS Code", FocusCategory::Coding),
    ("codium", "VSCodium", FocusCategory::Coding),
    ("zed", "Zed", FocusCategory::Coding),
    ("zeditor", "Zed", FocusCategory::Coding),
    ("idea", "IntelliJ IDEA", FocusCategory::Coding),
    ("idea64", "IntelliJ IDEA", FocusCategory::Coding),
    ("pycharm", "PyCharm", FocusCategory::Coding),
    ("pycharm64", "PyCharm", FocusCategory::Coding),
    ("rustrover", "RustRover", FocusCategory::Coding),
    ("rustrover64", "RustRover", FocusCategory::Coding),
    ("devenv", "Visual Studio", FocusCategory::Coding),
    ("sublime_text", "Sublime Text", FocusCategory::Coding),
    ("gimp", "GIMP", FocusCategory::Graphics),
    ("gimp-2.10", "GIMP", FocusCategory::Graphics),
    ("krita", "Krita", FocusCategory::Graphics),
    ("inkscape", "Inkscape", FocusCategory::Graphics),
    ("photoshop", "Photoshop", FocusCategory::Graphics),
];

/// Wrappers games run under when the game's own name is not the process.
const GAME_HOSTS: &[&str] = &[
    "gamescope",
    "wine64-preloader",
    "wine-preloader",
    "wine64",
    "wine",
];

/// `C:\Program Files\Blender\blender.exe` or `Blender.exe` to `blender`.
pub fn normalize(process: &str) -> String {
    let base = process
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(process)
        .trim()
        .to_ascii_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

/// The category for a process name. `games` are the user's library names
/// (opt-in, e.g. from the Steam source); a process that matches one, or a
/// game host, is a game.
pub fn categorize(process: &str, games: &[String]) -> Option<FocusApp> {
    let name = normalize(process);
    if name.is_empty() {
        return None;
    }
    if let Some((_, display, category)) = KNOWN.iter().find(|(k, _, _)| *k == name) {
        return Some(FocusApp {
            name: (*display).to_string(),
            category: *category,
        });
    }
    let squash = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let key = squash(name.trim_end_matches("-win64-shipping"));
    if key.len() >= 3 {
        if let Some(game) = games.iter().find(|g| squash(g) == key) {
            return Some(FocusApp {
                name: game.clone(),
                category: FocusCategory::Gaming,
            });
        }
    }
    if GAME_HOSTS.contains(&name.as_str()) || name.ends_with("-win64-shipping") {
        return Some(FocusApp {
            name: "a game".into(),
            category: FocusCategory::Gaming,
        });
    }
    None
}

impl FocusApp {
    pub fn chip_id(&self) -> String {
        format!("focus:{}:{}", self.category.key(), normalize(&self.name))
    }

    /// One composer chip for the app, with its reason as the hint.
    pub fn chip(&self) -> QuickChip {
        let (label, value) = self.category.ask(&self.name);
        QuickChip {
            id: self.chip_id(),
            label: label.to_string(),
            value,
            kind: ChipKind::Chat,
            score: 1.0,
            hint: self.because(),
            primary: false,
        }
    }

    pub fn because(&self) -> String {
        format!("Because you were in {}", self.name)
    }

    /// The one-line hint for the next turn.
    pub fn context_line(&self) -> String {
        format!(
            "[Context: the user was just in {} ({}). Use it only if it fits the ask.]",
            self.name,
            self.category.label()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(p: &str) -> Option<(String, FocusCategory)> {
        categorize(p, &["Hades".into(), "Deep Rock Galactic".into()]).map(|a| (a.name, a.category))
    }

    #[test]
    fn linux_process_names_map_to_categories() {
        assert_eq!(
            cat("blender"),
            Some(("Blender".into(), FocusCategory::Modelling))
        );
        assert_eq!(
            cat("orca-slicer"),
            Some(("OrcaSlicer".into(), FocusCategory::Printing))
        );
        assert_eq!(cat("code"), Some(("VS Code".into(), FocusCategory::Coding)));
        assert_eq!(
            cat("cursor"),
            Some(("Cursor".into(), FocusCategory::Coding))
        );
        assert_eq!(
            cat("krita"),
            Some(("Krita".into(), FocusCategory::Graphics))
        );
        assert_eq!(
            cat("gamescope"),
            Some(("a game".into(), FocusCategory::Gaming))
        );
        assert_eq!(cat("hades"), Some(("Hades".into(), FocusCategory::Gaming)));
        assert_eq!(cat("firefox"), None);
        assert_eq!(cat("bash"), None);
        assert_eq!(cat(""), None);
    }

    #[test]
    fn windows_exe_paths_map_the_same_way() {
        assert_eq!(
            cat(r"C:\Program Files\Blender Foundation\Blender 4.2\blender.exe"),
            Some(("Blender".into(), FocusCategory::Modelling))
        );
        assert_eq!(
            cat("CrealityPrint.exe"),
            Some(("Creality Print".into(), FocusCategory::Printing))
        );
        assert_eq!(
            cat("Cursor.exe"),
            Some(("Cursor".into(), FocusCategory::Coding))
        );
        assert_eq!(
            cat("idea64.exe"),
            Some(("IntelliJ IDEA".into(), FocusCategory::Coding))
        );
        assert_eq!(
            cat("FSD-Win64-Shipping.exe"),
            Some(("a game".into(), FocusCategory::Gaming))
        );
        assert_eq!(
            cat("DeepRockGalactic.exe"),
            Some(("Deep Rock Galactic".into(), FocusCategory::Gaming))
        );
        assert_eq!(cat("explorer.exe"), None);
    }

    #[test]
    fn the_chip_and_context_line_name_the_app_only() {
        let app = categorize("blender", &[]).unwrap();
        let chip = app.chip();
        assert_eq!(chip.id, "focus:modelling:blender");
        assert_eq!(chip.label, "Modelling tips");
        assert_eq!(chip.hint, "Because you were in Blender");
        assert_eq!(
            chip.value,
            "I was just in Blender. Give me three quick modelling tips I can use right now."
        );
        assert_eq!(chip.kind, ChipKind::Chat);
        assert_eq!(
            app.context_line(),
            "[Context: the user was just in Blender (3D modelling). Use it only if it fits the ask.]"
        );
        let print = categorize("CrealityPrint.exe", &[]).unwrap().chip();
        assert_eq!(print.label, "Print profile help");
        assert_eq!(print.id, "focus:printing:creality print");
    }
}
