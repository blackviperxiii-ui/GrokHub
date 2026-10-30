//! Companion files: one folder per companion with a `companion.toml` that
//! names its outfits and which mode each is worn for
//! (`assets/companions/<id>/companion.toml`, documented in
//! `assets/companions/README.md`). The home dock's corner box and the
//! desktop companion both pick their picture from here, so there is one
//! source of what the companion is doing and what she wears for it.
//!
//! Adults-only outfits never ship in the public repo. They load only from
//! `{config}/companions/<id>/adult/adult.toml` on the user's own PC, and
//! everything read from that folder counts as adult whatever the file says.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Nyx's shipped companion file.
pub const NYX_TOML: &str = include_str!("../assets/companions/nyx/companion.toml");

/// The folder under the config dir that holds a companion's local-only
/// adult outfits.
pub const ADULT_DIR: &str = "adult";
/// The outfit list inside that folder.
pub const ADULT_FILE: &str = "adult.toml";

/// What the companion is doing. A tool turn picks Working, Doctor, Reading
/// or Writing by what the tool does, a turn with no tool yet is Thinking, a
/// game the user just left is Gaming, and anything else is Hanging out.
/// Viper and Raunchy are outfits worn over every mode by the explicit
/// personalities, only with adults-only personalities on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mood {
    Working,
    Gaming,
    Doctor,
    Reading,
    HangingOut,
    Viper,
    Thinking,
    Writing,
    Raunchy,
}

impl Mood {
    /// Every mode, in the order the companion file documents them.
    pub const ALL: [Mood; 9] = [
        Mood::Working,
        Mood::Gaming,
        Mood::Doctor,
        Mood::Reading,
        Mood::HangingOut,
        Mood::Viper,
        Mood::Thinking,
        Mood::Writing,
        Mood::Raunchy,
    ];

    /// The mode's name in companion files and launch pins
    /// (`GROKHUB_DECK_MOOD`): `hanging-out`, `working`, ...
    pub fn slug(self) -> &'static str {
        match self {
            Mood::Working => "working",
            Mood::Gaming => "gaming",
            Mood::Doctor => "doctor",
            Mood::Reading => "reading",
            Mood::HangingOut => "hanging-out",
            Mood::Viper => "viper",
            Mood::Thinking => "thinking",
            Mood::Writing => "writing",
            Mood::Raunchy => "raunchy",
        }
    }

    /// The mode named by `raw`, trimmed and in any case; `None` for
    /// anything else.
    pub fn parse(raw: &str) -> Option<Mood> {
        let want = raw.trim().to_ascii_lowercase();
        Mood::ALL.into_iter().find(|m| m.slug() == want)
    }

    /// The eyes-closed and mouth patches only make sense on an outfit
    /// drawn for a mode; these two are outfits, not activities.
    pub fn is_outfit_only(self) -> bool {
        matches!(self, Mood::Viper | Mood::Raunchy)
    }
}

/// One picture laid over the outfit, anchored by its top-left corner in
/// the outfit's own pixels.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub file: String,
    pub x: f32,
    pub y: f32,
}

/// The three mouth patches lip-sync picks from, sharing one anchor.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MouthPatches {
    pub closed: String,
    pub half: String,
    pub open: String,
    pub x: f32,
    pub y: f32,
}

/// One outfit: a picture and the modes it is worn for.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Outfit {
    pub id: String,
    /// Shipped art is a path from the repo root (`brand/nyx-working.jpg`);
    /// local adult art is resolved against its folder when loaded.
    pub file: String,
    #[serde(default)]
    pub moods: Vec<Mood>,
    /// Shown only with adults-only personalities on. Never true for a
    /// shipped outfit; forced on for anything read from the adult folder.
    #[serde(default)]
    pub adult: bool,
    /// Eyes-closed patch for blinking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eyes: Option<Patch>,
    /// Mouth patches for talking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouth: Option<MouthPatches>,
}

impl Outfit {
    /// Whether this outfit can blink and talk, or only breathe and bob.
    pub fn animates_face(&self) -> bool {
        self.eyes.is_some() && self.mouth.is_some()
    }
}

/// A companion and her wardrobe.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    pub id: String,
    pub name: String,
    /// `#rrggbb` for the pedestal's rim and glow; the theme accent when blank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pedestal_tint: Option<String>,
    pub default_outfit: String,
    #[serde(rename = "outfit", default)]
    pub outfits: Vec<Outfit>,
}

/// The adult outfit list on the user's PC: just `[[outfit]]` entries.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdultFile {
    #[serde(rename = "outfit", default)]
    outfits: Vec<Outfit>,
}

/// Parse and check a `companion.toml`. The default outfit must exist,
/// ids must be unique and non-empty, every file must be named, and the
/// pedestal tint must be `#rrggbb`.
pub fn parse(toml_text: &str) -> Result<Companion, String> {
    let c: Companion = toml::from_str(toml_text).map_err(|e| e.to_string())?;
    validate(&c)?;
    Ok(c)
}

fn validate(c: &Companion) -> Result<(), String> {
    if c.id.trim().is_empty() {
        return Err("companion id is empty".into());
    }
    if c.name.trim().is_empty() {
        return Err(format!("companion {}: name is empty", c.id));
    }
    if let Some(tint) = &c.pedestal_tint {
        if !is_hex_rgb(tint) {
            return Err(format!(
                "companion {}: pedestal_tint {tint:?} is not #rrggbb",
                c.id
            ));
        }
    }
    if c.outfits.is_empty() {
        return Err(format!("companion {}: no outfits", c.id));
    }
    let mut seen: Vec<&str> = Vec::new();
    for o in &c.outfits {
        check_outfit(&c.id, o)?;
        if seen.contains(&o.id.as_str()) {
            return Err(format!(
                "companion {}: outfit {:?} listed twice",
                c.id, o.id
            ));
        }
        seen.push(&o.id);
    }
    if !seen.contains(&c.default_outfit.as_str()) {
        return Err(format!(
            "companion {}: default_outfit {:?} is not an outfit",
            c.id, c.default_outfit
        ));
    }
    Ok(())
}

fn check_outfit(companion: &str, o: &Outfit) -> Result<(), String> {
    if o.id.trim().is_empty() {
        return Err(format!("companion {companion}: an outfit has no id"));
    }
    if o.file.trim().is_empty() {
        return Err(format!(
            "companion {companion}: outfit {:?} has no file",
            o.id
        ));
    }
    if let Some(m) = &o.mouth {
        if [&m.closed, &m.half, &m.open]
            .iter()
            .any(|f| f.trim().is_empty())
        {
            return Err(format!(
                "companion {companion}: outfit {:?} mouth needs closed, half and open",
                o.id
            ));
        }
    }
    Ok(())
}

/// `#rrggbb`, case-insensitive.
pub fn is_hex_rgb(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// `#rrggbb` as (r, g, b), or `None` when it isn't one.
pub fn hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    if !is_hex_rgb(s) {
        return None;
    }
    let v = u32::from_str_radix(&s[1..], 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Companion {
    /// The outfit with `id`, if there is one.
    pub fn outfit(&self, id: &str) -> Option<&Outfit> {
        self.outfits.iter().find(|o| o.id == id)
    }

    /// The outfit worn when nothing else fits. [`parse`] makes sure it is
    /// there; a hand-built companion with no outfits gets an empty one.
    pub fn default_outfit(&self) -> &Outfit {
        static NONE: Outfit = Outfit {
            id: String::new(),
            file: String::new(),
            moods: Vec::new(),
            adult: false,
            eyes: None,
            mouth: None,
        };
        self.outfit(&self.default_outfit)
            .or_else(|| self.outfits.first())
            .unwrap_or(&NONE)
    }

    /// The outfit for `mood`: the first one listing that mode, skipping
    /// adult outfits unless `adults_ok`, else the default outfit.
    pub fn outfit_for(&self, mood: Mood, adults_ok: bool) -> &Outfit {
        self.outfits
            .iter()
            .filter(|o| adults_ok || !o.adult)
            .find(|o| o.moods.contains(&mood))
            .unwrap_or_else(|| self.default_outfit())
    }

    /// Whether any outfit is worn for `mood` without falling back.
    pub fn has_outfit_for(&self, mood: Mood, adults_ok: bool) -> bool {
        self.outfits
            .iter()
            .any(|o| (adults_ok || !o.adult) && o.moods.contains(&mood))
    }

    /// Whether any shipped outfit claims to be adult. Always false for a
    /// companion file in the repo; the CI guard and a test keep it so.
    pub fn ships_adult_art(&self) -> bool {
        self.outfits.iter().any(|o| o.adult)
    }

    /// Add the outfits in the local adult folder `dir` (see
    /// [`adult_dir`]). Every one of them is marked adult and its files
    /// are resolved against `dir`. A missing folder or file adds nothing;
    /// a broken file is reported and adds nothing, so a typo in it can't
    /// take the shipped outfits down with it. An id that matches a
    /// shipped outfit replaces it.
    pub fn with_local_adult(mut self, dir: &Path) -> (Self, Option<String>) {
        let path = dir.join(ADULT_FILE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (self, None);
        };
        let parsed: Result<AdultFile, _> = toml::from_str(&text);
        let file = match parsed {
            Ok(f) => f,
            Err(e) => return (self, Some(format!("{}: {e}", path.display()))),
        };
        for mut o in file.outfits {
            if let Err(e) = check_outfit(&self.id, &o) {
                return (self, Some(format!("{}: {e}", path.display())));
            }
            o.adult = true;
            o.file = under(dir, &o.file);
            if let Some(eyes) = &mut o.eyes {
                eyes.file = under(dir, &eyes.file);
            }
            if let Some(m) = &mut o.mouth {
                m.closed = under(dir, &m.closed);
                m.half = under(dir, &m.half);
                m.open = under(dir, &m.open);
            }
            self.outfits.retain(|have| have.id != o.id);
            self.outfits.push(o);
        }
        (self, None)
    }
}

/// `file` inside `dir`, unless it is already absolute.
fn under(dir: &Path, file: &str) -> String {
    let p = Path::new(file);
    if p.is_absolute() {
        file.to_string()
    } else {
        dir.join(p).to_string_lossy().into_owned()
    }
}

/// Where a companion's local-only adult outfits live:
/// `{config}/companions/<id>/adult`.
pub fn adult_dir(config_dir: &Path, companion_id: &str) -> PathBuf {
    config_dir
        .join("companions")
        .join(companion_id)
        .join(ADULT_DIR)
}

/// Nyx as shipped, parsed once. A test keeps the shipped file valid; if it
/// ever weren't, she falls back to hanging out in every mode rather than
/// taking the app down.
pub fn nyx() -> &'static Companion {
    static NYX: std::sync::OnceLock<Companion> = std::sync::OnceLock::new();
    NYX.get_or_init(|| parse(NYX_TOML).unwrap_or_else(|_| fallback_nyx()))
}

fn fallback_nyx() -> Companion {
    Companion {
        id: "nyx".into(),
        name: "Nyx".into(),
        pedestal_tint: None,
        default_outfit: "hanging-out".into(),
        outfits: vec![Outfit {
            id: "hanging-out".into(),
            file: "brand/nyx-hanging-out.jpg".into(),
            moods: vec![Mood::HangingOut],
            adult: false,
            eyes: None,
            mouth: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
id = "demo"
name = "Demo"
pedestal_tint = "#5AD1FF"
default_outfit = "chill"

[[outfit]]
id = "chill"
file = "brand/demo-chill.png"
moods = ["hanging-out"]

[[outfit]]
id = "desk"
file = "brand/demo-desk.png"
moods = ["working", "thinking"]

[outfit.eyes]
file = "brand/demo-desk-eyes.png"
x = 10
y = 20

[outfit.mouth]
closed = "brand/demo-desk-m0.png"
half = "brand/demo-desk-m1.png"
open = "brand/demo-desk-m2.png"
x = 12
y = 40
"##;

    #[test]
    fn the_sample_parses_into_outfits_with_patches() {
        let c = parse(SAMPLE).unwrap();
        assert_eq!(c.id, "demo");
        assert_eq!(c.name, "Demo");
        assert_eq!(c.pedestal_tint.as_deref(), Some("#5AD1FF"));
        assert_eq!(c.outfits.len(), 2);
        let desk = c.outfit("desk").unwrap();
        assert_eq!(desk.moods, vec![Mood::Working, Mood::Thinking]);
        assert!(!desk.adult);
        assert!(desk.animates_face());
        assert_eq!(desk.eyes.as_ref().unwrap().file, "brand/demo-desk-eyes.png");
        assert_eq!(desk.eyes.as_ref().unwrap().x, 10.0);
        assert_eq!(desk.mouth.as_ref().unwrap().open, "brand/demo-desk-m2.png");
        assert_eq!(desk.mouth.as_ref().unwrap().y, 40.0);
        assert!(!c.outfit("chill").unwrap().animates_face());
    }

    #[test]
    fn a_mood_picks_its_outfit_or_the_default() {
        let c = parse(SAMPLE).unwrap();
        assert_eq!(c.outfit_for(Mood::Working, false).id, "desk");
        assert_eq!(c.outfit_for(Mood::Thinking, false).id, "desk");
        assert_eq!(c.outfit_for(Mood::HangingOut, false).id, "chill");
        assert_eq!(
            c.outfit_for(Mood::Gaming, false).id,
            "chill",
            "no gaming outfit"
        );
        assert!(c.has_outfit_for(Mood::Working, false));
        assert!(!c.has_outfit_for(Mood::Gaming, false));
        assert_eq!(c.default_outfit().id, "chill");
    }

    #[test]
    fn broken_files_say_what_is_wrong() {
        let no_default = SAMPLE.replace("default_outfit = \"chill\"", "default_outfit = \"nope\"");
        assert!(parse(&no_default)
            .unwrap_err()
            .contains("default_outfit \"nope\" is not an outfit"));

        let twice = format!("{SAMPLE}\n[[outfit]]\nid = \"desk\"\nfile = \"x.png\"\n");
        assert!(parse(&twice).unwrap_err().contains("listed twice"));

        let no_file = SAMPLE.replace("file = \"brand/demo-chill.png\"", "file = \"\"");
        assert!(parse(&no_file).unwrap_err().contains("has no file"));

        let bad_tint = SAMPLE.replace("#5AD1FF", "5AD1FF");
        assert!(parse(&bad_tint).unwrap_err().contains("not #rrggbb"));

        let bad_mood = SAMPLE.replace("\"hanging-out\"", "\"dancing\"");
        assert!(parse(&bad_mood).is_err());

        let unknown_key = format!("{SAMPLE}\nhat = true\n");
        assert!(parse(&unknown_key).is_err());

        assert!(parse("").is_err());
    }

    #[test]
    fn mood_names_round_trip_and_reject_anything_else() {
        for m in Mood::ALL {
            assert_eq!(Mood::parse(m.slug()), Some(m));
            assert_eq!(Mood::parse(&m.slug().to_ascii_uppercase()), Some(m));
            let toml_text = format!("m = \"{}\"", m.slug());
            let back: std::collections::BTreeMap<String, Mood> =
                toml::from_str(&toml_text).unwrap();
            assert_eq!(back["m"], m);
        }
        assert_eq!(Mood::parse(" Hanging-Out "), Some(Mood::HangingOut));
        assert_eq!(Mood::parse("hanging_out"), None);
        assert_eq!(Mood::parse("hello"), None);
        assert_eq!(Mood::parse(""), None);
        assert!(Mood::Viper.is_outfit_only());
        assert!(Mood::Raunchy.is_outfit_only());
        assert!(!Mood::Working.is_outfit_only());
    }

    #[test]
    fn the_shipped_file_parses_and_the_fallback_is_only_for_a_broken_one() {
        assert!(parse(NYX_TOML).is_ok(), "{:?}", parse(NYX_TOML).err());
        let fb = fallback_nyx();
        assert_eq!(fb.outfits.len(), 1);
        assert_eq!(fb.outfit_for(Mood::Working, false).id, "hanging-out");
        let empty = Companion {
            outfits: Vec::new(),
            ..fb
        };
        assert_eq!(empty.default_outfit().id, "");
        assert_eq!(empty.outfit_for(Mood::Gaming, true).file, "");
    }

    #[test]
    fn hex_colours_parse() {
        assert_eq!(hex_rgb("#5ad1ff"), Some((0x5a, 0xd1, 0xff)));
        assert_eq!(hex_rgb("#000000"), Some((0, 0, 0)));
        assert_eq!(hex_rgb("5ad1ff"), None);
        assert_eq!(hex_rgb("#5ad1f"), None);
        assert_eq!(hex_rgb("#5ad1fg"), None);
        assert!(!is_hex_rgb("#5ad1ff00"));
    }

    #[test]
    fn nyx_ships_six_safe_outfits_covering_every_mode_but_raunchy() {
        let nyx = nyx();
        assert_eq!(nyx.id, "nyx");
        assert_eq!(nyx.name, "Nyx");
        assert_eq!(nyx.default_outfit, "hanging-out");
        assert_eq!(nyx.outfits.len(), 6);
        assert!(!nyx.ships_adult_art(), "adult art never ships in the repo");
        for o in &nyx.outfits {
            assert!(o.file.starts_with("brand/nyx-"), "{}", o.file);
            assert!(o.file.ends_with(".jpg"), "{}", o.file);
        }
        assert_eq!(nyx.outfit_for(Mood::Working, false).id, "working");
        assert_eq!(nyx.outfit_for(Mood::Thinking, false).id, "working");
        assert_eq!(nyx.outfit_for(Mood::Writing, false).id, "working");
        assert_eq!(nyx.outfit_for(Mood::Gaming, false).id, "gaming");
        assert_eq!(nyx.outfit_for(Mood::Doctor, false).id, "doctor");
        assert_eq!(nyx.outfit_for(Mood::Reading, false).id, "reading");
        assert_eq!(nyx.outfit_for(Mood::HangingOut, false).id, "hanging-out");
        assert_eq!(nyx.outfit_for(Mood::Viper, false).id, "viper");
        assert!(!nyx.has_outfit_for(Mood::Raunchy, true));
        assert_eq!(
            nyx.outfit_for(Mood::Raunchy, true).id,
            "hanging-out",
            "raunchy only comes from the local adult folder"
        );
    }

    #[test]
    fn the_adult_folder_is_under_config_and_its_outfits_are_always_adult() {
        let config = Path::new("/home/user/.config/GrokHub");
        assert_eq!(
            adult_dir(config, "nyx"),
            Path::new("/home/user/.config/GrokHub/companions/nyx/adult")
        );

        let tmp = std::env::temp_dir().join(format!(
            "grokhub-companion-adult-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join(ADULT_FILE),
            "[[outfit]]\nid = \"raunchy\"\nfile = \"raunchy.png\"\nmoods = [\"raunchy\"]\n\n\
             [[outfit]]\nid = \"viper\"\nfile = \"viper-late.png\"\nmoods = [\"viper\"]\nadult = false\n",
        )
        .unwrap();

        let (shipped, problem) = nyx().clone().with_local_adult(&tmp);
        assert_eq!(problem, None);
        assert_eq!(shipped.outfits.len(), 7, "viper replaced, raunchy added");
        let raunchy = shipped.outfit("raunchy").unwrap();
        assert!(raunchy.adult);
        assert_eq!(
            Path::new(&raunchy.file),
            tmp.join("raunchy.png"),
            "resolved against the adult folder"
        );
        let viper = shipped.outfit("viper").unwrap();
        assert!(viper.adult, "adult = false in the file is ignored");
        assert_eq!(Path::new(&viper.file), tmp.join("viper-late.png"));

        assert_eq!(shipped.outfit_for(Mood::Raunchy, true).id, "raunchy");
        assert_eq!(
            shipped.outfit_for(Mood::Raunchy, false).id,
            "hanging-out",
            "adults-only off hides it"
        );
        assert_eq!(
            shipped.outfit_for(Mood::Viper, false).id,
            "hanging-out",
            "the replaced viper outfit is adult now"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_missing_or_broken_adult_file_leaves_the_shipped_outfits_alone() {
        let nowhere = Path::new("/nonexistent/grokhub/companions/nyx/adult");
        let (shipped, problem) = nyx().clone().with_local_adult(nowhere);
        assert_eq!(problem, None);
        assert_eq!(shipped.outfits.len(), 6);

        let tmp = std::env::temp_dir().join(format!(
            "grokhub-companion-adult-bad-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(ADULT_FILE), "[[outfit]]\nid = \"x\"\n").unwrap();
        let (shipped, problem) = nyx().clone().with_local_adult(&tmp);
        assert!(problem.unwrap().contains("adult.toml"));
        assert_eq!(shipped.outfits.len(), 6);

        std::fs::write(
            tmp.join(ADULT_FILE),
            "[[outfit]]\nid = \"x\"\nfile = \"\"\n",
        )
        .unwrap();
        let (shipped, problem) = nyx().clone().with_local_adult(&tmp);
        assert!(problem.unwrap().contains("has no file"));
        assert_eq!(shipped.outfits.len(), 6);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
