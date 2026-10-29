//! Bite-sized learning paths from Know me hobby tags. Opt-in per hobby;
//! one tip, tutorial, or project idea at a time in Pulse. Progress and
//! difficulty stay on this computer. Pure rules; the cabin posts cards.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::know_me::KnowMeTag;
use crate::pulse::PulseMeta;
use crate::update_feed::{suggestion_card, UpdateAction, UpdateCard};

pub const FILE_NAME: &str = "hobby-paths.json";
pub const SOURCE_PREFIX: &str = "hobbypath:";
/// How often the cabin may call [`tick`].
pub const TICK_EVERY_MS: u64 = 30 * 60 * 1000;
/// At most one new tip per opted-in hobby inside this window.
pub const COOLDOWN_MS: u64 = 12 * 60 * 60 * 1000;
/// Cap on new path cards across all hobbies in one local day.
pub const DEFAULT_DAILY_MAX: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HobbyId {
    Printing3d,
    Modeling3d,
    Rust,
    GameModding,
    DigitalArt,
    Cad,
}

impl HobbyId {
    pub const ALL: [HobbyId; 6] = [
        Self::Printing3d,
        Self::Modeling3d,
        Self::Rust,
        Self::GameModding,
        Self::DigitalArt,
        Self::Cad,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Printing3d => "printing_3d",
            Self::Modeling3d => "modeling_3d",
            Self::Rust => "rust",
            Self::GameModding => "game_modding",
            Self::DigitalArt => "digital_art",
            Self::Cad => "cad",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Printing3d => "3D printing",
            Self::Modeling3d => "3D modeling",
            Self::Rust => "Rust",
            Self::GameModding => "game modding",
            Self::DigitalArt => "digital art",
            Self::Cad => "CAD",
        }
    }

    /// Know me topic / tag fragments that unlock this hobby.
    pub fn tag_words(self) -> &'static [&'static str] {
        match self {
            Self::Printing3d => &["3d printing", "printing"],
            Self::Modeling3d => &["3d modeling", "blender", "modelling"],
            Self::Rust => &["rust"],
            Self::GameModding => &["game mod", "modding", "pc gaming"],
            Self::DigitalArt => &["digital art", "krita", "gimp"],
            Self::Cad => &["cad", "freecad", "fusion"],
        }
    }

    /// Foreground app process fragments that prefer this hobby's tip.
    pub fn app_frags(self) -> &'static [&'static str] {
        match self {
            Self::Printing3d => &["cura", "prusaslicer", "orcaslicer", "bambu", "creality"],
            Self::Modeling3d => &["blender"],
            Self::Rust => &["rustrover", "code", "cursor", "codium", "zed"],
            Self::GameModding => &["steam", "heroic", "lutris"],
            Self::DigitalArt => &["krita", "gimp", "inkscape", "photoshop"],
            Self::Cad => &["freecad", "fusion360", "openscad"],
        }
    }
}

fn default_off() -> bool {
    false
}

fn default_daily_max() -> u32 {
    DEFAULT_DAILY_MAX
}

/// Settings → Pulse → Learning paths. Each hobby is opt-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HobbyPathSettings {
    #[serde(default = "default_off")]
    pub printing_3d: bool,
    #[serde(default = "default_off")]
    pub modeling_3d: bool,
    #[serde(default = "default_off")]
    pub rust: bool,
    #[serde(default = "default_off")]
    pub game_modding: bool,
    #[serde(default = "default_off")]
    pub digital_art: bool,
    #[serde(default = "default_off")]
    pub cad: bool,
    #[serde(default = "default_daily_max")]
    pub daily_max: u32,
}

impl Default for HobbyPathSettings {
    fn default() -> Self {
        Self {
            printing_3d: false,
            modeling_3d: false,
            rust: false,
            game_modding: false,
            digital_art: false,
            cad: false,
            daily_max: DEFAULT_DAILY_MAX,
        }
    }
}

impl HobbyPathSettings {
    pub fn on(&self, id: HobbyId) -> bool {
        match id {
            HobbyId::Printing3d => self.printing_3d,
            HobbyId::Modeling3d => self.modeling_3d,
            HobbyId::Rust => self.rust,
            HobbyId::GameModding => self.game_modding,
            HobbyId::DigitalArt => self.digital_art,
            HobbyId::Cad => self.cad,
        }
    }

    pub fn set_on(&mut self, id: HobbyId, on: bool) {
        match id {
            HobbyId::Printing3d => self.printing_3d = on,
            HobbyId::Modeling3d => self.modeling_3d = on,
            HobbyId::Rust => self.rust = on,
            HobbyId::GameModding => self.game_modding = on,
            HobbyId::DigitalArt => self.digital_art = on,
            HobbyId::Cad => self.cad = on,
        }
    }

    pub fn any_on(&self) -> bool {
        HobbyId::ALL.iter().any(|id| self.on(*id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Tip,
    Tutorial,
    Project,
}

impl StepKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tip => "Tip",
            Self::Tutorial => "Tutorial",
            Self::Project => "Project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathFeedback {
    Done,
    Skip,
    TooEasy,
    TooHard,
}

/// Difficulty band: 0 beginner, 1 intermediate, 2 advanced.
pub type Difficulty = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathStep {
    pub id: &'static str,
    pub kind: StepKind,
    pub difficulty: Difficulty,
    pub title: &'static str,
    pub body: &'static str,
    pub url: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HobbyProgress {
    #[serde(default)]
    pub difficulty: i8,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub skipped: Vec<String>,
}

impl HobbyProgress {
    pub fn band(&self) -> Difficulty {
        self.difficulty.clamp(0, 2) as Difficulty
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HobbyPathsLedger {
    #[serde(default)]
    pub last_tick_ms: u64,
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub posted_today: u32,
    #[serde(default)]
    pub last_posted: BTreeMap<String, u64>,
    #[serde(default)]
    pub progress: BTreeMap<String, HobbyProgress>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HobbyPathMeta {
    pub hobby: String,
    pub step_id: String,
    #[serde(default)]
    pub answered: bool,
}

pub struct TickInput<'a> {
    pub settings: &'a HobbyPathSettings,
    pub ledger: &'a mut HobbyPathsLedger,
    pub know_me: &'a [KnowMeTag],
    /// Normalized foreground process name, when the user was just in an app.
    pub focus_process: Option<&'a str>,
    pub today: &'a str,
    pub now_ms: u64,
}

pub struct TickOutput {
    pub cards: Vec<UpdateCard>,
}

pub fn load_ledger(path: &Path) -> HobbyPathsLedger {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_ledger(path: &Path, ledger: &HobbyPathsLedger) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())?;
    std::fs::write(path, body).map_err(|e| e.to_string())
}

/// Hobbies Know me tags point at, in catalog order.
pub fn hobbies_from_tags(tags: &[KnowMeTag]) -> Vec<HobbyId> {
    let topics: Vec<String> = tags
        .iter()
        .flat_map(|t| {
            [
                t.topic.trim().to_ascii_lowercase(),
                t.label.trim().to_ascii_lowercase(),
            ]
        })
        .filter(|s| !s.is_empty())
        .collect();
    HobbyId::ALL
        .into_iter()
        .filter(|id| {
            id.tag_words().iter().any(|w| {
                topics
                    .iter()
                    .any(|t| t.contains(w) || w.contains(t.as_str()))
            })
        })
        .collect()
}

/// Hobby preferred by a foreground process name, when it maps.
pub fn hobby_for_process(process: &str) -> Option<HobbyId> {
    let name = process
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(process)
        .trim()
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    HobbyId::ALL.into_iter().find(|id| {
        id.app_frags()
            .iter()
            .any(|frag| name.contains(frag) || frag.contains(name))
    })
}

/// Curated free-source steps for one hobby.
pub fn catalog(hobby: HobbyId) -> &'static [PathStep] {
    match hobby {
        HobbyId::Printing3d => &[
            PathStep {
                id: "print-bed-level",
                kind: StepKind::Tip,
                difficulty: 0,
                title: "Level the bed before the next print",
                body: "A sheet of paper should drag lightly under the nozzle at each corner. Uneven first layers usually start here.",
                url: Some("https://help.prusa3d.com/article/bed-leveling_2255"),
            },
            PathStep {
                id: "print-first-layer",
                kind: StepKind::Tutorial,
                difficulty: 0,
                title: "Dial in the first-layer squish",
                body: "Print a single-layer square. Lines should bond without ridges or gaps. Adjust Z offset a hair at a time.",
                url: Some("https://teachingtechyt.github.io/calibration.html#firstlayer"),
            },
            PathStep {
                id: "print-temp-tower",
                kind: StepKind::Project,
                difficulty: 1,
                title: "Run a temperature tower for your filament",
                body: "Print a temp tower once per filament brand. Note the sweet spot where overhangs and bridging look clean.",
                url: Some("https://www.printables.com/model/11716-temperature-tower"),
            },
            PathStep {
                id: "print-flow-rate",
                kind: StepKind::Tutorial,
                difficulty: 1,
                title: "Calibrate extrusion flow",
                body: "A single-wall cube tells you whether flow is high or low. Measure wall thickness and set flow = expected / measured.",
                url: Some("https://ellis3dp.com/Print-Tuning-Guide/articles/extrusion_multiplier.html"),
            },
            PathStep {
                id: "print-support-tree",
                kind: StepKind::Tip,
                difficulty: 2,
                title: "Try tree supports on organic shapes",
                body: "Tree supports touch less of the model and often peel cleaner than grids on organic prints.",
                url: Some("https://github.com/SoftFever/OrcaSlicer/wiki"),
            },
        ],
        HobbyId::Modeling3d => &[
            PathStep {
                id: "blend-viewport",
                kind: StepKind::Tip,
                difficulty: 0,
                title: "Orbit, pan, and zoom without fighting the view",
                body: "Middle-mouse orbit, Shift+middle pan, scroll zoom. Numpad . frames the selection.",
                url: Some("https://docs.blender.org/manual/en/latest/editors/3dview/navigate/introduction.html"),
            },
            PathStep {
                id: "blend-edit-mode",
                kind: StepKind::Tutorial,
                difficulty: 0,
                title: "Switch between Object and Edit mode",
                body: "Tab toggles Edit mode. In Edit mode, 1/2/3 pick vertex, edge, or face select.",
                url: Some("https://docs.blender.org/manual/en/latest/modeling/meshes/introduction.html"),
            },
            PathStep {
                id: "blend-modifiers",
                kind: StepKind::Tip,
                difficulty: 1,
                title: "Stack a Subdivision Surface modifier",
                body: "Add Modifier → Subdivision Surface, leave it unapplied while you model so the cage stays light.",
                url: Some("https://docs.blender.org/manual/en/latest/modeling/modifiers/generate/subdivision_surface.html"),
            },
            PathStep {
                id: "blend-donut",
                kind: StepKind::Project,
                difficulty: 1,
                title: "Finish one beginner mesh project",
                body: "Pick a short official tutorial (like the donut) and finish it end-to-end once. Muscle memory beats half-finished scenes.",
                url: Some("https://www.blender.org/support/tutorials/"),
            },
            PathStep {
                id: "blend-geometry-nodes",
                kind: StepKind::Tutorial,
                difficulty: 2,
                title: "Build a tiny Geometry Nodes scatter",
                body: "Scatter instances on a plane with a Distribute Points on Faces node. Keep the graph under ten nodes the first time.",
                url: Some("https://docs.blender.org/manual/en/latest/modeling/geometry_nodes/introduction.html"),
            },
        ],
        HobbyId::Rust => &[
            PathStep {
                id: "rust-cargo-new",
                kind: StepKind::Tip,
                difficulty: 0,
                title: "Start every experiment with cargo new",
                body: "`cargo new play` gives you a crate, a test harness, and `cargo run` for free. Skip loose .rs files.",
                url: Some("https://doc.rust-lang.org/cargo/guide/creating-a-new-project.html"),
            },
            PathStep {
                id: "rust-ownership",
                kind: StepKind::Tutorial,
                difficulty: 0,
                title: "Trace one ownership move with the compiler",
                body: "Write a function that takes ownership of a String, then try to use it after the call. Read the error; that is the lesson.",
                url: Some("https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html"),
            },
            PathStep {
                id: "rust-clippy",
                kind: StepKind::Tip,
                difficulty: 1,
                title: "Turn on clippy -D warnings for one crate",
                body: "`cargo clippy -- -D warnings` catches style and footguns early. Fix what it flags before adding features.",
                url: Some("https://doc.rust-lang.org/clippy/"),
            },
            PathStep {
                id: "rust-result-map",
                kind: StepKind::Tutorial,
                difficulty: 1,
                title: "Prefer Result + ? over unwrap in library code",
                body: "Replace one unwrap with a typed error and `?`. Callers then decide; panics stay at the edge.",
                url: Some("https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html"),
            },
            PathStep {
                id: "rust-small-cli",
                kind: StepKind::Project,
                difficulty: 2,
                title: "Ship a tiny CLI that reads stdin",
                body: "Build a filter that reads lines from stdin and prints a transform. Add one clap flag and one unit test.",
                url: Some("https://doc.rust-lang.org/rust-by-example/std_misc/arg/matching.html"),
            },
        ],
        HobbyId::GameModding => &[
            PathStep {
                id: "mod-backup-saves",
                kind: StepKind::Tip,
                difficulty: 0,
                title: "Back up saves before the first mod",
                body: "Copy the save folder once. Most broken loads are one restore away when you start experimenting.",
                url: None,
            },
            PathStep {
                id: "mod-one-at-a-time",
                kind: StepKind::Tutorial,
                difficulty: 0,
                title: "Install one mod, launch, then add the next",
                body: "Bisect failures: a stack that loads after each add is easier to debug than a pile installed at once.",
                url: None,
            },
            PathStep {
                id: "mod-read-log",
                kind: StepKind::Tip,
                difficulty: 1,
                title: "Read the game's log when something fails",
                body: "Crash and missing-asset lines usually name the offending pak or plugin. Search the log before reinstalling everything.",
                url: None,
            },
            PathStep {
                id: "mod-load-order",
                kind: StepKind::Tutorial,
                difficulty: 1,
                title: "Learn that load order is a dependency graph",
                body: "Put framework mods first, content next, patches last. Document your order in a short text file beside the install.",
                url: None,
            },
            PathStep {
                id: "mod-tiny-patch",
                kind: StepKind::Project,
                difficulty: 2,
                title: "Make a one-line config tweak as your first \"mod\"",
                body: "Change one documented cvar or ini value, prove it in-game, then package that single change. Scope beats ambition.",
                url: None,
            },
        ],
        HobbyId::DigitalArt => &[
            PathStep {
                id: "art-layers",
                kind: StepKind::Tip,
                difficulty: 0,
                title: "Keep sketch, color, and line on separate layers",
                body: "Lock the sketch layer at low opacity. Color underneath; ink on top. Undo stays cheap.",
                url: Some("https://docs.krita.org/en/user_manual/getting_started/basic_concepts.html"),
            },
            PathStep {
                id: "art-values",
                kind: StepKind::Tutorial,
                difficulty: 0,
                title: "Block values before chasing color",
                body: "Paint a three-value thumbnail in greyscale. If it reads, color will too.",
                url: Some("https://docs.krita.org/en/user_manual/introduction_from_other_software.html"),
            },
            PathStep {
                id: "art-brush-size",
                kind: StepKind::Tip,
                difficulty: 1,
                title: "Bind brush size to a shortcut or stylus",
                body: "Size changes more than any other setting. Map [ ] or the stylus rocker so you never dig through menus mid-stroke.",
                url: None,
            },
            PathStep {
                id: "art-study",
                kind: StepKind::Project,
                difficulty: 1,
                title: "Do one 20-minute master study",
                body: "Copy values from a photo or painting you like. Time-box it; the point is seeing, not finishing a portfolio piece.",
                url: None,
            },
            PathStep {
                id: "art-color-manage",
                kind: StepKind::Tutorial,
                difficulty: 2,
                title: "Paint in a managed RGB profile once",
                body: "Pick sRGB for screen work and stick to it for a week. Export previews in the same space you paint.",
                url: Some("https://docs.krita.org/en/user_manual/color_management.html"),
            },
        ],
        HobbyId::Cad => &[
            PathStep {
                id: "cad-sketch-constraints",
                kind: StepKind::Tip,
                difficulty: 0,
                title: "Fully constrain the sketch before padding",
                body: "Dimensions and constraints should turn the sketch blue/green (solver happy). Floating geometry bites later.",
                url: Some("https://wiki.freecad.org/Sketcher_Workbench"),
            },
            PathStep {
                id: "cad-datum",
                kind: StepKind::Tutorial,
                difficulty: 0,
                title: "Start from a clear origin and midplane",
                body: "Put the part on the origin. Symmetric features about the midplane save rework when sizes change.",
                url: Some("https://wiki.freecad.org/Getting_started"),
            },
            PathStep {
                id: "cad-param-names",
                kind: StepKind::Tip,
                difficulty: 1,
                title: "Name key dimensions as parameters",
                body: "Wall thickness, hole diameter, and overall length as named vars make variants a spreadsheet edit, not a redraw.",
                url: Some("https://wiki.freecad.org/Spreadsheet_Workbench"),
            },
            PathStep {
                id: "cad-simple-bracket",
                kind: StepKind::Project,
                difficulty: 1,
                title: "Model a printable L-bracket end to end",
                body: "Sketch, pad, fillet one edge, export STL. Print it if you can — CAD closes the loop when the part exists.",
                url: None,
            },
            PathStep {
                id: "cad-assembly",
                kind: StepKind::Tutorial,
                difficulty: 2,
                title: "Assemble two parts with one mate",
                body: "Keep each part a separate body/file. Mate one face pair. Exploded complexity comes after that works.",
                url: Some("https://wiki.freecad.org/Assembly"),
            },
        ],
    }
}

fn progress_mut(ledger: &mut HobbyPathsLedger, hobby: HobbyId) -> &mut HobbyProgress {
    ledger.progress.entry(hobby.key().to_string()).or_default()
}

fn seen(progress: &HobbyProgress, step_id: &str) -> bool {
    progress.done.iter().any(|id| id == step_id) || progress.skipped.iter().any(|id| id == step_id)
}

/// Next step for a hobby at its current difficulty band. Prefers an exact
/// band match, then the nearest unfinished step.
pub fn next_step(hobby: HobbyId, progress: &HobbyProgress) -> Option<&'static PathStep> {
    let band = progress.band();
    let steps = catalog(hobby);
    let open: Vec<_> = steps.iter().filter(|s| !seen(progress, s.id)).collect();
    if open.is_empty() {
        return None;
    }
    open.iter()
        .find(|s| s.difficulty == band)
        .or_else(|| {
            open.iter()
                .min_by_key(|s| (s.difficulty as i16 - band as i16).unsigned_abs())
        })
        .copied()
}

/// Apply done / skip / too easy / too hard. Returns the updated difficulty.
pub fn apply_feedback(
    progress: &mut HobbyProgress,
    step_id: &str,
    feedback: PathFeedback,
) -> Difficulty {
    match feedback {
        PathFeedback::Done => {
            if !progress.done.iter().any(|id| id == step_id) {
                progress.done.push(step_id.to_string());
            }
            progress.skipped.retain(|id| id != step_id);
        }
        PathFeedback::Skip => {
            if !progress.skipped.iter().any(|id| id == step_id) {
                progress.skipped.push(step_id.to_string());
            }
        }
        PathFeedback::TooEasy => {
            if !progress.done.iter().any(|id| id == step_id) {
                progress.done.push(step_id.to_string());
            }
            progress.skipped.retain(|id| id != step_id);
            progress.difficulty = (progress.difficulty + 1).min(2);
        }
        PathFeedback::TooHard => {
            if !progress.skipped.iter().any(|id| id == step_id) {
                progress.skipped.push(step_id.to_string());
            }
            progress.difficulty = (progress.difficulty - 1).max(0);
        }
    }
    progress.band()
}

pub fn is_hobby_path_card(card: &UpdateCard) -> bool {
    card.source_id.starts_with(SOURCE_PREFIX)
}

pub fn source_id(hobby: HobbyId, step_id: &str) -> String {
    format!("{SOURCE_PREFIX}{}:{step_id}", hobby.key())
}

pub fn hobby_path_card(hobby: HobbyId, step: &PathStep, now_ms: u64) -> UpdateCard {
    let source = source_id(hobby, step.id);
    let title = format!("{} · {}", hobby.label(), step.title);
    let kind = step.kind.label();
    let body = format!("{kind}: {}", step.body);
    let mut card = suggestion_card(&source, &title, &body, now_ms);
    card.action = step.url.map(|href| UpdateAction::DeepLink {
        href: href.to_string(),
    });
    card.citations = step.url.iter().map(|u| (*u).to_string()).collect();
    card.why = Some(format!(
        "Learning path · {} · difficulty {}",
        hobby.label(),
        step.difficulty
    ));
    card.pulse = PulseMeta {
        because: Some(format!("{} learning path", hobby.label())),
        hobby_path: Some(HobbyPathMeta {
            hobby: hobby.key().into(),
            step_id: step.id.into(),
            answered: false,
        }),
        ..Default::default()
    };
    card
}

fn ledger_day_reset(ledger: &mut HobbyPathsLedger, today: &str) {
    if ledger.day != today {
        ledger.day = today.to_string();
        ledger.posted_today = 0;
    }
}

fn on_cooldown(ledger: &HobbyPathsLedger, hobby: HobbyId, now_ms: u64) -> bool {
    ledger
        .last_posted
        .get(hobby.key())
        .is_some_and(|t| now_ms.saturating_sub(*t) < COOLDOWN_MS)
}

/// Eligible hobbies: opted in, Know me tagged (or empty tags when testing
/// fixtures that already filtered), not cooling down, with a next step.
pub fn eligible_hobbies(
    settings: &HobbyPathSettings,
    ledger: &HobbyPathsLedger,
    from_tags: &[HobbyId],
    now_ms: u64,
) -> Vec<HobbyId> {
    from_tags
        .iter()
        .copied()
        .filter(|id| settings.on(*id))
        .filter(|id| !on_cooldown(ledger, *id, now_ms))
        .filter(|id| {
            let progress = ledger.progress.get(id.key()).cloned().unwrap_or_default();
            next_step(*id, &progress).is_some()
        })
        .collect()
}

/// Pick one hobby: prefer the focus-app match, else the first eligible.
pub fn pick_hobby(eligible: &[HobbyId], focus: Option<HobbyId>) -> Option<HobbyId> {
    if let Some(focus) = focus {
        if eligible.contains(&focus) {
            return Some(focus);
        }
    }
    eligible.first().copied()
}

/// Post at most one learning-path tip when settings and tags allow.
pub fn tick(input: TickInput<'_>) -> TickOutput {
    let TickInput {
        settings,
        ledger,
        know_me,
        focus_process,
        today,
        now_ms,
    } = input;
    ledger_day_reset(ledger, today);
    ledger.last_tick_ms = now_ms;
    if !settings.any_on() || ledger.posted_today >= settings.daily_max.max(1) {
        return TickOutput { cards: Vec::new() };
    }
    let from_tags = hobbies_from_tags(know_me);
    if from_tags.is_empty() {
        return TickOutput { cards: Vec::new() };
    }
    let eligible = eligible_hobbies(settings, ledger, &from_tags, now_ms);
    let focus = focus_process.and_then(hobby_for_process);
    let Some(hobby) = pick_hobby(&eligible, focus) else {
        return TickOutput { cards: Vec::new() };
    };
    let progress = ledger
        .progress
        .get(hobby.key())
        .cloned()
        .unwrap_or_default();
    let Some(step) = next_step(hobby, &progress) else {
        return TickOutput { cards: Vec::new() };
    };
    ledger.last_posted.insert(hobby.key().to_string(), now_ms);
    ledger.posted_today = ledger.posted_today.saturating_add(1);
    TickOutput {
        cards: vec![hobby_path_card(hobby, step, now_ms)],
    }
}

/// Record feedback from a Pulse card action.
pub fn feedback_on_card(
    ledger: &mut HobbyPathsLedger,
    meta: &HobbyPathMeta,
    feedback: PathFeedback,
) -> Option<Difficulty> {
    let hobby = HobbyId::ALL.into_iter().find(|id| id.key() == meta.hobby)?;
    let progress = progress_mut(ledger, hobby);
    Some(apply_feedback(progress, &meta.step_id, feedback))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(label: &str, topic: &str) -> KnowMeTag {
        KnowMeTag {
            label: label.into(),
            topic: topic.into(),
        }
    }

    fn settings_on(ids: &[HobbyId]) -> HobbyPathSettings {
        let mut s = HobbyPathSettings::default();
        for id in ids {
            s.set_on(*id, true);
        }
        s
    }

    #[test]
    fn a_tip_reads_on_the_feed_where_its_controls_are() {
        let step = next_step(HobbyId::Rust, &HobbyProgress::default()).expect("step");
        let card = hobby_path_card(HobbyId::Rust, step, 5);
        assert_eq!(card.kind, crate::UpdateKind::Suggestion);
        assert!(!crate::pulse::is_idea_card(&card));
        assert!(crate::pulse::is_feed_card(&card));
        let mut plain = card.clone();
        plain.pulse.hobby_path = None;
        assert!(
            crate::pulse::is_idea_card(&plain),
            "other suggestions stay in Ideas"
        );
    }

    #[test]
    fn path_generation_from_fixture_tags() {
        let tags = vec![
            tag("hobby: 3D printing", "3D printing"),
            tag("dev: Rust", "Rust"),
            tag("plays: Hades", "Hades"),
        ];
        let hobbies = hobbies_from_tags(&tags);
        assert_eq!(hobbies, vec![HobbyId::Printing3d, HobbyId::Rust]);
        let print = next_step(HobbyId::Printing3d, &HobbyProgress::default()).expect("step");
        assert_eq!(print.id, "print-bed-level");
        assert_eq!(print.difficulty, 0);
        let rust = next_step(HobbyId::Rust, &HobbyProgress::default()).expect("step");
        assert_eq!(rust.id, "rust-cargo-new");
    }

    #[test]
    fn per_hobby_toggle_respected() {
        let tags = vec![
            tag("hobby: 3D printing", "3D printing"),
            tag("dev: Rust", "Rust"),
        ];
        let mut ledger = HobbyPathsLedger::default();
        let off = HobbyPathSettings::default();
        let out = tick(TickInput {
            settings: &off,
            ledger: &mut ledger,
            know_me: &tags,
            focus_process: None,
            today: "2026-10-28",
            now_ms: 1_000,
        });
        assert!(out.cards.is_empty(), "all off posts nothing");

        let only_rust = settings_on(&[HobbyId::Rust]);
        let out = tick(TickInput {
            settings: &only_rust,
            ledger: &mut ledger,
            know_me: &tags,
            focus_process: None,
            today: "2026-10-28",
            now_ms: 1_000,
        });
        assert_eq!(out.cards.len(), 1);
        assert!(out.cards[0].source_id.contains("rust:"));
        assert!(out.cards[0].title.contains("Rust"));
        assert!(!out.cards[0].source_id.contains("printing"));
    }

    #[test]
    fn progress_and_difficulty_adapt() {
        let mut progress = HobbyProgress::default();
        assert_eq!(progress.band(), 0);
        let first = next_step(HobbyId::Modeling3d, &progress).expect("first");
        assert_eq!(first.difficulty, 0);

        assert_eq!(
            apply_feedback(&mut progress, first.id, PathFeedback::TooEasy),
            1
        );
        assert!(progress.done.iter().any(|id| id == first.id));
        let next = next_step(HobbyId::Modeling3d, &progress).expect("next");
        assert_eq!(next.difficulty, 1, "band rose after too easy");

        assert_eq!(
            apply_feedback(&mut progress, next.id, PathFeedback::TooHard),
            0
        );
        assert!(progress.skipped.iter().any(|id| id == next.id));
        assert_eq!(progress.band(), 0);

        let after_skip = next_step(HobbyId::Modeling3d, &progress).expect("after skip");
        assert_ne!(after_skip.id, next.id);
        assert_eq!(
            apply_feedback(&mut progress, after_skip.id, PathFeedback::Done),
            0
        );
        assert!(progress.done.iter().any(|id| id == after_skip.id));
    }

    #[test]
    fn focus_app_prefers_matching_hobby() {
        assert_eq!(hobby_for_process("blender"), Some(HobbyId::Modeling3d));
        assert_eq!(
            hobby_for_process(r"C:\Program Files\OrcaSlicer\orcaslicer.exe"),
            Some(HobbyId::Printing3d)
        );
        assert_eq!(hobby_for_process("firefox"), None);

        let tags = vec![
            tag("hobby: 3D printing", "3D printing"),
            tag("hobby: 3D modeling", "3D modeling"),
        ];
        let settings = settings_on(&[HobbyId::Printing3d, HobbyId::Modeling3d]);
        let mut ledger = HobbyPathsLedger::default();
        let out = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            know_me: &tags,
            focus_process: Some("blender"),
            today: "2026-10-28",
            now_ms: 5_000,
        });
        assert_eq!(out.cards.len(), 1);
        assert!(
            out.cards[0].source_id.contains("modeling_3d:"),
            "{}",
            out.cards[0].source_id
        );
        assert!(out.cards[0]
            .pulse
            .hobby_path
            .as_ref()
            .is_some_and(|m| m.hobby == "modeling_3d"));
    }

    #[test]
    fn cooldown_and_daily_cap_hold_repeats() {
        let tags = vec![tag("dev: Rust", "Rust")];
        let settings = settings_on(&[HobbyId::Rust]);
        let mut ledger = HobbyPathsLedger::default();
        let first = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            know_me: &tags,
            focus_process: None,
            today: "2026-10-28",
            now_ms: 10_000,
        });
        assert_eq!(first.cards.len(), 1);
        let again = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            know_me: &tags,
            focus_process: None,
            today: "2026-10-28",
            now_ms: 10_000 + COOLDOWN_MS / 2,
        });
        assert!(again.cards.is_empty(), "cooldown holds the same hobby");
    }

    #[test]
    fn feedback_on_card_updates_ledger() {
        let mut ledger = HobbyPathsLedger::default();
        let meta = HobbyPathMeta {
            hobby: "rust".into(),
            step_id: "rust-cargo-new".into(),
            answered: false,
        };
        assert_eq!(
            feedback_on_card(&mut ledger, &meta, PathFeedback::TooEasy),
            Some(1)
        );
        let progress = ledger.progress.get("rust").expect("progress");
        assert_eq!(progress.band(), 1);
        assert_eq!(progress.done, vec!["rust-cargo-new".to_string()]);
    }
}
