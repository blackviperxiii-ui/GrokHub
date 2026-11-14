//! The Skills page (shell parity with the cabin's Skills page): the skills
//! chat can use, each with an on/off switch. Cabin skills are
//! `{config}/skills/*/SKILL.md`; the rest come from the native engine's own
//! discovery (project, user and plugin skills). Built-in skills live in the
//! Code tab, so they are not listed here, though they still run by name.
//! Creating and editing cabin skills happens here too, through the write
//! path both apps share (`grokhub_agent::skill_write`), and a skill's
//! verify script never runs from here.

use std::fs;
use std::path::{Path, PathBuf};

use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{plugins, SkillSource};
use serde::{Deserialize, Serialize};

/// A SKILL.md bigger than this is not a skill anyone wrote by hand.
const MAX_SKILL_MD: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Cabin,
    Project,
    User,
    Plugin,
    Builtin,
}

impl Source {
    fn of(source: SkillSource) -> Self {
        match source {
            SkillSource::Cabin => Self::Cabin,
            SkillSource::Project => Self::Project,
            SkillSource::User => Self::User,
            SkillSource::Plugin => Self::Plugin,
            SkillSource::Builtin => Self::Builtin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub description: String,
    /// What Use puts in the chat box.
    pub trigger: String,
    pub source: Source,
    /// The skill's folder, empty for a built-in.
    pub folder: String,
    /// Off: chat won't load it until it is switched back on.
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub rows: Vec<Row>,
}

/// A skill with no trigger phrase of its own is asked for by name; the
/// engine's skill list carries the same name.
fn by_name(name: &str) -> String {
    format!("Use the {name} skill.")
}

fn read_capped(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SKILL_MD {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// `{config}/skills/*/SKILL.md`, as the cabin lists them. One that can't be
/// read, holds a secret-looking line or names nothing is skipped.
fn cabin_rows(config_dir: &Path) -> Vec<(Row, PathBuf)> {
    let Ok(dirs) = fs::read_dir(config_dir.join("skills")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for dir in dirs.flatten() {
        let dir = dir.path();
        let Some(raw) = read_capped(&dir.join("SKILL.md")) else {
            continue;
        };
        if !grokhub_core::skill_safe(&raw) {
            continue;
        }
        let skill = grokhub_core::parse_skill_md(&raw);
        let name = skill.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        // A slash line never reaches the model in the shell's chat, so a
        // cabin `/command` trigger is asked for by name instead.
        let trigger = match skill.trigger.trim() {
            "" => by_name(&name),
            slash if slash.starts_with('/') => by_name(&name),
            phrase => phrase.to_string(),
        };
        let row = Row {
            on: !grokhub_agent::skill_is_off_in(&config_dir.join("skills"), &name),
            description: skill.description.trim().to_string(),
            trigger,
            source: Source::Cabin,
            folder: dir.display().to_string(),
            name,
        };
        out.push((row, dir));
    }
    out.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    out
}

/// The skills in this app's own folder that are on, by name, each with when
/// its SKILL.md last changed: what `/sync` and `/inhabit` name to a paired
/// computer. Project and plugin skills belong to their folder, not to you.
pub fn shared(config_dir: &Path) -> Vec<(String, u64)> {
    cabin_rows(config_dir)
        .into_iter()
        .filter(|(row, _)| row.on)
        .map(|(row, dir)| {
            let at = fs::metadata(dir.join("SKILL.md"))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            (row.name, at)
        })
        .collect()
}

/// Cabin skills first, then what the engine discovers for this workspace,
/// each with the folder it lives in (none for a built-in).
fn gather(config_dir: &Path, workspace: &Path, home: Option<&Path>) -> Vec<(Row, Option<PathBuf>)> {
    let _config = ConfigGuard::set(config_dir);
    let mut out: Vec<(Row, Option<PathBuf>)> = cabin_rows(config_dir)
        .into_iter()
        .map(|(row, dir)| (row, Some(dir)))
        .collect();
    // Off skills too, so the page can switch one back on.
    let found =
        grokhub_agent::discover_all_skills(workspace, home, &plugins::skill_dirs(workspace));
    let skills_dir = config_dir.join("skills");
    for skill in found {
        // The cabin rows above read the same folder with its trigger.
        if skill.source == SkillSource::Cabin {
            continue;
        }
        let source = Source::of(skill.source);
        let dir = (source != Source::Builtin).then_some(skill.dir);
        out.push((
            Row {
                on: source == Source::Builtin
                    || !grokhub_agent::skill_is_off_in(&skills_dir, &skill.name),
                trigger: by_name(&skill.name),
                description: skill.description,
                source,
                folder: dir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default(),
                name: skill.name,
            },
            dir,
        ));
    }
    out
}

/// The listed skills. Built-ins stay off this page; they live in the Code tab.
pub fn load(config_dir: &Path, workspace: &Path, home: Option<&Path>) -> Page {
    Page {
        rows: gather(config_dir, workspace, home)
            .into_iter()
            .map(|(row, _)| row)
            .filter(|row| row.source != Source::Builtin)
            .collect(),
    }
}

/// Switch a listed skill on or off for chat, then list again. A name that
/// isn't listed, or a built-in, is refused, so the off list only names real
/// skills.
pub fn set_on(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    name: &str,
    on: bool,
) -> Result<Page, String> {
    let page = load(config_dir, workspace, home);
    if !page.rows.iter().any(|row| row.name == name) {
        return Err(format!("{name} isn't in the skills list any more."));
    }
    grokhub_agent::set_skill_off_in(&config_dir.join("skills"), name, !on)
        .map_err(|e| e.to_string())?;
    Ok(load(config_dir, workspace, home))
}

/// The folder of the listed skill `name` from `source`, found again here so
/// the page never hands the shell a path to open.
pub fn folder(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    source: Source,
    name: &str,
) -> Result<PathBuf, String> {
    gather(config_dir, workspace, home)
        .into_iter()
        .find(|(row, _)| row.source == source && row.name == name)
        .ok_or_else(|| format!("{name} isn't in the skills list any more."))?
        .1
        .filter(|dir| dir.is_dir())
        .ok_or_else(|| format!("{name} has no folder to open."))
}

/// The fields Edit changes on a cabin skill, in the cabin tab's words:
/// what it does, when to use it, and its steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub instructions: String,
}

/// The listed cabin skill `name`, read back and parsed — the same skill
/// Edit starts from in the cabin. A skill from any other source, or one
/// that can't be read back whole, is refused.
fn cabin_skill(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    name: &str,
) -> Result<grokhub_core::SkillMd, String> {
    let (_, dir) = gather(config_dir, workspace, home)
        .into_iter()
        .find(|(row, _)| row.source == Source::Cabin && row.name == name)
        .ok_or_else(|| format!("{name} isn't one of this computer's skills."))?;
    let dir = dir.ok_or_else(|| format!("{name} has no folder to edit."))?;
    let raw = read_capped(&dir.join("SKILL.md"))
        .filter(|raw| grokhub_core::skill_safe(raw))
        .ok_or_else(|| format!("{name} couldn't be read back."))?;
    let skill = grokhub_core::parse_skill_md(&raw);
    if skill.name.trim().is_empty() {
        return Err(format!("{name} couldn't be read back."));
    }
    Ok(skill)
}

/// What Edit shows for the cabin skill `name`.
pub fn detail(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    name: &str,
) -> Result<Detail, String> {
    let skill = cabin_skill(config_dir, workspace, home, name)?;
    Ok(Detail {
        name: skill.name,
        description: skill.description,
        trigger: skill.trigger,
        instructions: skill.instructions,
    })
}

/// Make a cabin skill from a name and what it should do, then list again.
pub fn create(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    name: &str,
    what: &str,
) -> Result<Page, String> {
    grokhub_agent::skill_write::new_skill(config_dir, name, what)?;
    Ok(load(config_dir, workspace, home))
}

/// Edit a cabin skill's description, trigger and steps — the cabin tab's
/// three fields, trimmed the same way, saved through the ChangeLedger so
/// Undo can put the old version back. Everything else (slash, pitfalls,
/// verify, runs) stays as it was.
pub fn edit(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    name: &str,
    description: &str,
    trigger: &str,
    instructions: &str,
) -> Result<Page, String> {
    let mut skill = cabin_skill(config_dir, workspace, home, name)?;
    skill.description = description.trim().to_string();
    skill.trigger = trigger.trim().to_string();
    skill.instructions = instructions.trim().to_string();
    grokhub_agent::skill_write::save_skill_logged(
        config_dir,
        &skill,
        grokhub_agent::harness::Origin::User,
        "edited on the Skills tab",
    )?;
    Ok(load(config_dir, workspace, home))
}

/// Shows `dir` in the system file manager. `dir` comes from [`folder`].
#[cfg(desktop)]
pub fn open_folder(dir: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = Command::new(opener)
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Couldn't open the folder: {err}"))?;
    // Reap it so no zombie is left; the file manager outlives the opener.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(not(desktop))]
pub fn open_folder(_dir: &Path) -> Result<(), String> {
    Err("Opening a folder needs the desktop app.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-shell-skills-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["cfg", "home", "ws/.git"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        dir
    }

    fn write(path: &Path, text: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn skill(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nSteps.\n")
    }

    fn find<'a>(page: &'a Page, name: &str) -> &'a Row {
        page.rows
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("{name} not in {:?}", page.rows))
    }

    #[test]
    fn cabin_project_and_user_skills_list_with_their_source() {
        let root = scratch("sources");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        write(
            &cfg.join("skills/deploy-site/SKILL.md"),
            b"---\nname: deploy-site\ndescription: Ship the site\nslash: /deploy\ntrigger: deploy the site\nruns: 2\n---\n\n# deploy site\n\n## Steps\nbuild\n",
        );
        write(
            &cfg.join("skills/tidy/SKILL.md"),
            b"---\nname: tidy\ndescription: Tidy the repo\n---\n",
        );
        write(
            &cfg.join("skills/ship/SKILL.md"),
            grokhub_core::render_skill_md(&grokhub_core::SkillMd {
                name: "ship".into(),
                description: "Cut a release".into(),
                slash: "/ship".into(),
                ..Default::default()
            })
            .as_bytes(),
        );
        write(
            &ws.join(".grok/skills/lint-fix/SKILL.md"),
            skill("lint-fix", "Fix lint warnings").as_bytes(),
        );
        write(
            &home.join(".claude/skills/notes/SKILL.md"),
            skill("notes", "Take meeting notes").as_bytes(),
        );

        let page = load(&cfg, &ws, Some(&home));

        let deploy = find(&page, "deploy-site");
        assert_eq!(deploy.source, Source::Cabin);
        assert_eq!(deploy.description, "Ship the site");
        assert_eq!(deploy.trigger, "deploy the site");
        assert_eq!(
            deploy.folder,
            cfg.join("skills").join("deploy-site").display().to_string()
        );
        let tidy = find(&page, "tidy");
        assert_eq!(tidy.source, Source::Cabin);
        assert_eq!(tidy.trigger, "Use the tidy skill.");
        let ship = find(&page, "ship");
        assert_eq!(ship.source, Source::Cabin);
        assert_eq!(ship.description, "Cut a release");
        assert_eq!(ship.trigger, "Use the ship skill.");

        let lint = find(&page, "lint-fix");
        assert_eq!(lint.source, Source::Project);
        assert_eq!(lint.description, "Fix lint warnings");
        assert_eq!(lint.trigger, "Use the lint-fix skill.");
        let notes = find(&page, "notes");
        assert_eq!(notes.source, Source::User);
        assert_eq!(notes.description, "Take meeting notes");

        // Built-ins live in the Code tab, so the Skills page leaves them out.
        assert!(
            page.rows.iter().all(|r| r.source != Source::Builtin),
            "{:?}",
            page.rows
        );
        assert!(!page.rows.iter().any(|r| r.name == "qa-walkthrough"));
        assert!(page.rows.iter().all(|r| r.on));

        // The engine also finds the cabin folder; it is listed once.
        let cabin: Vec<_> = page.rows.iter().filter(|r| r.name == "tidy").collect();
        assert_eq!(cabin.len(), 1);
    }

    #[test]
    fn a_switched_off_skill_stays_listed_and_comes_back_on() {
        let root = scratch("on-off");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        write(
            &cfg.join("skills/tidy/SKILL.md"),
            b"---\nname: tidy\ndescription: Tidy the repo\n---\n",
        );
        write(
            &ws.join(".grok/skills/lint-fix/SKILL.md"),
            skill("lint-fix", "Fix lint warnings").as_bytes(),
        );
        let page = set_on(&cfg, &ws, Some(&home), "lint-fix", false).expect("off");
        assert!(
            !find(&page, "lint-fix").on,
            "an off skill is still listed, switched off"
        );
        assert!(find(&page, "tidy").on);
        let page = set_on(&cfg, &ws, Some(&home), "tidy", false).expect("off");
        assert!(!find(&page, "tidy").on);
        let page = set_on(&cfg, &ws, Some(&home), "lint-fix", true).expect("on");
        assert!(find(&page, "lint-fix").on);
        assert!(set_on(&cfg, &ws, Some(&home), "qa-walkthrough", false).is_err());
        assert!(set_on(&cfg, &ws, Some(&home), "no-such-skill", false).is_err());
    }

    #[test]
    fn a_malformed_skill_md_is_skipped_and_the_rest_still_list() {
        let root = scratch("malformed");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        write(
            &cfg.join("skills/garbled/SKILL.md"),
            &[0xff, 0xfe, 0x00, 0x9f],
        );
        write(&cfg.join("skills/nameless/SKILL.md"), b"just some words\n");
        write(
            &cfg.join("skills/leaky/SKILL.md"),
            b"---\nname: leaky\ndescription: key sk-abcdefghijklmnopqrstuv\n---\n",
        );
        write(
            &cfg.join("skills/empty-folder/notes.txt"),
            b"no SKILL.md here",
        );
        write(
            &ws.join(".claude/skills/broken/SKILL.md"),
            b"no front matter at all",
        );
        write(
            &cfg.join("skills/good/SKILL.md"),
            skill("good", "Still listed").as_bytes(),
        );

        let page = load(&cfg, &ws, Some(&home));

        let names: Vec<&str> = page
            .rows
            .iter()
            .filter(|r| r.source != Source::Builtin)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["good"]);
        assert_eq!(find(&page, "good").description, "Still listed");
    }

    #[test]
    fn an_enabled_plugin_skill_lists_as_plugin() {
        let root = scratch("plugin");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        let src = root.join("src/skill-pack");
        write(
            &src.join("plugin.json"),
            br#"{"name":"skill-pack","version":"1.0.0","description":"Demo bundle"}"#,
        );
        write(
            &src.join("skills/ship-it/SKILL.md"),
            skill("ship-it", "Release steps").as_bytes(),
        );
        crate::plugins::install(&cfg, &ws, src.to_str().unwrap());
        crate::plugins::act(&cfg, &ws, "skill-pack", crate::plugins::Act::Trust);
        let on = crate::plugins::act(&cfg, &ws, "skill-pack", crate::plugins::Act::Enable);
        assert_eq!(on.note, "Enabled skill-pack.");

        let page = load(&cfg, &ws, Some(&home));

        let ship = find(&page, "ship-it");
        assert_eq!(ship.source, Source::Plugin);
        assert_eq!(ship.description, "Release steps");
    }

    #[test]
    fn folder_is_found_again_by_source_and_name() {
        let root = scratch("folder");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        write(
            &cfg.join("skills/tidy/SKILL.md"),
            skill("tidy", "Tidy").as_bytes(),
        );

        assert_eq!(
            folder(&cfg, &ws, Some(&home), Source::Cabin, "tidy"),
            Ok(cfg.join("skills/tidy"))
        );
        assert_eq!(
            folder(&cfg, &ws, Some(&home), Source::Project, "tidy"),
            Err("tidy isn't in the skills list any more.".to_string())
        );
        assert_eq!(
            folder(&cfg, &ws, Some(&home), Source::Builtin, "qa-walkthrough"),
            Err("qa-walkthrough has no folder to open.".to_string())
        );
    }

    #[test]
    fn create_makes_a_cabin_skill_the_page_lists() {
        let root = scratch("create");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));

        let page = create(
            &cfg,
            &ws,
            Some(&home),
            "Clip Trimmer",
            "Trim my last game clip to 30 seconds",
        )
        .expect("create");
        let row = find(&page, "Clip Trimmer");
        assert_eq!(row.source, Source::Cabin);
        assert_eq!(row.description, "Trim my last game clip to 30 seconds");
        assert_eq!(
            row.folder,
            cfg.join("skills")
                .join("clip-trimmer")
                .display()
                .to_string()
        );
        // The file on disk is the one the cabin would write.
        let raw = fs::read_to_string(cfg.join("skills/clip-trimmer/SKILL.md")).unwrap();
        let parsed = grokhub_core::parse_skill_md(&raw);
        assert_eq!(parsed.slash, "/clip-trimmer");
        assert_eq!(
            parsed.instructions.trim(),
            "Trim my last game clip to 30 seconds"
        );

        let detail = detail(&cfg, &ws, Some(&home), "Clip Trimmer").expect("detail");
        assert_eq!(detail.name, "Clip Trimmer");
        assert_eq!(detail.description, "Trim my last game clip to 30 seconds");
        assert_eq!(detail.trigger, "Trim my last game clip to 30 seconds");
        assert_eq!(
            detail.instructions.trim(),
            "Trim my last game clip to 30 seconds"
        );

        assert!(create(&cfg, &ws, Some(&home), "clip trimmer", "again").is_err());
        assert!(create(&cfg, &ws, Some(&home), "", "x").is_err());
        assert!(create(&cfg, &ws, Some(&home), "x", " ").is_err());
    }

    #[test]
    fn edit_changes_the_three_fields_and_keeps_the_rest() {
        let root = scratch("edit");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        write(
            &ws.join(".grok/skills/lint-fix/SKILL.md"),
            skill("lint-fix", "Fix lint warnings").as_bytes(),
        );
        grokhub_agent::skill_write::save_skill(
            &cfg,
            &grokhub_core::SkillMd {
                name: "flash-pi".into(),
                description: "write an image".into(),
                slash: "/flash".into(),
                trigger: "flash the pi".into(),
                instructions: "1. dd the image".into(),
                pitfalls: "do not wipe the boot disk".into(),
                verify: "lsblk".into(),
                runs: 3,
            },
        )
        .expect("seed");

        let page = edit(
            &cfg,
            &ws,
            Some(&home),
            "flash-pi",
            "  write a new image  ",
            "flash the pi again",
            "1. dd the newer image",
        )
        .expect("edit");
        assert_eq!(find(&page, "flash-pi").description, "write a new image");

        let after = grokhub_core::parse_skill_md(
            &fs::read_to_string(cfg.join("skills/flash-pi/SKILL.md")).unwrap(),
        );
        assert_eq!(after.description, "write a new image");
        assert_eq!(after.trigger, "flash the pi again");
        assert_eq!(after.instructions.trim(), "1. dd the newer image");
        assert_eq!(after.slash, "/flash");
        assert_eq!(after.pitfalls.trim(), "do not wipe the boot disk");
        assert_eq!(after.verify, "lsblk");
        assert_eq!(after.runs, 3);

        // Only cabin skills are edited here; the rest are refused.
        assert!(edit(&cfg, &ws, Some(&home), "lint-fix", "d", "t", "i").is_err());
        assert!(detail(&cfg, &ws, Some(&home), "lint-fix").is_err());
        assert!(edit(&cfg, &ws, Some(&home), "no-such-skill", "d", "t", "i").is_err());
    }
}
